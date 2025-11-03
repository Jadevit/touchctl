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
    println!("touchctl daemon (stub). Use `touchctl --help` for commands.");
    Ok(())
}
