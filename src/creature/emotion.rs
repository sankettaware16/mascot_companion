//! Emotion engine — *no AI*. Just a small simulation of internal needs that
//! decay/recover over time and a winner-take-all mood derived from them.
//!
//! The trick to "feeling alive" is that the visible mood lags and hysteresis'es
//! behind the underlying numbers, so transitions look deliberate rather than
//! jittery.

use rand::Rng;

/// The visible mood. Drives sprite, movement speed and dialogue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    Happy,
    Energetic,
    Neutral,
    Sleepy,
    Sad,
    Crying,
    /// Provoked — e.g. picked up and thrown around too often.
    Angry,
}

impl Mood {
    /// Movement speed multiplier for this mood.
    pub fn speed_mul(self) -> f32 {
        match self {
            Mood::Energetic => 1.7,
            Mood::Angry => 1.5, // stomping off
            Mood::Happy => 1.2,
            Mood::Neutral => 1.0,
            Mood::Sad => 0.6,
            Mood::Crying => 0.4,
            Mood::Sleepy => 0.35,
        }
    }
}

/// Internal drives, each 0.0..=1.0. Higher = more satisfied.
#[derive(Debug, Clone, Copy)]
pub struct Needs {
    /// Physical energy. Drops while active, recovers while sleeping/resting.
    pub energy: f32,
    /// Social fulfilment. Rises with interaction (cursor nearby), decays alone.
    pub social: f32,
    /// Desire to play. Builds up, released by zooming/flying around.
    pub fun: f32,
    /// Affection toward the user. Slow to build, slow to fade.
    pub bond: f32,
}

impl Default for Needs {
    fn default() -> Self {
        Self { energy: 0.9, social: 0.6, fun: 0.5, bond: 0.2 }
    }
}

pub struct EmotionEngine {
    pub needs: Needs,
    pub mood: Mood,
    /// 0..1 — rises when mistreated (thrown around), decays over ~2.5 minutes.
    pub anger: f32,
    /// Seconds the current mood has been held. Used for hysteresis.
    mood_age: f32,
    /// Smoothed "excitement" used to bias toward energetic/happy.
    arousal: f32,
}

impl EmotionEngine {
    pub fn new() -> Self {
        Self { needs: Needs::default(), mood: Mood::Happy, anger: 0.0, mood_age: 0.0, arousal: 0.4 }
    }

    /// Advance the simulation.
    ///
    /// * `dt`            — seconds since last update.
    /// * `user_active`   — is the user currently moving mouse / working?
    /// * `cursor_near`   — is the cursor close to (or holding) the companion?
    /// * `is_sleeping`   — is the creature in a sleep behavior right now?
    /// * `is_resting`    — is it sitting idle (light recovery)?
    pub fn update(
        &mut self,
        dt: f32,
        user_active: bool,
        cursor_near: bool,
        is_sleeping: bool,
        is_resting: bool,
    ) {
        let n = &mut self.needs;

        // --- needs dynamics (per-minute rates scaled to dt) -----------------
        let m = dt / 60.0;
        if is_sleeping {
            n.energy = (n.energy + 0.50 * m).min(1.0);
        } else if is_resting {
            n.energy = (n.energy + 0.06 * m).min(1.0);
        } else {
            // Slow ambient drain (~45 min awake) — flying/running drains extra
            // via `exert`, so an active pet tires faster than a calm one.
            n.energy = (n.energy - 0.022 * m).max(0.0);
        }

        if cursor_near {
            n.social = (n.social + 0.50 * m).min(1.0);
            n.bond = (n.bond + 0.05 * m).min(1.0);
            n.fun = (n.fun + 0.10 * m).min(1.0);
        } else {
            n.social = (n.social - 0.08 * m).max(0.0);
        }

        // Boredom builds; relieved by activity (handled by brain calling `played`).
        n.fun = (n.fun - 0.04 * m).max(0.0);

        // Anger cools off on its own (~2.5 minutes from full).
        self.anger = (self.anger - dt / 150.0).max(0.0);

        // Arousal tracks recent user activity & social contact, smoothed.
        let target_arousal = if user_active { 0.75 } else { 0.25 }
            * (0.5 + 0.5 * n.energy)
            + if cursor_near { 0.15 } else { 0.0 };
        self.arousal += (target_arousal.clamp(0.0, 1.0) - self.arousal) * (dt / 6.0).min(1.0);

        // --- pick the *desired* mood from needs -----------------------------
        let desired = self.desired_mood();

        // --- hysteresis: only switch if the new mood persists ---------------
        self.mood_age += dt;
        if desired != self.mood {
            // Stronger urges switch faster; anger is near-instant.
            let inertia = match desired {
                Mood::Angry => 0.6,
                Mood::Crying => 1.5,
                Mood::Sleepy => 2.5,
                _ => 4.0,
            };
            if self.mood_age > inertia {
                self.mood = desired;
                self.mood_age = 0.0;
            }
        } else {
            self.mood_age = 0.0;
        }
    }

    fn desired_mood(&self) -> Mood {
        let n = &self.needs;
        // Priority order roughly mirrors human urgency.
        if n.energy < 0.15 {
            return Mood::Sleepy;
        }
        if self.anger > 0.45 {
            return Mood::Angry;
        }
        if n.social < 0.12 && n.bond > 0.25 {
            // Lonely *and* attached -> visibly upset.
            return Mood::Crying;
        }
        if n.social < 0.3 {
            return Mood::Sad;
        }
        if self.arousal > 0.7 && n.energy > 0.45 {
            return Mood::Energetic;
        }
        if n.social > 0.5 && n.energy > 0.4 {
            return Mood::Happy;
        }
        Mood::Neutral
    }

    /// Called by the brain when the creature does something playful.
    pub fn played(&mut self, amount: f32) {
        self.needs.fun = (self.needs.fun + amount).min(1.0);
    }

    /// Extra energy cost while flying/running hard.
    pub fn exert(&mut self, dt: f32) {
        self.needs.energy = (self.needs.energy - 0.05 * dt / 60.0).max(0.0);
    }

    /// Something exciting happened (a notification, a game) — perk up.
    pub fn excite(&mut self, amount: f32) {
        self.arousal = (self.arousal + amount).min(1.0);
        self.needs.fun = (self.needs.fun + amount * 0.5).min(1.0);
    }

    /// Mistreatment (dropped/thrown). Repeated quickly -> Angry mood.
    pub fn provoked(&mut self, amount: f32) {
        self.anger = (self.anger + amount).min(1.0);
        self.arousal = (self.arousal + amount * 0.4).min(1.0);
    }

    /// A little randomness so two companions never feel identical.
    pub fn jitter(&mut self, rng: &mut impl Rng) {
        self.needs.fun = (self.needs.fun + rng.gen_range(-0.02..0.02)).clamp(0.0, 1.0);
        self.arousal = (self.arousal + rng.gen_range(-0.03..0.03)).clamp(0.0, 1.0);
    }
}
