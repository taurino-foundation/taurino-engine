use std::sync::Arc;

use taurino_core::{
    WebViewId,
    anyhow::{Result, anyhow},
};
use taurino_window::webview::WebViewManager;

mod factory;
pub mod options;
mod webview_utils;
use crate::{
    manager::EngineManager,
    window::{
        options::WindowOptions,
        webview::{factory::create_webview, options::WebViewOptions},
    },
};

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
    window: &taurino_core::tao::window::Window,
    options: &WebViewOptions,
    window_options: &WindowOptions,
    engine_manager: Arc<EngineManager>,
    window_id: Arc<std::sync::Mutex<taurino_core::WindowId>>,
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
    )
    .map_err(|error| anyhow!("failed to create WebView {:?}: {error}", options.label))?;

    webview_manager.insert(webview)?;

    Ok(id)
}
