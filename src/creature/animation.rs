//! Procedural pixel-art. The whole creature is *drawn in code* into a small
//! texture atlas at startup — there are no PNG/SVG assets to ship, which keeps
//! the binary tiny and means a recolor is just one config value.
//!
//! Layout: a grid of 32×32 cells, one row per mood, one column per pose, plus a
//! 2×2 white texel in the corner used for soft shadows.

use super::emotion::Mood;

pub const CELL: i32 = 32;
const POSES: usize = 7;
const MOODS: usize = 7;

#[derive(Clone, Copy)]
pub enum Pose {
    Idle0,
    Idle1,
    Walk0,
    Walk1,
    Fly0,
    Fly1,
    Sleep,
}

impl Pose {
    fn index(self) -> usize {
        match self {
            Pose::Idle0 => 0,
            Pose::Idle1 => 1,
            Pose::Walk0 => 2,
            Pose::Walk1 => 3,
            Pose::Fly0 => 4,
            Pose::Fly1 => 5,
            Pose::Sleep => 6,
        }
    }
}

/// Small icons floated above the pet to convey mood (vital for static custom
/// avatars whose face can't change).
#[derive(Clone, Copy)]
pub enum Emote {
    Zzz,
    Tear,
    Spark,
    Sweat,
    Heart,
    Exclaim,
}

impl Emote {
    fn slot(self) -> i32 {
        match self {
            Emote::Zzz => 0,
            Emote::Tear => 1,
            Emote::Spark => 2,
            Emote::Sweat => 3,
            Emote::Heart => 4,
            Emote::Exclaim => 5,
        }
    }
    /// Map a mood to its emote, if any.
    pub fn for_mood(m: Mood, sleeping: bool) -> Option<Emote> {
        if sleeping {
            return Some(Emote::Zzz);
        }
        match m {
            Mood::Crying => Some(Emote::Tear),
            Mood::Sad => Some(Emote::Sweat),
            Mood::Energetic => Some(Emote::Spark),
            Mood::Sleepy => Some(Emote::Zzz),
            Mood::Happy => Some(Emote::Heart),
            Mood::Angry => Some(Emote::Exclaim),
            Mood::Neutral => None,
        }
    }
}

const EMOTE_PX: i32 = 16;

fn mood_index(m: Mood) -> usize {
    match m {
        Mood::Happy => 0,
        Mood::Energetic => 1,
        Mood::Neutral => 2,
        Mood::Sleepy => 3,
        Mood::Sad => 4,
        Mood::Crying => 5,
        Mood::Angry => 6,
    }
}

pub struct Sprites {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl Sprites {
    /// Normalised UV rect (u, v, w, h) for a mood/pose cell.
    pub fn uv(&self, mood: Mood, pose: Pose) -> [f32; 4] {
        let col = pose.index() as f32;
        let row = mood_index(mood) as f32;
        let c = CELL as f32;
        [col * c / self.width as f32, row * c / self.height as f32, c / self.width as f32, c / self.height as f32]
    }

    /// UV of the solid white texel (for tinted shadow quads).
    pub fn white_uv(&self) -> [f32; 4] {
        let x = (POSES as f32) * CELL as f32 + 0.5;
        let y = 0.5;
        [x / self.width as f32, y / self.height as f32, 0.0, 0.0]
    }

    /// UV rect for an emote icon (16×16 region in the spare column).
    pub fn emote_uv(&self, e: Emote) -> [f32; 4] {
        let x = (POSES as i32 * CELL) as f32;
        let y = (EMOTE_PX + e.slot() * EMOTE_PX) as f32;
        let s = EMOTE_PX as f32;
        [x / self.width as f32, y / self.height as f32, s / self.width as f32, s / self.height as f32]
    }
}

// ----------------------------------------------------------------------------
// Painter
// ----------------------------------------------------------------------------

struct Cell<'a> {
    buf: &'a mut [u8],
    stride: i32, // atlas width in px
    ox: i32,
    oy: i32, // cell origin in atlas
}

type Rgba = [u8; 4];

