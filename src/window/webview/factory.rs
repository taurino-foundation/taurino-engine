use std::sync::{Arc, Mutex};
use std::{
    collections::{
        HashSet,
        hash_map::Entry::{Occupied, Vacant},
    },
    rc::Rc,
    todo,
};
#[cfg(windows)]
use taurino_core::wry::WebViewExtWindows;
use taurino_core::{
    WebViewId, WindowId, anyhow, arc_mut,
    dpi::PhysicalPosition,
    lock,
    wry::{DragDropEvent as WryDragDropEvent, WebContext as WryContext, WebViewBuilder},
};

use crate::{
    manager::EngineManager,
    window::{
        options::{DragDropEvent, WindowOptions},
        webview::{
            WebViewManager,
            options::{BackgroundThrottlingPolicy, NewWindowAction, WebViewOptions, WebviewUrl},
        },
    },
};
use anyhow::{Result, anyhow};
use taurino_window::{utils::WebContext, webview::WebView};
use url::Url;

/// Information about the webview that initiated a new window request.
#[derive(Debug)]
pub struct NewWindowOpener {
    /// The instance of the webview that initiated the new window request.
    ///
    /// This must be set as the related view of the new webview. See [`WebviewAttributes::related_view`].
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
    ))]
    pub webview: taurino_core::webkit2gtk::WebView,
    /// The instance of the webview that initiated the new window request.
    ///
    /// The target webview environment **MUST** match the environment of the opener webview. See [`WebviewAttributes::with_environment`].
    #[cfg(windows)]
    pub webview: taurino_core::webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
    #[cfg(windows)]
    pub environment: taurino_core::webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment,
    /// The instance of the webview that initiated the new window request.
    #[cfg(target_os = "macos")]
    pub webview: taurino_core::objc2::rc::Retained<objc2_web_kit::WKWebView>,
    /// Configuration of the target webview.
    ///
    /// This **MUST** be used when creating the target webview. See [`WebviewAttributes::webview_configuration`].
    #[cfg(target_os = "macos")]
    pub target_configuration: taurino_core::objc2::rc::Retained<objc2_web_kit::WKWebViewConfiguration>,
}

/// Window features of a window requested to open.
#[derive(Debug)]
pub struct NewWindowFeatures {
    pub(crate) size: Option<taurino_core::dpi::LogicalSize<f64>>,
    pub(crate) position: Option<taurino_core::dpi::LogicalPosition<f64>>,
    pub(crate) opener: NewWindowOpener,
}

impl NewWindowFeatures {
    pub fn new(
        size: Option<taurino_core::dpi::LogicalSize<f64>>,
        position: Option<taurino_core::dpi::LogicalPosition<f64>>,
        opener: NewWindowOpener,
    ) -> Self {
        Self { size, position, opener }
    }

    /// Specifies the size of the content area
    /// as defined by the user's operating system where the new window will be generated.
    pub fn size(&self) -> Option<taurino_core::dpi::LogicalSize<f64>> {
        self.size
    }

    /// Specifies the position of the window relative to the work area
    /// as defined by the user's operating system where the new window will be generated.
    pub fn position(&self) -> Option<taurino_core::dpi::LogicalPosition<f64>> {
        self.position
    }

    /// Returns information about the webview that initiated a new window request.
    pub fn opener(&self) -> &NewWindowOpener {
        &self.opener
    }
}

/// Response for the new window request handler.
pub enum NewWindowResponse {
    /// Allow the window to be opened with the default implementation.
    Allow,
    /// Allow the window to be opened, with the given window.
    ///
    /// ## Platform-specific:
    ///
    /// **Linux**: The webview must be related to the caller webview. See [`WebviewAttributes::related_view`].
    /// **Windows**: The webview must use the same environment as the caller webview. See [`WebviewAttributes::with_environment`].
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    Create { window_id: WindowId },
    /// Deny the window from being opened.
    Deny,
}

fn new_window_handler(url: Url, frutur: NewWindowFeatures, managaer: Arc<EngineManager>) -> Result<NewWindowResponse> {
    Ok(NewWindowResponse::Allow)
}

pub(crate) fn create_webview(
    engine_manager: Arc<EngineManager>,
    window_id: Arc<Mutex<WindowId>>,
    id: WebViewId,
    options: &WebViewOptions,
    window_options: &WindowOptions,
    webview_manager: &mut WebViewManager,
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
    if window_options.enable_drag_drop {
        let _window_id_ = window_id.clone();
        webview_builder = webview_builder.with_drag_drop_handler(move |event| {
            let _event = match event {
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

            // send[`DragDropEvent`] over_proxy[_window_id_, _event]
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

    if let Some(_policy) = options.new_window_policy.clone() {
        let engine_manager = Arc::clone(&engine_manager);

        webview_builder = webview_builder.with_new_window_req_handler(move |raw_url, features| {
            let Ok(url) = raw_url.parse::<Url>() else {
                return taurino_core::wry::NewWindowResponse::Deny;
            };

            let response = new_window_handler(
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

    let inner = Rc::new(webview_builder.build(webview_manager.window)?);
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
