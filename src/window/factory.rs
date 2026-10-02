use std::sync::{Arc, Mutex, atomic::AtomicBool};

/* use taurino_window::window::Window; */
use taurino_core::{
    MonitorExt, WindowId as CoreWindowId, anyhow, arc_mut, calculate_window_center_position, dpi::PhysicalSize,
};

use crate::{
    handler::TaurinoWindowTarget,
    manager::EngineManager,
    window::{options::WindowOptions, webview::attach_webview},
};
use anyhow::Result;
#[cfg(windows)]
use taurino_core::softbuffer;
#[cfg(windows)]
use taurino_core::tao::platform::windows::WindowExtWindows;
use taurino_menu::RawWindow;
#[cfg(windows)]
use taurino_window::config::FocusState;
use taurino_window::{webview::WebViewManager, window::Window};

pub fn create_window(
    engine_manager: Arc<EngineManager>,
    id: CoreWindowId,
    window_target: &TaurinoWindowTarget,
    window_options: &WindowOptions,
) -> Result<Window> {
    let mut window_builder = taurino_window::WindowBuilder::new()
        .title(&window_options.title)
        .inner_size(window_options.width, window_options.height)
        .visible(window_options.visible);

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
            #[allow(unused_mut)]
            // Left and right window shadow counts as part of the window on Windows
            // We need to include it when calculating positions, but not size
            let mut shadow_width = 0;
            #[cfg(windows)]
            if window_builder.inner.window.decorations {
                use taurino_core::windows::Win32::UI::WindowsAndMessaging::{AdjustWindowRect, WS_OVERLAPPEDWINDOW};
                let mut rect = taurino_core::windows::Win32::Foundation::RECT::default();
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
        if let Some(target_monitor) = find_monitor_for_position(event_loop.available_monitors(), position) {
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
    let theme = window_builder.get_theme().unwrap_or(taurino_core::dpi::Theme::Light);
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

        arc_mut(Some(engine_manager.menu()?.create_window_menu(raw, theme, None)?))
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
    let mut webview_manager = WebViewManager::new()?;

    let wid = Arc::new(Mutex::new(id));

    let has_child_webviews = window_options.webviews.len() > 1;

    for view_options in &window_options.webviews {
        let mut view_options = view_options.clone();

        // If only a single WebView exists in the list and the user has still set
        // `child` to `true`, we must ensure that it is reset to `false`, because
        // a single WebView in the list cannot be a child WebView of a window.
        if !has_child_webviews {
            view_options.child = false;
        }

        attach_webview(
            &mut webview_manager,
            &window,
            &view_options,
            window_options,
            engine_manager.clone(),
            wid.clone(),
        )?;
    }

    let window = Arc::new(window);

    #[cfg(windows)]
    let surface = arc_mut(if is_window_transparent {
        if let Ok(context) = softbuffer::Context::new(window.clone()) {
            if let Ok(mut surface) = softbuffer::Surface::new(&context, window.clone()) {
                use taurino_core::WindowExt;

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
    });

    Ok(Window::new(
        id,
        Some(window),
        menu,
        webview_manager,
        window_options.label.clone(),
        arc_mut(background_color),
        is_window_transparent,
        surface,
        focused_webview,
        AtomicBool::new(has_child_webviews),
    ))
}

fn find_monitor_for_position(
    monitors: impl Iterator<Item = taurino_core::tao::monitor::MonitorHandle>,
    window_position: taurino_core::dpi::Position,
) -> Option<taurino_core::tao::monitor::MonitorHandle> {
    monitors.into_iter().find(|m| {
        let monitor_pos = m.position();
        let monitor_size = m.size();

        // type annotations required for 32bit targets.
        let window_position = window_position.to_physical::<i32>(m.scale_factor());

        monitor_pos.x <= window_position.x
            && window_position.x < monitor_pos.x + monitor_size.width as i32
            && monitor_pos.y <= window_position.y
            && window_position.y < monitor_pos.y + monitor_size.height as i32
    })
}
