use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
};

use taurino_core::anyhow::{Result, anyhow};
use taurino_core::{
    ArcMut, WindowId, arc_mut,
    tao::{event_loop::EventLoopWindowTarget, window::WindowId as TaoWindowId},
};

use taurino_window::window::Window;

use crate::{
    manager::EngineManager,
    window::{factory::create_window, options::WindowOptions},
};

mod factory;
mod options;
mod webview;

pub struct WindowManager {
    engine_manager: Option<Arc<EngineManager>>,

    /// Tao WindowId -> Window
    windows: HashMap<TaoWindowId, Window>,

    /// Taurino WindowId -> Tao WindowId
    windows_id_map: HashMap<WindowId, TaoWindowId>,

    /// Taurino WindowId -> label
    windows_label_map: HashMap<WindowId, String>,

    next_window_id: Arc<AtomicU32>,
}

impl WindowManager {
    // ------------------------------------------------------------------------
    // Construction
    // ------------------------------------------------------------------------

    pub fn new() -> Result<ArcMut<Self>> {
        Ok(arc_mut(Self {
            engine_manager: None,

            windows: HashMap::new(),
            windows_id_map: HashMap::new(),
            windows_label_map: HashMap::new(),

            next_window_id: Arc::new(AtomicU32::new(1)),
        }))
    }

    // ------------------------------------------------------------------------
    // EngineManager
    // ------------------------------------------------------------------------

    pub fn bind_manager(&mut self, manager: Arc<EngineManager>) {
        self.engine_manager = Some(manager);
    }

    pub fn engine_manager(&self) -> Result<&Arc<EngineManager>> {
        self.engine_manager
            .as_ref()
            .ok_or_else(|| anyhow!("EngineManager is not bound"))
    }

    pub fn engine_manager_cloned(&self) -> Result<Arc<EngineManager>> {
        self.engine_manager
            .clone()
            .ok_or_else(|| anyhow!("EngineManager is not bound"))
    }

    pub fn is_manager_bound(&self) -> bool {
        self.engine_manager.is_some()
    }

    // ------------------------------------------------------------------------
    // IDs
    // ------------------------------------------------------------------------

    pub fn next_window_id(&self) -> WindowId {
        self.next_window_id.fetch_add(1, Ordering::Relaxed).into()
    }

    pub fn tao_id(&self, id: WindowId) -> Option<TaoWindowId> {
        self.windows_id_map.get(&id).copied()
    }

    // ------------------------------------------------------------------------
    // Window creation
    // ------------------------------------------------------------------------

