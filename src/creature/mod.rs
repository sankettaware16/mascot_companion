pub mod animation;
pub mod brain;
pub mod emotion;
pub mod physics;

use crate::config::Config;
use crate::world::World;
use animation::Pose;
pub use brain::{Behavior, Locomotion};
use brain::{Brain, Ctx};
use emotion::EmotionEngine;
use glam::Vec2;
use physics::Body;

const WALK_SPEED: f32 = 78.0;
const RUN_SPEED: f32 = 205.0;
const FLY_THRUST: f32 = 950.0;

/// Everything the creature can perceive this frame.
pub struct Senses {
    pub cursor: Option<Vec2>,
    pub cursor_speed: f32,
    pub user_active: bool,
    pub user_idle: bool,
    /// Centre-top of the active window, if worth perching on.
    pub perch: Option<Vec2>,
    /// A fresh notification's position, while it's still interesting.
    pub noti: Option<Vec2>,
    /// A reminder is being delivered: come to the cursor and stay with it.
    pub summon: bool,
}

/// Everything that makes up the living companion at one instant.
pub struct Creature {
    pub body: Body,
    pub emotion: EmotionEngine,
    pub brain: Brain,
    anim_time: f32,
    pose: Pose,
    loco: Locomotion,
    held: bool,
    thrown: bool,
    /// Smoothed cursor velocity while held — becomes the throw velocity.
    drag_vel: Vec2,
    /// Visual tilt (radians): spin while tumbling, lean while flying.
    angle: f32,
    spin: f32,
}

impl Creature {
    pub fn new(start: Vec2, cfg: &Config) -> Self {
        Self {
            body: Body::new(start, 11.0 * cfg.scale),
            emotion: EmotionEngine::new(),
            brain: Brain::new(start),
            anim_time: 0.0,
            pose: Pose::Idle0,
            loco: Locomotion::Idle,
            held: false,
            thrown: false,
            drag_vel: Vec2::ZERO,
            angle: 0.0,
            spin: 0.0,
        }
    }

    pub fn mood(&self) -> emotion::Mood {
        self.emotion.mood
    }
    pub fn pose(&self) -> Pose {
        self.pose
    }
    pub fn pos(&self) -> Vec2 {
        self.body.pos
    }
    pub fn facing(&self) -> f32 {
        self.body.facing
    }
    /// Continuously-increasing animation clock (seconds) for procedural motion.
    pub fn anim_phase(&self) -> f32 {
        self.anim_time
    }
    pub fn locomotion(&self) -> Locomotion {
        self.loco
    }
    pub fn behavior(&self) -> Behavior {
        self.brain.behavior
    }
    /// Visual tilt in radians (throw spin / flight lean), already smoothed.
    pub fn visual_angle(&self) -> f32 {
        self.angle
    }

    // --- user interaction ----------------------------------------------------

    /// The user pinched the creature with the mouse.
    pub fn grab(&mut self) {
        self.held = true;
        self.thrown = false;
        self.spin = 0.0;
        self.drag_vel = Vec2::ZERO;
    }

    /// The user let go. Returns the release velocity; a fast release becomes a
    /// proper throw with tumble-spin.
    pub fn release(&mut self) -> Vec2 {
        self.held = false;
        let mut v = self.drag_vel;
        let speed = v.length();
        if speed > 2400.0 {
            v *= 2400.0 / speed;
        }
        self.body.vel = v;
        if v.length() > 520.0 {
            self.thrown = true;
            self.spin = (v.x * 0.006).clamp(-7.0, 7.0);
        }
        v
    }

    // --- per-frame update ------------------------------------------------------

