//! Behavior selection. A small, readable state machine (not a heavyweight
//! behavior-tree library) that turns mood + needs + senses into an *intent*:
//! where to go and how to get there. Physics does the rest.

use super::emotion::{Mood, Needs};
use crate::world::World;
use glam::Vec2;
use rand::Rng;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Behavior {
    Idle,
    Wander,
    FollowCursor,
    FleeCursor,
    /// Flying around for fun.
    Play,
    /// Walk to a monitor edge and look toward another screen.
    Peek,
    /// Cross over to a different monitor (walking or flying).
    Travel,
    /// Fly up and sit on top of the active window, watching you work.
    Perch,
    /// Rush over to a fresh notification to see what it's about.
    Investigate,
    /// Dangling from the user's cursor.
    Held,
    /// Ballistic after a throw — no control until landing.
    Tumble,
    Sleep,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Locomotion {
    Idle,
    Walk,
    Run,
    Fly,
    Sleep,
    Held,
    Tumble,
}

pub struct Intent {
    pub target: Vec2,
    pub loco: Locomotion,
    /// Wing-flap impulse this frame (also doubles as a hop while walking).
    pub flap: bool,
}

pub struct Ctx<'a> {
    pub dt: f32,
    pub mood: Mood,
    pub needs: Needs,
    pub pos: Vec2,
    pub on_ground: bool,
    pub half: f32,
    pub cursor: Option<Vec2>,
    pub cursor_speed: f32,
    pub held: bool,
    pub thrown: bool,
    pub user_active: bool,
    pub user_idle: bool,
    /// Centre-top of the active window, if worth perching on.
    pub perch: Option<Vec2>,
    /// A fresh notification's position, while it's still interesting.
    pub noti: Option<Vec2>,
    /// A reminder is being delivered: go to the cursor and stay with it.
    pub summon: bool,
    pub follow_cursor: bool,
    pub flee_cursor: bool,
    pub reduced_motion: bool,
    pub world: &'a World,
}

pub struct Brain {
    pub behavior: Behavior,
    timer: f32,
    target: Vec2,
    flap_timer: f32,
    /// Cooldown so cursor-following is an occasional delight, not a lock-on.
    follow_cd: f32,
    /// Whether the current Travel crosses by air.
    travel_fly: bool,
    /// Decided once per fall: flutter off (true) or just drop (false).
    fall_flutter: Option<bool>,
}

impl Brain {
    pub fn new(start: Vec2) -> Self {
        Self {
            behavior: Behavior::Idle,
            timer: 1.0,
            target: start,
            flap_timer: 0.0,
            follow_cd: 6.0,
            travel_fly: false,
            fall_flutter: None,
        }
    }

