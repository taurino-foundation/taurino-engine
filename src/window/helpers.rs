use crate::{manager::EngineManager, window::factory::create_webview};
use anyhow::{Result, anyhow};
use std::{
  collections::{
    HashMap, HashSet,
    hash_map::Entry::{Occupied, Vacant},
  },
  path::PathBuf,
  sync::{Arc, Mutex, MutexGuard},
};
#[cfg(windows)]
use taurino_core::native::tao::platform::windows::WindowExtWindows;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use taurino_core::schema::webview::WebContentProcessTerminatePolicy;
#[cfg(any(
  windows,
  target_os = "linux",
  target_os = "dragonfly",
  target_os = "freebsd",
  target_os = "netbsd",
  target_os = "openbsd",
))]
use taurino_core::window::undecorated_resizing;
use taurino_core::{
  EventLoopMessage,
  native::wry::{DragDropEvent as WryDragDropEvent, WebContext as WryContext},
  schema::{
    PhysicalPosition, Rect,
    event::{DragDropEvent, SynthesizedWindowEvent, WebViewEvent},
    webview::{InitializationScript, WebviewBounds},
  },
  tools::wrappers::RectWrapper,
};
use taurino_core::{
  NewWindowFeatures, NewWindowResponse, PermissionKind, PermissionResponse, WebViewManager,
  native::wry::WebViewBuilder,
  schema::{
    webview::{NewWindowAction, NewWindowPolicy, WebViewConfig, WebViewId},
    window::{WindowConfig, WindowId},
  },
  tools::{
    lock_state,
    stores::{WebContext, WebContextStore},
  },
};
#[cfg(windows)]
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
type OnWebContentProcessTerminateHandler = dyn Fn();

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

pub(crate) fn all_initialization_scripts(
  window_label: &str,
  webview_label: &str,
) -> Result<Vec<InitializationScript>> {
  let mut all_initialization_scripts: Vec<InitializationScript> = vec![];

  fn main_frame_script(script: String) -> InitializationScript {
    InitializationScript {
      script,
      for_main_frame_only: true,
    }
  }
  all_initialization_scripts.push(main_frame_script(
    r"
        Object.defineProperty(window, 'isTaurino', {
          value: true,
        });

        if (!window.__TAURINO_INTERNALS__) {
          Object.defineProperty(window, '__TAURINO_INTERNALS__', {
            value: {
              plugins: {}
            }
          })
        }
      "
    .to_owned(),
  ));

  // Aktuelle Prozess- und Thread-Informationen
  let pid = std::process::id();

  // Compile-Time-Informationen
  let os = std::env::consts::OS;
  let platform = std::env::consts::FAMILY;
  let arch = std::env::consts::ARCH;
  let target = format!("{arch}-{os}");

  all_initialization_scripts.push(main_frame_script(format!(
    r#"
        Object.defineProperty(window.__TAURINO_INTERNALS__, 'metadata', {{
            value: Object.freeze({{
                currentWindow: Object.freeze({{
                    label: {current_window_label}
                }}),

                currentWebview: Object.freeze({{
                    label: {current_webview_label}
                }}),

                process: Object.freeze({{
                    pid: {pid},
                }}),

                platform: Object.freeze({{
                    os: {os},
                    family: {family},
                    arch: {arch},
                    target: {target}
                }})
            }}),
            writable: false,
            configurable: false,
            enumerable: true
        }});
    "#,
    current_window_label = serde_json::to_string(window_label)?,
    current_webview_label = serde_json::to_string(webview_label)?,
    pid = pid,
    os = serde_json::to_string(os)?,
    family = serde_json::to_string(platform)?,
    arch = serde_json::to_string(arch)?,
    target = serde_json::to_string(&target)?,
  )));

  Ok(all_initialization_scripts)
}

