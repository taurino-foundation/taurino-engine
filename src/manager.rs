/// Shared registry and access point for engine subsystem managers.
///
/// `EngineManager` groups the long-lived services that make up the Taurino
/// runtime and provides synchronized access to them through one shared object.
///
/// The manager itself does not own the application event loop. Event-loop
/// ownership remains with [`Engine`]. Instead, `EngineManager` represents
/// runtime state that needs to be accessible from windows, WebViews, callbacks
/// and other engine components.
///
/// # Managed subsystems
///
/// The manager currently provides access to:
///
/// - the shared WebView context;
/// - the global resource table;
/// - the menu manager;
/// - the tray icon manager;
/// - the window manager;
/// - the global window-event callback;
/// - the global WebView-event callback.
///
/// # Shared ownership
///
/// Instances are always returned as [`Arc<EngineManager>`].
///
/// This allows callbacks and subsystem objects to retain access to the common
/// engine state without requiring ownership of the top-level [`Engine`].
///
/// # Synchronization
///
/// Mutable subsystem managers are stored behind mutex-backed [`ArcMut`] values.
/// Accessor methods return scoped [`MutexGuard`] values.
///
/// Guards should be kept alive only for the minimum amount of time necessary.
/// Calling external callbacks or acquiring another manager while holding a
/// manager guard should generally be avoided because it can introduce lock
/// ordering dependencies and deadlocks.
///
/// # Callbacks
///
/// Global event handlers are optional callbacks stored independently from the
/// subsystem managers.
///
/// Window and WebView event emitters clone the [`Arc<EngineManager>`] before
/// invoking the callback, allowing event consumers to access other engine
/// facilities when required.
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::{Result, anyhow};
use taurino_core::{
  EngineLoopClosed, EngineLoopProxy, EventLoopMessage,
  aio::IPCRuntime,
  core::{
    resources::ResourceTable,
    stores::{DeviceRegistry, WebContextStore},
  },
  lock,
  menu::MenuManager,
  schema::event::{WebViewEvent, WindowEvent},
  utils::{ArcMut, lock_state},
};

use crate::{trayicon::TrayIconManager, window::WindowManager};

/// Callback invoked for global window events.
///
/// The handler receives the shared [`EngineManager`], the label of the window
/// that emitted the event, and additional event-specific information.
///
/// # Parameters
///
/// - `manager` - Shared engine manager instance.
/// - `window_label` - Label identifying the window that emitted the event.
/// - `event` - Additional information associated with the event.
///
/// # Note
///
/// `event` is currently represented as a string slice. It may be
/// replaced by a dedicated event enum once the window event API is finalized.
pub type WindowEventHandler = Box<dyn Fn(Arc<EngineManager>, String, WindowEvent) + Send + Sync + 'static>;

/// Callback invoked for global WebView events.
///
/// The handler receives the shared [`EngineManager`], the label of the
/// containing window, the label of the WebView that emitted the event,
/// and additional event-specific information.
///
/// # Parameters
///
/// - `manager` - Shared engine manager instance.
/// - `window_label` - Label identifying the window containing the WebView.
/// - `webview_label` - Label identifying the WebView that emitted the event.
/// - `event` - Additional information associated with the event.
///
/// # Note
///
/// `event` is currently represented as a string slice. It may be
/// replaced by a dedicated event enum once the WebView event API is finalized.
pub type WebViewEventHandler = Box<dyn Fn(Arc<EngineManager>, String, String, WebViewEvent) + Send + Sync + 'static>;

/// Provides access to the engine's shared managers and global event handlers.
///
/// `EngineManager` acts as the central shared state container of the engine.
/// Instances are intended to be shared through [`Arc`].
pub struct EngineManager {
  _device_registry: ArcMut<DeviceRegistry>,
  /// Shared WebView context store.
  _webcontext: WebContextStore,

  /// Shared application resource table.
  _resource_table: ArcMut<ResourceTable>,

  /// Shared menu manager.
  _menu_manager: ArcMut<MenuManager>,

  /// Shared tray icon manager.
  _trayicon_manager: ArcMut<TrayIconManager>,

  /// Shared window manager.
  _window_manager: ArcMut<WindowManager>,

  /// Optional global handler invoked for window events.
  _global_window_event_handler: Mutex<Option<WindowEventHandler>>,

  /// Optional global handler invoked for WebView events.
  _global_webview_event_handler: Mutex<Option<WebViewEventHandler>>,

  pub(crate) proxy: EngineLoopProxy,
  config: taurino_core::config::Config,
  ipc_runtime: Arc<IPCRuntime>,
}

impl EngineManager {
  /// Creates and initializes a new shared engine manager.
  ///
  /// The newly created manager is automatically bound to the tray icon and
  /// window managers so that they can access the common engine state.
  pub fn new(
    ipc_runtime: Arc<IPCRuntime>,
    webcontext: WebContextStore,
    resource_table: ArcMut<ResourceTable>,
    menu_manager: ArcMut<MenuManager>,
    proxy: EngineLoopProxy,
    config: taurino_core::config::Config,
    device_registry: ArcMut<DeviceRegistry>,
  ) -> Result<Arc<Self>> {
    let trayicon_manager = TrayIconManager::new()?;
    let window_manager = WindowManager::new()?;

    let manager = Arc::new(Self {
      _device_registry: device_registry.clone(),
      _webcontext: webcontext,
      _resource_table: resource_table,
      _menu_manager: menu_manager,
      _trayicon_manager: trayicon_manager.clone(),
      _window_manager: window_manager.clone(),
      _global_window_event_handler: Mutex::new(None),
      _global_webview_event_handler: Mutex::new(None),
      proxy,
      config,
      ipc_runtime,
    });

    lock_state(&trayicon_manager, "TrayIconManager")?.bind_manager(manager.clone());
    lock_state(&window_manager, "WindowManager")?.bind_manager(manager.clone());

    Ok(manager)
  }

