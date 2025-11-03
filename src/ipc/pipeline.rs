use anyhow::Result;
use log::{error, info, warn};
use std::{thread, time::Duration};

use evdev::{AbsoluteAxisCode, Device, EventType, SynchronizationCode};

use super::server::DaemonEvent;
use crate::actions::UinputSink;
use crate::config::Profile;
use crate::gestures::{Gesture, GestureDetector};
use crate::input;
use crate::tracker::{FrameSummary, Tracker};
use std::sync::{Arc, Mutex};

pub fn run_pipeline(
    profile: Arc<Mutex<Profile>>,
    _tx_evt: std::sync::mpsc::Sender<DaemonEvent>,
) -> Result<()> {
    let devices = input::discover_multitouch();
    if devices.is_empty() {
        warn!("no multitouch devices detected; pipeline idle");
        loop {
            thread::sleep(Duration::from_secs(1));
        }
    }

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

    let mut tracker = Tracker::new();
    let th = { profile.lock().unwrap().thresholds.clone() };
    let mut detector = GestureDetector::new(th);
    let mut sink = UinputSink::new().unwrap_or_else(|_| UinputSink::noop());
    let mut prev_frame: Option<FrameSummary> = None;

    let mut grabbed = false;
    let mut scroll_acc: f32 = 0.0;
    let mut cur_slot: i32 = 0;
    let mut want_grab_next: Option<bool>;

    loop {
        let mut any_event = false;
        want_grab_next = None;

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

                            // schedule grab/ungrab after loop
                            want_grab_next = Some(frame.active_count >= 2);

                            // continuous scroll
                            if let Some(prev) = &prev_frame {
                                if frame.active_count == 2 {
                                    let th = { profile.lock().unwrap().thresholds.clone() };
                                    let dspan = (frame.span - prev.span).abs();
                                    let pinch_gate = 0.6 * th.pinch_step;

                                    if dspan < pinch_gate {
                                        let dy = frame.centroid.1 - prev.centroid.1;
                                        const STEP_NORM: f32 = 0.010;
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
                                if let Err(e) =
                                    super::dispatch::dispatch_gesture(&gesture, &profile, &mut sink)
                                {
                                    error!("dispatch failed: {e}");
                                }
                            }
                            prev_frame = Some(frame);
                        }
                    }
                }
            }
        }

        // apply grab/ungrab once
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
