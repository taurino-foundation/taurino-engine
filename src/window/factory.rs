use std::{
  collections::{
    HashSet,
    hash_map::Entry::{Occupied, Vacant},
  },
  sync::{Arc, Mutex},
};

use anyhow::Result;

use taurino_core::{
  EngineWindowTarget, EventLoopMessage, MonitorExt,
  core::{
    stores::WebContext,
    window::{Window, WindowBuilder},
  },
  menu::RawWindow,
  native::wry::{DragDropEvent as WryDragDropEvent, WebContext as WryContext, WebViewBuilder},
  schema::{
    PhysicalPosition, PhysicalSize,
    event::{DragDropEvent, SynthesizedWindowEvent, WebViewEvent},
    webview::{BackgroundThrottlingPolicy, WebViewConfig, WebviewBounds, WebviewUrl},
    window::{WindowConfig, WindowId, WindowId as CoreWindowId},
  },
  utils::{
    NewWindowFeatures, NewWindowOpener, NewWindowResponse, arc_mut, calculate_window_center_position,
    find_monitor_for_position, from_wry_permission_kind, lock_state, to_wry_permission_response,
  },
  webview::{WebView, WebViewManager},
  wrappers::RectWrapper,
};
#[cfg(windows)]
use taurino_core::{schema::FocusState, utils::ArcMut, windows::utils::apply_shadow_correction};
use url::Url;

use crate::{
  manager::EngineManager,
  window::helpers::{attach_webview, new_window_handler, permission_request_handler},
};
// ============================================================================
// iOS
// ============================================================================
#[cfg(any(target_os = "macos", target_os = "ios"))]
use crate::window::helpers::on_web_content_process_terminate_handler;
#[cfg(target_os = "ios")]
use taurino_core::native::wry::{WebViewBuilderExtDarwin, WebViewBuilderExtIos, WebViewExtDarwin};
// ============================================================================
// macOS
// ============================================================================
#[cfg(target_os = "macos")]
use taurino_core::native::tao::platform::macos::WindowBuilderExtMacOS;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use taurino_core::native::tao::window::Fullscreen;
#[cfg(target_os = "macos")]
use taurino_core::native::wry::{WebViewBuilderExtDarwin, WebViewExtMacOS};
#[cfg(any(
  target_os = "linux",
  target_os = "dragonfly",
  target_os = "freebsd",
  target_os = "netbsd",
  target_os = "openbsd"
))]
use taurino_core::{
  native::{
    tao::platform::unix::WindowExtUnix,
    wry::{WebViewBuilderExtUnix, WebViewExtUnix},
  },
  undecorated_resizing,
};
// ============================================================================
// Windows
// ============================================================================
#[cfg(target_os = "windows")]
use taurino_core::{
  native::{tao::platform::windows::WindowExtWindows, wry::WebViewExtWindows},
  undecorated_resizing,
};