    pub fn update(&mut self, dt: f32, world: &World, s: &Senses, cfg: &Config) {
        // While held, the pet hangs from the cursor; we track drag velocity so
        // a release becomes a throw.
        if self.held {
            if let Some(c) = s.cursor {
                let inst = (c - self.body.pos) / dt.max(1e-3);
                let k = (dt * 12.0).min(1.0);
                self.drag_vel = self.drag_vel * (1.0 - k) + inst * k;
                self.body.pos = c;
                self.body.vel = Vec2::ZERO;
                self.body.on_ground = false;
                if self.drag_vel.x.abs() > 40.0 {
                    self.body.facing = self.drag_vel.x.signum();
                }
            }
        }

        let cursor_near = self.held
            || s.cursor.map(|c| (c - self.body.pos).length() < 150.0).unwrap_or(false);

        let ctx = Ctx {
            dt,
            mood: self.emotion.mood,
            needs: self.emotion.needs,
            pos: self.body.pos,
            on_ground: self.body.on_ground,
            half: self.body.half,
            cursor: s.cursor,
            cursor_speed: s.cursor_speed,
            held: self.held,
            thrown: self.thrown,
            user_active: s.user_active,
            user_idle: s.user_idle,
            perch: s.perch,
            noti: s.noti,
            summon: s.summon,
            follow_cursor: cfg.follow_cursor,
            flee_cursor: cfg.flee_cursor,
            reduced_motion: cfg.reduced_motion,
            world,
        };
        let intent = self.brain.decide(&ctx);
        self.loco = intent.loco;

        let mood_mul = self.emotion.mood.speed_mul();
        match intent.loco {
            Locomotion::Held => {} // pinned to the cursor above
            Locomotion::Idle | Locomotion::Sleep | Locomotion::Tumble => {
                self.body.integrate(dt, 1.0, world);
            }
            Locomotion::Walk => {
                self.body.walk_toward(intent.target.x, WALK_SPEED * mood_mul, dt);
                self.body.integrate(dt, 1.0, world);
            }
            Locomotion::Run => {
                self.body.walk_toward(intent.target.x, RUN_SPEED * mood_mul, dt);
                self.body.integrate(dt, 1.0, world);
            }
            Locomotion::Fly => {
                self.body.fly_toward(intent.target, FLY_THRUST, dt);
                if intent.flap {
                    self.body.flap();
                }
                self.body.integrate(dt, 0.18, world);
            }
        }
        // A flap requested while grounded is a hop.
        if intent.flap && !matches!(intent.loco, Locomotion::Fly) && self.body.on_ground {
            self.body.flap();
        }

        if self.body.on_ground && self.thrown {
            self.thrown = false;
            self.spin = 0.0;
        }

        // Visual tilt: spin while thrown, lean while flying, settle otherwise.
        use std::f32::consts::{PI, TAU};
        if self.thrown && !self.body.on_ground {
            self.angle += self.spin * dt;
            self.spin *= 1.0 - (0.35 * dt).min(1.0);
        } else if matches!(self.loco, Locomotion::Fly) {
            let lean = (self.body.vel.x * 0.0005).clamp(-0.30, 0.30);
            self.angle += (lean - self.angle) * (dt * 6.0).min(1.0);
        } else {
            // Wrap multi-turn spin back to the nearest upright, then settle.
            if self.angle.abs() > PI {
                self.angle = self.angle.rem_euclid(TAU);
                if self.angle > PI {
                    self.angle -= TAU;
                }
            }
            self.angle += (0.0 - self.angle) * (dt * 8.0).min(1.0);
        }

        // Emotional bookkeeping.
        let sleeping = matches!(self.loco, Locomotion::Sleep);
        let resting = matches!(self.loco, Locomotion::Idle);
        self.emotion.update(dt, s.user_active, cursor_near, sleeping, resting);
        if matches!(self.loco, Locomotion::Fly | Locomotion::Run) {
            self.emotion.played(0.4 * dt);
            self.emotion.exert(dt);
        }
        let mut rng = rand::thread_rng();
        self.emotion.jitter(&mut rng);

        self.advance_anim(dt);
    }

    fn advance_anim(&mut self, dt: f32) {
        self.anim_time += dt;
        let speed = self.body.vel.x.abs();
        self.pose = match self.loco {
            Locomotion::Sleep => Pose::Sleep,
            Locomotion::Tumble => Pose::Fly1, // wings out, helpless
            Locomotion::Held => {
                // Slow dangle-flutter while carried.
                if (self.anim_time * 4.0) as i32 % 2 == 0 {
                    Pose::Fly0
                } else {
                    Pose::Fly1
                }
            }
            Locomotion::Fly => {
                if (self.anim_time * 8.0) as i32 % 2 == 0 {
                    Pose::Fly0
                } else {
                    Pose::Fly1
                }
            }
            Locomotion::Walk | Locomotion::Run if speed > 14.0 => {
                let rate = if matches!(self.loco, Locomotion::Run) { 11.0 } else { 7.0 };
                if (self.anim_time * rate) as i32 % 2 == 0 {
                    Pose::Walk0
                } else {
                    Pose::Walk1
                }
            }
            _ => {
                // gentle breathing
                if (self.anim_time * 1.6) as i32 % 2 == 0 {
                    Pose::Idle0
                } else {
                    Pose::Idle1
                }
            }
        };
    }
}
