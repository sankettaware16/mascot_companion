//! The winit application: creates one transparent, always-on-top, click-through
//! overlay window per monitor, runs the simulation on a capped clock, and draws
//! the companion onto whichever screen it currently occupies.

use crate::activity::Activity;
use crate::config::Config;
use crate::creature::animation::{self, Emote, Sprites};
use crate::creature::emotion::Mood;
use crate::creature::{Behavior, Creature, Locomotion, Senses};
use crate::notify::NotifyWatch;
use crate::pack::PetPack;
use crate::platform::Probe;
#[cfg(not(windows))]
use crate::creature::animation::CELL;
#[cfg(not(windows))]
use crate::render::{Gpu, Instance, WindowRender};
use crate::reminder::Wellness;
use crate::world::{Monitor, World};
#[cfg(windows)]
use crate::{
    banner::Banner,
    compose::{compose, ComposeArgs, Src},
    win_present::Layered,
};
use glam::Vec2;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
#[cfg(not(windows))]
use winit::dpi::PhysicalPosition;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Window, WindowId, WindowLevel};

/// On-screen geometry of a loaded custom pet (precomputed at load).
struct PetGeom {
    /// On-screen size of the (square) canvas at rest.
    display: f32,
    /// On-screen px from the canvas bottom up to the figure's feet.
    feet_inset: f32,
    /// Collision half-extent.
    half: f32,
}

pub struct App {
    cfg: Config,
    sprites: Sprites,
    pet: Option<PetGeom>,
    pet_pack: Option<PetPack>,
    world: World,
    #[cfg(not(windows))]
    gpu: Option<Gpu>,
    #[cfg(not(windows))]
    windows: Vec<WindowRender>,
    #[cfg(not(windows))]
    ids: Vec<WindowId>,
    // Windows presents via one small CPU-composed layered window instead of
    // per-monitor GPU overlays (DWM rejects per-pixel-alpha swapchains).
    #[cfg(windows)]
    layered: Option<Layered>,
    #[cfg(windows)]
    layer_window: Option<Arc<Window>>,
    #[cfg(windows)]
    layer_shown: bool,
    #[cfg(windows)]
    banner_img: Option<(String, Banner)>,
    creature: Option<Creature>,
    activity: Activity,
    wellness: Wellness,
    probe: Option<Probe>,
    notify: Option<NotifyWatch>,
    // --- interaction (grab & throw) ---
    prev_button: bool,
    grabbed: bool,
    drops: Vec<Instant>,
    // --- desktop awareness ---
    scout_timer: f32,
    perch: Option<Vec2>,
    noti: Option<(Vec2, f32)>,
    known_notifs: HashSet<u32>,
    // --- speech ---
    banner_text: Option<String>,
    banner_ttl: f32,
    event_banner_cd: f32,
    was_long_idle: bool,
    last: Instant,
    next_frame: Instant,
}

impl App {
    pub fn new(cfg: Config) -> Self {
        let sprites = animation::generate(cfg.body_color);
        let notify = if cfg.react_notifications { NotifyWatch::spawn() } else { None };
        Self {
            cfg,
            sprites,
            pet: None,
            pet_pack: None,
            world: World { monitors: vec![], platforms: vec![] },
            #[cfg(not(windows))]
            gpu: None,
            #[cfg(not(windows))]
            windows: vec![],
            #[cfg(not(windows))]
            ids: vec![],
            #[cfg(windows)]
            layered: None,
            #[cfg(windows)]
            layer_window: None,
            #[cfg(windows)]
            layer_shown: false,
            #[cfg(windows)]
            banner_img: None,
            creature: None,
            activity: Activity::new(),
            wellness: Wellness::new(),
            probe: Probe::new(),
            notify,
            prev_button: false,
            grabbed: false,
            drops: vec![],
            scout_timer: 0.0,
            perch: None,
            noti: None,
            known_notifs: HashSet::new(),
            banner_text: None,
            banner_ttl: 0.0,
            event_banner_cd: 0.0,
            was_long_idle: false,
            last: Instant::now(),
            next_frame: Instant::now(),
        }
    }

