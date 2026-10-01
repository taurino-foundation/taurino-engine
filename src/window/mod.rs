use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
};

use taurino_core::{ArcMut, WindowId, anyhow, arc_mut, tao::event_loop::EventLoopWindowTarget};

use crate::{
    manager::EngineManager,
    window::{factory::create_window, options::WindowOptions},
};
mod factory;
mod options;
mod webview;
use anyhow::{Result, anyhow};

pub struct WindowManager {
    engine_manager: Option<Arc<EngineManager>>,
    windows: HashMap<taurino_core::tao::window::WindowId, taurino_window::window::Window>,
    windows_id_map: HashMap<taurino_core::WindowId, taurino_core::tao::window::WindowId>,
    windows_label_map: HashMap<taurino_core::WindowId, String>,
    next_window_id: Arc<AtomicU32>,
}

impl WindowManager {
    pub fn new() -> Result<ArcMut<Self>> {
        Ok(arc_mut(WindowManager {
            engine_manager: None,
            windows: HashMap::new(),
            windows_id_map: HashMap::new(),
            windows_label_map: HashMap::new(),
            next_window_id: Arc::new(AtomicU32::new(1)),
        }))
    }

    pub fn bind_manager(&mut self, manager: Arc<EngineManager>) {
        self.engine_manager = Some(manager);
    }

    pub fn next_window_id(&self) -> taurino_core::WindowId {
        self.next_window_id.fetch_add(1, Ordering::Relaxed).into()
    }

    pub fn open_window<T: 'static>(
        &mut self,
        window_options: &WindowOptions,
        target: &EventLoopWindowTarget<T>,
    ) -> Result<WindowId> {
        let id = self.next_window_id();
        let engine_manager = self
            .engine_manager
            .clone()
            .ok_or(anyhow!("Engine manager not Bounded"))?;
        let _window = create_window(engine_manager, id, target, window_options)?;
        /*         let label = window.label().to_string();
        let tao_window_id = window.id();
        self.windows.insert(tao_window_id, window);
        self.windows_id_map.insert(window_id, tao_window_id);
        self.windows_label_map.insert(window_id, label); */
        Ok(id)
    }
}
