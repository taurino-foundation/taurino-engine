use crate::{manager::EngineManager, window::factory::create_webview};

use anyhow::{Result, anyhow};
/* use serialize_to_javascript::{Template, default_template}; */

use std::sync::{Arc, Mutex};
#[cfg(any(target_os = "macos"))]
pub  use taurino_core::native::wry::WebViewExtMacOS;
#[cfg(not(any(
  target_os = "windows",
  target_os = "macos",
  target_os = "ios",
  target_os = "android"
)))]
use taurino_core::native::wry::WebViewExtUnix;



use taurino_core::{
  EventLoopMessage, NewWindowFeatures, NewWindowOpener, NewWindowResponse, PermissionKind,
  PermissionResponse, WebViewManager,
  native::wry::{DragDropEvent as WryDragDropEvent, WebViewBuilder},
  schema::{
    PhysicalPosition,
    event::{DragDropEvent, SynthesizedWindowEvent, WebViewEvent},
    webview::{NewWindowAction, NewWindowPolicy, WebViewConfig, WebViewId},
    window::{WindowConfig, WindowId},
  },
};

#[cfg(any(target_os = "ios", target_os = "macos"))]
use taurino_core::schema::webview::WebContentProcessTerminatePolicy;

#[cfg(windows)]
use taurino_core::{native::wry::WebViewExtWindows};


use taurino_core::{schema::FocusState, tools::ArcMut};

use url::Url;

pub type PermissionRequestHandler = dyn Fn(PermissionKind) -> PermissionResponse + Send + Sync;

pub fn permission_request_handler(
  _eng: Arc<EngineManager>,
) -> Result<Box<PermissionRequestHandler>> {
  // This fixed permission policy does not require access to the engine manager.
  Ok(Box::new(|kind| match kind {
    PermissionKind::Microphone => PermissionResponse::Allow,
    PermissionKind::Camera => PermissionResponse::Allow,
    PermissionKind::Geolocation => PermissionResponse::Allow,
    _ => PermissionResponse::Deny,
  }))
}

pub fn new_window_handler(
  policy: &NewWindowPolicy,
  url: Url,
  _features: NewWindowFeatures,
  _engine_manager: Arc<EngineManager>,
) -> Result<NewWindowResponse> {
  match policy.evaluate(&url) {
    NewWindowAction::Allow => Ok(NewWindowResponse::Allow),
    NewWindowAction::Deny => Ok(NewWindowResponse::Deny),
    NewWindowAction::Create { window: _ } => {
      // Window creation is not implemented yet.
      Ok(NewWindowResponse::Deny)
    }
  }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub type OnWebContentProcessTerminateHandler = dyn Fn();

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub fn on_web_content_process_terminate_handler(
  engine_manager: Arc<EngineManager>,
  window_id: Arc<Mutex<WindowId>>,
  webview_id: WebViewId,
  policy: WebContentProcessTerminatePolicy,
) -> Box<OnWebContentProcessTerminateHandler> {
  // Wry runs this callback on the native WebView thread and does not require Send.
  Box::new(move || {
    if matches!(policy, WebContentProcessTerminatePolicy::Ignore) {
      return;
    }

    // Release the ID lock before accessing the window manager.
    let window_id = match window_id.lock() {
      Ok(id) => *id,
      Err(error) => {
        eprintln!("failed to lock window ID after web content process termination: {error}");
        return;
      }
    };

    let window_manager = match engine_manager.window() {
      Ok(manager) => manager,
      Err(error) => {
        eprintln!("failed to lock WindowManager: {error}");
        return;
      }
    };

    let Some(window) = window_manager.get_by_id(window_id) else {
      eprintln!(
        "window {:?} not found after web content process termination",
        window_id
      );
      return;
    };

    let Some(webview) = window.webview(webview_id) else {
      eprintln!("failed to find webview {}", webview_id.get());
      return;
    };

    if let Err(error) = webview.reload() {
      eprintln!("failed to reload webview {}: {error}", webview_id.get());
    }
  })
}

// =========================================================================
// Creation
// =========================================================================

/// Creates and registers a new WebView.
///
/// The WebView is inserted only after its native creation succeeds.
///
/// # Errors
///
/// Returns an error if:
///
/// - another WebView already uses the requested label,
/// - the generated WebView ID is already registered,
/// - native WebView creation fails.
pub fn attach_webview(
  webview_manager: &mut WebViewManager,
  window: &taurino_core::native::tao::window::Window,
  options: &WebViewConfig,
  window_options: &WindowConfig,
  engine_manager: Arc<EngineManager>,
  window_id: Arc<Mutex<WindowId>>,
  #[cfg(windows)] focused_webview: ArcMut<FocusState>,
) -> Result<WebViewId> {
  if webview_manager.contains_label(&options.label) {
    return Err(anyhow!(
      "WebView with label {:?} is already registered",
      options.label
    ));
  }

  let id = webview_manager.next_webview_id();

  if webview_manager.contains(id) {
    return Err(anyhow!("WebView with id {:?} is already registered", id));
  }

  let webview = create_webview(
    engine_manager,
    window_id,
    id,
    options,
    window_options,
    window,
    #[cfg(windows)]
    focused_webview,
  )
  .map_err(|error| anyhow!("failed to create WebView {:?}: {error}", options.label))?;

  webview_manager.insert(webview)?;

  Ok(id)
}

pub(crate) fn apply_drag_drop_handlers<'a>(
  manager: Arc<EngineManager>,
  webview_builder: WebViewBuilder<'a>,
  window_id: ArcMut<WindowId>,
  webview_id: WebViewId,
  window_enabled_drag_drop: bool,
  child: bool,
) -> WebViewBuilder<'a> {
  webview_builder.with_drag_drop_handler(move |event| {
    let event = match event {
      WryDragDropEvent::Enter {
        paths,
        position: (x, y),
      } => DragDropEvent::Enter {
        paths,
        position: PhysicalPosition::new(x as _, y as _),
      },

      WryDragDropEvent::Over { position: (x, y) } => DragDropEvent::Over {
        position: PhysicalPosition::new(x as _, y as _),
      },

      WryDragDropEvent::Drop {
        paths,
        position: (x, y),
      } => DragDropEvent::Drop {
        paths,
        position: PhysicalPosition::new(x as _, y as _),
      },

      WryDragDropEvent::Leave => DragDropEvent::Leave,

      _ => return false,
    };

    let window_id = *window_id.lock().unwrap();

    let message = if !child || window_enabled_drag_drop {
      EventLoopMessage::SynthesizedWindowEvent(
        window_id,
        webview_id,
        SynthesizedWindowEvent::DragDrop(event),
      )
    } else {
      EventLoopMessage::WebviewEvent(window_id, webview_id, WebViewEvent::DragDrop(event))
    };

    let _ = manager.proxy_emitter(message);

    true
  })
}

