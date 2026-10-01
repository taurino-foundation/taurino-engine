use std::sync::Arc;
mod handler;
mod manager;
mod trayicon;
mod window;

use taurino_core::{ArcMut, anyhow, arc_mut, resources::ResourceTable};

use crate::{handler::EngineEventHandler, manager::EngineManager};

pub struct Engine {
    event_loop: taurino_core::tao::event_loop::EventLoop<()>,
    table: ArcMut<ResourceTable>,
    manager: Arc<EngineManager>,
}

impl Engine {
    pub fn new() -> anyhow::Result<Self> {
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
        let manager = EngineManager::new(resource_table.clone(), menu_manager)?;
        Ok(Engine {
            event_loop,
            manager,
            table: resource_table,
        })
    }

    pub fn resource_table(&self) -> ArcMut<ResourceTable> {
        self.table.clone()
    }

    pub fn start(self) -> anyhow::Result<()> {
        let event_handler = EngineEventHandler::new(self.manager);
        self.event_loop.run(move |event, target, control_flow| {
            event_handler.handle_event(event, target, control_flow);
        });
    }
}

/*
fn main() -> anyhow::Result<()> {
    let engine = Engine::new()?;
    engine.start()
}
 */
