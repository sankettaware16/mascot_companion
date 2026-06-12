//! Screen-activity tracking. The companion only "sees" what any unprivileged
//! app can see without spying: whether the mouse is moving. That's enough to
//! know if you're actively working, and it keeps us honest about privacy — no
//! keylogging, no window titles, nothing leaves the machine.

use glam::Vec2;

pub struct Activity {
    last_cursor: Option<Vec2>,
    /// Seconds since the cursor last moved meaningfully.
    pub idle_seconds: f32,
    /// Seconds of (roughly) continuous activity in the current work stint.
    pub active_seconds: f32,
    /// Smoothed cursor speed, px/s — lets the pet tell "user waving the mouse
    /// around" from "cursor parked".
    pub cursor_speed: f32,
    /// Set true on the single frame a long stint ends (user stepped away).
    pub break_started: bool,
}

impl Activity {
    pub fn new() -> Self {
        Self {
            last_cursor: None,
            idle_seconds: 999.0,
            active_seconds: 0.0,
            cursor_speed: 0.0,
            break_started: false,
        }
    }

    pub fn update(&mut self, dt: f32, cursor: Option<Vec2>) {
        self.break_started = false;
        let (moved, inst_speed) = match (self.last_cursor, cursor) {
            (Some(prev), Some(now)) => {
                let d = (now - prev).length();
                (d > 2.0, d / dt.max(1e-3))
            }
            _ => (false, 0.0),
        };
        self.last_cursor = cursor;
        self.cursor_speed += (inst_speed - self.cursor_speed) * (dt * 6.0).min(1.0);

        if moved {
            if self.idle_seconds > 90.0 && self.active_seconds > 60.0 {
                // Returned from a long-enough break: a fresh stint begins.
                self.active_seconds = 0.0;
            }
            self.idle_seconds = 0.0;
        } else {
            self.idle_seconds += dt;
            // Crossing the "stepped away" line exactly once.
            let was_below = self.idle_seconds - dt < 90.0;
            if was_below && self.idle_seconds >= 90.0 {
                self.break_started = true;
            }
        }
        // Stint time accrues on the wall clock while *recently* active — you
        // are still "working" while typing or reading with the mouse parked
        // (we only see the cursor, never the keyboard). Counting only
        // mouse-motion frames made eye/break reminders nearly unreachable.
        if self.idle_seconds < 30.0 {
            self.active_seconds += dt;
        }
    }

    /// Is the user actively working right now?
    pub fn is_active(&self) -> bool {
        self.idle_seconds < 4.0
    }

    pub fn active_minutes(&self) -> f32 {
        self.active_seconds / 60.0
    }
}
