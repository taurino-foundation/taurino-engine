use std::{
    collections::{
        HashSet,
        hash_map::Entry::{Occupied, Vacant},
    },
    rc::Rc,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use url::Url;

use taurino_core::{
    WebViewId, WindowId, anyhow, arc_mut,
    dpi::PhysicalPosition,
    lock,
    wry::{DragDropEvent as WryDragDropEvent, WebContext as WryContext, WebViewBuilder},
};

use taurino_window::{utils::WebContext, webview::WebView};

use crate::{
    manager::{EngineManager, WebViewEvent},
    window::{
        events::DragDropEvent,
        webview_helpers::{
            NewWindowFeatures, NewWindowOpener, NewWindowResponse, from_wry_permission_kind, new_window_handler,
            permission_request_handler, to_wry_permission_response,
        },
        webview_options::{BackgroundThrottlingPolicy, WebViewOptions, WebviewUrl},
        window_options::WindowOptions,
    },
};

// ============================================================================
// Windows
// ============================================================================

#[cfg(target_os = "windows")]
use taurino_core::wry::{WebViewBuilderExtWindows, WebViewExtWindows};

#[cfg(target_os = "windows")]
use taurino_core::tao::platform::windows::WindowExtWindows;

#[cfg(target_os = "windows")]
use taurino_core::undecorated_resizing;

// ============================================================================
// macOS
// ============================================================================

#[cfg(target_os = "macos")]
use taurino_core::wry::{WebViewBuilderExtDarwin, WebViewBuilderExtMacos, WebViewExtDarwin, WebViewExtMacOS};

#[cfg(target_os = "macos")]
use crate::window::webview::webview_helpers::on_web_content_process_terminate_handler;

// ============================================================================
// iOS
// ============================================================================

#[cfg(target_os = "ios")]
use taurino_core::wry::{WebViewBuilderExtDarwin, WebViewBuilderExtIos, WebViewExtDarwin};

#[cfg(target_os = "ios")]
use crate::window::webview::webview_helpers::on_web_content_process_terminate_handler;

// ============================================================================
// Linux / BSD
// ============================================================================

#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
))]
use taurino_core::wry::{WebViewBuilderExtUnix, WebViewExtUnix};

#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
))]
use taurino_core::tao::platform::unix::WindowExtUnix;

#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
))]
use taurino_core::undecorated_resizing;

// ============================================================================
// Android
// ============================================================================

#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
))]
use taurino_core::wry::WebViewBuilderExtUnix;
#[cfg(target_os = "android")]
use taurino_core::wry::{WebViewBuilderExtAndroid, WebViewExtAndroid};

