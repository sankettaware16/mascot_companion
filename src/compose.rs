//! CPU frame compositor. Used on platforms where the desktop compositor
//! refuses per-pixel-alpha GPU surfaces (Windows): the whole pet frame —
//! banner, sprite (scaled / flipped / rotated / tinted), emote and shadow —
//! is rendered into one small premultiplied-BGRA buffer that
//! `UpdateLayeredWindow` can present directly.
//!
//! Pure portable Rust: compiles and is testable everywhere; only the
//! presenter glue is Windows-specific.

use crate::banner::Banner;

/// A source rectangle inside a straight-alpha RGBA image.
#[derive(Clone, Copy)]
pub struct Src<'a> {
    pub pixels: &'a [u8],
    /// Width in px of the whole source image (row stride / 4).
    pub stride: u32,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

pub struct ComposeArgs<'a> {
    pub sprite: Src<'a>,
    /// Destination scale per axis (squash-and-stretch).
    pub scale_x: f32,
    pub scale_y: f32,
    pub flip: bool,
    /// Rotation in radians around the sprite centre.
    pub angle: f32,
    /// Straight-alpha tint multiplier.
    pub tint: [f32; 4],
    /// Vertical bob: positive lifts the sprite up inside its box.
    pub bob: f32,
    pub emote: Option<Src<'a>>,
    pub emote_size: f32,
    pub banner: Option<&'a Banner>,
    /// Ground shadow (width px, alpha 0..1); drawn at the sprite-box bottom.
    pub shadow: Option<(f32, f32)>,
}

pub struct Frame {
    /// Premultiplied BGRA, row-major, `w * h * 4` bytes.
    pub bgra: Vec<u8>,
    pub w: i32,
    pub h: i32,
    /// Sprite centre within the canvas — align this to the creature's
    /// global position when placing the window.
    pub center_x: i32,
    pub center_y: i32,
}

pub fn compose(a: &ComposeArgs) -> Frame {
    // Box large enough that any rotation of the scaled sprite fits.
    let dw = a.sprite.w as f32 * a.scale_x;
    let dh = a.sprite.h as f32 * a.scale_y;
    let box_side = (dw.max(dh) * 1.45).ceil() as i32;

    let (ban_w, ban_h) = a
        .banner
        .map(|b| (b.width as i32, b.height as i32))
        .unwrap_or((0, 0));
    let gap = if ban_h > 0 { 4 } else { 0 };

    let w = box_side.max(ban_w);
    let h = ban_h + gap + box_side;
    let mut bgra = vec![0u8; (w * h * 4) as usize];

    let cx = w / 2;
    let box_top = ban_h + gap;
    let cy = box_top + box_side / 2;

    // --- shadow (under everything) ---------------------------------------
    if let Some((sw, salpha)) = a.shadow {
        let rx = sw * 0.5;
        let ry = (sw * 0.165).max(2.0);
        let sy = (box_top + box_side) as f32 - ry - 1.0;
        ellipse(&mut bgra, w, h, cx as f32, sy, rx, ry, [0, 0, 0], salpha);
    }

    // --- sprite: inverse-mapped nearest sampling with rotation ------------
    let (sin, cos) = a.angle.sin_cos();
    let half_box = box_side as f32 * 0.5;
    let bob = a.bob;
    for dy in 0..box_side {
        for dx in 0..box_side {
            // Destination px relative to sprite centre (bob lifts content up).
            let rx = dx as f32 + 0.5 - half_box;
            let ry = dy as f32 + 0.5 - half_box + bob;
            // Undo rotation.
            let ux = rx * cos + ry * sin;
            let uy = -rx * sin + ry * cos;
            // Undo scale into source-cell space.
            let mut sx = ux / a.scale_x + a.sprite.w as f32 * 0.5;
            let sy = uy / a.scale_y + a.sprite.h as f32 * 0.5;
            if a.flip {
                sx = a.sprite.w as f32 - sx;
            }
            if sx < 0.0 || sy < 0.0 || sx >= a.sprite.w as f32 || sy >= a.sprite.h as f32 {
                continue;
            }
            let si =
                (((a.sprite.y + sy as u32) * a.sprite.stride + a.sprite.x + sx as u32) * 4) as usize;
            let p = &a.sprite.pixels[si..si + 4];
            if p[3] == 0 {
                continue;
            }
            let alpha = p[3] as f32 / 255.0 * a.tint[3];
            let px = cx - box_side / 2 + dx;
            let py = box_top + dy;
            blend(
                &mut bgra,
                w,
                px,
                py,
                [
                    (p[0] as f32 * a.tint[0]).min(255.0),
                    (p[1] as f32 * a.tint[1]).min(255.0),
                    (p[2] as f32 * a.tint[2]).min(255.0),
                ],
                alpha,
            );
        }
    }

    // --- emote (top-right of the sprite box, no rotation) ------------------
    if let Some(e) = a.emote {
        let es = a.emote_size.max(8.0);
        let ex0 = cx as f32 + box_side as f32 * 0.18;
        let ey0 = box_top as f32 + box_side as f32 * 0.02;
        for dy in 0..es as i32 {
            for dx in 0..es as i32 {
                let sx = (dx as f32 / es * e.w as f32) as u32;
                let sy = (dy as f32 / es * e.h as f32) as u32;
                let si = (((e.y + sy) * e.stride + e.x + sx) * 4) as usize;
                let p = &e.pixels[si..si + 4];
                if p[3] == 0 {
                    continue;
                }
                blend(
                    &mut bgra,
                    w,
                    ex0 as i32 + dx,
                    ey0 as i32 + dy,
                    [p[0] as f32, p[1] as f32, p[2] as f32],
                    p[3] as f32 / 255.0,
                );
            }
        }
    }

    // --- banner (top-centre) -------------------------------------------------
    if let Some(b) = a.banner {
        let bx0 = (w - ban_w) / 2;
        for dy in 0..ban_h {
            for dx in 0..ban_w {
                let si = ((dy * ban_w + dx) * 4) as usize;
                let p = &b.pixels[si..si + 4];
                if p[3] == 0 {
                    continue;
                }
                blend(
                    &mut bgra,
                    w,
                    bx0 + dx,
                    dy,
                    [p[0] as f32, p[1] as f32, p[2] as f32],
                    p[3] as f32 / 255.0,
                );
            }
        }
    }

    Frame { bgra, w, h, center_x: cx, center_y: cy }
}

