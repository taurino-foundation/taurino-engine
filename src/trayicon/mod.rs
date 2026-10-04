use std::sync::Arc;

use taurino_core::{
    unsafe_impl_sync_send,
    utils::{ArcMut, arc_mut},
};

use crate::manager::EngineManager;

use anyhow::Result;

unsafe_impl_sync_send!(TrayIconManager);
pub struct TrayIconManager {
    _engine_manager: Option<Arc<EngineManager>>,
}

impl TrayIconManager {
    pub fn new() -> Result<ArcMut<Self>> {
        let manager = arc_mut(TrayIconManager { _engine_manager: None });
        Ok(manager)
    }

    pub fn bind_manager(&mut self, manager: Arc<EngineManager>) {
        self._engine_manager = Some(manager);
    }
}
