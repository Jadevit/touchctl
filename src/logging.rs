// src/logging.rs
pub fn init() {
    let _ = env_logger::builder().format_timestamp_secs().try_init();
}