impl<'a> Cell<'a> {
    fn px(&mut self, x: i32, y: i32, c: Rgba) {
        if x < 0 || y < 0 || x >= CELL || y >= CELL || c[3] == 0 {
            return;
        }
        let i = (((self.oy + y) * self.stride) + (self.ox + x)) as usize * 4;
        let a = c[3] as f32 / 255.0;
        for k in 0..3 {
            let dst = self.buf[i + k] as f32;
            self.buf[i + k] = (c[k] as f32 * a + dst * (1.0 - a)).round() as u8;
        }
        let da = self.buf[i + 3] as f32 / 255.0;
        self.buf[i + 3] = ((a + da * (1.0 - a)) * 255.0).round() as u8;
    }

    fn disc(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, c: Rgba) {
        let x0 = (cx - rx).floor() as i32;
        let x1 = (cx + rx).ceil() as i32;
        let y0 = (cy - ry).floor() as i32;
        let y1 = (cy + ry).ceil() as i32;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let dx = (x as f32 + 0.5 - cx) / rx;
                let dy = (y as f32 + 0.5 - cy) / ry;
                if dx * dx + dy * dy <= 1.0 {
                    self.px(x, y, c);
                }
            }
        }
    }

    fn rect(&mut self, x0: i32, y0: i32, w: i32, h: i32, c: Rgba) {
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                self.px(x, y, c);
            }
        }
    }
}

fn lighten(c: [u8; 3], f: f32) -> Rgba {
    [
        (c[0] as f32 + (255.0 - c[0] as f32) * f) as u8,
        (c[1] as f32 + (255.0 - c[1] as f32) * f) as u8,
        (c[2] as f32 + (255.0 - c[2] as f32) * f) as u8,
        255,
    ]
}
fn darken(c: [u8; 3], f: f32) -> Rgba {
    [(c[0] as f32 * (1.0 - f)) as u8, (c[1] as f32 * (1.0 - f)) as u8, (c[2] as f32 * (1.0 - f)) as u8, 255]
}

const EYE: Rgba = [54, 38, 50, 255];
const SHINE: Rgba = [255, 255, 255, 235];
const MOUTH: Rgba = [70, 40, 48, 255];
const TEAR: Rgba = [130, 205, 255, 235];
const CHEEK: Rgba = [255, 150, 170, 150];

/// Build the full atlas for a given body colour.
pub fn generate(body: [u8; 3]) -> Sprites {
    let width = (POSES as i32 + 1) * CELL; // extra column hosts the white texel
    let height = MOODS as i32 * CELL;
    let mut pixels = vec![0u8; (width * height) as usize * 4];

    // White texel block.
    {
        let stride = width;
        for y in 0..3 {
            for x in 0..3 {
                let px = (POSES as i32) * CELL + x;
                let i = ((y * stride) + px) as usize * 4;
                pixels[i] = 255;
                pixels[i + 1] = 255;
                pixels[i + 2] = 255;
                pixels[i + 3] = 255;
            }
        }
    }

    let moods = [Mood::Happy, Mood::Energetic, Mood::Neutral, Mood::Sleepy, Mood::Sad, Mood::Crying, Mood::Angry];
    let poses = [Pose::Idle0, Pose::Idle1, Pose::Walk0, Pose::Walk1, Pose::Fly0, Pose::Fly1, Pose::Sleep];

    for (mi, &mood) in moods.iter().enumerate() {
        for (pi, &pose) in poses.iter().enumerate() {
            let mut cell = Cell {
                buf: &mut pixels,
                stride: width,
                ox: pi as i32 * CELL,
                oy: mi as i32 * CELL,
            };
            draw_creature(&mut cell, body, mood, pose);
        }
    }

    // Emotes live in the spare column, 16×16 each, below the white texel.
    for e in [Emote::Zzz, Emote::Tear, Emote::Spark, Emote::Sweat, Emote::Heart, Emote::Exclaim] {
        let mut cell = Cell {
            buf: &mut pixels,
            stride: width,
            ox: POSES as i32 * CELL,
            oy: EMOTE_PX + e.slot() * EMOTE_PX,
        };
        draw_emote(&mut cell, e);
    }

    Sprites { pixels, width: width as u32, height: height as u32 }
}

