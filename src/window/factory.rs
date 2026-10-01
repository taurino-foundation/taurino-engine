use std::sync::Arc;

/* use taurino_window::window::Window; */
use taurino_core::{
    WindowId as CoreWindowId, anyhow,
    tao::{event_loop::EventLoopWindowTarget, window::Window},
};

use anyhow::Result;

use crate::{manager::EngineManager, window::options::WindowOptions};

pub fn create_window<T: 'static>(
    engine_manager: Arc<EngineManager>,
    id: CoreWindowId,
    window_target: &EventLoopWindowTarget<T>,
    window_options: &WindowOptions,
) -> Result<Window> {
    let window_builder = taurino_window::WindowBuilder::new()
        .title(&window_options.title)
        .inner_size(window_options.width, window_options.height)
        .visible(window_options.visible);
    let window = window_builder.inner.build(window_target)?;
    Ok(window)
}