  // =========================================================================
  // Web context
  // =========================================================================

  /// Returns the shared WebView context store.
  pub fn webcontext(self: &Arc<Self>) -> Result<WebContextStore> {
    Ok(self._webcontext.clone())
  }
  pub fn config(self: &Arc<Self>) -> Result<taurino_core::config::Config> {
    Ok(self.config.clone())
  }
  pub fn connection(self: &Arc<Self>) -> Result<Arc<IPCRuntime>> {
    Ok(self.ipc_runtime.clone())
  }
  pub fn proxy_emitter(self: &Arc<Self>, message: EventLoopMessage) -> Result<(), EngineLoopClosed> {
    Ok(self.proxy.send_event(message)?)
  }

  // =========================================================================
  // Global window events
  // =========================================================================

  /// Registers the global window event handler.
  ///
  /// Any previously registered handler is replaced.
  pub fn set_global_window_event_handler<F>(self: &Arc<Self>, handler: F) -> Result<()>
  where
    F: Fn(Arc<EngineManager>, String, WindowEvent) + Send + Sync + 'static,
  {
    let mut global_handler = self
      ._global_window_event_handler
      .lock()
      .map_err(|_| anyhow!("global window event handler mutex is poisoned"))?;

    *global_handler = Some(Box::new(handler));

    Ok(())
  }

  /// Removes the currently registered global window event handler.
  pub fn _clear_global_window_event_handler(self: &Arc<Self>) -> Result<()> {
    let mut global_handler = self
      ._global_window_event_handler
      .lock()
      .map_err(|_| anyhow!("global window event handler mutex is poisoned"))?;

    *global_handler = None;

    Ok(())
  }

  /// Emits an event to the registered global window event handler.
  ///
  /// If no handler is registered, this method performs no action.
  pub fn emit_global_window_event(self: &Arc<Self>, window_label: impl Into<String>, event: WindowEvent) -> Result<()> {
    let handler = self
      ._global_window_event_handler
      .lock()
      .map_err(|_| anyhow!("global window event handler mutex is poisoned"))?;

    if let Some(handler) = handler.as_ref() {
      handler(self.clone(), window_label.into(), event); // <- Callback läuft MIT gehaltenem Lock!
    }

    Ok(())
  }

  // =========================================================================
  // Global WebView events
  // =========================================================================

  /// Registers the global WebView event handler.
  ///
  /// Any previously registered handler is replaced.
  pub fn set_global_webview_event_handler<F>(self: &Arc<Self>, handler: F) -> Result<()>
  where
    F: Fn(Arc<EngineManager>, String, String, WebViewEvent) + Send + Sync + 'static,
  {
    let mut global_handler = self
      ._global_webview_event_handler
      .lock()
      .map_err(|_| anyhow!("global WebView event handler mutex is poisoned"))?;

    *global_handler = Some(Box::new(handler));

    Ok(())
  }

  /// Removes the currently registered global WebView event handler.
  pub fn _clear_global_webview_event_handler(self: &Arc<Self>) -> Result<()> {
    let mut global_handler = self
      ._global_webview_event_handler
      .lock()
      .map_err(|_| anyhow!("global WebView event handler mutex is poisoned"))?;

    *global_handler = None;

    Ok(())
  }

  /// Emits an event to the registered global WebView event handler.
  ///
  /// If no handler is registered, this method performs no action.
  pub fn emit_global_webview_event(
    self: &Arc<Self>,
    window_label: impl Into<String>,
    webview_label: impl Into<String>,
    event: WebViewEvent,
  ) -> Result<()> {
    let handler = self
      ._global_webview_event_handler
      .lock()
      .map_err(|_| anyhow!("global WebView event handler mutex is poisoned"))?;

    if let Some(handler) = handler.as_ref() {
      handler(self.clone(), window_label.into(), webview_label.into(), event);
    }

    Ok(())
  }

  // =========================================================================
  // Shared managers
  // =========================================================================

  /// Locks and returns the shared resource table.
  pub fn _resource_table(self: &Arc<Self>) -> Result<MutexGuard<'_, ResourceTable>> {
    lock!(self._resource_table)
  }

  /// Locks and returns the shared menu manager.
  pub fn menu(self: &Arc<Self>) -> Result<MutexGuard<'_, MenuManager>> {
    lock!(self._menu_manager)
  }

  /// Locks and returns the shared tray icon manager.
  pub fn _trayicon(self: &Arc<Self>) -> Result<MutexGuard<'_, TrayIconManager>> {
    lock!(self._trayicon_manager)
  }

  /// Locks and returns the shared window manager.
  pub fn window(self: &Arc<Self>) -> Result<MutexGuard<'_, WindowManager>> {
    lock!(self._window_manager)
  }

  pub fn device_registry(self: &Arc<Self>) -> Result<MutexGuard<'_, DeviceRegistry>> {
    lock!(self._device_registry)
  }
}