    #[cfg(not(windows))]
    fn overlay_attributes(
        origin: PhysicalPosition<i32>,
        size: PhysicalSize<u32>,
    ) -> winit::window::WindowAttributes {
        let attrs = Window::default_attributes()
            .with_title("companion")
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_position(origin)
            .with_inner_size(size);
        #[cfg(target_os = "linux")]
        let attrs = {
            use winit::platform::x11::WindowAttributesExtX11;
            attrs.with_override_redirect(true)
        };
        attrs
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.creature.is_some() {
            return; // already initialised (e.g. on a later resume)
        }

        // Enumerate monitors -> one overlay window each.
        let handles: Vec<_> = event_loop.available_monitors().collect();
        if handles.is_empty() {
            // Headless fallback so it still runs somewhere.
            self.world.monitors.push(Monitor {
                origin: Vec2::ZERO,
                size: Vec2::new(960.0, 600.0),
                bottom_inset: 0.0,
            });
        } else {
            for h in &handles {
                let p = h.position();
                let s = h.size();
                self.world.monitors.push(Monitor {
                    origin: Vec2::new(p.x as f32, p.y as f32),
                    size: Vec2::new(s.width as f32, s.height as f32),
                    bottom_inset: 0.0,
                });
            }
        }

        #[cfg(not(windows))]
        for m in self.world.monitors.clone().iter() {
            let origin = PhysicalPosition::new(m.origin.x as i32, m.origin.y as i32);
            let size = PhysicalSize::new(m.size.x as u32, m.size.y as u32);
            let window = Arc::new(
                event_loop
                    .create_window(Self::overlay_attributes(origin, size))
                    .expect("create overlay window"),
            );
            let _ = window.set_cursor_hittest(false); // click-through
            window.set_outer_position(origin);

            let wr = if let Some(gpu) = self.gpu.as_mut() {
                gpu.add_window(window.clone())
            } else {
                let (gpu, wr) = Gpu::new(window.clone(), &self.sprites);
                self.gpu = Some(gpu);
                wr
            };
            self.ids.push(window.id());
            self.windows.push(wr);
        }

        #[cfg(windows)]
        {
            // One tiny window; UpdateLayeredWindow controls its size, position,
            // pixels and alpha each frame. Hidden until the first frame lands.
            use winit::platform::windows::WindowAttributesExtWindows;
            use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
            let window = Arc::new(
                event_loop
                    .create_window(
                        Window::default_attributes()
                            .with_title("companion")
                            .with_decorations(false)
                            .with_resizable(false)
                            .with_transparent(false)
                            .with_visible(false)
                            .with_skip_taskbar(true)
                            .with_window_level(WindowLevel::AlwaysOnTop)
                            .with_inner_size(PhysicalSize::new(4u32, 4u32)),
                    )
                    .expect("create pet window"),
            );
            let _ = window.set_cursor_hittest(false); // click-through
            if let Ok(handle) = window.window_handle() {
                if let RawWindowHandle::Win32(h) = handle.as_raw() {
                    self.layered =
                        Some(Layered::new(h.hwnd.get() as *mut core::ffi::c_void));
                }
            }
            self.layer_window = Some(window);
        }

        eprintln!(
            "[companion] {} monitor(s), cursor: {}, grab: {}, climb: {}, notifications: {}",
            self.world.monitors.len(),
            self.probe.is_some(),
            self.cfg.allow_grab && self.probe.is_some(),
            self.cfg.climb_windows && self.probe.is_some() && cfg!(unix),
            self.cfg.react_notifications
                && (self.notify.is_some() || (self.probe.is_some() && cfg!(unix))),
        );

        // Spawn the creature near the bottom-centre of the primary monitor.
        let m0 = self.world.monitors[0];
        let start = Vec2::new(m0.center().x, m0.floor() - 40.0);
        let mut creature = Creature::new(start, &self.cfg);

        // Optional: replace the built-in creature with a custom pet pack.
        if let Some(name) = self.cfg.pet.clone() {
            if let Some(pack) = PetPack::load(&name) {
                // Desired on-screen figure height scales with the user's `scale`.
                let figure_target = 40.0 * self.cfg.scale;
                let sf = figure_target / pack.fig_height as f32;
                let geom = PetGeom {
                    display: pack.width as f32 * sf, // canvas is square (w == h)
                    feet_inset: pack.bottom_margin as f32 * sf,
                    half: figure_target * 0.5,
                };
                creature.body.half = geom.half;
                creature.body.pos.y = m0.floor() - geom.half;
                #[cfg(not(windows))]
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.set_pet(&pack.pixels, pack.width, pack.height);
                }
                self.pet = Some(geom);
                self.pet_pack = Some(pack);
                eprintln!("[companion] using custom pet '{name}'");
            }
        }
        self.creature = Some(creature);

