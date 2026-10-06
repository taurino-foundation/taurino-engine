use crate::{log_if_err, manager::EngineManager};

use anyhow::{Result, anyhow};
use std::sync::{
  Arc,
  atomic::{AtomicBool, Ordering},
};
#[cfg(target_os = "macos")]
use taurino_core::native::tao::platform::macos::EventLoopWindowTargetExtMacOS;
use taurino_core::{
  EngineLoopEvent, EngineWindowTarget, EventLoopMessage,
  native::tao::{
    event::{Event, WindowEvent as TaoWindowEvent},
    event_loop::ControlFlow,
    window::WindowId as TaoWindowId,
  },
  schema::{
    event::{SynthesizedWindowEvent, WebViewEvent, WindowEvent, WindowEventWrapper},
    webview::WebViewId,
    window::WindowId,
  },
};

/// Central event dispatcher and lifecycle coordinator for the engine.
///
/// Native Tao window events, synthesized window events and WebView events are
/// intentionally handled through separate paths:
///
/// - native Tao events -> `WindowEventWrapper::parse(...)`
/// - synthesized window events -> `WindowEventWrapper::from(...)`
/// - WebView events -> forwarded as `WebViewEvent`
///
/// This mirrors the event routing used by Tauri's Wry runtime.
pub struct EngineEventHandler {
  /// Shared access to all engine subsystem managers.
  manager: Arc<EngineManager>,

  /// Prevents shutdown logic from executing more than once.
  shutting_down: AtomicBool,
}

impl EngineEventHandler {
  pub fn new(manager: Arc<EngineManager>) -> Self {
    Self {
      manager,
      shutting_down: AtomicBool::new(false),
    }
  }

