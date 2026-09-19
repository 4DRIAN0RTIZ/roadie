use roadie::{backend, tui};

fn main() -> anyhow::Result<()> {
    let backend = std::sync::Arc::new(backend::GhBackend::from_env());
    tui::run(backend)
}
