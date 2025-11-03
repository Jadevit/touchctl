mod actions;
mod cli;
mod config;
mod gestures;
mod input;
mod ipc;
mod logging;
mod tracker;

fn main() -> anyhow::Result<()> {
    logging::init();
    cli::run()
}
