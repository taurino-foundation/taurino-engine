use std::sync::{Arc, Mutex};

use anyhow::Result;

use taurino_core::{
  EngineWindowTarget, WebView, WebViewManager, from_wry_permission_kind,
  menu::RawWindow,
  native::wry::WebViewBuilder,
  platforms::{MonitorExt, calculate_window_center_position},
  schema::{
    PhysicalSize,
    webview::{BackgroundThrottlingPolicy, WebViewConfig, WebviewUrl},
    window::{WindowConfig, WindowId, WindowId as CoreWindowId},
  },
  to_wry_permission_response,
  tools::{arc_mut, find_monitor_for_position},
  window::{
    Window, WindowBuilder,
    util::{
      all_initialization_scripts, apply_build_webview, apply_webview_bounds, apply_webview_context,
    },
  },
};
#[cfg(windows)]
use taurino_core::{
  platforms::windows::utils::apply_shadow_correction, schema::FocusState, tools::ArcMut,
};
use url::Url;

use crate::{
  manager::EngineManager,
  window::helpers::{
    apply_drag_drop_handlers, apply_new_window_requested, attach_webview,
    permission_request_handler,
  },
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
use taurino_core::native::wry::{WebViewBuilderExtDarwin, /* WebViewExtMacOS */};
#[cfg(any(
  target_os = "linux",
  target_os = "dragonfly",
  target_os = "freebsd",
  target_os = "netbsd",
  target_os = "openbsd"
))]
use taurino_core::native::{
  tao::platform::unix::WindowExtUnix,
  /* wry::{WebViewBuilderExtUnix, WebViewExtUnix}, */
};

// ============================================================================
// Windows
// ============================================================================
#[cfg(windows)]
use taurino_core::native::tao::platform::windows::WindowExtWindows;

pub(crate) fn create_webview(
  engine_manager: Arc<EngineManager>,
  window_id: Arc<Mutex<WindowId>>,
  id: taurino_core::schema::webview::WebViewId,
  options: &WebViewConfig,
  window_options: &WindowConfig,
  window: &taurino_core::native::tao::window::Window,
  #[cfg(windows)] focused_webview: ArcMut<FocusState>,
) -> Result<WebView> {
  let child = options.child;
  let all_initialization_scripts = all_initialization_scripts(
    &window_options.label,
    &options.label,
    options.use_https_scheme,
    None,
  )?;

  let manager = engine_manager.clone();
  let proxy = manager.proxy.clone();

  let browser_context = manager.webcontext()?;

  let (mut contexts, web_context_key, context_key) = apply_webview_context(
    "TAURINO_WEBVIEW_AUTOMATION",
    options.label.clone(),
    &browser_context,
    options.data_directory.clone(),
  )?;

  let web_context = contexts
    .get_mut(&web_context_key)
    .expect("WebContext must exist");

  let webview_builder = WebViewBuilder::new_with_web_context(&mut web_context.inner)
    .with_devtools(true)
    .with_id(&options.label)
    .with_focused(window_options.focus)
    .with_transparent(window_options.transparent)
    .with_accept_first_mouse(window_options.accept_first_mouse)
    .with_incognito(options.incognito)
    .with_clipboard(options.enable_clipboard_access)
    .with_hotkeys_zoom(options.zoom_hotkeys_enabled)
    .with_general_autofill_enabled(options.general_autofill_enabled)
    .with_ipc_handler(move |req| println!("Request:{:?}", req));

  let (webview_bounds, mut webview_builder) = apply_webview_bounds(
    window,
    webview_builder,
    options.bounds,
    options.auto_resize,
    child,
  );
  if let Some(background_throttling) = &options.background_throttling {
    webview_builder = webview_builder.with_background_throttling(match background_throttling {
      BackgroundThrottlingPolicy::Disabled => {
        taurino_core::native::wry::BackgroundThrottlingPolicy::Disabled
      }
      BackgroundThrottlingPolicy::Suspend => {
        taurino_core::native::wry::BackgroundThrottlingPolicy::Suspend
      }
      BackgroundThrottlingPolicy::Throttle => {
        taurino_core::native::wry::BackgroundThrottlingPolicy::Throttle
      }
    });
  }
  if options.javascript_disabled {
    webview_builder = webview_builder.with_javascript_disabled();
  }
  if let Some(color) = window_options.background_color {
    webview_builder = webview_builder.with_background_color(color.into());
  }

  if options.drag_drop_enabled {
    let window_id_ = window_id.clone();
    let engine_manager = manager.clone();
    let window_drag_drop = window_options.enable_drag_drop;
    webview_builder = apply_drag_drop_handlers(
      engine_manager,
      webview_builder,
      window_id_,
      id,
      window_drag_drop,
      child,
    );
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

  let mut webview_builder = apply_new_window_requested(
    engine_manager.clone(),
    webview_builder,
    id,
    options.new_window_policy.clone(),
  )?;
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

  let mut webview_builder = engine_manager.clone().connection()?.apply(
    webview_builder,
    &options.url,
    options.use_https_scheme,
  )?;

  for initialization_script in all_initialization_scripts {
    webview_builder = if initialization_script.for_main_frame_only {
      webview_builder.with_initialization_script_for_main_only(initialization_script.script, true)
    } else {
      webview_builder.with_initialization_script(initialization_script.script)
    };
  }
  let webview = apply_build_webview(&window, webview_builder, &options.label, child)?;
  // webview.evaluate_script("console.log(window.__TAURINO_INTERNALS__)")?;
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
            inner_size.width = inner_size
              .width
              .saturating_sub(window_size.width - constraint.width);
            window_size.width = constraint.width;
          }
          if window_size.height > constraint.height {
            inner_size.height = inner_size
              .height
              .saturating_sub(window_size.height - constraint.height);
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
    if let Some(target_monitor) =
      find_monitor_for_position(window_target.available_monitors(), position)
    {
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
  let theme = window_builder
    .get_theme()
    .unwrap_or(taurino_core::schema::Theme::Light);
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
    Some(
      engine_manager
        .menu()?
        .create_window_menu(raw, theme, None)?,
    )
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
