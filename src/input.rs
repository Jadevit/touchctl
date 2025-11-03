//! Input device discovery & event stream (evdev 0.13.2 compatible)

use evdev::{AbsoluteAxisCode, Device, EventType};

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub path: String,
    pub name: String,
}

pub fn discover_multitouch() -> Vec<DeviceInfo> {
    let mut out = vec![];
    if let Ok(rd) = std::fs::read_dir("/dev/input") {
        for e in rd.flatten() {
            let p = e.path();
            if p.file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.starts_with("event"))
                .unwrap_or(false)
            {
                if let Ok(dev) = Device::open(&p) {
                    let has_abs = dev.supported_events().contains(EventType::ABSOLUTE);
                    let axes = dev.supported_absolute_axes();
                    let has_mt = axes.map_or(false, |a| {
                        a.contains(AbsoluteAxisCode::ABS_MT_SLOT)
                            && a.contains(AbsoluteAxisCode::ABS_MT_POSITION_X)
                            && a.contains(AbsoluteAxisCode::ABS_MT_POSITION_Y)
                    });
                    if has_abs && has_mt {
                        out.push(DeviceInfo {
                            path: p.display().to_string(),
                            name: dev.name().unwrap_or("unknown").to_string(),
                        });
                    }
                }
            }
        }
    }
    out
}