pub(crate) fn apply_webview_bounds<'a>(
  window: &'a taurino_core::native::tao::window::Window,
  mut webview_builder: WebViewBuilder<'a>,
  bounds: Option<Rect>,
  auto_resize: bool,
  child: bool,
) -> (Option<WebviewBounds>, WebViewBuilder<'a>) {
  let webview_bounds = if let Some(bounds) = bounds {
    let bounds: RectWrapper = bounds.into();
    let bounds = bounds.0;
    let scale_factor = window.scale_factor();
    let position = bounds.position.to_logical::<f32>(scale_factor);
    let size = bounds.size.to_logical::<f32>(scale_factor);
    webview_builder = webview_builder.with_bounds(bounds);
    let window_size = window.inner_size().to_logical::<f32>(scale_factor);
    if auto_resize {
      Some(WebviewBounds {
        x_rate: position.x / window_size.width,
        y_rate: position.y / window_size.height,
        width_rate: size.width / window_size.width,
        height_rate: size.height / window_size.height,
      })
    } else {
      None
    }
  } else {
    if child {
      webview_builder = webview_builder.with_bounds(taurino_core::native::wry::Rect {
        position: taurino_core::schema::LogicalPosition::new(0, 0).into(),
        size: window.inner_size().into(),
      });
      Some(WebviewBounds {
        x_rate: 0.,
        y_rate: 0.,
        width_rate: 1.,
        height_rate: 1.,
      })
    } else {
      None
    }
  };
  (webview_bounds, webview_builder)
}

pub(crate) fn apply_webview_context<'a>(
  webview_label: String,
  browser_context: &'a WebContextStore,
  data_directory: Option<PathBuf>,
) -> Result<(
  MutexGuard<'a, HashMap<Option<PathBuf>, WebContext>>,
  Option<PathBuf>,
  Option<PathBuf>,
)> {
  let mut contexts = lock_state(browser_context, "browser_context")?;

  let is_first_context = contexts.is_empty();

  // Identisch zum Original
  let automation_enabled = std::env::var("TAURINO_WEBVIEW_AUTOMATION").as_deref() == Ok("true");

  let web_context_key = data_directory;

  match contexts.entry(web_context_key.clone()) {
    Occupied(occupied) => {
      let occupied = occupied.into_mut();

      occupied.referenced_by_webviews.insert(webview_label);
    }

    Vacant(vacant) => {
      let mut web_context = WryContext::new(web_context_key.clone());

      web_context.set_allows_automation(if automation_enabled {
        is_first_context
      } else {
        false
      });

      vacant.insert(WebContext {
        inner: web_context,
        referenced_by_webviews: [webview_label].into(),
        registered_custom_protocols: HashSet::new(),
      });
    }
  }

  // Exakt dieselbe Logik wie im Original
  let context_key = if automation_enabled {
    None
  } else {
    web_context_key.clone()
  };

  Ok((contexts, web_context_key, context_key))
}

pub fn apply_build_webview<'a>(
  window: &taurino_core::native::tao::window::Window,
  webview_builder: WebViewBuilder<'a>,
  webview_label: &str,
  child: bool,
) -> Result<taurino_core::native::wry::WebView> {
  let webview = match child {
    #[cfg(not(any(
      target_os = "windows",
      target_os = "macos",
      target_os = "ios",
      target_os = "android"
    )))]
    true => {
      let vbox = window.default_vbox().ok_or_else(|| {
        anyhow::anyhow!(
          "failed to create child WebView `{}`: \
                     window does not provide a GTK default vbox",
          options.label
        )
      })?;
      webview_builder.build_gtk(vbox)
    }
    #[cfg(any(
      target_os = "windows",
      target_os = "macos",
      target_os = "ios",
      target_os = "android"
    ))]
    true => webview_builder.build_as_child(window),
    false => {
      #[cfg(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "ios",
        target_os = "android"
      ))]
      let builder = webview_builder.build(window);
      #[cfg(not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "ios",
        target_os = "android"
      )))]
      let builder = {
        let vbox = window.default_vbox().ok_or_else(|| {
          anyhow::anyhow!(
            "failed to create WebView `{}`: \
                         window does not provide a GTK default vbox",
            webview_label
          )
        })?;
        webview_builder.build_gtk(vbox)
      };
      builder
    }
  }
  .map_err(|error| anyhow::anyhow!("failed to build WebView `{}`: {error}", webview_label))?;
  if child == false {
    #[cfg(any(
      target_os = "linux",
      target_os = "dragonfly",
      target_os = "freebsd",
      target_os = "netbsd",
      target_os = "openbsd"
    ))]
    undecorated_resizing::attach_resize_handler(&webview);

    #[cfg(windows)]
    if window.is_resizable() && !window.is_decorated() {
      undecorated_resizing::attach_resize_handler(window.hwnd(), window.has_undecorated_shadow());
    }
  }

  Ok(webview)
}

