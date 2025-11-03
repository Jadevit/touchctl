use anyhow::{Result, anyhow};
use directories::UserDirs;
use log::{error, info, warn};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    thread,
    time::Duration,
};

use crate::actions::UinputSink;
use crate::config::{self, DaemonConfigState};

fn runtime_dir() -> PathBuf {
    // ~/.local/run
    let home = UserDirs::new().unwrap().home_dir().to_path_buf();
    let dir = home.join(".local").join("run");
    let _ = fs::create_dir_all(&dir);
    dir
}

fn socket_path() -> PathBuf {
    runtime_dir().join("touchctl.sock")
}

pub fn run_daemon() -> Result<()> {
    // Prepare socket
    let sock = socket_path();
    if sock.exists() {
        let _ = fs::remove_file(&sock);
    }
    let listener = UnixListener::bind(&sock)?;
    info!("daemon: listening on {}", sock.display());

    // Load config & uinput sink
    let mut state = DaemonState::new()?;
    info!("daemon: active profile '{}'", state.cfg.active_name);

    // Accept loop
    listener.set_nonblocking(true)?;
    loop {
        match listener.accept() {
            Ok((stream, _addr)) => {
                let mut st = state.clone_shallow(); // share basic fields; we keep it simple
                thread::spawn(move || {
                    if let Err(e) = handle_client(stream, &mut st) {
                        error!("ipc client error: {e}");
                    }
                });
            }
            Err(_e) => {
                // tick— placeholder for gesture/input loop later
                thread::sleep(Duration::from_millis(10));
            }
        }
        // TODO: here later we’ll poll input devices & drive recognizers.
    }
}

fn handle_client(mut stream: UnixStream, st: &mut DaemonState) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    line.clear();
    reader.read_line(&mut line)?;
    if line.trim().is_empty() {
        return Ok(());
    }
    let req: serde_json::Value = serde_json::from_str(&line)?;
    let op = req.get("op").and_then(|v| v.as_str()).unwrap_or("");

    let resp = match op {
        "status" => {
            serde_json::json!({
                "ok": true,
                "data": {
                    "enabled": st.enabled,
                    "active_profile": st.cfg.active_name,
                    "socket": socket_path(),
                    "devices": st.cfg.detected_devices,
                }
            })
        }
        "reload" => match st.cfg.reload() {
            Ok(_) => {
                serde_json::json!({"ok": true, "data": {"active_profile": st.cfg.active_name}})
            }
            Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
        },
        "use" => {
            let name = req.get("profile").and_then(|v| v.as_str()).unwrap_or("");
            match st.cfg.set_active(name) {
                Ok(_) => {
                    serde_json::json!({"ok": true, "data": {"active_profile": st.cfg.active_name}})
                }
                Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
            }
        }
        "list" => {
            let list = st.cfg.list_profiles();
            serde_json::json!({"ok": true, "data": {"profiles": list, "active": st.cfg.active_name}})
        }
        "doctor" => {
            let report = st.cfg.doctor_report();
            serde_json::json!({"ok": true, "data": report})
        }
        "shutdown" => {
            // Brutal but fine for now: exit after responding
            let _ = write!(
                stream,
                "{}\n",
                serde_json::json!({"ok": true, "data": "shutting down"})
            );
            std::process::exit(0);
        }
        _ => serde_json::json!({"ok": false, "error": format!("unknown op: {op}")}),
    };

    write!(stream, "{}\n", resp)?;
    Ok(())
}

// Daemon in-memory state
struct DaemonState {
    pub enabled: bool,
    pub cfg: DaemonConfigState,
    #[allow(dead_code)]
    pub sink: UinputSink,
}

impl DaemonState {
    fn new() -> Result<Self> {
        let cfg = DaemonConfigState::load_or_install_default()?;
        let sink = UinputSink::new()?;
        Ok(Self {
            enabled: true,
            cfg,
            sink,
        })
    }
    fn clone_shallow(&self) -> Self {
        Self {
            enabled: self.enabled,
            cfg: self.cfg.clone(),
            sink: UinputSink::new().unwrap_or_else(|_| UinputSink::noop()),
        }
    }
}

// Client helper
pub fn client_request(req: serde_json::Value) -> Result<serde_json::Value> {
    let sock = socket_path();
    if !sock.exists() {
        return Err(anyhow!(
            "touchctl daemon is not running (socket missing at {})",
            sock.display()
        ));
    }
    let mut stream = UnixStream::connect(sock)?;
    let line = serde_json::to_string(&req)? + "\n";
    stream.write_all(line.as_bytes())?;
    let mut reader = BufReader::new(stream);
    let mut resp = String::new();
    reader.read_line(&mut resp)?;
    let v: serde_json::Value = serde_json::from_str(&resp)?;
    Ok(v)
}
