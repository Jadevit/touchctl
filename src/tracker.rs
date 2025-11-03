//! Per-slot touch tracking and frame snapshots.

use std::time::Instant;

#[derive(Debug, Clone, Default)]
pub struct SlotState {
    pub tracking_id: i32, // -1 = inactive
    pub x_norm: f32,
    pub y_norm: f32,
    pub t_first_ms: u128,
    pub t_last_ms: u128,
    pub moved_norm: f32,
    // internal
    last_x_norm: f32,
    last_y_norm: f32,
    seen_x: bool, // ⬅️ NEW: baseline flags
    seen_y: bool, // ⬅️ NEW
    pub(crate) active: bool,
}

#[derive(Debug, Clone)]
pub struct SlotSnapshot {
    pub tracking_id: i32,
    pub x_norm: f32,
    pub y_norm: f32,
    pub moved_norm: f32,
    pub age_ms: u64,
}

#[derive(Debug, Clone)]
pub struct FrameSummary {
    pub timestamp_ms: u128,
    pub active_count: usize,
    pub centroid: (f32, f32),
    pub span: f32,
    pub slots: Vec<SlotSnapshot>,
}

#[derive(Debug)]
pub struct Tracker {
    slots: Vec<SlotState>,
    cur_slot: i32,
    // normalization
    x_min: i32,
    x_max: i32,
    y_min: i32,
    y_max: i32,
    // time
    start_instant: Instant,
    pub active_count: usize,
    pub centroid: (f32, f32),
    pub span: f32,
}

impl Default for Tracker {
    fn default() -> Self {
        Self::new()
    }
}

impl Tracker {
    pub fn new() -> Self {
        Self {
            slots: vec![SlotState::default(); 10],
            cur_slot: 0,
            x_min: 0,
            x_max: 4096,
            y_min: 0,
            y_max: 4096,
            start_instant: Instant::now(),
            active_count: 0,
            centroid: (0.0, 0.0),
            span: 0.0,
        }
    }

    pub fn set_norm_ranges(&mut self, x_min: i32, x_max: i32, y_min: i32, y_max: i32) {
        self.x_min = x_min;
        self.x_max = x_max.max(x_min + 1);
        self.y_min = y_min;
        self.y_max = y_max.max(y_min + 1);
    }

    fn now_ms(&self) -> u128 {
        self.start_instant.elapsed().as_millis()
    }

    pub fn on_slot(&mut self, slot: i32) {
        self.cur_slot = slot.clamp(0, (self.slots.len() as i32) - 1);
    }

    pub fn on_tracking_id(&mut self, tracking_id: i32) {
        let now = self.now_ms();
        let s = &mut self.slots[self.cur_slot as usize];
        if tracking_id < 0 {
            // release
            s.tracking_id = -1;
            s.active = false;
            s.t_last_ms = now;
        } else {
            // new touch → reset movement and clear baselines
            *s = SlotState {
                tracking_id,
                x_norm: s.x_norm,
                y_norm: s.y_norm,
                t_first_ms: now,
                t_last_ms: now,
                moved_norm: 0.0,
                last_x_norm: s.x_norm,
                last_y_norm: s.y_norm,
                seen_x: false,
                seen_y: false,
                active: true,
            };
        }
    }

    pub fn on_pos_x(&mut self, raw: i32) {
        let now = self.now_ms();
        let x_min = self.x_min;
        let x_max = self.x_max;
        let nx = ((raw - x_min) as f32 / (x_max - x_min) as f32).clamp(0.0, 1.0);

        let s = &mut self.slots[self.cur_slot as usize];
        if s.seen_x && s.seen_y {
            let dx = nx - s.last_x_norm;
            s.moved_norm += dx.abs();
        } else {
            // establish baseline, don't count movement yet
            s.last_x_norm = nx;
            s.seen_x = true;
        }
        s.x_norm = nx;
        s.t_last_ms = now;
    }

    pub fn on_pos_y(&mut self, raw: i32) {
        let now = self.now_ms();
        let y_min = self.y_min;
        let y_max = self.y_max;
        let ny = ((raw - y_min) as f32 / (y_max - y_min) as f32).clamp(0.0, 1.0);

        let s = &mut self.slots[self.cur_slot as usize];
        if s.seen_x && s.seen_y {
            let dy = ny - s.last_y_norm;
            s.moved_norm += dy.abs();
        } else {
            s.last_y_norm = ny;
            s.seen_y = true;
        }
        s.y_norm = ny;
        s.t_last_ms = now;
    }

    pub fn on_syn_report(&mut self) -> FrameSummary {
        // active slots
        let act: Vec<&SlotState> = self
            .slots
            .iter()
            .filter(|s| s.active && s.tracking_id >= 0)
            .collect();
        self.active_count = act.len();

        // centroid
        if self.active_count > 0 {
            let sumx: f32 = act.iter().map(|s| s.x_norm).sum();
            let sumy: f32 = act.iter().map(|s| s.y_norm).sum();
            self.centroid = (
                sumx / self.active_count as f32,
                sumy / self.active_count as f32,
            );
        } else {
            self.centroid = (0.5, 0.5);
        }

        // span = avg distance from centroid
        if self.active_count > 0 {
            let mut acc = 0.0f32;
            for s in &act {
                let dx = s.x_norm - self.centroid.0;
                let dy = s.y_norm - self.centroid.1;
                acc += (dx * dx + dy * dy).sqrt();
            }
            self.span = acc / self.active_count as f32;
        } else {
            self.span = 0.0;
        }

        let now = self.now_ms();
        let slots = act
            .into_iter()
            .map(|s| SlotSnapshot {
                tracking_id: s.tracking_id,
                x_norm: s.x_norm,
                y_norm: s.y_norm,
                moved_norm: s.moved_norm,
                age_ms: (now - s.t_first_ms) as u64,
            })
            .collect();

        FrameSummary {
            timestamp_ms: now,
            active_count: self.active_count,
            centroid: self.centroid,
            span: self.span,
            slots,
        }
    }
}
