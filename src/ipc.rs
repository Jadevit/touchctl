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
use crate::config::{DaemonConfigState, Profile};
use crate::gestures::{Gesture, GestureDetector};
use crate::input;
use crate::tracker::{FrameSummary, Tracker};

// ---------------- runtime paths ----------------

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

// ---------------- daemon ----------------

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

    // Gesture pipeline channels
    let (tx_req, rx_req) = std::sync::mpsc::channel::<IpcMsg>();
    let (tx_evt, rx_evt) = std::sync::mpsc::channel::<DaemonEvent>();

    // Start gesture pipeline thread
    let mut gesture_thread = GestureThread::start(state.cfg.profile.clone(), tx_evt.clone())?;

    // Accept loop
    listener.set_nonblocking(true)?;
    loop {
        // Accept IPC clients
        match listener.accept() {
            Ok((stream, _addr)) => {
                let tx = tx_req.clone();
                let tx_evt_clone = tx_evt.clone();
                let st_snapshot = state.clone_shallow();
                thread::spawn(move || {
                    if let Err(e) = handle_client(stream, st_snapshot, tx, tx_evt_clone) {
                        error!("ipc client error: {e}");
                    }
                });
            }
            Err(_e) => { /* no client this tick */ }
        }

        // Process internal events from gesture thread
        while let Ok(evt) = rx_evt.try_recv() {
            match evt {
                DaemonEvent::Log(s) => info!("[gesture] {s}"),
            }
        }

        // Handle requests that modify gesture thread config
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
                    // graceful shutdown
                    return Ok(());
                }
            }
        }

        thread::sleep(Duration::from_millis(5));
    }
}

// ---------------- client handler ----------------

