//! Platform glue (X11 / XWayland).
//!
//! A click-through overlay never receives input events, so everything the pet
//! "senses" about the desktop comes from polling one X11 connection:
//!
//! * `pointer()` — global cursor position *and* left-button state (this is what
//!   makes grab-and-throw possible without ever blocking your clicks).
//! * `scout()`   — EWMH window enumeration: the visible top edges of open
//!   windows become walkable platforms, the active window becomes a perch
//!   target, and notification popups become things to run over and gawk at.
//!
//! Read-only, privacy-safe: geometry and window *types* only — never titles,
//! never content. On a pure-Wayland session these return `None` and the pet
//! simply roams on its own.

use crate::world::Plat;
use glam::Vec2;

pub struct PointerState {
    pub pos: Vec2,
    pub button1: bool,
}

#[derive(Default)]
pub struct ScoutInfo {
    /// Visible window-top segments, bottom-to-top stacking order.
    pub platforms: Vec<Plat>,
    /// Widest visible top segment of the focused window.
    pub active_top: Option<Plat>,
    /// Notification popups currently on screen: (window id, centre).
    pub notifications: Vec<(u32, Vec2)>,
}

#[cfg(unix)]
mod imp {
    use super::*;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, KeyButMask, Window};
    use x11rb::rust_connection::RustConnection;

    pub struct Probe {
        conn: RustConnection,
        root: Window,
        atoms: Atoms,
    }

    struct Atoms {
        client_list_stacking: u32,
        active_window: u32,
        wm_window_type: u32,
        t_normal: u32,
        t_dialog: u32,
        t_notification: u32,
        t_dock: u32,
        t_desktop: u32,
        t_menu: u32,
        t_toolbar: u32,
        t_splash: u32,
        t_utility: u32,
        wm_state: u32,
        st_hidden: u32,
        st_fullscreen: u32,
        gtk_frame_extents: u32,
        current_desktop: u32,
        wm_desktop: u32,
    }

    fn atom(conn: &RustConnection, name: &str) -> Option<u32> {
        Some(conn.intern_atom(false, name.as_bytes()).ok()?.reply().ok()?.atom)
    }

    impl Probe {
        pub fn new() -> Option<Self> {
            let (conn, screen_num) = x11rb::connect(None).ok()?;
            let root = conn.setup().roots.get(screen_num)?.root;
            let atoms = Atoms {
                client_list_stacking: atom(&conn, "_NET_CLIENT_LIST_STACKING")?,
                active_window: atom(&conn, "_NET_ACTIVE_WINDOW")?,
                wm_window_type: atom(&conn, "_NET_WM_WINDOW_TYPE")?,
                t_normal: atom(&conn, "_NET_WM_WINDOW_TYPE_NORMAL")?,
                t_dialog: atom(&conn, "_NET_WM_WINDOW_TYPE_DIALOG")?,
                t_notification: atom(&conn, "_NET_WM_WINDOW_TYPE_NOTIFICATION")?,
                t_dock: atom(&conn, "_NET_WM_WINDOW_TYPE_DOCK")?,
                t_desktop: atom(&conn, "_NET_WM_WINDOW_TYPE_DESKTOP")?,
                t_menu: atom(&conn, "_NET_WM_WINDOW_TYPE_MENU")?,
                t_toolbar: atom(&conn, "_NET_WM_WINDOW_TYPE_TOOLBAR")?,
                t_splash: atom(&conn, "_NET_WM_WINDOW_TYPE_SPLASH")?,
                t_utility: atom(&conn, "_NET_WM_WINDOW_TYPE_UTILITY")?,
                wm_state: atom(&conn, "_NET_WM_STATE")?,
                st_hidden: atom(&conn, "_NET_WM_STATE_HIDDEN")?,
                st_fullscreen: atom(&conn, "_NET_WM_STATE_FULLSCREEN")?,
                gtk_frame_extents: atom(&conn, "_GTK_FRAME_EXTENTS")?,
                current_desktop: atom(&conn, "_NET_CURRENT_DESKTOP")?,
                wm_desktop: atom(&conn, "_NET_WM_DESKTOP")?,
            };
            Some(Self { conn, root, atoms })
        }

        pub fn pointer(&self) -> Option<PointerState> {
            let r = self.conn.query_pointer(self.root).ok()?.reply().ok()?;
            let button1 = u16::from(r.mask) & u16::from(KeyButMask::BUTTON1) != 0;
            Some(PointerState {
                pos: Vec2::new(r.root_x as f32, r.root_y as f32),
                button1,
            })
        }

        fn prop32(&self, win: Window, prop: u32, ty: AtomEnum) -> Option<Vec<u32>> {
            let r = self.conn.get_property(false, win, prop, ty, 0, 1024).ok()?.reply().ok()?;
            let vals: Vec<u32> = r.value32()?.collect();
            Some(vals)
        }

        pub fn scout(&self) -> Option<ScoutInfo> {
            let a = &self.atoms;
            let stacking = self.prop32(self.root, a.client_list_stacking, AtomEnum::WINDOW)?;
            let active = self
                .prop32(self.root, a.active_window, AtomEnum::WINDOW)
                .and_then(|v| v.first().copied())
                .unwrap_or(0);
            let cur_desk = self
                .prop32(self.root, a.current_desktop, AtomEnum::CARDINAL)
                .and_then(|v| v.first().copied());

            struct Rect {
                id: u32,
                x0: f32,
                y0: f32,
                x1: f32,
                y1: f32,
            }
            let mut rects: Vec<Rect> = Vec::new();
            let mut notifications = Vec::new();

            // Bottom-to-top stacking; cap to the topmost 30 for cheapness.
            let start = stacking.len().saturating_sub(30);
            for &w in &stacking[start..] {
                let types = self.prop32(w, a.wm_window_type, AtomEnum::ATOM).unwrap_or_default();
                let states = self.prop32(w, a.wm_state, AtomEnum::ATOM).unwrap_or_default();
                if states.contains(&a.st_hidden) {
                    continue; // minimised
                }
                // Other workspace? (0xFFFFFFFF = sticky/all desktops)
                if let (Some(cd), Some(desk)) = (
                    cur_desk,
                    self.prop32(w, a.wm_desktop, AtomEnum::CARDINAL).and_then(|v| v.first().copied()),
                ) {
                    if desk != cd && desk != u32::MAX {
                        continue;
                    }
                }

                // Geometry in root coordinates, shadow margins stripped.
                let Ok(geo) = self.conn.get_geometry(w).map(|c| c.reply()) else { continue };
                let Ok(geo) = geo else { continue };
                let Ok(tr) = self.conn.translate_coordinates(w, self.root, 0, 0).map(|c| c.reply())
                else {
                    continue;
                };
                let Ok(tr) = tr else { continue };
                let (mut x0, mut y0) = (tr.dst_x as f32, tr.dst_y as f32);
                let (mut x1, mut y1) = (x0 + geo.width as f32, y0 + geo.height as f32);
                if let Some(e) = self.prop32(w, a.gtk_frame_extents, AtomEnum::CARDINAL) {
                    if e.len() >= 4 {
                        // CSD apps include invisible shadow margins — without
                        // this the pet would float ~30px above every window.
                        x0 += e[0] as f32;
                        x1 -= e[1] as f32;
                        y0 += e[2] as f32;
                        y1 -= e[3] as f32;
                    }
                }

                if types.contains(&a.t_notification) {
                    notifications.push((w, Vec2::new((x0 + x1) * 0.5, (y0 + y1) * 0.5)));
                    continue;
                }
                let skip = [a.t_dock, a.t_desktop, a.t_menu, a.t_toolbar, a.t_splash, a.t_utility]
                    .iter()
                    .any(|t| types.contains(t));
                if skip || states.contains(&a.st_fullscreen) {
                    continue;
                }
                if !types.is_empty() && !types.contains(&a.t_normal) && !types.contains(&a.t_dialog)
                {
                    continue;
                }
                if (x1 - x0) < 140.0 || (y1 - y0) < 90.0 {
                    continue;
                }
                rects.push(Rect { id: w, x0, y0, x1, y1 });
            }

            // A window's top edge is walkable only where no higher window
            // covers it.
            let mut platforms = Vec::new();
            let mut active_top: Option<Plat> = None;
            for (i, r) in rects.iter().enumerate() {
                let mut segs: Vec<(f32, f32)> = vec![(r.x0, r.x1)];
                for c in rects.iter().skip(i + 1) {
                    if c.y0 < r.y0 && c.y1 > r.y0 {
                        segs = subtract(&segs, (c.x0, c.x1));
                    }
                }
                for (s0, s1) in segs {
                    if s1 - s0 >= 90.0 {
                        let p = Plat { x0: s0 + 6.0, x1: s1 - 6.0, y: r.y0 };
                        if r.id == active {
                            let wider = active_top.map(|q| (p.x1 - p.x0) > (q.x1 - q.x0));
                            if wider.unwrap_or(true) {
                                active_top = Some(p);
                            }
                        }
                        platforms.push(p);
                    }
                }
            }
            platforms.truncate(32);

            Some(ScoutInfo { platforms, active_top, notifications })
        }
    }

    /// Subtract a covering x-interval from a list of segments.
    fn subtract(segs: &[(f32, f32)], cover: (f32, f32)) -> Vec<(f32, f32)> {
        let mut out = Vec::with_capacity(segs.len() + 1);
        for &(a, b) in segs {
            if cover.1 <= a || cover.0 >= b {
                out.push((a, b)); // no overlap
            } else {
                if cover.0 > a {
                    out.push((a, cover.0));
                }
                if cover.1 < b {
                    out.push((cover.1, b));
                }
            }
        }
        out
    }
}

#[cfg(not(unix))]
mod imp {
    use super::*;
    pub struct Probe;
    impl Probe {
        pub fn new() -> Option<Self> {
            None
        }
        pub fn pointer(&self) -> Option<PointerState> {
            None
        }
        pub fn scout(&self) -> Option<ScoutInfo> {
            None
        }
    }
}

pub use imp::Probe;