    pub fn decide(&mut self, ctx: &Ctx) -> Intent {
        self.timer -= ctx.dt;
        self.flap_timer -= ctx.dt;
        self.follow_cd -= ctx.dt;
        let mut rng = rand::thread_rng();

        // --- hard physical states (every frame) -----------------------------
        if ctx.held {
            self.behavior = Behavior::Held;
            self.timer = 0.3;
            return Intent { target: ctx.pos, loco: Locomotion::Held, flap: false };
        }
        if ctx.thrown && !ctx.on_ground {
            self.behavior = Behavior::Tumble;
            self.timer = 0.3;
            return Intent { target: ctx.pos, loco: Locomotion::Tumble, flap: false };
        }
        if self.behavior == Behavior::Tumble {
            // Just landed from a throw — shake it off, then re-decide.
            self.behavior = Behavior::Idle;
            self.timer = rng.gen_range(0.4..1.0);
        }
        if self.behavior == Behavior::Held {
            self.behavior = Behavior::Idle;
            self.timer = 0.2;
        }

        // --- reminder delivery: bring the banner to the user -----------------
        // Wakes it from sleep, overrides anger — being a caring friend comes
        // before everything except physics.
        if ctx.summon && ctx.cursor.is_some() {
            self.behavior = Behavior::FollowCursor;
            self.timer = 0.5;
        }

        // --- a notification! rush over to see -------------------------------
        if let Some(n) = ctx.noti {
            if !ctx.summon
                && ctx.mood != Mood::Angry
                && !matches!(self.behavior, Behavior::Investigate | Behavior::Sleep)
            {
                self.behavior = Behavior::Investigate;
                // Hover just below the popup, but never below the floor.
                self.target = Vec2::new(
                    n.x,
                    (n.y + 100.0).min(ctx.world.ground_y(n.x) - ctx.half - 8.0),
                );
                self.timer = 7.0;
            }
        } else if self.behavior == Behavior::Investigate && self.timer > 1.2 {
            // Popup gone — lose interest shortly.
            self.timer = 1.2;
        }

        // --- sleep is earned, not random -------------------------------------
        // Only nod off when the *user* is away (or utterly exhausted).
        if !ctx.summon && (ctx.needs.energy < 0.05 || (ctx.needs.energy < 0.16 && ctx.user_idle)) {
            self.behavior = Behavior::Sleep;
        }
        if self.behavior == Behavior::Sleep && !ctx.user_idle && ctx.needs.energy > 0.4 {
            self.timer = 0.0; // wake up when you come back
        }

        // --- cursor reactions -------------------------------------------------
        let cursor_d = ctx.cursor.map(|c| (c - ctx.pos).length());
        let angry = ctx.mood == Mood::Angry;
        if let Some(d) = cursor_d {
            if ctx.summon {
                // stay on delivery duty — skip flee/chase games
            } else if angry && d < 300.0 && self.behavior != Behavior::Sleep {
                // Don't touch me right now.
                self.behavior = Behavior::FleeCursor;
                self.timer = self.timer.max(0.4);
            } else if ctx.follow_cursor
                && !angry
                && self.follow_cd <= 0.0
                && d < 280.0
                && ctx.cursor_speed > 240.0
                && matches!(
                    self.behavior,
                    Behavior::Idle | Behavior::Wander | Behavior::Peek | Behavior::Play
                )
            {
                // The cursor zoomed past nearby — chase it for a bit, then a
                // long cooldown so it stays a treat (and the pet keeps a life
                // of its own: flying, travelling, perching).
                self.behavior =
                    if ctx.flee_cursor { Behavior::FleeCursor } else { Behavior::FollowCursor };
                self.timer = rng.gen_range(2.5..5.5);
                self.follow_cd = rng.gen_range(20.0..45.0);
            }
            if self.behavior == Behavior::FollowCursor && d > 650.0 && !ctx.summon {
                self.timer = 0.0; // it got away; never mind
            }
        }

        // --- airborne without a plan: flutter off or just drop ----------------
        if !ctx.on_ground
            && !ctx.thrown
            && !ctx.reduced_motion
            && matches!(self.behavior, Behavior::Idle | Behavior::Wander | Behavior::Peek | Behavior::Sleep)
        {
            let flutter = *self
                .fall_flutter
                .get_or_insert_with(|| rng.gen_bool(0.45));
            if flutter {
                // "Left in mid-air? Fine — I can fly." Off it goes somewhere random.
                self.behavior = Behavior::Play;
                self.timer = rng.gen_range(3.0..6.0);
                self.target = random_air_target(ctx, &mut rng);
            }
        }
        if ctx.on_ground {
            self.fall_flutter = None;
        }

        if self.timer <= 0.0 {
            self.choose(ctx, &mut rng);
        }

        self.realize(ctx, &mut rng)
    }

    // -------------------------------------------------------------------------

