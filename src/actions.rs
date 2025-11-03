use anyhow::{Result, anyhow};
use log::{info, warn};

pub struct UinputSink {
    enabled: bool,
    #[allow(dead_code)]
    linux: Option<Box<LinuxUinput>>,
}

impl UinputSink {
    pub fn new() -> Result<Self> {
        #[cfg(target_os = "linux")]
        {
            let dev = LinuxUinput::create()?;
            return Ok(Self {
                enabled: true,
                linux: Some(Box::new(dev)),
            });
        }
        #[allow(unreachable_code)]
        {
            warn!("uinput not available; running in NO-OP mode");
            Ok(Self {
                enabled: true,
                linux: None,
            })
        }
    }

    pub fn noop() -> Self {
        Self {
            enabled: true,
            linux: None,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
    pub fn set_enabled(&mut self, en: bool) {
        self.enabled = en;
    }

    pub fn click_right(&mut self) -> Result<()> {
        self.click_mouse("right")
    }

    pub fn scroll_vertical(&mut self, steps: i32) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        #[cfg(target_os = "linux")]
        if let Some(dev) = self.linux.as_mut() {
            dev.scroll_vertical(steps)?;
        }
        Ok(())
    }

    pub fn click_mouse(&mut self, which: &str) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        #[cfg(target_os = "linux")]
        if let Some(dev) = self.linux.as_mut() {
            match which.to_ascii_lowercase().as_str() {
                "left" => dev.click_left()?,
                "right" => dev.click_right()?,
                "middle" => dev.click_middle()?,
                other => return Err(anyhow!("unknown mouse button: {other}")),
            }
        }
        Ok(())
    }

    /// Send a chord like "CTRL+EQUAL" or single "TAB"
    pub fn key_chord(&mut self, chord: &str) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        #[cfg(target_os = "linux")]
        if let Some(dev) = self.linux.as_mut() {
            let parts: Vec<_> = chord
                .split('+')
                .map(|s| s.trim().to_ascii_uppercase())
                .collect();
            let mut keys = Vec::with_capacity(parts.len());
            for p in parts {
                keys.push(map_key(&p)?);
            }
            // press in order
            for k in &keys {
                dev.key_send(*k, 1)?;
            }
            dev.sync()?;
            // release in reverse
            for k in keys.iter().rev() {
                dev.key_send(*k, 0)?;
            }
            dev.sync()?;
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn map_key(tok: &str) -> Result<uinput::event::keyboard::Key> {
    use uinput::event::keyboard::Key as K;
    let k = match tok {
        "CTRL" | "CONTROL" => K::LeftControl,
        "ALT" => K::LeftAlt,
        "SHIFT" => K::LeftShift,
        "SUPER" | "META" | "WIN" => K::LeftMeta,
        "TAB" => K::Tab,
        "MINUS" | "-" => K::Minus,
        "EQUAL" | "=" => K::Equal,
        // you can add more here later (A..Z, digits, arrows, etc.)
        other => return Err(anyhow!("unsupported key token: {other}")),
    };
    Ok(k)
}

#[cfg(target_os = "linux")]
struct LinuxUinput {
    dev: uinput::device::Device,
}

#[cfg(target_os = "linux")]
impl LinuxUinput {
    fn create() -> Result<Self> {
        use uinput::event::{controller::Mouse, keyboard, relative};

        let dev = uinput::default()?
            .name("Touchctl Virtual Input")?
            // relative axes + wheel
            .event(relative::Position::X)?
            .event(relative::Position::Y)?
            .event(relative::Wheel::Vertical)?
            .event(relative::Wheel::Horizontal)?
            // mouse buttons
            .event(Mouse::Left)?
            .event(Mouse::Right)?
            .event(Mouse::Middle)?
            // keys for our chords
            .event(keyboard::Key::LeftControl)?
            .event(keyboard::Key::LeftAlt)?
            .event(keyboard::Key::LeftShift)?
            .event(keyboard::Key::LeftMeta)?
            .event(keyboard::Key::Tab)?
            .event(keyboard::Key::Minus)?
            .event(keyboard::Key::Equal)?
            .create()?;

        info!("uinput: created virtual device");
        Ok(Self { dev })
    }

    fn sync(&mut self) -> Result<()> {
        self.dev.synchronize()?;
        Ok(())
    }

    fn key_send(&mut self, key: uinput::event::keyboard::Key, val: i32) -> Result<()> {
        self.dev.send(key, val)?;
        Ok(())
    }

    fn click_left(&mut self) -> Result<()> {
        use uinput::event::controller::Mouse;
        self.dev.send(Mouse::Left, 1)?;
        self.sync()?;
        self.dev.send(Mouse::Left, 0)?;
        self.sync()?;
        Ok(())
    }
    fn click_right(&mut self) -> Result<()> {
        use uinput::event::controller::Mouse;
        self.dev.send(Mouse::Right, 1)?;
        self.sync()?;
        self.dev.send(Mouse::Right, 0)?;
        self.sync()?;
        Ok(())
    }
    fn click_middle(&mut self) -> Result<()> {
        use uinput::event::controller::Mouse;
        self.dev.send(Mouse::Middle, 1)?;
        self.sync()?;
        self.dev.send(Mouse::Middle, 0)?;
        self.sync()?;
        Ok(())
    }
    fn scroll_vertical(&mut self, steps: i32) -> Result<()> {
        use uinput::event::relative::Wheel;
        self.dev.send(Wheel::Vertical, steps)?;
        self.sync()
    }
}