  /// Handles a single Tao event-loop event.
  pub fn handle_event<'a>(
    &self,
    event: EngineLoopEvent<'a>,
    target: &EngineWindowTarget,
    control_flow: &mut ControlFlow,
  ) {
    if self.shutting_down.load(Ordering::Acquire) {
      return;
    }

    /*
     * `event` is consumed by the match below, so inspect LoopDestroyed
     * beforehand.
     */
    let loop_destroyed = matches!(&event, Event::LoopDestroyed);

    let result = (|| -> Result<()> {
      if !matches!(*control_flow, ControlFlow::ExitWithCode(_)) {
        *control_flow = ControlFlow::Wait;
      }

      match event {
        /*
         * Synthesized window events.
         *
         * These originate from Wry/WebView behavior but semantically
         * represent window events.
         *
         * This arm must come before the generic UserEvent arm.
         */
        Event::UserEvent(EventLoopMessage::SynthesizedWindowEvent(window_id, _webview_id, event)) => {
          self.handle_synthesized_window_event(window_id, event)?;
        }

        /*
         * WebView-specific events.
         *
         * These remain WebView events and are not converted into
         * WindowEvent.
         */
        Event::UserEvent(EventLoopMessage::WebviewEvent(window_id, webview_id, event)) => {
          self.handle_webview_event(window_id, webview_id, event)?;
        }

        /*
         * Native Tao window events.
         */
        Event::WindowEvent { event, window_id, .. } => {
          self.handle_window_event(event, window_id, control_flow)?;
        }

        /*
         * Remaining engine messages.
         */
        Event::UserEvent(message) => {
          self.handle_user_message(message, target, control_flow)?;
        }

        _ => {}
      }

      Ok(())
    })();

    log_if_err!(result);

    /*
     * Global cleanup must run at most once.
     */
    if (loop_destroyed || matches!(*control_flow, ControlFlow::ExitWithCode(_)))
      && !self.shutting_down.swap(true, Ordering::AcqRel)
    {
      log_if_err!(self.cleanup_app_resources());
    }
  }

  /// Handles window events synthesized by WebView/Wry.
  ///
  /// This intentionally mirrors Tauri:
  ///
  /// `SynthesizedWindowEvent`
  ///     -> `WindowEventWrapper::from(...)`
  ///     -> public `WindowEvent`
  ///     -> global window event handler
  fn handle_synthesized_window_event(&self, window_id: WindowId, event: SynthesizedWindowEvent) -> Result<()> {
    let Some(event) = WindowEventWrapper::from(event).0 else {
      return Ok(());
    };

    /*
     * Resolve only the information required by the callback while the
     * WindowManager lock is held.
     */
    let window_label = {
      let window_manager = self.manager.window()?;

      let window = window_manager
        .get_by_id(window_id)
        .ok_or_else(|| anyhow!("window with id {window_id:?} not found for synthesized window event"))?;

      window.label.clone()
    };

    /*
     * The manager lock is released before invoking external callbacks.
     */
    self.manager.emit_global_window_event(window_label, event)?;

    Ok(())
  }

  /// Handles WebView-specific events.
  ///
  /// Unlike synthesized window events, WebView events are not converted
  /// through `WindowEventWrapper`. They remain `WebViewEvent` values.
  fn handle_webview_event(&self, window_id: WindowId, webview_id: WebViewId, event: WebViewEvent) -> Result<()> {
    /*
     * Resolve labels while holding the WindowManager lock only briefly.
     */
    let (window_label, webview_label) = {
      let window_manager = self.manager.window()?;

      let window = window_manager
        .get_by_id(window_id)
        .ok_or_else(|| anyhow!("window with id {window_id:?} not found for WebView event"))?;

      let webview = window
        .webview(webview_id)
        .ok_or_else(|| anyhow!("WebView with id {webview_id:?} not found in window {window_id:?}"))?;

      (window.label.clone(), webview.label().to_owned())
    };

    /*
     * Emit after releasing the WindowManager lock.
     */
    self
      .manager
      .emit_global_webview_event(window_label, webview_label, event)?;

    Ok(())
  }

  /// Handles engine user messages which are not Window/WebView events.
  fn handle_user_message(
    &self,
    message: EventLoopMessage,
    target: &EngineWindowTarget,
    control_flow: &mut ControlFlow,
  ) -> Result<()> {
    match message {
      EventLoopMessage::Shutdown => {
        println!("Keyboard interrupt detected. Exiting...");
        self.shutdown(control_flow)?;
      }

      EventLoopMessage::TaskWithTarget(task) => {
        task(target, control_flow)?;
      }

      EventLoopMessage::Task(task) => {
        task();
      }

      EventLoopMessage::RequestExit(_exit) => {
        /*
         * Exit-request handling can be implemented here if the engine
         * requires a dedicated prevent/confirm flow.
         */
      }
      #[cfg(target_os = "macos")]
      EventLoopMessage::SetDockVisibility(visible) => target.set_dock_visibility(visible),
      /*
       * These variants are intercepted in `handle_event` before the
       * generic UserEvent path reaches this function.
       */
      EventLoopMessage::SynthesizedWindowEvent(..) => {
        unreachable!("SynthesizedWindowEvent must be handled before handle_user_message");
      }

      EventLoopMessage::WebviewEvent(..) => {
        unreachable!("WebviewEvent must be handled before handle_user_message");
      }
      EventLoopMessage::ContainsFullScreenElementChanged(window_id, fullscreen) => {
        let window = {
          let window_manager = self.manager.window()?;

          window_manager
            .get_by_id(window_id)
            .cloned()
            .ok_or_else(|| anyhow!("window with id {window_id:?} not found"))?
        };

        window.set_fullscreen(fullscreen)?;
      }
    }

    Ok(())
  }

  /// Handles a native Tao window event.
  ///
  /// Native Tao events require `WindowEventWrapper::parse(...)` because the
  /// mapping may depend on the current window state.
  fn handle_window_event(
    &self,
    event: TaoWindowEvent<'_>,
    window_id: TaoWindowId,
    control_flow: &mut ControlFlow,
  ) -> Result<()> {
    /*
     * Destruction is lifecycle handling rather than ordinary event
     * forwarding.
     */
    if matches!(event, TaoWindowEvent::Destroyed) {
      return self.handle_window_destroyed(window_id, control_flow);
    }

    /*
     * CloseRequested also has dedicated lifecycle semantics.
     */
    if matches!(event, TaoWindowEvent::CloseRequested { .. }) {
      return self.handle_close_requested(window_id, control_flow);
    }

    let (window_label, mapped_event) = {
      let device_registry = self.manager.device_registry()?;
      let window_manager = self.manager.window()?;

      let window = window_manager
        .get_by_tao_id(window_id)
        .ok_or_else(|| anyhow!("window with Tao id {window_id:?} not found"))?;

      let Some(mapped_event) = WindowEventWrapper::parse(window, &event, device_registry.clone())?.0 else {
        return Ok(());
      };

      (window.label.clone(), mapped_event)
    };

    self.manager.emit_global_window_event(window_label, mapped_event)?;

    Ok(())
  }

  /// Handles a native close request.
  fn handle_close_requested(&self, window_id: TaoWindowId, control_flow: &mut ControlFlow) -> Result<()> {
    let window_label = {
      let window_manager = self.manager.window()?;

      let window = window_manager
        .get_by_tao_id(window_id)
        .ok_or_else(|| anyhow!("window with Tao id {window_id:?} not found"))?;

      window.label.clone()
    };

    self
      .manager
      .emit_global_window_event(window_label.clone(), WindowEvent::CloseRequested)?;

    self.close_window(window_id, &window_label, control_flow)
  }

  /// Handles Tao's final native destruction notification.
  fn handle_window_destroyed(&self, window_id: TaoWindowId, control_flow: &mut ControlFlow) -> Result<()> {
    /*
     * The window may already have been removed when CloseRequested was
     * handled. In that case Tao is only confirming destruction.
     */
    let window = {
      let mut window_manager = self.manager.window()?;
      window_manager.remove_by_tao_id(window_id)
    };

    let Some(window) = window else {
      return Ok(());
    };

    let window_label = window.label.clone();

    self
      .manager
      .emit_global_window_event(window_label.clone(), WindowEvent::Destroyed)?;

    self.cleanup_window_resources(&window_label, control_flow)?;

    Ok(())
  }

  /// Removes a window and releases its associated resources.
  fn close_window(&self, window_id: TaoWindowId, window_label: &str, control_flow: &mut ControlFlow) -> Result<()> {
    let removed_window = {
      let mut window_manager = self.manager.window()?;
      window_manager.remove_by_tao_id(window_id)
    };

    /*
     * Cleanup may acquire other manager locks, therefore it must happen
     * after the WindowManager guard has been released.
     */
    if removed_window.is_some() {
      self.cleanup_window_resources(window_label, control_flow)?;
    }

    let no_windows_left = {
      let window_manager = self.manager.window()?;
      window_manager.is_empty()
    };

    if no_windows_left {
      *control_flow = ControlFlow::Exit;
    }

    Ok(())
  }

  /// Releases resources owned by one window.
  fn cleanup_window_resources(&self, _window_label: &str, control_flow: &mut ControlFlow) -> Result<()> {
    /*
     * Window-specific manager cleanup belongs here.
     *
     * Example:
     *
     * self.manager
     *     .trayicon()?
     *     .destroy_for_window(window_label)?;
     *
     * self.manager
     *     .menu()?
     *     .remove_window_menu(window_label)?;
     */

    if self.manager.window()?.is_empty() {
      *control_flow = ControlFlow::Exit;
    }

    Ok(())
  }

  /// Releases application-global resources.
  fn cleanup_app_resources(&self) -> Result<()> {
    Ok(())
  }

  /// Performs an explicit application shutdown.
  fn shutdown(&self, control_flow: &mut ControlFlow) -> Result<()> {
    /*
     * Shutdown is idempotent.
     */
    if self.shutting_down.swap(true, Ordering::AcqRel) {
      return Ok(());
    }

    /*
     * Collect window labels and clear the registry while the manager is
     * locked.
     */
    let window_labels = {
      let mut window_manager = self.manager.window()?;

      let labels = window_manager.labels().map(str::to_owned).collect::<Vec<_>>();

      window_manager.clear();

      labels
    };

    /*
     * The WindowManager lock is released before cleanup and callbacks.
     */
    for window_label in window_labels {
      log_if_err!(self.cleanup_window_resources(&window_label, control_flow));

      log_if_err!(
        self
          .manager
          .emit_global_window_event(window_label, WindowEvent::Destroyed,)
      );
    }

    self.cleanup_app_resources()?;

    *control_flow = ControlFlow::Exit;

    Ok(())
  }
}

