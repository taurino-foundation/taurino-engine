use std::{
    sync::{Arc, Mutex},
    todo,
};

/* use taurino_window::window::Window; */
use taurino_core::{WindowId as CoreWindowId, anyhow, tao::event_loop::EventLoopWindowTarget};

use anyhow::Result;
use taurino_window::window::Window;

use crate::{
    manager::EngineManager,
    window::{options::WindowOptions, webview::WebViewManager},
};

pub fn create_window<T: 'static>(
    engine_manager: Arc<EngineManager>,
    id: CoreWindowId,
    window_target: &EventLoopWindowTarget<T>,
    window_options: &WindowOptions,
) -> Result<Window> {
    let window_builder = taurino_window::WindowBuilder::new()
        .title(&window_options.title)
        .inner_size(window_options.width, window_options.height)
        .visible(window_options.visible);
    let window = window_builder.inner.build(window_target)?;
    let mut webview_manager = WebViewManager::new(&window)?;

    let wid = Arc::new(Mutex::new(id));
    for view_options in &window_options.webviews {
        webview_manager.create_webview(view_options, window_options, engine_manager.clone(), wid.clone());
    }
    Ok(todo!())
}

/*





*/
/*


fn create_window<T: UserEvent, F: Fn(RawWindow) + Send + 'static>(
  window_id: WindowId,
  webview_id: u32,
  event_loop: &EventLoopWindowTarget<Message<T>>,
  context: &Context<T>,
  pending: PendingWindow<T, Wry<T>>,
  after_window_creation: Option<F>,
) -> Result<WindowWrapper> {
  #[allow(unused_mut)]
  let PendingWindow {
    mut window_builder,
    label,
    webview,
  } = pending;

  #[cfg(feature = "tracing")]
  let _webview_create_span = tracing::debug_span!("wry::webview::create").entered();
  #[cfg(feature = "tracing")]
  let window_draw_span = tracing::debug_span!("wry::window::draw").entered();
  #[cfg(feature = "tracing")]
  let window_create_span =
    tracing::debug_span!(parent: &window_draw_span, "wry::window::create").entered();

  let window_event_listeners = WindowEventListeners::default();

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

  #[cfg(desktop)]
  if window_builder.prevent_overflow.is_some() || window_builder.center {
    let monitor = if let Some(window_position) = &window_builder.inner.window.position {
      find_monitor_for_position(event_loop.available_monitors(), *window_position)
    } else {
      event_loop.primary_monitor()
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
      #[allow(unused_mut)]
      // Left and right window shadow counts as part of the window on Windows
      // We need to include it when calculating positions, but not size
      let mut shadow_width = 0;
      #[cfg(windows)]
      if window_builder.inner.window.decorations {
        use windows::Win32::UI::WindowsAndMessaging::{AdjustWindowRect, WS_OVERLAPPEDWINDOW};
        let mut rect = windows::Win32::Foundation::RECT::default();
        let result = unsafe { AdjustWindowRect(&mut rect, WS_OVERLAPPEDWINDOW, false) };
        if result.is_ok() {
          shadow_width = (rect.right - rect.left) as u32;
          // rect.bottom is made out of shadow, and we don't care about it
          window_size.height += -rect.top as u32;
        }
      }

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
        let position = window::calculate_window_center_position(window_size, monitor);
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
      find_monitor_for_position(event_loop.available_monitors(), position)
    {
      window_builder.inner.window.fullscreen = Some(Fullscreen::Borderless(Some(target_monitor)));
    }
  }

  let window = window_builder
    .inner
    .build(event_loop)
    .inspect_err(|e| log::error!("Error creating window: {e:?}"))
    .map_err(|_| Error::CreateWindow)?;

  // On macOS, `with_position` uses the content origin; the title bar is added
  // above it. `set_outer_position` is needed for precise window placement.
  #[cfg(target_os = "macos")]
  if !is_fullscreen {
    if let Some(position) = initial_position {
      window.set_outer_position(position);
    }
  }

  #[cfg(feature = "tracing")]
  {
    drop(window_create_span);

    context
      .main_thread
      .active_tracing_spans
      .0
      .borrow_mut()
      .push(ActiveTracingSpan::WindowDraw {
        id: window.id(),
        span: window_draw_span,
      });
  }

  context.window_id_map.insert(window.id(), window_id);

  if let Some(handler) = after_window_creation {
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
    handler(raw);
  }

  let mut webviews = Vec::new();

  #[cfg(windows)]
  let focused_webview = Arc::new(Mutex::new(FocusState::default()));

  #[cfg(feature = "unstable")]
  let has_children = webview.is_some();
  #[cfg(not(feature = "unstable"))]
  let has_children = false;

  if let Some(webview) = webview {
    webviews.push(create_webview(
      #[cfg(feature = "unstable")]
      WebviewKind::WindowChild,
      #[cfg(not(feature = "unstable"))]
      WebviewKind::WindowContent,
      &window,
      Arc::new(Mutex::new(window_id)),
      webview_id,
      context,
      webview,
      #[cfg(windows)]
      focused_webview.clone(),
    )?);
  }

  let window = Arc::new(window);

  #[cfg(windows)]
  let surface = if is_window_transparent {
    if let Ok(context) = softbuffer::Context::new(window.clone()) {
      if let Ok(mut surface) = softbuffer::Surface::new(&context, window.clone()) {
        window.draw_surface(&mut surface, background_color);
        Some(surface)
      } else {
        None
      }
    } else {
      None
    }
  } else {
    None
  };

  Ok(WindowWrapper {
    label,
    has_children: AtomicBool::new(has_children),
    inner: Some(window),
    webviews,
    window_event_listeners,
    #[cfg(windows)]
    background_color,
    #[cfg(windows)]
    is_window_transparent,
    #[cfg(windows)]
    surface,
    #[cfg(windows)]
    focused_webview,
  })
}


*/
// -------------------------------------------------------------------nicht löschen !!!!!!!!!!!!!!

