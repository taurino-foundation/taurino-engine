use std::sync::Arc;
mod handler;
mod manager;
mod trayicon;
mod window;

use taurino_core::{ArcMut, anyhow, arc_mut, resources::ResourceTable};
use taurino_window::utils::WebContextStore;

use crate::{handler::EngineEventHandler, manager::EngineManager};

pub struct Engine {
    webcontext: WebContextStore,
    event_loop: taurino_core::tao::event_loop::EventLoop<()>,
    table: ArcMut<ResourceTable>,
    manager: Arc<EngineManager>,
}

impl Engine {
    pub fn new() -> anyhow::Result<Self> {
        let webcontext = WebContextStore::default();
        let resource_table = arc_mut(ResourceTable::default());
        let menu_manager = taurino_menu::MenuManager::new()?;
        let mut loop_builder = taurino_core::tao::event_loop::EventLoopBuilder::<()>::with_user_event();
        #[cfg(windows)]
        {
            use taurino_core::tao::platform::windows::EventLoopBuilderExtWindows;
            let msg = taurino_menu::MenuManager::install_msg_hook(menu_manager.clone());
            loop_builder.with_msg_hook(msg);
        }
        let event_loop = loop_builder.build();
        let manager = EngineManager::new(webcontext.clone(), resource_table.clone(), menu_manager)?;
        Ok(Engine {
            webcontext,
            event_loop,
            manager,
            table: resource_table,
        })
    }

    pub fn resource_table(&self) -> ArcMut<ResourceTable> {
        self.table.clone()
    }
    pub fn webcontext(&self) -> WebContextStore {
        self.webcontext.clone()
    }
    pub fn start(self) -> anyhow::Result<()> {
        let event_handler = EngineEventHandler::new(self.manager);
        self.event_loop.run(move |event, target, control_flow| {
            event_handler.handle_event(event, target, control_flow);
        });
    }
}
