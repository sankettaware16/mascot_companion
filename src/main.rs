//! COMPANION — a tiny living pixel creature for your desktop.
//!
//! No cloud, no telemetry, no AI models. Just a little friend that walks,
//! flies, follows your cursor, feels moods and reminds you to drink water.

// On Windows, don't pop a console window for this GUI app.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod activity;
mod app;
mod banner;
mod config;
mod creature;
mod notify;
mod pack;
mod platform;
mod reminder;
mod render;
mod world;

use app::App;
use config::Config;
use winit::event_loop::EventLoop;

fn main() {
    let cfg = Config::load();
    eprintln!(
        "[companion] starting '{}' — scale {}, follow_cursor {}",
        cfg.name, cfg.scale, cfg.follow_cursor
    );

    // Prefer the X11 backend so global-cursor + overlay positioning work even
    // inside a Wayland session (via XWayland).
    let mut builder = EventLoop::builder();
    #[cfg(target_os = "linux")]
    {
        use winit::platform::x11::EventLoopBuilderExtX11;
        builder.with_x11();
    }
    let event_loop = builder.build().expect("create event loop");

    let mut app = App::new(cfg);
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("[companion] event loop error: {e}");
    }
}