pub(crate) fn apply_new_window_requested<'a>(
  manager: Arc<EngineManager>,
  mut webview_builder: WebViewBuilder<'a>,
  webview_id: WebViewId,
  new_window_policy: Option<NewWindowPolicy>,
) -> Result<WebViewBuilder<'a>> {
  if let Some(policy) = new_window_policy {
    let engine_manager = manager.clone();

    webview_builder = webview_builder.with_new_window_req_handler(move |raw_url, features| {
      let Ok(url) = raw_url.parse::<Url>() else {
        return taurino_core::native::wry::NewWindowResponse::Deny;
      };

      let response = new_window_handler(
        &policy,
        url,
        NewWindowFeatures::new(
          features.size,
          features.position,
          NewWindowOpener {
            webview: features.opener.webview,

            #[cfg(windows)]
            environment: features.opener.environment,

            #[cfg(target_os = "macos")]
            target_configuration: features.opener.target_configuration,
          },
        ),
        engine_manager.clone(),
      );

      match response {
        Ok(NewWindowResponse::Allow) => taurino_core::native::wry::NewWindowResponse::Allow,

        Ok(NewWindowResponse::Create { window_id }) => {
          let manager = engine_manager.clone();

          let window_manager = match manager.window() {
            Ok(manager) => manager,
            Err(error) => {
              eprintln!("failed to lock WindowManager for new-window request: {error}");

              return taurino_core::native::wry::NewWindowResponse::Deny;
            }
          };

          let Some(window) = window_manager.get_by_id(window_id) else {
            eprintln!("window {:?} not found for new-window request", window_id);

            return taurino_core::native::wry::NewWindowResponse::Deny;
          };

          let Some(webview) = window.webview(webview_id.clone()) else {
            eprintln!(
              "webview {} not found in window {:?}",
              webview_id.get(),
              window_id
            );

            return taurino_core::native::wry::NewWindowResponse::Deny;
          };

          taurino_core::native::wry::NewWindowResponse::Create {
            #[cfg(target_os = "macos")]
            webview: webview.as_wry().webview().into_super(),

            #[cfg(any(
              target_os = "linux",
              target_os = "dragonfly",
              target_os = "freebsd",
              target_os = "netbsd",
              target_os = "openbsd",
            ))]
            webview: webview.as_wry().webview(),

            #[cfg(windows)]
            webview: webview.as_wry().webview(),
          }
        }

        Ok(NewWindowResponse::Deny) => taurino_core::native::wry::NewWindowResponse::Deny,

        Err(error) => {
          eprintln!("new-window handler failed: {error}");

          taurino_core::native::wry::NewWindowResponse::Deny
        }
      }
    });
  }

  Ok(webview_builder)
}
