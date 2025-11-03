//! Per-slot touch tracking data structures (to be implemented next).

#[derive(Debug, Clone, Default)]
pub struct SlotState {
    pub tracking_id: i32, // -1 = inactive
    pub x_norm: f32,
    pub y_norm: f32,
    pub t_first_ms: u128,
    pub t_last_ms: u128,
    pub moved_norm: f32,
}

#[derive(Debug, Default)]
pub struct Tracker {
    pub slots: Vec<SlotState>,
    pub active_count: usize,
    pub centroid: (f32, f32),
    pub span: f32,
}

impl Tracker {
    pub fn new() -> Self {
        Self {
            slots: vec![SlotState::default(); 10],
            ..Default::default()
        }
    }
}
