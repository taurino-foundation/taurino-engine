use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};
mod factory;
use taurino_core::{WebViewId, anyhow, tao::event_loop::EventLoopWindowTarget};

use taurino_window::webview::WebView;

use crate::{
    manager::EngineManager,
    window::{
        options::WindowOptions,
        webview::{factory::create_webview, options::WebViewOptions},
    },
};

pub mod options;

pub struct WebViewManager {
    webviews: Vec<WebView>,
    next_webview_id: Arc<AtomicU32>,
}

impl WebViewManager {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            webviews: Vec::new(),
            next_webview_id: Arc::new(AtomicU32::new(1)),
        })
    }

    pub fn next_webview_id(&self) -> taurino_core::WebViewId {
        self.next_webview_id.fetch_add(1, Ordering::Relaxed).into()
    }

    pub fn get_by_id(&self, id: WebViewId) -> Option<&WebView> {
        self.webviews.iter().find(|webview| webview.id() == id)
    }

    pub fn get_by_label(&self, label: &str) -> Option<&WebView> {
        self.webviews
            .iter()
            .find(|webview| webview.label() == label)
    }

    pub fn webviews(&self) -> &[WebView] {
        &self.webviews
    }

    pub fn create_webview(
        &mut self,
        window: &taurino_core::tao::window::Window,
        options: &WebViewOptions,
        window_options: &WindowOptions,
        engine_manager: Arc<EngineManager>,
        window_id: Arc<std::sync::Mutex<taurino_core::WindowId>>,
    ) -> anyhow::Result<WebViewId> {
        let id = self.next_webview_id();

        let webview = create_webview(
            engine_manager,
            window_id,
            id,
            options,
            window_options,
            window,
        )?;
        self.webviews.push(webview);

        Ok(id)
    }
}