    pub fn open_window<T: 'static>(
        &mut self,
        window_options: &WindowOptions,
        target: &EventLoopWindowTarget<T>,
    ) -> Result<WindowId> {
        let id = self.next_window_id();

        let engine_manager = self.engine_manager_cloned()?;

        let window = create_window(engine_manager, id, target, window_options)?;

        self.insert_window(id, window)?;

        Ok(id)
    }

    // ------------------------------------------------------------------------
    // Register window
    // ------------------------------------------------------------------------

    fn insert_window(&mut self, id: WindowId, window: Window) -> Result<()> {
        if self.windows_id_map.contains_key(&id) {
            return Err(anyhow!("window with id {id:?} is already registered"));
        }

        let tao_id = window.tao()?.id();

        if self.windows.contains_key(&tao_id) {
            return Err(anyhow!("window with Tao id {tao_id:?} is already registered"));
        }

        if self.get_by_label(&window.label).is_some() {
            return Err(anyhow!("window with label {:?} is already registered", window.label));
        }

        let label = window.label.clone();

        self.windows_id_map.insert(id, tao_id);
        self.windows_label_map.insert(id, label);
        self.windows.insert(tao_id, window);

        Ok(())
    }

    // ------------------------------------------------------------------------
    // Lookup by Taurino WindowId
    // ------------------------------------------------------------------------
    pub fn get_window(&self, id: &Arc<std::sync::Mutex<WindowId>>) -> Result<&Window> {
        let id = *id.lock().map_err(|_| anyhow!("window id mutex is poisoned"))?;

        self.get(id).ok_or_else(|| anyhow!("window with id {id:?} not found"))
    }

    pub fn get(&self, id: WindowId) -> Option<&Window> {
        let tao_id = self.windows_id_map.get(&id)?;

        self.windows.get(tao_id)
    }

    pub fn get_mut(&mut self, id: WindowId) -> Option<&mut Window> {
        let tao_id = *self.windows_id_map.get(&id)?;

        self.windows.get_mut(&tao_id)
    }

    pub fn get_by_id(&self, id: WindowId) -> Option<&Window> {
        self.get(id)
    }

    pub fn get_by_id_mut(&mut self, id: WindowId) -> Option<&mut Window> {
        self.get_mut(id)
    }

    // ------------------------------------------------------------------------
    // Lookup by Tao WindowId
    // ------------------------------------------------------------------------

    pub fn get_by_tao_id(&self, tao_id: TaoWindowId) -> Option<&Window> {
        self.windows.get(&tao_id)
    }

    pub fn get_by_tao_id_mut(&mut self, tao_id: TaoWindowId) -> Option<&mut Window> {
        self.windows.get_mut(&tao_id)
    }

    pub fn id_from_tao_id(&self, tao_id: TaoWindowId) -> Option<WindowId> {
        self.windows_id_map
            .iter()
            .find_map(|(id, registered_tao_id)| (*registered_tao_id == tao_id).then_some(*id))
    }

    // ------------------------------------------------------------------------
    // Lookup by label
    // ------------------------------------------------------------------------

    pub fn get_by_label(&self, label: &str) -> Option<&Window> {
        let id = self.id_by_label(label)?;

        self.get(id)
    }

    pub fn get_by_label_mut(&mut self, label: &str) -> Option<&mut Window> {
        let id = self.id_by_label(label)?;

        self.get_mut(id)
    }

    pub fn id_by_label(&self, label: &str) -> Option<WindowId> {
        self.windows_label_map
            .iter()
            .find_map(|(id, registered_label)| (registered_label == label).then_some(*id))
    }

    pub fn label(&self, id: WindowId) -> Option<&str> {
        self.windows_label_map.get(&id).map(String::as_str)
    }

    // ------------------------------------------------------------------------
    // Existence
    // ------------------------------------------------------------------------

    pub fn contains(&self, id: WindowId) -> bool {
        self.windows_id_map.contains_key(&id)
    }

    pub fn contains_tao_id(&self, tao_id: TaoWindowId) -> bool {
        self.windows.contains_key(&tao_id)
    }

    pub fn contains_label(&self, label: &str) -> bool {
        self.id_by_label(label).is_some()
    }

    // ------------------------------------------------------------------------
    // Remove window
    // ------------------------------------------------------------------------

    pub fn remove(&mut self, id: WindowId) -> Option<Window> {
        let tao_id = self.windows_id_map.remove(&id)?;

        self.windows_label_map.remove(&id);

        self.windows.remove(&tao_id)
    }

    pub fn remove_by_tao_id(&mut self, tao_id: TaoWindowId) -> Option<Window> {
        let id = self.id_from_tao_id(tao_id)?;

        self.windows_id_map.remove(&id);
        self.windows_label_map.remove(&id);

        self.windows.remove(&tao_id)
    }

    pub fn remove_by_label(&mut self, label: &str) -> Option<Window> {
        let id = self.id_by_label(label)?;

        self.remove(id)
    }

    // ------------------------------------------------------------------------
    // Collections
    // ------------------------------------------------------------------------

    pub fn windows(&self) -> impl Iterator<Item = &Window> {
        self.windows.values()
    }

    pub fn windows_mut(&mut self) -> impl Iterator<Item = &mut Window> {
        self.windows.values_mut()
    }

    pub fn ids(&self) -> impl Iterator<Item = WindowId> + '_ {
        self.windows_id_map.keys().copied()
    }

    pub fn labels(&self) -> impl Iterator<Item = &str> {
        self.windows_label_map.values().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.windows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    // ------------------------------------------------------------------------
    // Clear
    // ------------------------------------------------------------------------

    pub fn clear(&mut self) {
        self.windows.clear();
        self.windows_id_map.clear();
        self.windows_label_map.clear();
    }
}
