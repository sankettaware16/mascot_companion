//! Gentle wellness nudges + flavour dialogue. Designed to *never nag*: every
//! channel has a cooldown, there's a global minimum gap between any two
//! messages, and nothing ever guilt-trips. The companion only speaks when it
//! has something kind and timely to say.

use crate::activity::Activity;
use crate::config::Config;
use rand::seq::SliceRandom;
use rand::Rng;

pub struct Speech {
    pub text: String,
    /// How long to keep the banner up, seconds.
    pub ttl: f32,
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
            if act.active_minutes() > cfg.reminders.break_minutes as f32 * 0.5 {
                return self.say(praise(), 5.0);
            }
        }

        if self.since_last_speech < MIN_GAP {
            return None;
        }

        if cfg.reminders.enabled {
            // Eye rest — once per stint when the threshold is crossed.
            if !self.eye_warned && act.active_minutes() >= cfg.reminders.eye_rest_minutes as f32 {
                self.eye_warned = true;
                return self.say("Eyes tired? Look far away for 20 seconds.", 6.0);
            }
            // Stretch/break — once per stint.
            if !self.break_warned && act.active_minutes() >= cfg.reminders.break_minutes as f32 {
                self.break_warned = true;
                return self.say("You've been at it a while. Stretch?", 6.0);
            }
            // Water — periodic, only while you're around to hear it.
            if self.water_accum >= cfg.reminders.water_minutes as f32 * 60.0 && act.is_active() {
                self.water_accum = 0.0;
                return self.say("Psst... drink some water!", 6.0);
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
        Some(Speech { text: text.to_string(), ttl })
    }
    fn say_owned(&mut self, text: String, ttl: f32) -> Option<Speech> {
        self.since_last_speech = 0.0;
        Some(Speech { text, ttl })
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
