// Copyright 2019-2024 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use std::sync::Arc;

use crate::{
    manager::EngineManager,
    window::webview::options::{NewWindowAction, NewWindowPolicy},
};
use anyhow::Result;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use taurino_core::{WindowId, anyhow};
use url::Url;

/// Permission types that can be requested by the webview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PermissionKind {
    /// Microphone access permission.
    Microphone,
    /// Camera access permission.
    Camera,
    /// Geolocation access permission.
    ///
    /// ## Platform-specific
    ///
    /// - **Windows**: Supported via `COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION`.
    /// - **Linux**: Supported via `GeolocationPermissionRequest`.
    /// - **Android**: Supported via `WebChromeClient.onGeolocationPermissionsShowPrompt`.
    /// - **macOS / iOS**: Not yet supported by platform backends.
    Geolocation,
    /// Notifications permission.
    ///
    /// ## Platform-specific
    ///
    /// - **Windows**: Supported via `COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS`.
    /// - **Linux**: Supported via `NotificationPermissionRequest`.
    /// - **macOS / Android / iOS**: Not yet supported by platform backends.
    Notifications,
    /// Clipboard read permission.
    ///
    /// ## Platform-specific
    ///
    /// - **Windows**: Supported via `COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ`.
    /// - **macOS / Linux / Android / iOS**: Not yet supported by platform backends.
    ClipboardRead,
    /// Display capture permission (for getDisplayMedia).
    DisplayCapture,
    /// Midi access permission.
    ///
    /// ## Platform-specific
    ///
    /// - **Windows**: Supported via `COREWEBVIEW2_PERMISSION_KIND_MIDI_SYSTEM_EXCLUSIVE_MESSAGES`.
    /// - **Android**: Supported via `android.webkit.resource.MIDI_SYSEX`.
    /// - **macOS / Linux / iOS**: Not yet supported by platform backends.
    Midi,
    /// Sensors (accelerometer, gyroscope, etc.) access permission.
    ///
    /// ## Platform-specific
    ///
    /// - **Windows**: Supported via `COREWEBVIEW2_PERMISSION_KIND_OTHER_SENSORS`.
    /// - **macOS / Linux / Android / iOS**: Not yet supported by platform backends.
    Sensors,
    /// Media key system access permission.
    ///
    /// ## Platform-specific
    ///
    /// - **Android**: Supported via `android.webkit.resource.PROTECTED_MEDIA_ID`.
    /// - **Windows / macOS / Linux / iOS**: Not yet supported by platform backends.
    MediaKeySystemAccess,
    /// Local fonts access permission.
    ///
    /// ## Platform-specific
    ///
    /// - **Windows**: Supported via `COREWEBVIEW2_PERMISSION_KIND_LOCAL_FONTS`.
    /// - **macOS / Linux / Android / iOS**: Not yet supported by platform backends.
    LocalFonts,
    /// Window management permission.
    ///
    /// ## Platform-specific
    ///
    /// - **Windows**: Supported via `COREWEBVIEW2_PERMISSION_KIND_WINDOW_MANAGEMENT`.
    /// - **macOS / Linux / Android / iOS**: Not yet supported by platform backends.
    WindowManagement,
    /// Pointer lock permission.
    ///
    /// ## Platform-specific
    ///
    /// - **Linux**: Supported via `PointerLockPermissionRequest`.
    /// - **Windows / macOS / Android / iOS**: Not yet supported by platform backends.
    PointerLock,
    /// Automatic downloads permission (multiple downloads without user interaction).
    ///
    /// ## Platform-specific
    ///
    /// - **Windows**: Supported via `COREWEBVIEW2_PERMISSION_KIND_MULTIPLE_AUTOMATIC_DOWNLOADS`.
    /// - **macOS / Linux / Android / iOS**: Not yet supported by platform backends.
    AutomaticDownloads,
    /// File system access permission (read/write via File System Access API).
    ///
    /// ## Platform-specific
    ///
    /// - **Windows**: Supported via `COREWEBVIEW2_PERMISSION_KIND_FILE_READ_WRITE`.
    /// - **macOS / Linux / Android / iOS**: Not yet supported by platform backends.
    FileSystemAccess,
    /// Media autoplay permission.
    ///
    /// ## Platform-specific
    ///
    /// - **Windows**: Supported via `COREWEBVIEW2_PERMISSION_KIND_AUTOPLAY`.
    /// - **macOS / Linux / Android / iOS**: Not yet supported by platform backends.
    Autoplay,
    /// Other unrecognized permission type.
    Other,
}