/*
pub enum ApplicationMessage {
  #[cfg(target_os = "macos")]
  Show,
  #[cfg(target_os = "macos")]
  Hide,
  #[cfg(any(target_os = "macos", target_os = "ios"))]
  FetchDataStoreIdentifiers(Box<dyn FnOnce(Vec<[u8; 16]>) + Send + 'static>),
  #[cfg(any(target_os = "macos", target_os = "ios"))]
  RemoveDataStore([u8; 16], Box<dyn FnOnce(Result<()>) + Send + 'static>),
}

    #[cfg(target_os = "macos")]
    Message::SetActivationPolicy(activation_policy) => {
      event_loop.set_activation_policy_at_runtime(tao_activation_policy(activation_policy))
    }
    #[cfg(target_os = "macos")]
    Message::SetDockVisibility(visible) => event_loop.set_dock_visibility(visible),

Message::Application(application_message) => match application_message {
      #[cfg(target_os = "macos")]
      ApplicationMessage::Show => {
        event_loop.show_application();
      }
      #[cfg(target_os = "macos")]
      ApplicationMessage::Hide => {
        event_loop.hide_application();
      }
      #[cfg(any(target_os = "macos", target_os = "ios"))]
      ApplicationMessage::FetchDataStoreIdentifiers(cb) => {
        if let Err(e) = WebView::fetch_data_store_identifiers(cb) {
          // this shouldn't ever happen because we're running on the main thread
          // but let's be safe and warn here
          log::error!("failed to fetch data store identifiers: {e}");
        }
      }
      #[cfg(any(target_os = "macos", target_os = "ios"))]
      ApplicationMessage::RemoveDataStore(uuid, cb) => {
        WebView::remove_data_store(&uuid, move |res| {
          cb(res.map_err(|_| Error::FailedToRemoveDataStore))
        })
      }
    },


*/