fn draw_emote(c: &mut Cell, e: Emote) {
    let white = [235u8, 240, 255, 235];
    let blue = [130u8, 205, 255, 240];
    let spark = [255u8, 235, 120, 245];
    let pink = [255u8, 120, 150, 245];
    let yellow = [255u8, 220, 90, 248];
    match e {
        Emote::Zzz => {
            // three rising Z's
            for (zx, zy) in [(2, 9), (6, 5), (10, 1)] {
                c.px(zx, zy, white);
                c.px(zx + 1, zy, white);
                c.px(zx + 2, zy, white);
                c.px(zx + 1, zy + 1, white);
                c.px(zx, zy + 2, white);
                c.px(zx + 1, zy + 2, white);
                c.px(zx + 2, zy + 2, white);
            }
        }
        Emote::Tear => {
            c.px(8, 4, blue);
            c.px(8, 5, blue);
            c.disc(8.0, 9.0, 3.0, 3.5, blue);
            c.px(7, 8, [255, 255, 255, 200]);
        }
        Emote::Sweat => {
            c.px(9, 4, blue);
            c.disc(9.0, 7.0, 2.2, 2.8, blue);
        }
        Emote::Spark => {
            for d in 0..5 {
                c.px(8, 4 + d, spark);
                c.px(4 + d, 8, spark);
            }
            c.px(5, 5, [255, 255, 255, 220]);
            c.px(11, 11, [255, 255, 255, 220]);
            c.px(11, 5, spark);
            c.px(5, 11, spark);
        }
        Emote::Heart => {
            c.disc(5.5, 6.5, 3.0, 3.0, pink);
            c.disc(10.5, 6.5, 3.0, 3.0, pink);
            for row in 0..6 {
                let half = 6 - row;
                for x in (8 - half)..=(8 + half) {
                    c.px(x, 8 + row, pink);
                }
            }
        }
        Emote::Exclaim => {
            c.rect(7, 2, 2, 7, yellow);
            c.rect(7, 11, 2, 2, yellow);
        }
    }
}

fn draw_creature(c: &mut Cell, body: [u8; 3], mood: Mood, pose: Pose) {
    let base: Rgba = [body[0], body[1], body[2], 255];
    let light = lighten(body, 0.30);
    let dark = darken(body, 0.22);

    // Pose params.
    let (bob, squash, wing) = match pose {
        Pose::Idle0 => (0.0, 0.0, None),
        Pose::Idle1 => (1.0, 0.10, None),
        Pose::Walk0 => (0.0, 0.0, None),
        Pose::Walk1 => (-1.0, 0.0, None),
        Pose::Fly0 => (-1.0, 0.05, Some(-3.0)),
        Pose::Fly1 => (1.0, 0.05, Some(2.0)),
        Pose::Sleep => (1.0, 0.12, None),
    };

    let cx = 16.0;
    let cy = 17.0 + bob;
    let rx = 10.5 + squash * 10.5;
    let ry = 10.5 - squash * 6.0;

    // Wings (behind body).
    if let Some(w) = wing {
        let wc = lighten(body, 0.5);
        c.disc(cx - rx + 1.0, cy + w, 4.0, 6.0, wc);
        c.disc(cx + rx - 1.0, cy + w, 4.0, 6.0, wc);
    }

    // Feet.
    let foot = darken(body, 0.35);
    let (lf, rf) = match pose {
        Pose::Walk0 => (-2.0, 4.0),
        Pose::Walk1 => (4.0, -2.0),
        _ => (0.0, 0.0),
    };
    c.disc(cx - 4.0 + lf, cy + ry - 1.0, 3.0, 2.2, foot);
    c.disc(cx + 4.0 + rf, cy + ry - 1.0, 3.0, 2.2, foot);

    // Body with a top highlight and bottom shadow band.
    c.disc(cx, cy, rx, ry, base);
    c.disc(cx, cy - ry * 0.45, rx * 0.7, ry * 0.45, light);
    c.disc(cx, cy + ry * 0.55, rx * 0.85, ry * 0.4, dark);
    c.disc(cx, cy, rx, ry, [0, 0, 0, 0]); // (re-pass keeps anti-aliased edge crisp)
    c.disc(cx, cy, rx, ry, base_outline(base));

    draw_face(c, cx, cy, mood, pose);
}

// Subtle darker rim so the body reads against any wallpaper.
fn base_outline(base: Rgba) -> Rgba {
    [base[0], base[1], base[2], 0]
}