impl std::fmt::Display for PermissionKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Microphone => write!(f, "microphone"),
            Self::Camera => write!(f, "camera"),
            Self::Geolocation => write!(f, "geolocation"),
            Self::Notifications => write!(f, "notifications"),
            Self::ClipboardRead => write!(f, "clipboard-read"),
            Self::DisplayCapture => write!(f, "display-capture"),
            Self::Midi => write!(f, "midi"),
            Self::Sensors => write!(f, "sensors"),
            Self::MediaKeySystemAccess => write!(f, "media-key-system-access"),
            Self::LocalFonts => write!(f, "local-fonts"),
            Self::WindowManagement => write!(f, "window-management"),
            Self::PointerLock => write!(f, "pointer-lock"),
            Self::AutomaticDownloads => write!(f, "automatic-downloads"),
            Self::FileSystemAccess => write!(f, "file-system-access"),
            Self::Autoplay => write!(f, "autoplay"),
            Self::Other => write!(f, "other"),
        }
    }
}

/// Response for permission requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PermissionResponse {
    /// Grant the permission.
    ///
    /// ## Platform-specific
    ///
    /// - **Android**: Not supported for runtime permissions; the normal Android
    ///   permission flow is used instead.
    Allow,
    /// Deny the permission.
    Deny,
    /// Use the platform or browser default behavior.
    ///
    /// ## Platform-specific
    ///
    /// - **Windows / macOS / Android**: The default behavior is to continue the
    ///   platform or browser permission flow.
    /// - **Linux**: The default behavior is [`Self::Deny`]
    #[default]
    Default,
}

impl std::fmt::Display for PermissionResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Allow => write!(f, "allow"),
            Self::Deny => write!(f, "deny"),
            Self::Default => write!(f, "default"),
        }
    }
}

pub type PermissionRequestHandler = dyn Fn(PermissionKind) -> PermissionResponse + Send + Sync;

pub fn permission_request_handler(
    eng: Arc<EngineManager>,
) -> taurino_core::anyhow::Result<Box<PermissionRequestHandler>> {
    Ok(Box::new(move |kind| {
        let manager = eng.clone();
        match kind {
            PermissionKind::Microphone => PermissionResponse::Allow,
            PermissionKind::Camera => PermissionResponse::Allow,
            PermissionKind::Geolocation => PermissionResponse::Allow,
            _ => PermissionResponse::Deny,
        }
    }))
}

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

pub fn new_window_handler(
    policy: &NewWindowPolicy,
    url: Url,
    features: NewWindowFeatures,
    engine_manager: Arc<EngineManager>,
) -> Result<NewWindowResponse> {
    match policy.evaluate(&url) {
        NewWindowAction::Allow => Ok(NewWindowResponse::Allow),

        NewWindowAction::Deny => Ok(NewWindowResponse::Deny),

        NewWindowAction::Create { window } => {
            // später implementieren
            Ok(NewWindowResponse::Deny)
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
type OnWebContentProcessTerminateHandler = dyn Fn() + Send;

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub fn on_web_content_process_terminate_handler(
    engine_manager: Arc<EngineManager>,
    window_id: Arc<Mutex<taurino_core::WindowId>>,
    webview_id: WebViewId,
    policy: WebContentProcessTerminatePolicy,
) -> Box<OnWebContentProcessTerminateHandler> {
    Box::new(move || {
        log::debug!("web content process terminated for webview {}", webview_id.get());

        if matches!(policy, WebContentProcessTerminatePolicy::Ignore) {
            return;
        }

        let window_manager = match engine_manager.window() {
            Ok(manager) => manager,
            Err(error) => {
                log::error!("failed to lock WindowManager: {error}");
                return;
            }
        };

        let window = match window_manager.get_window(&window_id) {
            Ok(window) => window,
            Err(error) => {
                log::error!("failed to get window: {error}");
                return;
            }
        };

        let Some(webview) = window.webview(webview_id) else {
            log::error!("failed to find webview {}", webview_id.get());
            return;
        };

        if let Err(error) = webview.reload() {
            log::error!("failed to reload webview {}: {error}", webview_id.get());
        } else {
            log::debug!("webview {} reloaded", webview_id.get());
        }
    })
}