pub(crate) fn create_webview(
    engine_manager: Arc<EngineManager>,
    window_id: Arc<Mutex<WindowId>>,
    id: WebViewId,
    options: &WebViewOptions,
    window_options: &WindowOptions,
    window: &taurino_core::tao::window::Window,
) -> Result<WebView> {
    let browser_context = engine_manager.webcontext()?;
    let mut web_context = lock!(browser_context)?;
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

    #[cfg(target_os = "macos")]
    if let Some(webview_configuration) = webview_attributes.webview_configuration {
        webview_builder = webview_builder.with_webview_configuration(webview_configuration);
    }

    #[cfg(any(target_os = "windows", target_os = "android"))]
    {
        use taurino_core::wry::WebViewBuilderExtWindows;
        webview_builder = webview_builder.with_https_scheme(options.use_https_scheme);
    }

    if let Some(background_throttling) = &options.background_throttling {
        webview_builder = webview_builder.with_background_throttling(match background_throttling {
            BackgroundThrottlingPolicy::Disabled => taurino_core::wry::BackgroundThrottlingPolicy::Disabled,
            BackgroundThrottlingPolicy::Suspend => taurino_core::wry::BackgroundThrottlingPolicy::Suspend,
            BackgroundThrottlingPolicy::Throttle => taurino_core::wry::BackgroundThrottlingPolicy::Throttle,
        });
    }

    if options.javascript_disabled {
        webview_builder = webview_builder.with_javascript_disabled();
    }

    if let Some(color) = window_options.background_color {
        webview_builder = webview_builder.with_background_color(color.into());
    }
    // Before building the drag-drop handler, extract owned copies of what it needs:
    let is_child = options.child;
    let window_label_for_dragdrop = window_options.label.clone();
    let webview_label_for_dragdrop = options.label.clone();

    if options.drag_drop_enabled {
        let _window_id_ = window_id.clone();
        let engine_manager = Arc::clone(&engine_manager);
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

            if !is_child {
                if window_enabled_drag_drop {
                    engine_manager
                        .emit_global_window_event(
                            window_label_for_dragdrop.clone(),
                            super::events::WindowEvent::DragDrop(event),
                        )
                        .unwrap();
                }
            } else {
                engine_manager
                    .emit_global_webview_event(
                        window_label_for_dragdrop.clone(),
                        webview_label_for_dragdrop.clone(),
                        WebViewEvent::DragDrop(event),
                    )
                    .unwrap();
            };

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
        let engine_manager = Arc::clone(&engine_manager);

        webview_builder = webview_builder.with_new_window_req_handler(move |raw_url, features| {
            let Ok(url) = raw_url.parse::<Url>() else {
                return taurino_core::wry::NewWindowResponse::Deny;
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
                Arc::clone(&engine_manager),
            );

            match response {
                Ok(NewWindowResponse::Allow) => taurino_core::wry::NewWindowResponse::Allow,

                Ok(NewWindowResponse::Create { window_id }) => {
                    let window_manager = match engine_manager.window() {
                        Ok(manager) => manager,
                        Err(error) => {
                            eprintln!("failed to lock WindowManager for new-window request: {error}");
                            return taurino_core::wry::NewWindowResponse::Deny;
                        }
                    };

                    let Some(window) = window_manager.get_by_id(window_id) else {
                        eprintln!("window {:?} not found for new-window request", window_id);
                        return taurino_core::wry::NewWindowResponse::Deny;
                    };

                    let Some(webview) = window.webview(id.clone()) else {
                        eprintln!("webview {} not found in window {:?}", id.get(), window_id);
                        return taurino_core::wry::NewWindowResponse::Deny;
                    };

                    taurino_core::wry::NewWindowResponse::Create {
                        #[cfg(target_os = "macos")]
                        webview: taurino_core::wry::WebViewExtMacOS::webview(&*webview).as_super().into(),

                        #[cfg(any(
                            target_os = "linux",
                            target_os = "dragonfly",
                            target_os = "freebsd",
                            target_os = "netbsd",
                            target_os = "openbsd",
                        ))]
                        webview: webview.webview(),

                        #[cfg(windows)]
                        webview: webview.webview(),
                    }
                }

                Ok(NewWindowResponse::Deny) => taurino_core::wry::NewWindowResponse::Deny,

                Err(error) => {
                    eprintln!("new-window handler failed: {error}");
                    taurino_core::wry::NewWindowResponse::Deny
                }
            }
        });
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        if let Some(policy) = options.web_content_process_terminate_policy.clone() {
            webview_builder = webview_builder.with_on_web_content_process_terminate_handler(
                on_web_content_process_terminate_handler(engine_manager.clone(), window_id.clone(), id, policy),
            );
        }
    }
    if let Some(_policy) = options.permission_request_policy.clone() {
        webview_builder = webview_builder.with_permission_handler(move |kind| {
            let kind = from_wry_permission_kind(kind);
            let response = permission_request_handler(engine_manager.clone()).unwrap();
            to_wry_permission_response(response(kind))
        });
    }

    if !options.url.is_about_blank() {
        webview_builder = webview_builder.with_url(options.url.to_string());
    }
    let webview = match options.child {
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

    if options.child == false {
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
    let inner = Rc::new(webview);
    let context_key = if automation_enabled {
        None
    } else {
        web_context_key.clone()
    };
    let webview = WebView::new(
        id,
        options.label.clone(),
        window_id,
        inner,
        context_key,
        browser_context.clone(),
        arc_mut(None),
    );
    Ok(webview)
}
