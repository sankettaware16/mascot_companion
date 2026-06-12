//! The "world" the companion lives in: the union of all monitors expressed in
//! one global virtual-desktop coordinate space. This is what makes the
//! creature multi-monitor aware — it simply moves through global coordinates
//! and each monitor's window draws whatever part of it overlaps.

use glam::Vec2;

#[derive(Debug, Clone, Copy)]
pub struct Monitor {
    /// Top-left of the monitor in global coordinates.
    pub origin: Vec2,
    pub size: Vec2,
    /// Pixels reserved at the bottom for a taskbar/dock to stand on top of,
    /// instead of behind.
    pub bottom_inset: f32,
}

impl Monitor {
    pub fn rect_contains_x(&self, x: f32) -> bool {
        x >= self.origin.x && x < self.origin.x + self.size.x
    }
    pub fn floor(&self) -> f32 {
        self.origin.y + self.size.y - self.bottom_inset
    }
    pub fn center(&self) -> Vec2 {
        self.origin + self.size * 0.5
    }
}

/// A walkable horizontal segment — the visible top edge of an open window.
/// This is what lets the pet climb onto your browser or editor.
#[derive(Debug, Clone, Copy)]
pub struct Plat {
    pub x0: f32,
    pub x1: f32,
    pub y: f32,
}

#[derive(Debug, Clone)]
pub struct World {
    pub monitors: Vec<Monitor>,
    /// Window-top platforms, refreshed every ~0.6s by the X11 scout.
    pub platforms: Vec<Plat>,
}

impl World {
    /// The y of the floor under a given global x (bottom of the monitor the
    /// creature is over). Falls back to the lowest floor if x is between
    /// monitors so it never falls forever.
    pub fn ground_y(&self, x: f32) -> f32 {
        self.monitors
            .iter()
            .filter(|m| m.rect_contains_x(x))
            .map(|m| m.floor())
            .next()
            .unwrap_or_else(|| {
                self.monitors.iter().map(|m| m.floor()).fold(f32::MIN, f32::max)
            })
    }

    /// Horizontal extent available at a given global y (so the creature can
    /// roam across all monitors that exist at that height).
    pub fn x_bounds_at(&self, y: f32) -> (f32, f32) {
        let band: Vec<&Monitor> = self
            .monitors
            .iter()
            .filter(|m| y >= m.origin.y - 4.0 && y <= m.origin.y + m.size.y + 4.0)
            .collect();
        let src: &[&Monitor] = if band.is_empty() {
            // Above the tallest monitor: use everything so flight stays bounded.
            return (self.total_min_x(), self.total_max_x());
        } else {
            &band
        };
        let min_x = src.iter().map(|m| m.origin.x).fold(f32::MAX, f32::min);
        let max_x = src.iter().map(|m| m.origin.x + m.size.x).fold(f32::MIN, f32::max);
        (min_x, max_x)
    }

    fn total_min_x(&self) -> f32 {
        self.monitors.iter().map(|m| m.origin.x).fold(f32::MAX, f32::min)
    }
    fn total_max_x(&self) -> f32 {
        self.monitors.iter().map(|m| m.origin.x + m.size.x).fold(f32::MIN, f32::max)
    }

    /// The surface the creature would land on: the nearest support (monitor
    /// floor or window-top platform) at or below `prev_feet`. Platforms above
    /// the feet don't count, so the pet can fly up *past* a window and land on
    /// it from above, but never gets stuck underneath one.
    pub fn support_y(&self, x: f32, prev_feet: f32) -> f32 {
        let mut best = self.ground_y(x);
        for p in &self.platforms {
            if x >= p.x0 && x <= p.x1 && p.y >= prev_feet - 4.0 && p.y < best {
                best = p.y;
            }
        }
        best
    }

    /// The platform the creature is currently standing on, if any.
    pub fn platform_under(&self, x: f32, feet: f32) -> Option<Plat> {
        self.platforms
            .iter()
            .copied()
            .find(|p| x >= p.x0 && x <= p.x1 && (p.y - feet).abs() < 6.0)
    }

    /// Index of the monitor containing a global point, if any.
    pub fn monitor_at(&self, p: Vec2) -> Option<usize> {
        self.monitors.iter().position(|m| {
            p.x >= m.origin.x
                && p.x < m.origin.x + m.size.x
                && p.y >= m.origin.y
                && p.y < m.origin.y + m.size.y
        })
    }

}