        self.last = Instant::now();
        self.next_frame = Instant::now();
        event_loop.set_control_flow(ControlFlow::Poll);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if now < self.next_frame {
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
            return;
        }

        let dt = (now - self.last).as_secs_f32().min(0.1);
        self.last = now;
        self.simulate(dt);

        // Adaptive frame pacing: idle/sleeping -> slow clock to save CPU.
        let fps = self.target_fps();
        self.next_frame = now + std::time::Duration::from_secs_f32(1.0 / fps as f32);
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));

        #[cfg(windows)]
        self.present_layered();

        #[cfg(not(windows))]
        for w in &self.windows {
            w.window.request_redraw();
        }
    }

    #[allow(unused_variables)]
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) =>
            {
                #[cfg(not(windows))]
                if let (Some(gpu), Some(idx)) = (self.gpu.as_ref(), self.index_of(id)) {
                    self.windows[idx].resize(gpu, size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested =>
            {
                #[cfg(not(windows))]
                if let Some(idx) = self.index_of(id) {
                    self.draw_window(idx);
                }
            }
            _ => {}
        }
    }
}

impl App {
    #[cfg(not(windows))]
    fn index_of(&self, id: WindowId) -> Option<usize> {
        self.ids.iter().position(|x| *x == id)
    }

    fn target_fps(&self) -> u32 {
        let creature = match &self.creature {
            Some(c) => c,
            None => return 8,
        };
        let resting = creature.body.on_ground
            && creature.body.vel.length() < 8.0
            && self.banner_text.is_none()
            && !self.grabbed;
        if resting {
            8
        } else {
            self.cfg.fps
        }
    }

    fn say(&mut self, text: &str, ttl: f32) {
        if self.event_banner_cd > 0.0 {
            return;
        }
        self.event_banner_cd = 8.0;
        self.banner_text = Some(text.to_string());
        self.banner_ttl = ttl;
    }

    fn trigger_notification(&mut self, point: Vec2, app: Option<&str>) {
        if self.noti.is_some() {
            return; // already busy gawking at one
        }
        self.noti = Some((point, 8.0));
        if let Some(cr) = self.creature.as_mut() {
            cr.emotion.excite(0.35);
        }
        let label = match app {
            Some(a) if !a.is_empty() => {
                let short: String = a.chars().take(18).collect();
                format!("Ooh! {short}!")
            }
            _ => "Ooh! What's that?".to_string(),
        };
        self.say(&label, 4.0);
    }

