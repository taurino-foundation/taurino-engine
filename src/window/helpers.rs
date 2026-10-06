// Copyright 2019-2024 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use std::sync::{Arc, Mutex};

use crate::{manager::EngineManager, window::factory::create_webview};
use anyhow::{Result, anyhow};
#[cfg(any(target_os = "macos", target_os = "ios"))]
use taurino_core::schema::webview::WebContentProcessTerminatePolicy;
#[cfg(windows)]
use taurino_core::{schema::FocusState, tools::ArcMut};
use taurino_core::{
  schema::{
    webview::{NewWindowAction, NewWindowPolicy, WebViewConfig, WebViewId},
    window::{WindowConfig, WindowId},
  },
  webview::{NewWindowFeatures, NewWindowResponse, PermissionKind, PermissionResponse, WebViewManager},
};
use url::Url;

pub type PermissionRequestHandler = dyn Fn(PermissionKind) -> PermissionResponse + Send + Sync;

pub fn permission_request_handler(_eng: Arc<EngineManager>) -> Result<Box<PermissionRequestHandler>> {
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
      eprintln!("window {:?} not found after web content process termination", window_id);
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
    return Err(anyhow!("WebView with label {:?} is already registered", options.label));
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
