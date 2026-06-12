//! Speech-bubble rasteriser. Renders a short line of text into a small RGBA
//! texture using the built-in 8×8 bitmap font (no font files to ship). The
//! result is uploaded as a single quad above the creature's head.

use font8x8::{UnicodeFonts, BASIC_FONTS};

pub struct Banner {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

const SCALE: i32 = 2;
const PAD: i32 = 7;
const GLYPH: i32 = 8;
const TAIL: i32 = 6;

const BG: [u8; 4] = [26, 28, 40, 230];
const BORDER: [u8; 4] = [120, 132, 165, 240];
const INK: [u8; 4] = [240, 242, 255, 255];

impl Banner {
    pub fn render(text: &str) -> Banner {
        let text: String = text.chars().take(42).collect();
        let n = text.chars().count().max(1) as i32;

        let text_w = n * GLYPH * SCALE;
        let text_h = GLYPH * SCALE;
        let box_w = text_w + PAD * 2;
        let box_h = text_h + PAD * 2;
        let w = box_w;
        let h = box_h + TAIL;

        let mut px = vec![0u8; (w * h) as usize * 4];
        let mut set = |x: i32, y: i32, c: [u8; 4]| {
            if x < 0 || y < 0 || x >= w || y >= h {
                return;
            }
            let i = (y * w + x) as usize * 4;
            // straight-alpha source-over onto whatever's there
            let a = c[3] as f32 / 255.0;
            for k in 0..3 {
                px[i + k] = (c[k] as f32 * a + px[i + k] as f32 * (1.0 - a)) as u8;
            }
            px[i + 3] = px[i + 3].max(c[3]);
        };

        // Rounded box body.
        let r = 5;
        for y in 0..box_h {
            for x in 0..box_w {
                let corner = (x < r && y < r && dist(x, y, r, r) > r as f32)
                    || (x >= box_w - r && y < r && dist(x, y, box_w - r, r) > r as f32)
                    || (x < r && y >= box_h - r && dist(x, y, r, box_h - r) > r as f32)
                    || (x >= box_w - r && y >= box_h - r && dist(x, y, box_w - r, box_h - r) > r as f32);
                if corner {
                    continue;
                }
                let edge = x == 0 || x == box_w - 1 || y == 0 || y == box_h - 1 || near_corner(x, y, box_w, box_h, r);
                set(x, y, if edge { BORDER } else { BG });
            }
        }

        // Downward tail.
        for t in 0..TAIL {
            let half = TAIL - t;
            let cx = box_w / 2;
            for x in (cx - half)..(cx + half) {
                set(x, box_h + t, BG);
            }
        }

        // Text.
        for (gi, ch) in text.chars().enumerate() {
            let glyph = BASIC_FONTS.get(ch).unwrap_or([0u8; 8]);
            let gx = PAD + gi as i32 * GLYPH * SCALE;
            let gy = PAD;
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..8 {
                    if bits & (1 << col) != 0 {
                        for sy in 0..SCALE {
                            for sx in 0..SCALE {
                                set(gx + col * SCALE + sx, gy + row as i32 * SCALE + sy, INK);
                            }
                        }
                    }
                }
            }
        }

        Banner { pixels: px, width: w as u32, height: h as u32 }
    }
}

fn dist(x: i32, y: i32, cx: i32, cy: i32) -> f32 {
    (((x - cx).pow(2) + (y - cy).pow(2)) as f32).sqrt()
}

fn near_corner(x: i32, y: i32, w: i32, h: i32, r: i32) -> bool {
    let d = |cx: i32, cy: i32| ((x - cx).pow(2) + (y - cy).pow(2)) as f32 <= (r * r) as f32 + 1.0
        && ((x - cx).pow(2) + (y - cy).pow(2)) as f32 >= (r * r) as f32 - (r as f32 * 1.5);
    d(r, r) || d(w - r, r) || d(r, h - r) || d(w - r, h - r)
}