/// Source-over blend of a straight-alpha RGB+alpha sample onto the
/// premultiplied-BGRA canvas.
fn blend(buf: &mut [u8], w: i32, x: i32, y: i32, rgb: [f32; 3], alpha: f32) {
    if x < 0 || y < 0 || x >= w {
        return;
    }
    let i = ((y * w + x) * 4) as usize;
    if i + 3 >= buf.len() {
        return;
    }
    let inv = 1.0 - alpha;
    // BGRA, premultiplied.
    buf[i] = (rgb[2] * alpha + buf[i] as f32 * inv) as u8;
    buf[i + 1] = (rgb[1] * alpha + buf[i + 1] as f32 * inv) as u8;
    buf[i + 2] = (rgb[0] * alpha + buf[i + 2] as f32 * inv) as u8;
    buf[i + 3] = (alpha * 255.0 + buf[i + 3] as f32 * inv) as u8;
}

#[allow(clippy::too_many_arguments)]
fn ellipse(buf: &mut [u8], w: i32, h: i32, cx: f32, cy: f32, rx: f32, ry: f32, rgb: [u8; 3], alpha: f32) {
    let x0 = (cx - rx).floor().max(0.0) as i32;
    let x1 = (cx + rx).ceil().min(w as f32 - 1.0) as i32;
    let y0 = (cy - ry).floor().max(0.0) as i32;
    let y1 = (cy + ry).ceil().min(h as f32 - 1.0) as i32;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let dx = (x as f32 + 0.5 - cx) / rx;
            let dy = (y as f32 + 0.5 - cy) / ry;
            let d = dx * dx + dy * dy;
            if d <= 1.0 {
                // soft edge
                let a = alpha * (1.0 - d * d * 0.6);
                blend(buf, w, x, y, [rgb[0] as f32, rgb[1] as f32, rgb[2] as f32], a);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2×2 pure-red opaque sprite, scaled up: the canvas centre must be
    /// premultiplied-BGRA red, and corners (outside the rotated sprite but
    /// inside the box margin) transparent.
    #[test]
    fn sprite_lands_centered_in_bgra() {
        let px = [255u8, 0, 0, 255].repeat(4); // 2x2 RGBA red
        let f = compose(&ComposeArgs {
            sprite: Src { pixels: &px, stride: 2, x: 0, y: 0, w: 2, h: 2 },
            scale_x: 8.0,
            scale_y: 8.0,
            flip: false,
            angle: 0.0,
            tint: [1.0; 4],
            bob: 0.0,
            emote: None,
            emote_size: 0.0,
            banner: None,
            shadow: None,
        });
        let c = ((f.center_y * f.w + f.center_x) * 4) as usize;
        assert_eq!(f.bgra[c], 0, "blue channel");
        assert_eq!(f.bgra[c + 2], 255, "red channel");
        assert_eq!(f.bgra[c + 3], 255, "alpha");
        let corner = 0usize;
        assert_eq!(f.bgra[corner + 3], 0, "corner transparent");
    }

    /// A banner adds height above the sprite box and lands at the top.
    #[test]
    fn banner_sits_above_sprite() {
        let px = [0u8, 255, 0, 255].repeat(4);
        let banner = crate::banner::Banner::render("hi");
        let f = compose(&ComposeArgs {
            sprite: Src { pixels: &px, stride: 2, x: 0, y: 0, w: 2, h: 2 },
            scale_x: 4.0,
            scale_y: 4.0,
            flip: false,
            angle: 0.0,
            tint: [1.0; 4],
            bob: 0.0,
            emote: None,
            emote_size: 0.0,
            banner: Some(&banner),
            shadow: None,
        });
        assert!(f.w >= banner.width as i32);
        assert!(f.h > banner.height as i32);
        // Some banner pixel near the top row range must be opaque-ish.
        let any_banner = (0..banner.height as i32)
            .any(|y| (0..f.w).any(|x| f.bgra[((y * f.w + x) * 4 + 3) as usize] > 100));
        assert!(any_banner, "banner pixels present at top");
    }

    /// 180° rotation keeps an asymmetric sprite inside the box and flips it.
    #[test]
    fn rotation_stays_in_bounds() {
        // 2x1 sprite: left red, right blue.
        let px = [255u8, 0, 0, 255, 0, 0, 255, 255].to_vec();
        let f = compose(&ComposeArgs {
            sprite: Src { pixels: &px, stride: 2, x: 0, y: 0, w: 2, h: 1 },
            scale_x: 10.0,
            scale_y: 10.0,
            flip: false,
            angle: std::f32::consts::PI,
            tint: [1.0; 4],
            bob: 0.0,
            emote: None,
            emote_size: 0.0,
            banner: None,
            shadow: None,
        });
        // After 180°, the RIGHT of the canvas should be red (was left).
        let y = f.center_y;
        let xr = f.center_x + 4;
        let i = ((y * f.w + xr) * 4) as usize;
        assert_eq!(f.bgra[i + 2], 255, "red now on the right");
        assert_eq!(f.bgra[i], 0, "blue gone from the right");
    }
}