/*
 */

/*
fn new_window_handler(
    policy: &NewWindowPolicy,
    url: Url,
    features: NewWindowFeatures,
    engine_manager: Arc<EngineManager>,
) -> Result<NewWindowResponse> {
    match policy.evaluate(&url) {
        NewWindowAction::Allow => Ok(NewWindowResponse::Allow),

        NewWindowAction::Deny => Ok(NewWindowResponse::Deny),

        NewWindowAction::Create { window } => {
            let mut window_options = *window;

            // requested URL in die Konfiguration des neuen WebViews übernehmen
            //
            // z. B.:
            // window_options.webview.url = WebviewUrl::External(url);

            // window.open()-Features ggf. übernehmen
            //
            // if let Some(size) = features.size() {
            //     window_options.width = Some(size.width);
            //     window_options.height = Some(size.height);
            // }
            //
            // if let Some(position) = features.position() {
            //     window_options.x = Some(position.x);
            //     window_options.y = Some(position.y);
            // }

            let window_id = engine_manager.create_window_from_new_window_request(
                window_options,
                features,
            )?;

            Ok(NewWindowResponse::Create { window_id })
        }
    }
}





*/

/*

new_window_handler




NewWindowAction::Create { window } => {
    let mut window_options = *window;

    // requested URL setzen
    // window_options.webview.url = WebviewUrl::External(url);

    let (tx, rx) = std::sync::mpsc::channel();

    engine_manager
        .proxy()?
        .send_event(Message::CreateWindow(CreateWindowRequest {
            options: window_options,
            response: tx,
        }))
        .map_err(|_| anyhow!("failed to send CreateWindow request"))?;

    let window_id = rx
        .recv()
        .map_err(|_| anyhow!("CreateWindow response channel closed"))??;

    Ok(NewWindowResponse::Create { window_id })
}


eventloop



match event {
    Event::UserEvent(Message::CreateWindow(request)) => {
        let result = {
            let mut window_manager = engine_manager.window_mut()?;

            window_manager.open_window(
                &request.options,
                event_loop_target,
            )
        };

        let _ = request.response.send(result);
    }

    // ...
}


*/

/*

let browser_context = manager.webcontext()?;

let (
    mut contexts,
    web_context_key,
    context_key,
) = apply_webview_context(
    options.label.clone(),
    &browser_context,
    options.data_directory.clone(),
)?;

let web_context = contexts
    .get_mut(&web_context_key)
    .expect("WebContext must exist");

// Identisch zum Original
let webview_builder =
    WebViewBuilder::new_with_web_context(
        &mut web_context.inner,
    )
    .with_devtools(true)
    .with_id(&options.label)
    .with_focused(window_options.focus)
    .with_transparent(window_options.transparent)
    .with_accept_first_mouse(window_options.accept_first_mouse)
    .with_incognito(options.incognito)
    .with_clipboard(options.enable_clipboard_access)
    .with_hotkeys_zoom(options.zoom_hotkeys_enabled)
    .with_general_autofill_enabled(
        options.general_autofill_enabled,
    );

*/
