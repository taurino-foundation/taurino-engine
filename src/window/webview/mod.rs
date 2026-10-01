use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use taurino_core::anyhow;
use taurino_window::webview::WebView;

pub(crate) mod options;
pub struct WebViewManager {
    _webviews: Vec<WebView>,
    next_webview_id: Arc<AtomicU32>,
}

impl WebViewManager {
    pub fn new() -> anyhow::Result<Self> {
        Ok(WebViewManager {
            _webviews: Vec::new(),
            next_webview_id: Arc::new(AtomicU32::new(1)),
        })
    }

    pub fn next_webview_id(&self) -> taurino_core::WebViewId {
        self.next_webview_id.fetch_add(1, Ordering::Relaxed).into()
    }
}