pub(crate) fn create_webview(
  engine_manager: Arc<EngineManager>,
  window_id: Arc<Mutex<WindowId>>,
  id: taurino_core::schema::webview::WebViewId,
  options: &WebViewConfig,
  window_options: &WindowConfig,
  window: &taurino_core::native::tao::window::Window,
  #[cfg(windows)] focused_webview: ArcMut<FocusState>,
) -> Result<WebView> {
  let manager = engine_manager.clone();
  let proxy = manager.proxy.clone();

  let browser_context = manager.webcontext()?;
  let mut web_context = lock_state(&browser_context, "browser_context")?;
  let is_first_context = web_context.is_empty();
  // the context must be stored on the HashMap because it must outlive the WebView on macOS
  let automation_enabled = std::env::var("TAURI_WEBVIEW_AUTOMATION").as_deref() == Ok("true");
  let web_context_key = &options.data_directory;
  let entry = web_context.entry(web_context_key.clone());
  let web_context = match entry {
    Occupied(occupied) => {
      let occupied = occupied.into_mut();
      occupied.referenced_by_webviews.insert(options.label.clone());
      occupied
    }
    Vacant(vacant) => {
      let mut web_context = WryContext::new(web_context_key.clone());
      web_context.set_allows_automation(if automation_enabled { is_first_context } else { false });
      vacant.insert(WebContext {
        inner: web_context,
        referenced_by_webviews: [options.label.clone()].into(),
        registered_custom_protocols: HashSet::new(),
      })
    }
  };

  let mut webview_builder = WebViewBuilder::new_with_web_context(&mut web_context.inner)
    .with_id(&options.label)
    .with_focused(window_options.focus)
    .with_transparent(window_options.transparent)
    .with_accept_first_mouse(window_options.accept_first_mouse)
    .with_incognito(options.incognito)
    .with_clipboard(options.enable_clipboard_access)
    .with_hotkeys_zoom(options.zoom_hotkeys_enabled)
    .with_general_autofill_enabled(options.general_autofill_enabled);

  if let Some(background_throttling) = &options.background_throttling {
    webview_builder = webview_builder.with_background_throttling(match background_throttling {
      BackgroundThrottlingPolicy::Disabled => taurino_core::native::wry::BackgroundThrottlingPolicy::Disabled,
      BackgroundThrottlingPolicy::Suspend => taurino_core::native::wry::BackgroundThrottlingPolicy::Suspend,
      BackgroundThrottlingPolicy::Throttle => taurino_core::native::wry::BackgroundThrottlingPolicy::Throttle,
    });
  }
  if options.javascript_disabled {
    webview_builder = webview_builder.with_javascript_disabled();
  }
  if let Some(color) = window_options.background_color {
    webview_builder = webview_builder.with_background_color(color.into());
  }
  // Before building the drag-drop handler, extract owned copies of what it needs:
  let child = options.child;
  if options.drag_drop_enabled {
    let _window_id_ = window_id.clone();
    let engine_manager = manager.clone();
    // Wert herauskopieren, damit der Closure 'static ist
    let window_enabled_drag_drop = window_options.enable_drag_drop;
    webview_builder = webview_builder.with_drag_drop_handler(move |event| {
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
        _ => unimplemented!(),
      };
      let message = if child == false || window_enabled_drag_drop {
        EventLoopMessage::SynthesizedWindowEvent(
          *_window_id_.lock().unwrap(),
          id,
          SynthesizedWindowEvent::DragDrop(event),
        )
      } else {
        EventLoopMessage::WebviewEvent(*_window_id_.lock().unwrap(), id, WebViewEvent::DragDrop(event))
      };
      let _ = engine_manager.proxy_emitter(message);
      true
    });
  }
  if let Some(policy) = options.navigation_policy.clone() {
    let initial_url = match &options.url {
      WebviewUrl::External(url) | WebviewUrl::CustomProtocol(url) => Some(url.clone()),
      WebviewUrl::App(_) => None,
      #[allow(unreachable_patterns)]
      _ => None,
    };
    webview_builder = webview_builder.with_navigation_handler(move |raw_url| {
      let Ok(target_url) = raw_url.parse::<Url>() else {
        return false;
      };
      policy.allows(initial_url.as_ref(), &target_url)
    });
  }
  if let Some(policy) = options.new_window_policy.clone() {
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
          let Some(webview) = window.webview(id.clone()) else {
            eprintln!("webview {} not found in window {:?}", id.get(), window_id);
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
  #[cfg(any(target_os = "macos", target_os = "ios"))]
  {
    if let Some(policy) = options.web_content_process_terminate_policy.clone() {
      webview_builder = webview_builder.with_on_web_content_process_terminate_handler(
        on_web_content_process_terminate_handler(manager.clone(), window_id.clone(), id, policy),
      );
    }
  }
  if let Some(_policy) = options.permission_request_policy.clone() {
    webview_builder = webview_builder.with_permission_handler(move |kind| {
      let kind = from_wry_permission_kind(kind);
      let response = permission_request_handler(manager.clone()).unwrap();
      to_wry_permission_response(response(kind))
    });
  }
  let webview_bounds = if let Some(bounds) = options.bounds {
    let bounds: RectWrapper = bounds.into();
    let bounds = bounds.0;
    let scale_factor = window.scale_factor();
    let position = bounds.position.to_logical::<f32>(scale_factor);
    let size = bounds.size.to_logical::<f32>(scale_factor);
    webview_builder = webview_builder.with_bounds(bounds);
    let window_size = window.inner_size().to_logical::<f32>(scale_factor);
    if options.auto_resize {
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
  let webview_builder =
    engine_manager
      .clone()
      .connection()?
      .apply(webview_builder, &options.url, options.use_https_scheme)?;
  let webview = match child {
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "ios", target_os = "android")))]
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
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "ios", target_os = "android"))]
    true => webview_builder.build_as_child(window),
    false => {
      #[cfg(any(target_os = "windows", target_os = "macos", target_os = "ios", target_os = "android"))]
      let builder = webview_builder.build(window);
      #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "ios", target_os = "android")))]
      let builder = {
        let vbox = window.default_vbox().ok_or_else(|| {
          anyhow::anyhow!(
            "failed to create WebView `{}`: \
                         window does not provide a GTK default vbox",
            options.label
          )
        })?;
        webview_builder.build_gtk(vbox)
      };
      builder
    }
  }
  .map_err(|error| anyhow::anyhow!("failed to build WebView `{}`: {error}", options.label))?;
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
  let context_key = if automation_enabled {
    None
  } else {
    web_context_key.clone()
  };
  let webview = WebView::new(
    id,
    options.label.clone(),
    window_id,
    #[cfg(windows)]
    focused_webview,
    webview,
    context_key,
    browser_context.clone(),
    arc_mut(webview_bounds),
    proxy,
  );
  Ok(webview)
}