    fn choose(&mut self, ctx: &Ctx, rng: &mut impl Rng) {
        // Stay asleep until rested — long naps while you're away, quick ones
        // otherwise.
        if self.behavior == Behavior::Sleep {
            let stay = ctx.needs.energy < 0.45 || (ctx.user_idle && ctx.needs.energy < 0.97);
            if stay {
                self.timer = 2.0;
                return;
            }
        }

        let multi = ctx.world.monitors.len() > 1;
        let can_perch = ctx.perch.is_some() && ctx.user_active && !ctx.reduced_motion;
        let bored = 1.0 - ctx.needs.fun;

        // Weighted menu. Weights shift with mood & needs so the *same* creature
        // behaves differently as it feels differently.
        let mut menu: Vec<(Behavior, f32)> = Vec::new();
        match ctx.mood {
            Mood::Energetic => {
                menu.push((Behavior::Play, 4.0 + 3.0 * bored));
                menu.push((Behavior::Wander, 2.5));
                if multi {
                    menu.push((Behavior::Travel, 2.0));
                }
                if can_perch {
                    menu.push((Behavior::Perch, 1.8));
                }
                menu.push((Behavior::Peek, 1.0));
                menu.push((Behavior::Idle, 0.5));
            }
            Mood::Happy => {
                menu.push((Behavior::Wander, 3.0));
                menu.push((Behavior::Play, 2.0 + 2.0 * bored));
                if can_perch {
                    menu.push((Behavior::Perch, 2.0));
                }
                if multi {
                    menu.push((Behavior::Travel, 1.2));
                }
                menu.push((Behavior::Peek, 1.0));
                menu.push((Behavior::Idle, 1.5));
            }
            Mood::Neutral => {
                menu.push((Behavior::Wander, 2.5));
                menu.push((Behavior::Idle, 2.5));
                if can_perch {
                    menu.push((Behavior::Perch, 1.2));
                }
                if multi {
                    menu.push((Behavior::Travel, 0.8));
                }
                menu.push((Behavior::Peek, 1.0));
                menu.push((Behavior::Play, 1.5 * bored));
            }
            Mood::Sad | Mood::Crying => {
                menu.push((Behavior::Idle, 3.0));
                menu.push((Behavior::Wander, 1.0));
            }
            Mood::Sleepy => {
                menu.push((Behavior::Idle, 3.0));
                menu.push((Behavior::Wander, 0.7));
            }
            Mood::Angry => {
                // Storms around, maybe storms off to the other screen entirely.
                menu.push((Behavior::Wander, 3.0));
                if multi {
                    menu.push((Behavior::Travel, 2.0));
                }
                menu.push((Behavior::Idle, 1.0));
            }
        }
        // Naps happen when *you* are away.
        if ctx.user_idle && ctx.needs.energy < 0.85 {
            menu.push((Behavior::Sleep, 1.5 + 3.0 * (1.0 - ctx.needs.energy)));
        }
        if ctx.reduced_motion {
            for entry in menu.iter_mut() {
                if matches!(entry.0, Behavior::Play | Behavior::Perch) {
                    entry.0 = Behavior::Wander;
                }
            }
        }

        let total: f32 = menu.iter().map(|(_, w)| w).sum();
        let mut roll = rng.gen_range(0.0..total.max(0.001));
        let mut chosen = Behavior::Idle;
        for (b, w) in &menu {
            if roll < *w {
                chosen = *b;
                break;
            }
            roll -= w;
        }
        self.behavior = chosen;

        // Duration + target for the chosen behavior.
        let n = ctx.world.monitors.len();
        let cur = ctx.world.monitor_at(ctx.pos).unwrap_or(0);
        let m = ctx.world.monitors.get(cur).copied();
        match chosen {
            Behavior::Idle => self.timer = rng.gen_range(1.5..4.0),
            Behavior::Sleep => self.timer = rng.gen_range(8.0..20.0),
            Behavior::Wander => {
                self.timer = rng.gen_range(2.0..5.0);
                // If standing on a window, mostly patrol along it; sometimes
                // step off the edge (gravity makes that fun on its own).
                let feet = ctx.pos.y + ctx.half;
                if let Some(p) = ctx.world.platform_under(ctx.pos.x, feet) {
                    if rng.gen_bool(0.65) {
                        self.target.x = rng.gen_range(p.x0 + 8.0..(p.x1 - 8.0).max(p.x0 + 9.0));
                        return;
                    }
                }
                if let Some(m) = m {
                    self.target.x =
                        rng.gen_range(m.origin.x + m.size.x * 0.1..m.origin.x + m.size.x * 0.9);
                }
            }
            Behavior::Play => {
                self.timer = rng.gen_range(3.0..7.0);
                // Sometimes the flight crosses to a different screen.
                let mi = if n > 1 && rng.gen_bool(0.35) { rng.gen_range(0..n) } else { cur };
                let m = ctx.world.monitors[mi];
                self.target = Vec2::new(
                    rng.gen_range(m.origin.x + 60.0..m.origin.x + m.size.x - 60.0),
                    rng.gen_range(m.origin.y + 60.0..m.floor() - 120.0),
                );
            }
            Behavior::Travel => {
                self.timer = rng.gen_range(6.0..12.0);
                let others: Vec<usize> = (0..n).filter(|i| *i != cur).collect();
                let mi = others[rng.gen_range(0..others.len().max(1))];
                let m = ctx.world.monitors[mi];
                self.target = Vec2::new(
                    rng.gen_range(m.origin.x + m.size.x * 0.15..m.origin.x + m.size.x * 0.85),
                    m.floor(),
                );
                self.travel_fly =
                    !ctx.reduced_motion && ctx.needs.energy > 0.4 && rng.gen_bool(0.75);
            }
            Behavior::Perch => {
                self.timer = rng.gen_range(6.0..14.0);
                if let Some(p) = ctx.perch {
                    self.target = Vec2::new(p.x, p.y - ctx.half - 4.0);
                } else {
                    self.behavior = Behavior::Idle;
                    self.timer = 1.0;
                }
            }
            Behavior::Peek => {
                self.timer = rng.gen_range(2.0..4.0);
                if let Some(m) = m {
                    let go_right = ctx
                        .cursor
                        .map(|c| c.x > m.center().x)
                        .unwrap_or(rng.gen_bool(0.5));
                    self.target.x =
                        if go_right { m.origin.x + m.size.x - 30.0 } else { m.origin.x + 30.0 };
                }
            }
            Behavior::FollowCursor | Behavior::FleeCursor => self.timer = 0.5,
            Behavior::Investigate | Behavior::Held | Behavior::Tumble => {}
        }
    }

