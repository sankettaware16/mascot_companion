//! Pet-pack loader (engine side). Reads a pack produced by `companion-convert`
//! WITHOUT pulling in any image decoder — `sprite.rgba` is a raw header + RGBA
//! blob, so the resident binary stays tiny and starts instantly.

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct PetMeta {
    #[allow(dead_code)]
    name: String,
    canvas: u32,
    fig_height: u32,
    bottom_margin: u32,
}

/// A loaded custom pet: its texture plus the geometry needed to place it.
pub struct PetPack {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Figure height within the square canvas (source px).
    pub fig_height: u32,
    /// Transparent px below the figure's feet (source px).
    pub bottom_margin: u32,
}

impl PetPack {
    /// Look for a pet named `name` under `./pets/<name>/` or
    /// `~/.config/companion/pets/<name>/`.
    pub fn load(name: &str) -> Option<PetPack> {
        for base in search_roots() {
            let dir = base.join(name);
            if dir.join("sprite.rgba").exists() {
                match Self::load_dir(&dir) {
                    Ok(p) => return Some(p),
                    Err(e) => eprintln!("[companion] pet '{name}' load error: {e}"),
                }
            }
        }
        eprintln!("[companion] pet '{name}' not found (run companion-convert first)");
        None
    }

    fn load_dir(dir: &Path) -> Result<PetPack, String> {
        let meta_s = std::fs::read_to_string(dir.join("pet.toml")).map_err(|e| e.to_string())?;
        let meta: PetMeta = toml::from_str(&meta_s).map_err(|e| e.to_string())?;

        let raw = std::fs::read(dir.join("sprite.rgba")).map_err(|e| e.to_string())?;
        if raw.len() < 8 {
            return Err("sprite.rgba too small".into());
        }
        let width = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
        let height = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]);
        let want = 8 + (width as usize * height as usize * 4);
        if raw.len() != want {
            return Err(format!("sprite.rgba size mismatch: {} != {want}", raw.len()));
        }
        let _ = meta.canvas;
        Ok(PetPack {
            pixels: raw[8..].to_vec(),
            width,
            height,
            fig_height: meta.fig_height.max(1),
            bottom_margin: meta.bottom_margin,
        })
    }
}

fn search_roots() -> Vec<PathBuf> {
    let mut v = vec![PathBuf::from("pets")];
    if let Ok(home) = std::env::var("HOME") {
        v.push(PathBuf::from(home).join(".config/companion/pets"));
    }
    v
}
