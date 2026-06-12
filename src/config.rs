//! User configuration. Loaded from `config.toml` next to the binary (or the
//! current working dir). Everything is optional with friendly defaults — the
//! companion runs happily with no config at all.

use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Display name the companion "answers" to (used in dialogue).
    pub name: String,
    /// Pixel scale of the creature. 1 sprite px -> `scale` screen px.
    pub scale: f32,
    /// Base RGB body colour, 0..=255.
    pub body_color: [u8; 3],
    /// Follow the mouse cursor when it comes near.
    pub follow_cursor: bool,
    /// Run *away* from the cursor instead of toward it (shy personality).
    pub flee_cursor: bool,
    /// Target frames-per-second while active. Lowered automatically when idle.
    pub fps: u32,
    /// Accessibility: damp all motion (no flying, gentle walks only).
    pub reduced_motion: bool,
    /// Name of a custom pet pack (folder under `pets/`) made with
    /// `companion-convert`. `None` = the built-in procedural creature.
    pub pet: Option<String>,
    /// Let the user pick the pet up with the mouse and throw it.
    pub allow_grab: bool,
    /// Treat open windows' top edges as platforms the pet can land/walk on.
    pub climb_windows: bool,
    /// Rush over excitedly when a desktop notification appears.
    pub react_notifications: bool,
    /// Master switch for wellness reminders.
    pub reminders: Reminders,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Reminders {
    pub enabled: bool,
    /// Remind to drink water every N minutes.
    pub water_minutes: u64,
    /// Remind to rest your eyes after N continuous active minutes.
    pub eye_rest_minutes: u64,
    /// Suggest a stretch/break after N continuous active minutes.
    pub break_minutes: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            name: "Pico".to_string(),
            scale: 3.0,
            body_color: [255, 176, 84], // warm pumpkin
            follow_cursor: true,
            flee_cursor: false,
            fps: 30,
            reduced_motion: false,
            pet: None,
            allow_grab: true,
            climb_windows: true,
            react_notifications: true,
            reminders: Reminders::default(),
        }
    }
}

impl Default for Reminders {
    fn default() -> Self {
        Self {
            enabled: true,
            water_minutes: 45,
            eye_rest_minutes: 25,
            break_minutes: 55,
        }
    }
}

impl Config {
    /// Load config, checking (in order) the working directory then the XDG
    /// config dir, otherwise returning defaults.
    pub fn load() -> Self {
        let mut candidates = vec!["config.toml".to_string(), "./companion.toml".to_string()];
        if let Ok(home) = std::env::var("HOME") {
            candidates.push(format!("{home}/.config/companion/config.toml"));
        }
        for candidate in &candidates {
            let candidate = candidate.as_str();
            if Path::new(candidate).exists() {
                match std::fs::read_to_string(candidate) {
                    Ok(text) => match toml::from_str::<Config>(&text) {
                        Ok(cfg) => {
                            log_info(&format!("loaded config from {candidate}"));
                            return cfg.sanitized();
                        }
                        Err(e) => log_info(&format!("config parse error ({candidate}): {e}")),
                    },
                    Err(e) => log_info(&format!("config read error ({candidate}): {e}")),
                }
            }
        }
        Config::default()
    }

    fn sanitized(mut self) -> Self {
        self.scale = self.scale.clamp(1.0, 12.0);
        self.fps = self.fps.clamp(8, 120);
        self
    }
}

fn log_info(msg: &str) {
    eprintln!("[companion] {msg}");
}
