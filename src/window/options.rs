use taurino_core::serde::{Deserialize, Serialize};

use crate::window::webview::options::WebViewOptions;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "taurino_core::serde", rename_all = "camelCase")]
pub struct WindowOptions {
    pub title: String,
    pub width: f64,
    pub height: f64,
    pub resizable: bool,
    pub fullscreen: bool,
    pub visible: bool,
    pub webview: Vec<WebViewOptions>,
}