    fn simulate(&mut self, dt: f32) {
        self.event_banner_cd -= dt;

        // --- senses: global cursor + left button -----------------------------
        let ptr = self.probe.as_ref().and_then(|p| p.pointer());
        let cursor = ptr.as_ref().map(|p| p.pos);
        let button = ptr.as_ref().map(|p| p.button1).unwrap_or(false);

        let was_long_idle = self.was_long_idle;
        self.activity.update(dt, cursor);
        if self.activity.is_active() {
            if was_long_idle && rand::random::<f32>() < 0.5 {
                self.say("Welcome back!", 4.0);
            }
            self.was_long_idle = false;
        } else if self.activity.idle_seconds > 180.0 {
            self.was_long_idle = true;
        }

        // --- grab & throw ------------------------------------------------------
        let pressed = button && !self.prev_button;
        let released = !button && self.prev_button;
        self.prev_button = button;
        if self.cfg.allow_grab {
            if let (Some(c), Some(cr)) = (cursor, self.creature.as_mut()) {
                let radius = (cr.body.half * 1.5).max(48.0);
                if pressed && !self.grabbed && (c - cr.pos()).length() < radius {
                    self.grabbed = true;
                    cr.grab();
                }
            }
            if self.grabbed && (released || cursor.is_none()) {
                self.grabbed = false;
                if let Some(cr) = self.creature.as_mut() {
                    let v = cr.release();
                    let now = Instant::now();
                    self.drops.retain(|t| now.duration_since(*t).as_secs_f32() < 60.0);
                    self.drops.push(now);
                    cr.emotion.provoked(0.10);
                    if self.drops.len() >= 3 {
                        cr.emotion.provoked(0.45);
                        self.say("Hey!! Stop throwing me!", 4.0);
                    } else if v.length() > 950.0 {
                        self.say("Wheee!", 2.0);
                    }
                }
            }
        }

        // --- desktop awareness: windows & notification popups (X11) ------------
        self.scout_timer -= dt;
        if self.scout_timer <= 0.0 && (self.cfg.climb_windows || self.cfg.react_notifications) {
            self.scout_timer = 0.6;
            if let Some(info) = self.probe.as_ref().and_then(|p| p.scout()) {
                if self.cfg.climb_windows {
                    let pet_height = 36.0 * self.cfg.scale;
                    self.perch = info.active_top.and_then(|p| {
                        let cx = (p.x0 + p.x1) * 0.5;
                        let ground = self.world.ground_y(cx);
                        let mon_top = self
                            .world
                            .monitor_at(Vec2::new(cx, p.y + 1.0))
                            .map(|i| self.world.monitors[i].origin.y)
                            .unwrap_or(f32::MIN);
                        // Worth perching only if it's meaningfully off the floor,
                        // wide enough to stand on, and far enough below the screen
                        // top that the pet stays visible (maximised windows fail
                        // this — sitting there would put it off-screen).
                        ((ground - p.y) > 140.0
                            && (p.x1 - p.x0) > 160.0
                            && (p.y - mon_top) > pet_height)
                            .then(|| Vec2::new(cx, p.y))
                    });
                    self.world.platforms = info.platforms;
                }
                if self.cfg.react_notifications {
                    let current: HashSet<u32> =
                        info.notifications.iter().map(|(id, _)| *id).collect();
                    for (id, center) in &info.notifications {
                        if !self.known_notifs.contains(id) {
                            self.trigger_notification(*center, None);
                        }
                    }
                    self.known_notifs = current;
                }
                if std::env::var_os("COMPANION_DEBUG").is_some() {
                    eprintln!(
                        "[companion:debug] platforms={} perch={:?} notif_windows={}",
                        self.world.platforms.len(),
                        self.perch,
                        self.known_notifs.len()
                    );
                }
            }
        }

        // --- notifications via D-Bus (covers GNOME's in-shell banners) ----------
        if self.cfg.react_notifications {
            let mut app_evt = None;
            if let Some(w) = self.notify.as_ref() {
                while let Some(app) = w.try_event() {
                    app_evt = Some(app);
                }
            }
            if let Some(app) = app_evt {
                // Banners appear top-centre of the monitor you're working on.
                let mi = cursor.and_then(|c| self.world.monitor_at(c)).unwrap_or(0);
                let m = self.world.monitors[mi];
                let p = Vec2::new(m.center().x, m.origin.y + 70.0);
                if std::env::var_os("COMPANION_DEBUG").is_some() {
                    eprintln!("[companion:debug] dbus notification from '{app}' -> {p}");
                }
                self.trigger_notification(p, Some(&app));
            }
        }
        if let Some((_, ttl)) = self.noti.as_mut() {
            *ttl -= dt;
            if *ttl <= 0.0 {
                self.noti = None;
            }
        }

        // --- creature ------------------------------------------------------------
        let senses = Senses {
            cursor,
            cursor_speed: self.activity.cursor_speed,
            user_active: self.activity.is_active(),
            user_idle: self.activity.idle_seconds > 20.0,
            perch: self.perch,
            noti: self.noti.map(|(p, _)| p),
        };
        if let Some(c) = self.creature.as_mut() {
            c.update(dt, &self.world, &senses, &self.cfg);
        }

        // --- wellness & speech ------------------------------------------------------
        if let Some(speech) = self.wellness.update(dt, &self.activity, &self.cfg) {
            if self.banner_text.is_none() {
                self.banner_text = Some(speech.text);
                self.banner_ttl = speech.ttl;
            }
        }
        if self.banner_ttl > 0.0 {
            self.banner_ttl -= dt;
            if self.banner_ttl <= 0.0 {
                self.banner_text = None;
            }
        }
        #[cfg(not(windows))]
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.set_banner(self.banner_text.as_deref());
        }
    }

    /// Windows: CPU-compose the whole frame and push it via UpdateLayeredWindow.
    #[cfg(windows)]
    fn present_layered(&mut self) {
        let (Some(layered), Some(creature)) = (self.layered.as_ref(), self.creature.as_ref())
        else {
            return;
        };

        // Banner texture cache, re-rasterised only when the text changes.
        match self.banner_text.as_deref() {
            Some(text) => {
                let stale = self.banner_img.as_ref().map(|(t, _)| t != text).unwrap_or(true);
                if stale {
                    self.banner_img = Some((text.to_string(), Banner::render(text)));
                }
            }
            None => self.banner_img = None,
        }
        let banner = self.banner_img.as_ref().map(|(_, b)| b);

        let sleeping = matches!(creature.locomotion(), Locomotion::Sleep);
        let event_emote = match creature.behavior() {
            Behavior::Investigate => Some(Emote::Exclaim),
            _ if creature.mood() == Mood::Angry => Some(Emote::Exclaim),
            _ => None,
        };
        let emote_src = |e: Emote| {
            let (x, y, w, h) = self.sprites.emote_rect(e);
            Src { pixels: &self.sprites.pixels, stride: self.sprites.width, x, y, w, h }
        };

        // Shadow fades out with height (the window only spans the pet, so a
        // far-below ground shadow can't be drawn here).
        let pos = creature.pos();
        let feet = pos.y + creature.body.half;
        let support = self.world.support_y(pos.x, feet);
        let air = (support - feet).max(0.0);
        let shadow = (air < 60.0).then(|| {
            let shrink = 1.0 / (1.0 + air / 60.0);
            let w = self
                .pet
                .as_ref()
                .map(|p| p.display * 0.5)
                .unwrap_or(24.0 * self.cfg.scale);
            (w * shrink, 0.22 * shrink)
        });

        // Compose: custom pet pack if loaded, else the built-in atlas cell.
        let (frame, center_y_world) =
            if let (Some(geom), Some(pack)) = (self.pet.as_ref(), self.pet_pack.as_ref()) {
                let (bob, sxq, syq, motion_angle) = pet_motion(
                    creature.locomotion(),
                    creature.anim_phase(),
                    self.cfg.reduced_motion,
                    geom.display,
                );
                let emote = event_emote.or(Emote::for_mood(creature.mood(), sleeping));
                let f = compose(&ComposeArgs {
                    sprite: Src {
                        pixels: &pack.pixels,
                        stride: pack.width,
                        x: 0,
                        y: 0,
                        w: pack.width,
                        h: pack.height,
                    },
                    scale_x: geom.display / pack.width as f32 * sxq,
                    scale_y: geom.display / pack.height as f32 * syq,
                    flip: creature.facing() < 0.0,
                    angle: motion_angle + creature.visual_angle(),
                    tint: mood_tint(creature.mood()),
                    bob,
                    emote: emote.map(emote_src),
                    emote_size: geom.display * 0.26,
                    banner,
                    shadow,
                });
                // Pack canvas is bottom-anchored: its centre sits above the feet.
                let cc_y = feet + geom.feet_inset - geom.display * 0.5;
                (f, cc_y)
            } else {
                let (x, y, w, h) = self.sprites.cell_rect(creature.mood(), creature.pose());
                let f = compose(&ComposeArgs {
                    sprite: Src { pixels: &self.sprites.pixels, stride: self.sprites.width, x, y, w, h },
                    scale_x: self.cfg.scale,
                    scale_y: self.cfg.scale,
                    flip: creature.facing() < 0.0,
                    angle: creature.visual_angle(),
                    tint: [1.0; 4],
                    bob: 0.0,
                    emote: event_emote.map(emote_src),
                    emote_size: 13.0 * self.cfg.scale,
                    banner,
                    shadow,
                });
                // GPU path draws the 32px cell centred 1*scale above body centre.
                (f, pos.y - self.cfg.scale)
            };

        let x = pos.x as i32 - frame.center_x;
        let y = center_y_world as i32 - frame.center_y;
        if layered.present(&frame.bgra, frame.w, frame.h, x, y) && !self.layer_shown {
            if let Some(w) = self.layer_window.as_ref() {
                w.set_visible(true);
            }
            self.layer_shown = true;
        }
    }

    #[cfg(not(windows))]
    fn draw_window(&mut self, idx: usize) {
        let (gpu, creature) = match (self.gpu.as_ref(), self.creature.as_ref()) {
            (Some(g), Some(c)) => (g, c),
            _ => return,
        };
        let monitor = self.world.monitors[idx];
        let origin = monitor.origin;
        let scale = self.cfg.scale;
        let cell = CELL as f32 * scale;

        let pos = creature.pos();
        let local = pos - origin;
        let widest = cell.max(self.pet.as_ref().map(|p| p.display).unwrap_or(0.0));
        let cull = local.x < -widest || local.x > monitor.size.x + widest;

        let mut instances: Vec<Instance> = Vec::with_capacity(3);
        let mut pet_instance: Option<Instance> = None;

        // --- shadow on whatever it stands above (floor or window top) --------
        let feet = pos.y + creature.body.half;
        let support = self.world.support_y(pos.x, feet);
        let air = (support - feet).max(0.0);
        let shrink = 1.0 / (1.0 + air / 260.0);
        let shadow_w =
            self.pet.as_ref().map(|p| p.display * 0.5).unwrap_or(24.0 * scale) * shrink;
        let sh = shadow_w * 0.33;
        let sxx = local.x - shadow_w * 0.5;
        let syy = (support - origin.y) - sh * 0.5;
        instances.push(Instance::new(
            [sxx, syy, shadow_w, sh],
            self.sprites.white_uv(),
            [0.0, 0.0, 0.0, 0.22 * shrink],
            false,
        ));

        // Mood/state emote: events override mood.
        let sleeping = matches!(creature.locomotion(), Locomotion::Sleep);
        let event_emote = match creature.behavior() {
            Behavior::Investigate => Some(Emote::Exclaim),
            _ if creature.mood() == Mood::Angry => Some(Emote::Exclaim),
            _ => None,
        };

        // --- the body: custom pet, or built-in creature ---------------------
        if !cull {
            let tilt = creature.visual_angle();
            if let Some(geom) = self.pet.as_ref() {
                let (bob, sxq, syq, motion_angle) = pet_motion(
                    creature.locomotion(),
                    creature.anim_phase(),
                    self.cfg.reduced_motion,
                    geom.display,
                );
                let flip = creature.facing() < 0.0;
                let dw = geom.display * sxq;
                let dh = geom.display * syq;
                let feet_local = feet - origin.y;
                let canvas_bottom = feet_local + geom.feet_inset;
                let rect = [local.x - dw * 0.5, canvas_bottom - dh - bob, dw, dh];
                let tint = mood_tint(creature.mood());
                pet_instance =
                    Some(Instance::angled(rect, [0.0, 0.0, 1.0, 1.0], tint, flip, motion_angle + tilt));

                let emote = event_emote.or(Emote::for_mood(creature.mood(), sleeping));
                if let Some(e) = emote {
                    let es = geom.display * 0.26;
                    let float = (creature.anim_phase() * 2.2).sin() * es * 0.12;
                    let ex = local.x + dw * 0.22;
                    let ey = rect[1] + dh * 0.06 - float;
                    instances.push(Instance::new(
                        [ex, ey, es, es],
                        self.sprites.emote_uv(e),
                        [1.0, 1.0, 1.0, 1.0],
                        false,
                    ));
                }
            } else {
                let rect = [local.x - 16.0 * scale, local.y - 17.0 * scale, cell, cell];
                instances.push(Instance::angled(
                    rect,
                    self.sprites.uv(creature.mood(), creature.pose()),
                    [1.0, 1.0, 1.0, 1.0],
                    creature.facing() < 0.0,
                    tilt,
                ));
                // The built-in face is expressive on its own; float an emote
                // only for special events (notifications, anger).
                if let Some(e) = event_emote {
                    let es = 13.0 * scale;
                    let float = (creature.anim_phase() * 2.2).sin() * es * 0.12;
                    let ex = local.x + 8.0 * scale;
                    let ey = (local.y - 17.0 * scale) - es * 0.6 - float;
                    instances.push(Instance::new(
                        [ex, ey, es, es],
                        self.sprites.emote_uv(e),
                        [1.0, 1.0, 1.0, 1.0],
                        false,
                    ));
                }
            }
        }

        // --- banner (only on the screen the creature is on) -----------------
        let mut banner_instance = None;
        let here = self.world.monitor_at(pos) == Some(idx);
        if here {
            if let (Some(_), Some((bw, bh))) = (self.banner_text.as_ref(), gpu.banner_size()) {
                let bw = bw as f32;
                let bh = bh as f32;
                let bx = (local.x - bw * 0.5).clamp(2.0, (monitor.size.x - bw - 2.0).max(2.0));
                let by = (local.y - 17.0 * scale) - bh - 6.0;
                banner_instance = Some(Instance::new(
                    [bx, by.max(2.0), bw, bh],
                    [0.0, 0.0, 1.0, 1.0],
                    [1.0, 1.0, 1.0, 1.0],
                    false,
                ));
            }
        }

        self.windows[idx].render(gpu, &instances, pet_instance, banner_instance);
    }
}

