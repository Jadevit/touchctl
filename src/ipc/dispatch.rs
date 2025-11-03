use crate::actions::UinputSink;
use crate::config::Profile;
use crate::gestures::Gesture;
use anyhow::{Result, anyhow};
use std::sync::{Arc, Mutex};

pub fn dispatch_gesture(
    g: &Gesture,
    profile_arc: &Arc<Mutex<Profile>>,
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
        }
        return Ok(());
    }
    if let Some(rest) = action.strip_prefix("key:") {
        sink.key_chord(rest.trim())?;
        return Ok(());
    }
    if action.starts_with("cmd:") {
        // gated elsewhere; implement later
        return Ok(());
    }

    Err(anyhow!(
        "unknown action mapping for {} -> '{}'",
        key,
        action
    ))
}
