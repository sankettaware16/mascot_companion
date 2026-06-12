//! companion-convert — turn a downloaded avatar into a working pet pack.
//!
//!     companion-convert avatars/thor.svg
//!     companion-convert avatars/thor.png  thor
//!
//! Accepts SVG (rasterised) or any common raster image (PNG/JPG/GIF/WEBP).
//! Auto-trims the transparent margin, bottom-anchors the figure (so it stands
//! on the ground), squares the canvas (so it can tilt/rotate without clipping),
//! and writes `pets/<name>/`:
//!   * sprite.rgba   raw 8-byte header (w,h LE u32) + straight-alpha RGBA — the
//!                   engine reads this directly, no image decoder needed.
//!   * preview.png   so you can eyeball the result.
//!   * pet.toml      metadata the engine uses to place/scale the figure.

use image::{imageops, GenericImageView, RgbaImage};
use std::path::{Path, PathBuf};

const MAX_DIM: u32 = 160; // working resolution of the figure
const ALPHA_CUTOFF: u8 = 16;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: companion-convert <image.svg|png|jpg> [name]");
        std::process::exit(2);
    }
    let input = PathBuf::from(&args[1]);
    let name = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| input.file_stem().and_then(|s| s.to_str()).unwrap_or("pet").to_string());
    let name = sanitize(&name);

    match run(&input, &name) {
        Ok(dir) => {
            println!("\n✓ Created pet '{name}' in {}", dir.display());
            println!("\nNext steps:");
            println!("  1. add this line to config.toml:   pet = \"{name}\"");
            println!("  2. run:                             companion\n");
        }
        Err(e) => {
            eprintln!("convert failed: {e}");
            std::process::exit(1);
        }
    }
}

fn run(input: &Path, name: &str) -> Result<PathBuf, String> {
    let bytes = std::fs::read(input).map_err(|e| format!("cannot read {}: {e}", input.display()))?;
    let ext = input.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

    let raw = if ext == "svg" {
        rasterize_svg(&bytes)?
    } else {
        image::load_from_memory(&bytes)
            .map_err(|e| format!("unsupported image: {e}"))?
            .to_rgba8()
    };

    // 1. trim transparent margin.
    let trimmed = trim(&raw).ok_or("image is fully transparent")?;

    // 2. downscale to a sane working size (never upscale).
    let (tw, th) = trimmed.dimensions();
    let maxd = tw.max(th);
    let fig = if maxd > MAX_DIM {
        let s = MAX_DIM as f32 / maxd as f32;
        imageops::resize(&trimmed, (tw as f32 * s) as u32, (th as f32 * s) as u32, imageops::FilterType::Lanczos3)
    } else {
        trimmed
    };
    let (fw, fh) = fig.dimensions();

    // 3. square canvas, figure centred horizontally and bottom-anchored.
    let pad = (maxd as f32 * 0.10).max(4.0) as u32;
    let side = fw.max(fh) + pad * 2;
    let mut canvas = RgbaImage::new(side, side);
    let ox = (side - fw) / 2;
    let oy = side - pad - fh; // bottom-anchored
    imageops::overlay(&mut canvas, &fig, ox as i64, oy as i64);

    // 4. write the pack.
    let dir = PathBuf::from("pets").join(name);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;

    write_rgba(&dir.join("sprite.rgba"), &canvas).map_err(|e| e.to_string())?;
    canvas
        .save(dir.join("preview.png"))
        .map_err(|e| format!("save preview: {e}"))?;

    let toml = format!(
        "name = \"{name}\"\n# square canvas side, in source pixels\ncanvas = {side}\n# figure height within the canvas\nfig_height = {fh}\n# transparent pixels below the figure's feet\nbottom_margin = {pad}\n"
    );
    std::fs::write(dir.join("pet.toml"), toml).map_err(|e| format!("write pet.toml: {e}"))?;

    println!("  source figure: {fw}x{fh}px -> canvas {side}x{side}px");
    Ok(dir)
}

fn rasterize_svg(bytes: &[u8]) -> Result<RgbaImage, String> {
    use resvg::tiny_skia;
    use resvg::usvg;

    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_data(bytes, &opt).map_err(|e| format!("bad svg: {e}"))?;
    let size = tree.size();
    // Render so the larger side is ~256px for a crisp source.
    let target = 256.0;
    let scale = target / size.width().max(size.height());
    let pw = (size.width() * scale).ceil().max(1.0) as u32;
    let ph = (size.height() * scale).ceil().max(1.0) as u32;

    let mut pixmap = tiny_skia::Pixmap::new(pw, ph).ok_or("pixmap alloc failed")?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());

    // tiny-skia stores premultiplied; un-premultiply to straight alpha.
    let mut img = RgbaImage::new(pw, ph);
    for (px, out) in pixmap.pixels().iter().zip(img.pixels_mut()) {
        let c = px.demultiply();
        *out = image::Rgba([c.red(), c.green(), c.blue(), c.alpha()]);
    }
    Ok(img)
}

/// Bounding box of non-(near-)transparent pixels, cropped.
fn trim(img: &RgbaImage) -> Option<RgbaImage> {
    let (w, h) = img.dimensions();
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0u32, 0u32);
    for (x, y, p) in img.enumerate_pixels() {
        if p.0[3] > ALPHA_CUTOFF {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if x1 < x0 || y1 < y0 {
        return None;
    }
    Some(img.view(x0, y0, x1 - x0 + 1, y1 - y0 + 1).to_image())
}

fn write_rgba(path: &Path, img: &RgbaImage) -> std::io::Result<()> {
    let (w, h) = img.dimensions();
    let mut out = Vec::with_capacity(8 + (w * h * 4) as usize);
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&h.to_le_bytes());
    out.extend_from_slice(img.as_raw());
    std::fs::write(path, out)
}

fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c.to_ascii_lowercase() } else { '-' })
        .collect();
    if s.is_empty() { "pet".to_string() } else { s }
}
