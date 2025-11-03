use anyhow::{Result, anyhow};
use directories::UserDirs;
use log::{info, warn};
use serde::{Deserialize, Deserializer};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Deserialize)]
pub struct Meta {
    pub name: Option<String>,
    #[serde(default)]
    pub allow_commands: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Thresholds {
    pub tap_ms: u64,
    pub hold_ms: u64,
    pub move_tol: f32,
    pub swipe_min_dist: f32,
    pub swipe_max_ms: u64,
    pub pinch_sensitivity: f32,
    pub pinch_step: f32,
    pub smooth_ema: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
    pub meta: Meta,
    pub thresholds: Thresholds,

    // 🔧 Accept nested/dotted tables and flatten them into "a.b" -> "value"
    #[serde(deserialize_with = "deserialize_bindings_flat")]
    pub bindings: HashMap<String, String>,
}

// --------- custom bindings deserializer (tolerant) ----------
fn deserialize_bindings_flat<'de, D>(
    de: D,
) -> std::result::Result<HashMap<String, String>, D::Error>
where
    D: Deserializer<'de>,
{
    let val = toml::Value::deserialize(de)?;
    let table = match val {
        toml::Value::Table(t) => t,
        other => {
            return Err(serde::de::Error::custom(format!(
                "bindings must be a table, got {:?}",
                other.type_str()
            )));
        }
    };

    let mut out = HashMap::new();
    flatten_table("", &table, &mut out).map_err(serde::de::Error::custom)?;
    Ok(out)
}

fn flatten_table(
    prefix: &str,
    table: &toml::value::Table,
    out: &mut HashMap<String, String>,
) -> std::result::Result<(), String> {
    for (k, v) in table {
        let key = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        match v {
            toml::Value::String(s) => {
                out.insert(key, s.clone());
            }
            toml::Value::Table(sub) => {
                flatten_table(&key, sub, out)?;
            }
            other => {
                return Err(format!(
                    "binding '{}' value must be a string, got {}",
                    key,
                    other.type_str()
                ));
            }
        }
    }
    Ok(())
}
// ------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DaemonConfigState {
    pub active_name: String,
    pub profile: Profile,
    pub config_dir: PathBuf,
    pub profiles_dir: PathBuf,
    pub active_ptr: PathBuf,
    pub detected_devices: Vec<String>,
}

fn config_dir() -> PathBuf {
    let home = UserDirs::new().unwrap().home_dir().to_path_buf();
    home.join(".config").join("touchctl")
}

fn profiles_dir() -> PathBuf {
    config_dir().join("profiles")
}

fn active_ptr_path() -> PathBuf {
    config_dir().join("active")
}

fn default_profile_text() -> &'static str {
    include_str!("../profiles/default.toml")
}

impl DaemonConfigState {
    pub fn load_or_install_default() -> Result<Self> {
        let cfgdir = config_dir();
        let profdir = profiles_dir();
        fs::create_dir_all(&profdir)?;

        let def_path = profdir.join("default.toml");
        if !def_path.exists() {
            fs::write(&def_path, default_profile_text())?;
            info!("installed default profile at {}", def_path.display());
        }

        let active_ptr = active_ptr_path();
        if !active_ptr.exists() {
            let mut f = fs::File::create(&active_ptr)?;
            f.write_all(b"default")?;
        }

        let active_name = fs::read_to_string(&active_ptr)?.trim().to_string();
        let profile = Self::load_profile(&active_name)?;
        let detected_devices = detect_multitouch_devices();

        Ok(Self {
            active_name,
            profile,
            config_dir: cfgdir,
            profiles_dir: profdir,
            active_ptr,
            detected_devices,
        })
    }

    pub fn reload(&mut self) -> Result<()> {
        self.profile = Self::load_profile(&self.active_name)?;
        Ok(())
    }

    pub fn set_active(&mut self, name: &str) -> Result<()> {
        let p = self.profiles_dir.join(format!("{name}.toml"));
        if !p.exists() {
            return Err(anyhow!("profile not found: {}", p.display()));
        }
        fs::write(&self.active_ptr, name.as_bytes())?;
        self.active_name = name.to_string();
        self.reload()?;
        Ok(())
    }

    pub fn list_profiles(&self) -> Vec<String> {
        let mut v = Vec::new();
        if let Ok(rd) = fs::read_dir(&self.profiles_dir) {
            for e in rd.flatten() {
                if let Some(ext) = e.path().extension() {
                    if ext == "toml" {
                        if let Some(stem) = e.path().file_stem().and_then(|s| s.to_str()) {
                            v.push(stem.to_string());
                        }
                    }
                }
            }
        }
        v.sort();
        v
    }

    fn load_profile(name: &str) -> Result<Profile> {
        let path = profiles_dir().join(format!("{name}.toml"));
        let txt = fs::read_to_string(&path)
            .map_err(|e| anyhow!("failed to read {}: {e}", path.display()))?;
        let profile: Profile =
            toml::from_str(&txt).map_err(|e| anyhow!("failed to parse {}: {e}", path.display()))?;
        validate_profile(&profile)?;
        Ok(profile)
    }

    pub fn doctor_report(&self) -> serde_json::Value {
        let uinput_ok = Path::new("/dev/uinput").exists();
        let in_input_group = check_in_input_group();
        serde_json::json!({
            "uinput_present": uinput_ok,
            "input_group_member": in_input_group,
            "profiles_dir": self.profiles_dir,
            "active_profile": self.active_name,
            "devices": self.detected_devices,
            "hints": {
                "udev_rule": "/etc/udev/rules.d/80-uinput.rules",
                "add_user_to_input_group": "sudo usermod -aG input $USER && newgrp input"
            }
        })
    }
}

fn validate_profile(p: &Profile) -> Result<()> {
    if p.thresholds.tap_ms == 0 || p.thresholds.hold_ms == 0 {
        return Err(anyhow!("thresholds must be positive durations"));
    }
    if !(0.0..1.0).contains(&p.thresholds.move_tol) {
        return Err(anyhow!(
            "thresholds.move_tol must be in (0,1) normalized units"
        ));
    }

    for (k, v) in &p.bindings {
        if k.trim().is_empty() {
            return Err(anyhow!("empty binding key"));
        }
        if v.trim().is_empty() {
            return Err(anyhow!("binding '{}' has empty action", k));
        }

        let ok = v.starts_with("mouse:")
            || v.starts_with("scroll:")
            || v.starts_with("key:")
            || v == "toggle"
            || v.starts_with("cmd:");
        if !ok {
            return Err(anyhow!("binding '{}' has invalid action '{}'", k, v));
        }
        if v.starts_with("cmd:") && !p.meta.allow_commands {
            return Err(anyhow!(
                "binding '{}' uses cmd: but allow_commands=false",
                k
            ));
        }
    }
    Ok(())
}

fn detect_multitouch_devices() -> Vec<String> {
    use evdev::{AbsoluteAxisCode, Device, EventType};
    let mut out = vec![];
    if let Ok(rd) = fs::read_dir("/dev/input") {
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
                        let name = dev.name().unwrap_or("unknown").to_string();
                        out.push(format!("{} ({})", name, p.display()));
                    }
                }
            }
        }
    }
    out
}

fn check_in_input_group() -> bool {
    if let Ok(s) = fs::read_to_string("/etc/group") {
        let user = whoami::username();
        for line in s.lines() {
            if line.starts_with("input:") || line.starts_with("input:x:") {
                if line
                    .split(':')
                    .nth(3)
                    .unwrap_or("")
                    .split(',')
                    .any(|u| u == user)
                {
                    return true;
                }
            }
        }
    }
    false
}
