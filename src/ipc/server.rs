use anyhow::Result;
use log::{error, info};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    thread,
    time::Duration,
};

use super::pipeline::run_pipeline;
use super::runtime::socket_path;
use crate::config::{DaemonConfigState, Profile};

pub fn run_daemon() -> Result<()> {
    // socket
    let sock = socket_path();
    if sock.exists() {
        let _ = std::fs::remove_file(&sock);
    }
    let listener = UnixListener::bind(&sock)?;
    info!("daemon: listening on {}", sock.display());

    // state
    let mut state = DaemonState::new()?;
    info!("daemon: active profile '{}'", state.cfg.active_name);

    // channels
    let (tx_req, rx_req) = std::sync::mpsc::channel::<IpcMsg>();
    let (tx_evt, rx_evt) = std::sync::mpsc::channel::<DaemonEvent>();

    // gesture thread
    let mut gesture_thread = GestureThread::start(state.cfg.profile.clone(), tx_evt.clone())?;

    // accept loop
    listener.set_nonblocking(true)?;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                let tx = tx_req.clone();
                let st_snapshot = state.clone_shallow();
                let tx_evt_clone = tx_evt.clone();
                thread::spawn(move || {
                    if let Err(e) = handle_client(stream, st_snapshot, tx, tx_evt_clone) {
                        error!("ipc client error: {e}");
                    }
                });
            }
            Err(_) => {}
        }

        while let Ok(evt) = rx_evt.try_recv() {
            if let DaemonEvent::Log(s) = evt {
                info!("[gesture] {s}");
            }
        }

        while let Ok(msg) = rx_req.try_recv() {
            match msg {
                IpcMsg::Reload => {
                    if let Err(e) = state.cfg.reload() {
                        error!("reload failed: {e}");
                    } else {
                        let new_prof = state.cfg.profile.clone();
                        gesture_thread.update_profile(new_prof);
                        info!("profile reloaded");
                    }
                }
                IpcMsg::UseProfile(name) => {
                    if let Err(e) = state.cfg.set_active(&name) {
                        error!("use profile failed: {e}");
                    } else {
                        let new_prof = state.cfg.profile.clone();
                        gesture_thread.update_profile(new_prof);
                        info!("switched active profile to {}", state.cfg.active_name);
                    }
                }
                IpcMsg::Shutdown => {
                    return Ok(());
                }
            }
        }

        thread::sleep(Duration::from_millis(5));
    }
}

fn handle_client(
    mut stream: UnixStream,
    mut st: DaemonState,
    tx_req: std::sync::mpsc::Sender<IpcMsg>,
    _tx_evt: std::sync::mpsc::Sender<DaemonEvent>,
) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    if line.trim().is_empty() {
        return Ok(());
    }
    let req: serde_json::Value = serde_json::from_str(&line)?;
    let op = req.get("op").and_then(|v| v.as_str()).unwrap_or("");

    let resp = match op {
        "status" => serde_json::json!({"ok": true, "data": {
            "enabled": st.enabled,
            "active_profile": st.cfg.active_name,
            "socket": super::runtime::socket_path(),
            "devices": st.cfg.detected_devices,
        }}),
        "reload" => {
            let _ = tx_req.send(IpcMsg::Reload);
            serde_json::json!({"ok": true, "data": {"active_profile": st.cfg.active_name}})
        }
        "use" => {
            let name = req.get("profile").and_then(|v| v.as_str()).unwrap_or("");
            let _ = tx_req.send(IpcMsg::UseProfile(name.to_string()));
            serde_json::json!({"ok": true, "data": {"active_profile": name}})
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
            let _ = tx_req.send(IpcMsg::Shutdown);
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

struct DaemonState {
    pub enabled: bool,
    pub cfg: DaemonConfigState,
}

impl DaemonState {
    fn new() -> Result<Self> {
        let cfg = DaemonConfigState::load_or_install_default()?;
        Ok(Self { enabled: true, cfg })
    }
    fn clone_shallow(&self) -> Self {
        Self {
            enabled: self.enabled,
            cfg: self.cfg.clone(),
        }
    }
}

enum IpcMsg {
    Reload,
    UseProfile(String),
    Shutdown,
}
pub enum DaemonEvent {
    Log(String),
}

struct GestureThread {
    profile: std::sync::Arc<std::sync::Mutex<Profile>>,
    _thread: thread::JoinHandle<()>,
}

impl GestureThread {
    fn start(profile: Profile, tx_evt: std::sync::mpsc::Sender<DaemonEvent>) -> Result<Self> {
        let profile_arc = std::sync::Arc::new(std::sync::Mutex::new(profile));
        let prof_clone = profile_arc.clone();
        let handle = thread::spawn(move || {
            if let Err(e) = run_pipeline(prof_clone, tx_evt) {
                error!("gesture pipeline failed: {e}");
            }
        });
        Ok(Self {
            profile: profile_arc,
            _thread: handle,
        })
    }
    fn update_profile(&mut self, new_profile: Profile) {
        if let Ok(mut p) = self.profile.lock() {
            *p = new_profile;
        }
    }
}

// client helper
pub fn client_request(req: serde_json::Value) -> Result<serde_json::Value> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    let sock = super::runtime::socket_path();
    if !sock.exists() {
        return Err(anyhow::anyhow!(
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