    // -------------------------------------------------------------------------

    fn realize(&mut self, ctx: &Ctx, rng: &mut impl Rng) -> Intent {
        let mut flap = false;

        let (target, loco) = match self.behavior {
            Behavior::Idle => (ctx.pos, Locomotion::Idle),
            Behavior::Sleep => (ctx.pos, Locomotion::Sleep),
            Behavior::Held => (ctx.pos, Locomotion::Held),
            Behavior::Tumble => (ctx.pos, Locomotion::Tumble),
            Behavior::Wander | Behavior::Peek => {
                let loco = match ctx.mood {
                    Mood::Energetic | Mood::Angry if !ctx.reduced_motion => Locomotion::Run,
                    _ => Locomotion::Walk,
                };
                // Playful hop now and then while energetic.
                if ctx.mood == Mood::Energetic
                    && ctx.on_ground
                    && !ctx.reduced_motion
                    && rng.gen_bool((ctx.dt as f64 * 0.25).min(1.0))
                {
                    flap = true;
                }
                (Vec2::new(self.target.x, ctx.world.ground_y(self.target.x)), loco)
            }
            Behavior::FollowCursor => {
                let c = ctx.cursor.unwrap_or(ctx.pos);
                let loco = if (c - ctx.pos).length() > 180.0 && !ctx.reduced_motion {
                    Locomotion::Run
                } else {
                    Locomotion::Walk
                };
                (Vec2::new(c.x, ctx.world.ground_y(c.x)), loco)
            }
            Behavior::FleeCursor => {
                let c = ctx.cursor.unwrap_or(ctx.pos);
                let away = (ctx.pos - c).normalize_or(Vec2::X) * 320.0;
                let tx = ctx.pos.x + away.x;
                (Vec2::new(tx, ctx.world.ground_y(tx)), Locomotion::Run)
            }
            Behavior::Play => {
                flap = flap_toward(&mut self.flap_timer, ctx.pos, self.target, rng);
                (self.target, Locomotion::Fly)
            }
            Behavior::Travel => {
                let dest_x = self.target.x;
                let dest_ground = ctx.world.ground_y(dest_x);
                let dx = dest_x - ctx.pos.x;
                if ctx.reduced_motion || !self.travel_fly {
                    let loco = if dx.abs() > 400.0 { Locomotion::Run } else { Locomotion::Walk };
                    (Vec2::new(dest_x, dest_ground), loco)
                } else if dx.abs() > 200.0 {
                    // Cruise at altitude toward the other screen.
                    let cruise = Vec2::new(ctx.pos.x + dx.signum() * 260.0, dest_ground - 180.0);
                    flap = flap_toward(&mut self.flap_timer, ctx.pos, cruise, rng);
                    (cruise, Locomotion::Fly)
                } else {
                    // Final descent.
                    (Vec2::new(dest_x, dest_ground), Locomotion::Walk)
                }
            }
            Behavior::Perch => {
                let t = self.target;
                let arrived = ctx.on_ground
                    && (t.x - ctx.pos.x).abs() < 50.0
                    && (t.y - ctx.pos.y).abs() < 60.0;
                if arrived {
                    (ctx.pos, Locomotion::Idle) // sit and watch you work
                } else {
                    flap = flap_toward(&mut self.flap_timer, ctx.pos, t, rng);
                    (t, Locomotion::Fly)
                }
            }
            Behavior::Investigate => {
                // Hover excitedly near the popup the whole time.
                flap = flap_toward(&mut self.flap_timer, ctx.pos, self.target, rng);
                (self.target, Locomotion::Fly)
            }
        };

        Intent { target, loco, flap }
    }
}

/// Flap when sagging below the flight target (with the occasional extra flap
/// for a lively, fluttery arc). Shared by every flying behavior.
fn flap_toward(flap_timer: &mut f32, pos: Vec2, target: Vec2, rng: &mut impl Rng) -> bool {
    if *flap_timer <= 0.0 && (pos.y > target.y - 8.0 || rng.gen_bool(0.05)) {
        *flap_timer = rng.gen_range(0.18..0.34);
        true
    } else {
        false
    }
}

fn random_air_target(ctx: &Ctx, rng: &mut impl Rng) -> Vec2 {
    let n = ctx.world.monitors.len();
    let cur = ctx.world.monitor_at(ctx.pos).unwrap_or(0);
    let mi = if n > 1 && rng.gen_bool(0.3) { rng.gen_range(0..n) } else { cur };
    let m = ctx.world.monitors[mi];
    Vec2::new(
        rng.gen_range(m.origin.x + 60.0..m.origin.x + m.size.x - 60.0),
        rng.gen_range(m.origin.y + 80.0..m.floor() - 120.0),
    )
}