pub fn create_window(
  engine_manager: Arc<EngineManager>,
  id: CoreWindowId,
  window_target: &EngineWindowTarget,
  window_options: &WindowConfig,
) -> Result<Arc<Window>> {
  let mut window_builder = WindowBuilder::with_config(window_options);
  if window_builder.prevent_overflow.is_some() || window_builder.center {
    let monitor = if let Some(window_position) = &window_builder.inner.window.position {
      find_monitor_for_position(window_target.available_monitors(), *window_position)
    } else {
      window_target.primary_monitor()
    };
    if let Some(monitor) = monitor {
      let scale_factor = monitor.scale_factor();
      let desired_size = window_builder
        .inner
        .window
        .inner_size
        .unwrap_or_else(|| PhysicalSize::new(800, 600).into());
      let mut inner_size = window_builder
        .inner
        .window
        .inner_size_constraints
        .clamp(desired_size, scale_factor)
        .to_physical::<u32>(scale_factor);
      let mut window_size = inner_size;
      #[cfg(windows)]
      let decorations = window_builder.inner.window.decorations;
      // Left and right window shadow counts as part of the window on Windows
      // We need to include it when calculating positions, but not size
      #[cfg(windows)]
      let shadow_width = apply_shadow_correction(decorations, &mut window_size)?;
      #[cfg(not(windows))]
      let shadow_width = 0;
      if let Some(margin) = window_builder.prevent_overflow {
        let work_area = monitor.work_area();
        let margin = margin.to_physical::<u32>(scale_factor);
        let constraint = PhysicalSize::new(
          work_area.size.width - margin.width,
          work_area.size.height - margin.height,
        );
        if window_size.width > constraint.width || window_size.height > constraint.height {
          if window_size.width > constraint.width {
            inner_size.width = inner_size.width.saturating_sub(window_size.width - constraint.width);
            window_size.width = constraint.width;
          }
          if window_size.height > constraint.height {
            inner_size.height = inner_size.height.saturating_sub(window_size.height - constraint.height);
            window_size.height = constraint.height;
          }
          window_builder.inner.window.inner_size = Some(inner_size.into());
        }
      }
      if window_builder.center {
        window_size.width += shadow_width;
        let position = calculate_window_center_position(window_size, monitor);
        let logical_position = position.to_logical::<f64>(scale_factor);
        window_builder = window_builder.position(logical_position.x, logical_position.y);
      }
    }
  };
  #[cfg(any(target_os = "macos", target_os = "linux"))]
  let (initial_position, is_fullscreen) = (
    window_builder.inner.window.position,
    window_builder.inner.window.fullscreen.is_some(),
  );
  // If fullscreen is requested with an explicit position, resolve the target
  // monitor up front so the window is created fullscreen on that display.
  #[cfg(any(target_os = "macos", target_os = "linux"))]
  if let (true, Some(position)) = (is_fullscreen, initial_position) {
    if let Some(target_monitor) = find_monitor_for_position(window_target.available_monitors(), position) {
      window_builder.inner.window.fullscreen = Some(Fullscreen::Borderless(Some(target_monitor)));
    }
  }
  #[cfg(windows)]
  let background_color = window_builder.inner.window.background_color;
  #[cfg(windows)]
  let is_window_transparent = window_builder.inner.window.transparent;
  #[cfg(target_os = "macos")]
  {
    if window_builder.tabbing_identifier.is_none()
      || window_builder.inner.window.transparent
      || !window_builder.inner.window.decorations
    {
      window_builder.inner = window_builder.inner.with_automatic_window_tabbing(false);
    }
  }
  let theme = window_builder.get_theme().unwrap_or(taurino_core::schema::Theme::Light);
  let window = window_builder.inner.build(window_target)?;
  let menu = {
    let raw = RawWindow {
      #[cfg(windows)]
      hwnd: window.hwnd(),
      #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
      ))]
      gtk_window: window.gtk_window(),
      #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
      ))]
      default_vbox: window.default_vbox(),
      _marker: &std::marker::PhantomData,
    };
    Some(engine_manager.menu()?.create_window_menu(raw, theme, None)?)
  };
  // On macOS, `with_position` uses the content origin; the title bar is added
  // above it. `set_outer_position` is needed for precise window placement.
  #[cfg(target_os = "macos")]
  if !is_fullscreen {
    if let Some(position) = initial_position {
      window.set_outer_position(position);
    }
  }
  #[cfg(windows)]
  let focused_webview = Arc::new(Mutex::new(FocusState::default()));
  let mut webviews_manager = WebViewManager::new()?;
  let window_id = Arc::new(Mutex::new(id));
  let has_children = window_options.webviews.len() > 1;
  for view_options in &window_options.webviews {
    let mut view_options = view_options.clone();
    // If only a single WebView exists in the list and the user has still set
    // `child` to `true`, we must ensure that it is reset to `false`, because
    // a single WebView in the list cannot be a child WebView of a window.
    if !has_children {
      view_options.child = false;
    }
    attach_webview(
      &mut webviews_manager,
      &window,
      &view_options,
      window_options,
      engine_manager.clone(),
      window_id.clone(),
      #[cfg(windows)]
      focused_webview.clone(),
    )?;
  }
  Ok(Window::new(
    window,
    id,
    window_options.label.clone(),
    menu,
    #[cfg(windows)]
    background_color,
    #[cfg(windows)]
    is_window_transparent,
    #[cfg(windows)]
    focused_webview,
    has_children,
    webviews_manager,
  ))
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
