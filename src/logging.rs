pub fn init() {
    let _ = env_logger::builder()
        .format_timestamp_secs()
        .format_level(true)
        .try_init();
}
