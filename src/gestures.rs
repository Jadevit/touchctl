//! Gesture recognition (to be implemented next).
//! Will consume frames from Tracker and emit gesture enum.

#[derive(Debug, Clone)]
pub enum Gesture {
    TwoFingerTap,
    TwoFingerSwipeUp,
    TwoFingerSwipeDown,
    TwoFingerSwipeLeft,
    TwoFingerSwipeRight,
    PinchScaleIn,
    PinchScaleOut,
    ThreeFingerTap,
}
