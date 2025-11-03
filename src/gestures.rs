use crate::config::Thresholds;
use crate::tracker::FrameSummary;

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

#[derive(Debug, Default, Clone)]
struct TwoFingerState {
    start_time_ms: u128,
    start_centroid: (f32, f32),
    start_span: f32,
    armed: bool,
    classified: bool,
}

#[derive(Debug)]
pub struct GestureDetector {
    th: Thresholds,
    two: TwoFingerState,
    three_start_ms: Option<u128>,
    last_two_frame: Option<FrameSummary>, // ⬅️ NEW: stash the last frame with exactly two touches
}

impl GestureDetector {
    pub fn new(th: Thresholds) -> Self {
        Self {
            th,
            two: TwoFingerState::default(),
            three_start_ms: None,
            last_two_frame: None,
        }
    }

    pub fn update(
        &mut self,
        frame: &FrameSummary,
        _prev: Option<&FrameSummary>, // no longer relied on for taps
    ) -> Option<Gesture> {
        let a = frame.active_count;

        // --- track the last exact-2 frame for stable tap detection ---
        if a == 2 {
            // (re)arm on entering/staying in 2-finger state
            if !self.two.armed {
                self.two.armed = true;
                self.two.classified = false;
                self.two.start_time_ms = frame.timestamp_ms;
                self.two.start_centroid = frame.centroid;
                self.two.start_span = frame.span;
            }
            // always keep the freshest two-finger snapshot
            self.last_two_frame = Some(frame.clone());

            // while still in 2-finger state, allow swipe / pinch classification
            if !self.two.classified {
                // swipe?
                let dt = (frame.timestamp_ms - self.two.start_time_ms) as u64;
                if dt <= self.th.swipe_max_ms {
                    let dx = frame.centroid.0 - self.two.start_centroid.0;
                    let dy = frame.centroid.1 - self.two.start_centroid.1;
                    let ax = dx.abs();
                    let ay = dy.abs();
                    if ax >= ay && ax >= self.th.swipe_min_dist {
                        self.two.classified = true;
                        return Some(if dx > 0.0 {
                            Gesture::TwoFingerSwipeRight
                        } else {
                            Gesture::TwoFingerSwipeLeft
                        });
                    } else if ay > ax && ay >= self.th.swipe_min_dist {
                        self.two.classified = true;
                        return Some(if dy > 0.0 {
                            Gesture::TwoFingerSwipeDown
                        } else {
                            Gesture::TwoFingerSwipeUp
                        });
                    }
                }
                // pinch?
                let dspan = frame.span - self.two.start_span;
                if dspan.abs() >= self.th.pinch_step {
                    self.two.classified = true;
                    return Some(if dspan < 0.0 {
                        Gesture::PinchScaleIn
                    } else {
                        Gesture::PinchScaleOut
                    });
                }
            }
        } else {
            // leaving 2-finger state
            if self.two.armed {
                if !self.two.classified {
                    // evaluate TAP using the *saved* last two-finger frame
                    if let Some(last2) = &self.last_two_frame {
                        let tap_ok = last2.slots.len() == 2
                            && last2.slots.iter().all(|s| {
                                s.age_ms <= self.th.tap_ms && s.moved_norm <= self.th.move_tol
                            });
                        if tap_ok {
                            self.two = TwoFingerState::default();
                            self.last_two_frame = None;
                            return Some(Gesture::TwoFingerTap);
                        }
                    }
                }
                // reset state if we didn’t classify
                self.two = TwoFingerState::default();
                self.last_two_frame = None;
            }
        }

        // --- three-finger tap (unchanged logic, but benefits from the same robustness) ---
        if a == 3 {
            if self.three_start_ms.is_none() {
                self.three_start_ms = Some(frame.timestamp_ms);
            }
        } else if a == 0 {
            if let Some(t0) = self.three_start_ms.take() {
                let dt = (frame.timestamp_ms - t0) as u64;
                // we can afford to be lenient here; noises are smaller with three down
                if dt <= self.th.tap_ms {
                    return Some(Gesture::ThreeFingerTap);
                }
            }
        }

        None
    }
}
