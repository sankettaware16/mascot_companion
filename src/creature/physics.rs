//! Tiny rigid-body for the companion. Global "virtual desktop" coordinates,
//! origin at the top-left of the left-most monitor, +y points *down* (screen
//! convention). Units are screen pixels, time in seconds.

use crate::world::World;
use glam::Vec2;

/// Acceleration of gravity, px/s^2. Tuned to feel like a light, springy plush.
pub const GRAVITY: f32 = 2200.0;

pub struct Body {
    pub pos: Vec2,
    pub vel: Vec2,
    /// -1.0 facing left, +1.0 facing right.
    pub facing: f32,
    pub on_ground: bool,
    /// Half-extent of the creature in screen px (collision radius-ish).
    pub half: f32,
}

impl Body {
    pub fn new(pos: Vec2, half: f32) -> Self {
        Self { pos, vel: Vec2::ZERO, facing: 1.0, on_ground: false, half }
    }

    /// Integrate one step. `gravity_scale` lets the brain make flight floaty
    /// (≈0.15) versus walking (1.0). Collides with the floor of whatever
    /// monitor the creature is currently above, and with the outer walls of
    /// the whole desktop so it can never escape into the void.
    pub fn integrate(&mut self, dt: f32, gravity_scale: f32, world: &World) {
        let prev_feet = self.pos.y + self.half;

        self.vel.y += GRAVITY * gravity_scale * dt;

        // Air drag (and rolling friction on the ground) keep motion smooth and
        // bounded without an explicit speed clamp.
        let drag = if self.on_ground { 6.0 } else { 0.6 };
        self.vel.x -= self.vel.x * (drag * dt).min(1.0);

        self.pos += self.vel * dt;

        // --- floor / platforms ----------------------------------------------
        // Land on whatever support was below the feet before this step:
        // monitor floor or a window-top platform (so fast falls can't tunnel
        // through a thin window edge).
        let floor = world.support_y(self.pos.x, prev_feet) - self.half;
        if self.pos.y >= floor {
            self.pos.y = floor;
            if self.vel.y > 0.0 {
                // Bounce; hard impacts (throws) bounce livelier, then settle.
                let restitution = if self.vel.y > 900.0 { 0.35 } else { 0.18 };
                self.vel.y = -self.vel.y * restitution;
                if self.vel.y.abs() < 40.0 {
                    self.vel.y = 0.0;
                }
            }
            self.on_ground = true;
        } else {
            self.on_ground = false;
        }

        // --- walls (clamp to the union of all monitors) ---------------------
        let (min_x, max_x) = world.x_bounds_at(self.pos.y);
        if self.pos.x < min_x + self.half {
            self.pos.x = min_x + self.half;
            self.vel.x = self.vel.x.abs() * 0.4;
        } else if self.pos.x > max_x - self.half {
            self.pos.x = max_x - self.half;
            self.vel.x = -self.vel.x.abs() * 0.4;
        }

        if self.facing.abs() < 0.01 {
            self.facing = 1.0;
        }
    }

    /// Push toward a horizontal target, accelerating smoothly. Updates facing.
    pub fn walk_toward(&mut self, target_x: f32, speed: f32, dt: f32) {
        let dx = target_x - self.pos.x;
        if dx.abs() > 2.0 {
            self.facing = dx.signum();
            let desired = self.facing * speed;
            // Accelerate toward desired velocity (feels like legs spinning up).
            self.vel.x += (desired - self.vel.x) * (8.0 * dt).min(1.0);
        }
    }

    /// Seek a 2-D point with thrust — used for flight.
    pub fn fly_toward(&mut self, target: Vec2, thrust: f32, dt: f32) {
        let to = target - self.pos;
        if to.length_squared() > 1.0 {
            let dir = to.normalize();
            self.vel += dir * thrust * dt;
            if dir.x.abs() > 0.05 {
                self.facing = dir.x.signum();
            }
        }
        // Soft top speed for flight so it arcs rather than rockets.
        let max = 520.0;
        if self.vel.length() > max {
            self.vel = self.vel.normalize() * max;
        }
    }

    /// Instantaneous wing-flap upward impulse.
    pub fn flap(&mut self) {
        self.vel.y -= 260.0;
    }
}