/* pub fn event(f:taurino_core::native::tao::event::WindowEvent){
    match f {
        TaoWindowEvent::Resized(physical_size){},
        TaoWindowEvent::Moved(physical_position){},
        TaoWindowEvent::CloseRequested{},
        TaoWindowEvent::Destroyed{},
        TaoWindowEvent::Started{},
        TaoWindowEvent::Suspended{},
        TaoWindowEvent::Resumed{},
        TaoWindowEvent::Stopped{},
        TaoWindowEvent::DroppedFile{},
        TaoWindowEvent::HoveredFile{},
        TaoWindowEvent::HoveredFileCancelled{},
        TaoWindowEvent::ReceivedImeText{},
        TaoWindowEvent::Focused{},
        TaoWindowEvent::KeyboardInput { device_id, event, is_synthetic }{},
        TaoWindowEvent::ModifiersChanged(modifiers_state){},
        TaoWindowEvent::CursorMoved { device_id, position, modifiers }{},
        TaoWindowEvent::CursorEntered { device_id }{},
        TaoWindowEvent::CursorLeft { device_id }{},
        TaoWindowEvent::MouseWheel { device_id, delta, phase, modifiers }{},
        TaoWindowEvent::MouseInput { device_id, state, button, modifiers }{},
        TaoWindowEvent::TouchpadPressure { device_id, pressure, stage }{},
        TaoWindowEvent::AxisMotion { device_id, axis, value }{},
        TaoWindowEvent::Touch(touch){},
        TaoWindowEvent::ScaleFactorChanged { scale_factor, new_inner_size }{},
        TaoWindowEvent::ThemeChanged(theme){},
        TaoWindowEvent::DecorationsClick{},
        _{},
    }
} */