/*


pub fn create_window<T: 'static>(
    engine_manager: Arc<EngineManager>,
    id: CoreWindowId,
    target: &EventLoopWindowTarget<T>,
    options: &WindowOptions,
    new_window_features: Option<NewWindowFeatures>,
) -> Result<CreatedWindow> {
    // ------------------------------------------------------------------------
    // Window geometry
    // ------------------------------------------------------------------------

    let mut width = options.width;
    let mut height = options.height;
    let mut x = options.x;
    let mut y = options.y;

    // window.open(..., "width=...,height=...,left=...,top=...")
    //
    // Python's WindowOptions remains the base configuration.
    // Browser-requested features override geometry when present.
    if let Some(features) = &new_window_features {
        if let Some(size) = features.size {
            width = size.width;
            height = size.height;
        }

        if let Some(position) = features.position {
            x = Some(position.x);
            y = Some(position.y);
        }
    }

    // ------------------------------------------------------------------------
    // Tao Window
    // ------------------------------------------------------------------------

    let mut builder = taurino_window::WindowBuilder::new()
        .title(&options.title)
        .inner_size(width, height)
        .visible(options.visible);

    if let (Some(x), Some(y)) = (x, y) {
        builder = builder.position(x, y);
    }

    let window = builder.inner.build(target)?;

    // ------------------------------------------------------------------------
    // WebViews
    // ------------------------------------------------------------------------

    let mut webviews = Vec::with_capacity(
        options.webviews.len(),
    );

    // There is only one opener context.
    //
    // For a browser-created popup, normally the first/root WebView must receive
    // it because WRY's NewWindowResponse::Create refers to exactly one WebView.
    let mut opener = new_window_features;

    for (index, webview_options) in
        options.webviews.iter().enumerate()
    {
        let webview_id = (index as u32 + 1).into();

        let webview = create_webview(
            engine_manager.clone(),
            &window,
            Arc::new(Mutex::new(id)),
            webview_id,
            webview_options,
            options,

            // Only the root WebView belongs to the window.open request.
            if index == 0 {
                opener.take()
            } else {
                None
            },

            #[cfg(windows)]
            Arc::new(Mutex::new(
                taurino_window::config::FocusState::default(),
            )),
        )?;

        webviews.push(webview);
    }

    Ok(CreatedWindow {
        window,
        webviews,
    })
}



// ============================================================================
// webview/factory.rs
// ============================================================================

pub(crate) fn create_webview(
    engine_manager: Arc<EngineManager>,
    window: &taurino_core::tao::window::Window,
    window_id: Arc<Mutex<WindowId>>,
    id: WebViewId,
    options: &WebViewOptions,
    window_options: &WindowOptions,

    // Present only when this WebView is being created as the response to
    // window.open / target="_blank".
    new_window_features: Option<
        taurino_core::wry::NewWindowFeatures
    >,

    #[cfg(windows)]
    focused_webview: Arc<Mutex<FocusState>>,
) -> Result<WebView> {
    // ------------------------------------------------------------------------
    // WebContext
    // ------------------------------------------------------------------------

    let mut web_context =
        engine_manager.webcontext()?;

    let is_first_context =
        web_context.is_empty();

    let automation_enabled =
        std::env::var("TAURI_WEBVIEW_AUTOMATION")
            .as_deref()
            == Ok("true");

    let web_context_key =
        options.data_directory.clone();

    let entry =
        web_context.entry(web_context_key.clone());

    let web_context = match entry {
        Occupied(entry) => {
            let context = entry.into_mut();

            context
                .referenced_by_webviews
                .insert(options.label.clone());

            context
        }

        Vacant(entry) => {
            let mut inner =
                WryContext::new(web_context_key);

            inner.set_allows_automation(
                automation_enabled
                    && is_first_context,
            );

            entry.insert(WebContext {
                inner,
                referenced_by_webviews:
                    [options.label.clone()].into(),
                registered_custom_protocols:
                    HashSet::new(),
            })
        }
    };

    // ------------------------------------------------------------------------
    // Base builder
    // ------------------------------------------------------------------------

    let mut builder =
        WebViewBuilder::new_with_web_context(
            &mut web_context.inner,
        )
        .with_id(&options.label)
        .with_focused(window_options.focus)
        .with_transparent(
            window_options.transparent,
        )
        .with_accept_first_mouse(
            window_options.accept_first_mouse,
        )
        .with_incognito(options.incognito)
        .with_clipboard(
            options.enable_clipboard_access,
        )
        .with_hotkeys_zoom(
            options.zoom_hotkeys_enabled,
        )
        .with_general_autofill_enabled(
            options.general_autofill_enabled,
        );

    // ------------------------------------------------------------------------
    // IMPORTANT:
    // inherit opener-specific WebView state required by WRY.
    // ------------------------------------------------------------------------

    if let Some(features) = new_window_features {
        // Windows:
        // target WebView MUST use the same WebView2 environment.
        #[cfg(windows)]
        {
            use taurino_core::wry::
                WebViewBuilderExtWindows;

            builder = builder.with_environment(
                features.opener.environment,
            );
        }

        // Linux:
        // target WebView MUST be related to the opener WebView.
        #[cfg(any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd",
        ))]
        {
            use taurino_core::wry::
                WebViewBuilderExtUnix;

            builder = builder.with_related_view(
                features.opener.webview,
            );
        }

        // macOS:
        // target WebView MUST use WRY's supplied WKWebViewConfiguration.
        #[cfg(target_os = "macos")]
        {
            use taurino_core::wry::
                WebViewBuilderExtMacos;

            builder =
                builder.with_webview_configuration(
                    features
                        .opener
                        .target_configuration,
                );
        }
    }

    // ------------------------------------------------------------------------
    // URL
    // ------------------------------------------------------------------------

    builder = builder.with_url(
        options.url.to_string(),
    );

    // ... remaining configuration ...

    // ------------------------------------------------------------------------
    // Build
    // ------------------------------------------------------------------------

    #[cfg(any(
        target_os = "windows",
        target_os = "macos",
    ))]
    let webview = builder.build(window)?;

    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
    ))]
    let webview = {
        use taurino_core::wry::
            WebViewBuilderExtUnix;

        let container = window
            .default_vbox()
            .ok_or_else(|| {
                anyhow!(
                    "window has no GTK container"
                )
            })?;

        builder.build_gtk(container)?
    };

    Ok(webview)
}


*/
