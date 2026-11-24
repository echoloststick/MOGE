//! Desktop runner for quick iteration: `cargo run --release` from this folder.
fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match winit::event_loop::EventLoop::new() {
        Ok(el) => moge_app::run(el),
        Err(e) => eprintln!("Failed to create the event loop: {e}"),
    }
}
