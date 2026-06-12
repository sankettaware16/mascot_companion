//! Gentle wellness nudges + flavour dialogue. Designed to *never nag*: every
//! channel has a cooldown, there's a global minimum gap between any two
//! messages, and nothing ever guilt-trips. The companion only speaks when it
//! has something kind and timely to say.

use crate::activity::Activity;
use crate::config::Config;
use rand::seq::SliceRandom;
use rand::Rng;

/// How insistently a message is delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    /// Plain banner: shows for `ttl` seconds wherever the pet is.
    Passive,
    /// Pet brings it to your cursor; gives up quickly if you stay busy
    /// (never blocks important work).
    Gentle,
    /// Pet brings it to your cursor and keeps it up until you actually
    /// pause — used for hydration.
    Strict,
}

pub struct Speech {
    pub text: String,
    /// How long to keep the banner up, seconds (Passive only).
    pub ttl: f32,
    pub urgency: Urgency,
}

pub struct Wellness {
    water_accum: f32,
    since_last_speech: f32,
    eye_warned: bool,
    break_warned: bool,
    flavor_accum: f32,
}

const MIN_GAP: f32 = 40.0; // never two banners closer than this

impl Wellness {
    pub fn new() -> Self {
        Self {
            water_accum: 0.0,
            since_last_speech: MIN_GAP,
            eye_warned: false,
            break_warned: false,
            flavor_accum: 0.0,
        }
    }

    pub fn update(&mut self, dt: f32, act: &Activity, cfg: &Config) -> Option<Speech> {
        self.since_last_speech += dt;
        self.flavor_accum += dt;
        if cfg.reminders.enabled {
            self.water_accum += dt;
        }

        // A finished work stint clears warnings and earns a little praise.
        if act.break_started {
            self.eye_warned = false;
            self.break_warned = false;
            if act.active_minutes() > cfg.reminders.break_minutes * 0.5 {
                return self.say(praise(), 5.0);
            }
        }

        if self.since_last_speech < MIN_GAP {
            return None;
        }

        if cfg.reminders.enabled {
            // Eye rest — once per stint when the threshold is crossed. Gentle:
            // delivered to the cursor but backs off if you're clearly busy.
            if !self.eye_warned && act.active_minutes() >= cfg.reminders.eye_rest_minutes {
                self.eye_warned = true;
                return self.say_urgent("Eyes tired? Look far away for 20 seconds.", Urgency::Gentle);
            }
            // Stretch/break — once per stint. Gentle for the same reason.
            if !self.break_warned && act.active_minutes() >= cfg.reminders.break_minutes {
                self.break_warned = true;
                return self.say_urgent("You've been at it a while. Stretch?", Urgency::Gentle);
            }
            // Water — periodic, only while you're around to hear it. Strict:
            // the pet stays with your cursor until you actually pause.
            if self.water_accum >= cfg.reminders.water_minutes * 60.0 && act.is_active() {
                self.water_accum = 0.0;
                return self.say_urgent("Psst... drink some water!", Urgency::Strict);
            }
        }

        // Rare ambient flavour so it feels alive even when all is well.
        if self.flavor_accum > 150.0 {
            self.flavor_accum = 0.0;
            let mut rng = rand::thread_rng();
            if rng.gen_bool(0.5) {
                let line = flavor(cfg).to_string();
                return self.say_owned(line, 4.5);
            }
        }
        None
    }

    fn say(&mut self, text: &str, ttl: f32) -> Option<Speech> {
        self.since_last_speech = 0.0;
        Some(Speech { text: text.to_string(), ttl, urgency: Urgency::Passive })
    }
    fn say_owned(&mut self, text: String, ttl: f32) -> Option<Speech> {
        self.since_last_speech = 0.0;
        Some(Speech { text, ttl, urgency: Urgency::Passive })
    }
    fn say_urgent(&mut self, text: &str, urgency: Urgency) -> Option<Speech> {
        self.since_last_speech = 0.0;
        Some(Speech { text: text.to_string(), ttl: 6.0, urgency })
    }
}

fn praise() -> &'static str {
    let lines = ["Nice work :)", "Good job today!", "You did great.", "Proud of you!"];
    *lines.choose(&mut rand::thread_rng()).unwrap()
}

fn flavor(cfg: &Config) -> String {
    let lines = [
        "...just keeping you company.".to_string(),
        "What a nice screen we have.".to_string(),
        "I like it here.".to_string(),
        format!("{} reporting for duty!", cfg.name),
        "Don't mind me.".to_string(),
        "Hope today is kind to you.".to_string(),
    ];
    lines.choose(&mut rand::thread_rng()).unwrap().clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::Activity;
    use crate::config::Config;
    use glam::Vec2;

    const DT: f32 = 1.0 / 30.0;

    /// Water must fire ~1 minute in while the cursor is moving.
    #[test]
    fn water_fires_after_configured_minutes() {
        let mut act = Activity::new();
        let mut wl = Wellness::new();
        let mut cfg = Config::default();
        cfg.reminders.water_minutes = 1.0;

        let mut x = 0.0f32;
        for frame in 0..(120 * 30) {
            x += 5.0; // keep the cursor moving
            act.update(DT, Some(Vec2::new(100.0 + (x % 400.0), 100.0)));
            if let Some(s) = wl.update(DT, &act, &cfg) {
                assert_eq!(s.urgency, Urgency::Strict, "water is strict");
                assert!(s.text.to_lowercase().contains("water"), "got: {}", s.text);
                let t = frame as f32 * DT;
                assert!((55.0..80.0).contains(&t), "fired at {t}s, expected ~60s");
                return;
            }
        }
        panic!("water reminder never fired in 2 simulated minutes");
    }

    /// Eye rest must fire at 1.5 active minutes even when the mouse only
    /// twitches occasionally (typing-style work) — wall-clock accrual.
    #[test]
    fn eye_rest_fires_with_sparse_mouse_movement() {
        let mut act = Activity::new();
        let mut wl = Wellness::new();
        let mut cfg = Config::default();
        cfg.reminders.water_minutes = 999.0; // isolate the eye reminder
        cfg.reminders.eye_rest_minutes = 1.5;

        let mut pos = Vec2::new(100.0, 100.0);
        for frame in 0..(180 * 30) {
            // Touch the mouse for one frame every 20 seconds, else parked.
            if frame % (20 * 30) == 0 {
                pos.x += 50.0;
            }
            act.update(DT, Some(pos));
            if let Some(s) = wl.update(DT, &act, &cfg) {
                assert_eq!(s.urgency, Urgency::Gentle, "eye rest is gentle");
                assert!(s.text.contains("Eyes"), "got: {}", s.text);
                // Stint starts at the FIRST movement (t=20s), so 1.5 active
                // minutes completes at ~110s.
                let t = frame as f32 * DT;
                assert!((105.0..120.0).contains(&t), "fired at {t}s, expected ~110s");
                return;
            }
        }
        panic!("eye-rest reminder never fired in 3 simulated minutes");
    }
}
