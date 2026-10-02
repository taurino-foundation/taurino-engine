use taurino_core::anyhow;
use taurino_engine::Engine;

fn main() -> anyhow::Result<()> {
    let engine = Engine::new()?;
    engine.start()
}
