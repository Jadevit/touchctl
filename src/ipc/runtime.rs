use directories::UserDirs;
use std::{fs, path::PathBuf};

pub fn runtime_dir() -> PathBuf {
    let home = UserDirs::new().unwrap().home_dir().to_path_buf();
    let dir = home.join(".local").join("run");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn socket_path() -> PathBuf {
    runtime_dir().join("touchctl.sock")
}