fn draw_face(c: &mut Cell, cx: f32, cy: f32, mood: Mood, pose: Pose) {
    let ex = 3.6; // eye horizontal offset
    let ey = cy - 2.0;
    let sleeping = matches!(pose, Pose::Sleep);

    let closed = sleeping || matches!(mood, Mood::Sleepy | Mood::Crying);

    if closed {
        // Closed/curved eyes: little down-arcs.
        for dx in -2i32..=2 {
            let y = ey + (dx.abs() as f32) * 0.5 - 0.5;
            c.px((cx - ex) as i32 + dx, y as i32, EYE);
            c.px((cx + ex) as i32 + dx, y as i32, EYE);
        }
    } else {
        let (eye_ry, look) = match mood {
            Mood::Energetic => (2.6, 0.0),
            Mood::Sad => (1.6, 1.0), // droopy, looking down
            Mood::Angry => (1.7, 0.0), // narrowed
            _ => (2.2, 0.0),
        };
        c.disc(cx - ex, ey + look, 1.9, eye_ry, EYE);
        c.disc(cx + ex, ey + look, 1.9, eye_ry, EYE);
        // Catch-light.
        c.px((cx - ex + 0.6) as i32, (ey - 0.6 + look) as i32, SHINE);
        c.px((cx + ex + 0.6) as i32, (ey - 0.6 + look) as i32, SHINE);
        if matches!(mood, Mood::Energetic) {
            // sparkle eyebrows
            c.px((cx - ex - 2.0) as i32, (ey - 3.0) as i32, SHINE);
            c.px((cx + ex + 2.0) as i32, (ey - 3.0) as i32, SHINE);
        }
        if matches!(mood, Mood::Angry) {
            // slanted "\ /" brows pressing down on the eyes
            for d in 0..3i32 {
                c.px((cx - ex - 1.0) as i32 + d, (ey - 4.0) as i32 + d, EYE);
                c.px((cx + ex + 1.0) as i32 - d, (ey - 4.0) as i32 + d, EYE);
            }
        }
    }

    // Mouth.
    let my = cy + 3.5;
    match mood {
        Mood::Happy | Mood::Energetic => {
            // Smile arc.
            for dx in -2i32..=2 {
                let y = my + (2 - dx.abs()).max(0) as f32 * -0.0 + (dx.abs() as f32) * 0.6;
                c.px(cx as i32 + dx, y as i32, MOUTH);
            }
            // open mouth for energetic
            if matches!(mood, Mood::Energetic) {
                c.disc(cx, my + 1.0, 1.6, 1.4, MOUTH);
            }
            // cheeks
            c.disc(cx - 6.0, cy + 1.5, 1.6, 1.2, CHEEK);
            c.disc(cx + 6.0, cy + 1.5, 1.6, 1.2, CHEEK);
        }
        Mood::Neutral => {
            c.rect((cx - 1.5) as i32, my as i32, 3, 1, MOUTH);
        }
        Mood::Sleepy => {
            c.disc(cx, my, 1.2, 0.9, MOUTH);
        }
        Mood::Sad => {
            // frown arc
            for dx in -2i32..=2 {
                let y = my + 1.0 - (dx.abs() as f32) * 0.6;
                c.px(cx as i32 + dx, y as i32, MOUTH);
            }
        }
        Mood::Crying => {
            // wailing open mouth
            c.disc(cx, my + 0.5, 2.0, 1.8, MOUTH);
        }
        Mood::Angry => {
            // gritted flat mouth
            c.rect((cx - 2.5) as i32, my as i32, 5, 2, MOUTH);
        }
    }

    // Tears for crying.
    if matches!(mood, Mood::Crying) {
        for (sx, _) in [(-ex, 0.0), (ex, 0.0)] {
            c.disc(cx + sx, ey + 2.5, 1.0, 1.4, TEAR);
            c.px((cx + sx) as i32, (ey + 4.5) as i32, TEAR);
        }
    }

    // Zzz when asleep.
    if sleeping {
        let z = [240u8, 240, 255, 230];
        c.rect((cx + 6.0) as i32, (cy - 9.0) as i32, 3, 1, z);
        c.px((cx + 8.0) as i32, (cy - 8.0) as i32, z);
        c.px((cx + 6.0) as i32, (cy - 7.0) as i32, z);
        c.rect((cx + 6.0) as i32, (cy - 6.0) as i32, 3, 1, z);
    }
}