fn handle_client(
    mut stream: UnixStream,
    mut st: DaemonState,
    tx_req: std::sync::mpsc::Sender<IpcMsg>,
    _tx_evt: std::sync::mpsc::Sender<DaemonEvent>,
) -> Result<()> {
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

// ---------------- daemon state ----------------

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

// ---------------- gesture thread ----------------

enum IpcMsg {
    Reload,
    UseProfile(String),
    Shutdown,
}

enum DaemonEvent {
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

// ---------------- pipeline: evdev → tracker → gestures → actions ----------------

fn run_pipeline(
    profile: std::sync::Arc<std::sync::Mutex<Profile>>,
    _tx_evt: std::sync::mpsc::Sender<DaemonEvent>,
) -> Result<()> {
    use evdev::{AbsoluteAxisCode, Device, EventType, SynchronizationCode};

    // pick devices
    let devices = input::discover_multitouch();
    if devices.is_empty() {
        warn!("no multitouch devices detected; pipeline idle");
        loop {
            thread::sleep(Duration::from_secs(1));
        }
    }

    // open all devices
    let mut devs: Vec<Device> = vec![];
    for d in devices {
        match Device::open(&d.path) {
            Ok(mut dev) => {
                let _ = dev.set_nonblocking(true);
                devs.push(dev);
            }
            Err(e) => warn!("failed to open {}: {e}", d.path),
        }
    }
    if devs.is_empty() {
        warn!("failed to open all detected devices; pipeline idle");
        loop {
            thread::sleep(Duration::from_secs(1));
        }
    }

    // tracker/gesture state
    let mut tracker = Tracker::new();
    let th = { profile.lock().unwrap().thresholds.clone() };
    let mut detector = GestureDetector::new(th);
    let mut sink = UinputSink::new().unwrap_or_else(|_| UinputSink::noop());
    let mut prev_frame: Option<FrameSummary> = None;

    // hybrid-mode book-keeping
    let mut grabbed = false; // whether we've grabbed touch devices (>=2 fingers)
    let mut scroll_acc: f32 = 0.0;
    let mut cur_slot: i32 = 0;

    // NEW: desired grab state for this tick (set during processing, applied after)
    let mut want_grab_next: Option<bool>;

    loop {
        let mut any_event = false;
        want_grab_next = None; // reset each tick

        for dev in devs.iter_mut() {
            if let Ok(events) = dev.fetch_events() {
                for ev in events {
                    any_event = true;

                    if ev.event_type() == EventType::ABSOLUTE {
                        match ev.code() {
                            c if c == AbsoluteAxisCode::ABS_MT_SLOT.0 => {
                                cur_slot = ev.value();
                                tracker.on_slot(cur_slot);
                            }
                            c if c == AbsoluteAxisCode::ABS_MT_TRACKING_ID.0 => {
                                tracker.on_tracking_id(ev.value());
                            }
                            c if c == AbsoluteAxisCode::ABS_MT_POSITION_X.0 => {
                                tracker.on_pos_x(ev.value());
                            }
                            c if c == AbsoluteAxisCode::ABS_MT_POSITION_Y.0 => {
                                tracker.on_pos_y(ev.value());
                            }
                            _ => {}
                        }
                    } else if ev.event_type() == EventType::SYNCHRONIZATION {
                        if ev.code() == SynchronizationCode::SYN_REPORT.0 {
                            let frame = tracker.on_syn_report();

                            // Record the desired grab state (but don't touch `devs` here)
                            want_grab_next = Some(frame.active_count >= 2);

                            // Continuous 2-finger pan -> wheel accumulation (skip during pinch)
                            if let Some(prev) = &prev_frame {
                                if frame.active_count == 2 {
                                    let th = { profile.lock().unwrap().thresholds.clone() };
                                    let dspan = (frame.span - prev.span).abs();
                                    let pinch_gate = 0.6 * th.pinch_step;

                                    if dspan < pinch_gate {
                                        let dy = frame.centroid.1 - prev.centroid.1;

                                        // tune these two
                                        const STEP_NORM: f32 = 0.010; // smaller = more sensitive
                                        const GAIN: f32 = 1.0;

                                        scroll_acc += dy;
                                        let steps = ((scroll_acc / STEP_NORM) * GAIN) as i32;

                                        if steps.abs() >= 1 {
                                            if let Err(e) = sink.scroll_vertical(-steps) {
                                                error!("scroll emit failed: {e}");
                                            }
                                            scroll_acc -= (steps as f32) * STEP_NORM / GAIN;
                                        }
                                    }
                                } else {
                                    scroll_acc = 0.0;
                                }
                            }

                            if let Some(gesture) = detector.update(&frame, prev_frame.as_ref()) {
                                if let Err(e) = dispatch_gesture(&gesture, &profile, &mut sink) {
                                    error!("dispatch failed: {e}");
                                }
                            }
                            prev_frame = Some(frame);
                        }
                    }
                }
            }
        }

        // Apply grab/ungrab once per tick, *after* we finish iterating `devs`
        if let Some(want) = want_grab_next {
            if want && !grabbed {
                for d in devs.iter_mut() {
                    let _ = d.grab();
                }
                grabbed = true;
                info!("grabbed touch devices (>=2 fingers)");
            } else if !want && grabbed {
                for d in devs.iter_mut() {
                    let _ = d.ungrab();
                }
                grabbed = false;
                info!("released touch devices (<2 fingers)");
            }
        }

        if !any_event {
            thread::sleep(Duration::from_millis(4));
        }
    }
}

// Map Gesture → binding key → action
fn dispatch_gesture(
    g: &Gesture,
    profile_arc: &std::sync::Arc<std::sync::Mutex<Profile>>,
    sink: &mut UinputSink,
) -> Result<()> {
    let (key, action) = {
        let p = profile_arc.lock().unwrap();
        let key = match g {
            Gesture::TwoFingerTap => "two_finger.tap",
            Gesture::TwoFingerSwipeUp => "two_finger.swipe_up",
            Gesture::TwoFingerSwipeDown => "two_finger.swipe_down",
            Gesture::TwoFingerSwipeLeft => "two_finger.swipe_left",
            Gesture::TwoFingerSwipeRight => "two_finger.swipe_right",
            Gesture::PinchScaleIn => "pinch.scale_in",
            Gesture::PinchScaleOut => "pinch.scale_out",
            Gesture::ThreeFingerTap => "three_finger.tap",
        };
        let action = p.bindings.get(key).cloned().unwrap_or_default();
        (key.to_string(), action)
    };

    if action.is_empty() {
        return Ok(());
    }

    if action == "toggle" {
        // TODO: implement enable/disable
        return Ok(());
    }

    if let Some(rest) = action.strip_prefix("mouse:") {
        sink.click_mouse(rest.trim())?;
        return Ok(());
    }
    if let Some(rest) = action.strip_prefix("scroll:") {
        let parts: Vec<_> = rest.split('@').collect();
        let axis = parts.get(0).map(|s| s.trim()).unwrap_or("vertical");
        let steps_str = parts.get(1).copied().unwrap_or("+1");
        let steps: i32 = steps_str.parse().unwrap_or(1);
        if axis.eq_ignore_ascii_case("vertical") {
            sink.scroll_vertical(steps)?;
        } else {
            // horizontal could be added later
        }
        return Ok(());
    }
    if let Some(rest) = action.strip_prefix("key:") {
        sink.key_chord(rest.trim())?;
        return Ok(());
    }
    if action.starts_with("cmd:") {
        // guarded by allow_commands; implement later if desired
        return Ok(());
    }

    Err(anyhow!(
        "unknown action mapping for {} -> '{}'",
        key,
        action
    ))
}

// ---------------- client helper (restored) ----------------

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
