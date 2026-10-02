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

pub struct WebViewManager<'a> {
    webviews: Vec<WebView>,
    next_webview_id: Arc<AtomicU32>,
    window: &'a taurino_core::tao::window::Window,
}

impl<'a> WebViewManager<'a> {
    pub fn new(window: &'a taurino_core::tao::window::Window) -> anyhow::Result<Self> {
        Ok(Self {
            webviews: Vec::new(),
            next_webview_id: Arc::new(AtomicU32::new(1)),
            window,
        })
    }

    pub fn next_webview_id(&self) -> taurino_core::WebViewId {
        self.next_webview_id.fetch_add(1, Ordering::Relaxed).into()
    }

    pub fn get_by_id(&self, id: WebViewId) -> Option<&WebView> {
        self.webviews.iter().find(|webview| webview.id() == id)
    }

    pub fn get_by_label(&self, label: &str) -> Option<&WebView> {
        self.webviews.iter().find(|webview| webview.label() == label)
    }

    pub fn webviews(&self) -> &[WebView] {
        &self.webviews
    }

    pub fn create_webview(
        &mut self,
        options: &WebViewOptions,
        window_options: &WindowOptions,
        engine_manager: Arc<EngineManager>,
        window_id: Arc<std::sync::Mutex<taurino_core::WindowId>>,
    ) -> anyhow::Result<&WebView> {
        let id = self.next_webview_id();

        let webview = create_webview(engine_manager, window_id, id, options, window_options, self)?;

        Ok(self.webviews.last().unwrap())
    }
}