/// Procedural "aliveness" transform for a static custom avatar.
/// Returns (vertical bob px, scale-x, scale-y, rotation radians).
/// Velocity-based tilt (flight lean / throw spin) comes from
/// `Creature::visual_angle` and is added by the caller.
fn pet_motion(loco: Locomotion, phase: f32, reduced: bool, display: f32) -> (f32, f32, f32, f32) {
    let amp = display / 100.0; // bob scales with pet size
    if reduced {
        // Gentle breathing only.
        let s = (phase * 1.6).sin();
        return (0.0, 1.0 + 0.02 * s, 1.0 - 0.02 * s, 0.0);
    }
    match loco {
        Locomotion::Walk => {
            let b = (phase * 10.0).sin().abs();
            (b * 3.0 * amp, 1.0, 1.0, (phase * 10.0).sin() * 0.07)
        }
        Locomotion::Run => {
            let b = (phase * 13.0).sin().abs();
            (b * 4.0 * amp, 1.02, 0.98, (phase * 13.0).sin() * 0.10)
        }
        Locomotion::Fly => ((phase * 6.0).sin() * 6.0 * amp, 1.0, 1.0, 0.0),
        Locomotion::Held => {
            // Dangling from the cursor, swaying gently.
            (0.0, 1.0, 1.0, (phase * 3.0).sin() * 0.09)
        }
        Locomotion::Tumble => (0.0, 1.0, 1.0, 0.0), // spin comes from visual_angle
        Locomotion::Sleep => {
            let s = (phase * 1.2).sin();
            (s.max(0.0) * amp, 1.0 + 0.04 * s, 1.0 - 0.04 * s, 0.0)
        }
        Locomotion::Idle => {
            let s = (phase * 2.0).sin();
            (s * 1.5 * amp, 1.0 + 0.03 * s, 1.0 - 0.03 * s, (phase * 1.5).sin() * 0.015)
        }
    }
}

/// Subtle full-figure tint so a static avatar still reads its mood.
fn mood_tint(mood: Mood) -> [f32; 4] {
    match mood {
        Mood::Crying => [0.80, 0.88, 1.05, 1.0],
        Mood::Sad => [0.86, 0.90, 1.0, 1.0],
        Mood::Sleepy => [0.90, 0.90, 0.96, 1.0],
        Mood::Energetic => [1.06, 1.04, 0.96, 1.0],
        Mood::Happy => [1.04, 1.0, 1.0, 1.0],
        Mood::Angry => [1.12, 0.90, 0.88, 1.0],
        Mood::Neutral => [1.0, 1.0, 1.0, 1.0],
    }
}
