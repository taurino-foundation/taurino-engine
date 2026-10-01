use crate::{trayicon::TrayIconManager, window::WindowManager};
use anyhow::Result;
use std::sync::{Arc, MutexGuard};
use taurino_core::{ArcMut, anyhow, arc, lock, resources::ResourceTable};
use taurino_menu::MenuManager;

pub struct EngineManager {
    _resource_table: ArcMut<ResourceTable>,
    _menu_manager: ArcMut<MenuManager>,
    _trayicon_manager: ArcMut<TrayIconManager>,
    _window_manager: ArcMut<WindowManager>,
}

impl EngineManager {
    pub fn new(resource_table: ArcMut<ResourceTable>, menu_manager: ArcMut<MenuManager>) -> Result<Arc<Self>> {
        let trayicon_manager = TrayIconManager::new()?;
        let window_manager = WindowManager::new()?;
        let manager = arc(EngineManager {
            _resource_table: resource_table,
            _menu_manager: menu_manager,
            _trayicon_manager: trayicon_manager.clone(),
            _window_manager: window_manager.clone(),
        });
        lock!(trayicon_manager)?.bind_manager(manager.clone());
        lock!(window_manager)?.bind_manager(manager.clone());
        Ok(manager)
    }

    pub fn resource_table(self: &Arc<Self>) -> Result<MutexGuard<'_, ResourceTable>> {
        lock!(self._resource_table)
    }
    pub fn menu(self: &Arc<Self>) -> Result<MutexGuard<'_, MenuManager>> {
        lock!(self._menu_manager)
    }

    pub fn trayicon(self: &Arc<Self>) -> Result<MutexGuard<'_, TrayIconManager>> {
        lock!(self._trayicon_manager)
    }

    pub fn window(self: &Arc<Self>) -> Result<MutexGuard<'_, WindowManager>> {
        lock!(self._window_manager)
    }
}
