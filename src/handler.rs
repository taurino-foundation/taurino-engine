use std::sync::Arc;

use crate::manager::EngineManager;

pub struct EngineEventHandler {
    manager: Arc<EngineManager>,
}

impl EngineEventHandler {
    pub fn new(manager: Arc<EngineManager>) -> Self {
        EngineEventHandler { manager }
    }

    pub fn handle_event(
        &self,
        event: taurino_core::tao::event::Event<()>,
        _target: &taurino_core::tao::event_loop::EventLoopWindowTarget<()>,
        control_flow: &mut taurino_core::tao::event_loop::ControlFlow,
    ) {
        *control_flow = taurino_core::tao::event_loop::ControlFlow::Wait;
        match event {
            taurino_core::tao::event::Event::WindowEvent { event, .. } => match event {
                taurino_core::tao::event::WindowEvent::CloseRequested => {
                    *control_flow = taurino_core::tao::event_loop::ControlFlow::Exit;
                }
                _ => {}
            },
            _ => {}
        }
    }
}
