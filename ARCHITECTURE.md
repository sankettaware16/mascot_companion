# Companion — Architecture

A guided tour of how a tiny pixel creature comes to feel alive on your desktop,
and how the pieces fit so a single developer can keep extending it.

## Design goals (in priority order)

1. **Extremely lightweight** — no webview/Chromium/Electron; event-driven, render
   only on change, drop the frame clock when idle.
2. **Feels alive** — emotion + behaviour come from a small simulation with
   *hysteresis* and *randomness*, never an AI model.
3. **Offline & private** — no network, no telemetry, no disk spying. The only thing
   it observes is whether the mouse is moving.
4. **Portable core** — all simulation is platform-agnostic Rust; only window
   creation and the global-cursor probe touch the OS.

## Stack & why

| Concern | Choice | Why |
|---|---|---|
| Window / event loop | **winit 0.30** | Cross-platform transparent, borderless, always-on-top, click-through windows (`set_cursor_hittest(false)`), X11 override-redirect overlays. |
| Rendering | **wgpu 29** | Vulkan/Metal/DX12/GL via one API. Premultiplied-alpha swapchain = true per-pixel desktop transparency. Tiny GPU footprint. |
| Global cursor | **x11rb** | A click-through overlay never receives pointer events, so we ask X11 `QueryPointer` directly (works under XWayland too). |
| Text | **font8x8** | 8×8 bitmap font baked into the binary — no font files, on-theme pixel look. |
| Sprites | **procedural** | Drawn in code into a texture atlas at startup. Zero asset files; recolour = one number. |
| Config | **toml + serde** | Human-editable, all-optional. |
| Math | **glam** | Small, fast `Vec2`. |

> The original brief named Tauri but also forbade Chromium. Those conflict (Tauri =
> WebView2/WebKitGTK). We chose the pure-native path, which actually satisfies the
> lightweight/offline/no-Chromium constraints.

## Module map

```
src/
  main.rs            entry; forces X11 backend; builds the event loop
  app.rs             winit ApplicationHandler — windows, frame clock, per-monitor draw
  config.rs          TOML config + defaults
  world.rs           the multi-monitor virtual-desktop coordinate space
  activity.rs        "is the user working?" from cursor motion (privacy-safe)
  reminder.rs        wellness nudges + flavour dialogue, paced to never nag
  banner.rs          rasterises a speech bubble into an RGBA texture
  platform.rs        X11 global-cursor probe (stubbed elsewhere)
  render.rs          wgpu device + per-window surfaces + instanced quad pipeline
  creature/
    mod.rs           Creature: composes body+emotion+brain, picks the anim pose
    emotion.rs       needs simulation → mood (with hysteresis)
    brain.rs         behaviour state machine → movement intent
    physics.rs       gravity, walking, flight, bounce, walls/floor
    animation.rs     procedural pixel-art atlas (one expression per mood)
```

## Data flow (one frame)

```
                    ┌─────────────── about_to_wait (capped clock) ───────────────┐
 X11 QueryPointer ─►│ activity.update ─► creature.update ─► wellness.update       │
                    │      │                  │  │  │            │                │
                    │   is_active        emotion brain physics  banner text       │
                    └──────┼──────────────────┼──┼──┼────────────┼────────────────┘
                           │                  │  │  │            │
                           ▼                  ▼  ▼  ▼            ▼
              per monitor:  build instances (shadow, creature@global−origin, banner)
                           ▼
                    WindowRender.render ─► wgpu pass (clear transparent, draw quads)
                           ▼
                    composited onto the desktop (premultiplied alpha)
```

Simulation runs **once** per tick; each monitor's window then draws the shared
creature state translated into its local pixel space. A creature straddling two
monitors is simply drawn by both windows — that's the whole multi-monitor trick.

## Emotion engine (`emotion.rs`)

No AI. Four **needs** in `0..1` decay/recover over time:

* `energy` — drops while active, recovers while sleeping
* `social` — rises when the cursor is near, decays alone
* `fun` — boredom builds, released by zooming/flying
* `bond` — slow-building affection

A smoothed `arousal` tracks recent activity. Each frame a *desired* mood is derived
by priority (exhausted→sleepy, lonely+attached→crying, low-social→sad,
aroused+rested→energetic, social+rested→happy, else neutral). The **visible** mood
only switches after the desired one persists past a mood-specific *inertia* — this
hysteresis is what makes transitions read as deliberate instead of flickery.

## Behaviour brain (`brain.rs`)

A compact weighted state machine, not a heavyweight behaviour-tree lib. Hard
physical states (`Held`, `Tumble` after a throw) and reactive overrides
(notification → `Investigate`; exhaustion+user-idle → `Sleep`; angry+cursor-near →
`FleeCursor`) are checked every frame; otherwise, when the current behaviour's
timer expires, a new one is drawn from a **mood-weighted menu** (energetic favours
`Play`/flight and `Travel`; angry storms around; sad sits). The chosen behaviour
produces an `Intent { target, locomotion, flap }` that physics realises.

Behaviours: `Idle, Wander, FollowCursor, FleeCursor, Play(fly), Peek, Travel
(cross-monitor), Perch (active window top), Investigate (notification), Held,
Tumble, Sleep`.

Three deliberate "aliveness" rules learned from tuning:
* **Following is a treat, not a lock-on** — it triggers only when the cursor
  *sweeps past* nearby (speed + distance gate) and then goes on a 20–45 s
  cooldown. Without this the pet shadows the cursor forever and never flies.
* **Sleep is earned** — naps happen when the *user* is idle (>20 s), with a
  long-nap bias while you're away and a wake-up when you return. Exhaustion
  alone only slows it down.
* **Falls are decided once** — walking off a window edge flips one coin:
  flutter away in flight, or just drop. Re-rolling every frame made it always
  fly and never charmingly plummet.

## Physics (`physics.rs`)

A 1-body integrator in global coordinates. Gravity is scaled per-locomotion (≈1.0
walking, ≈0.18 flying) so flight is floaty; flight adds discrete wing-flap impulses.
Drag (higher on the ground) smooths motion; the floor is the bottom of whichever
monitor the creature is over; outer walls clamp it to the union of all monitors.

## Multi-monitor model (`world.rs`)

All monitors live in one global virtual space. `ground_y(x)` returns the floor of
the monitor under `x`; `x_bounds_at(y)` the roamable horizontal extent at that
height; `monitor_at(p)` which screen a point is on. Windows are created in the same
order as `World.monitors`, so window *i* ↔ monitor *i* and drawing is just
`creature.pos − monitor.origin`.

## Render pipeline (`render.rs`)

* One shared `Device`/`Queue`/`RenderPipeline`; one `Surface` per monitor.
* Everything is an **instanced textured quad**: `{rect, uv, color, flip}`. A 6-vertex
  unit quad is expanded in the vertex shader; a uniform carries the viewport size.
* **Premultiplied-alpha** output + `(One, OneMinusSrcAlpha)` blend + a transparent
  clear → the swapchain composites cleanly onto the desktop. Surface alpha mode is
  negotiated (`PreMultiplied → PostMultiplied → Inherit → …`) with a warning fallback.
* Two textures: the static creature **atlas** and a dynamic **banner** texture
  re-rasterised only when the message changes.
* Pixel-art stays crisp via `Nearest` sampling.

## Desktop awareness & interaction (`platform.rs` + `notify.rs`)

A click-through overlay receives no input events, so every "sense" comes from
polling one read-only X11 connection (~2 Hz for windows, per-frame for the
pointer — a few µs each):

* **Grab & throw** — `QueryPointer` returns the global cursor *and* the mouse
  button mask. Press near the pet → it's held (dangles from the cursor, drag
  velocity tracked); release → that velocity becomes a throw with tumble-spin
  (real quad rotation), lively bounces, and a *"Wheee!"*. Three drops within a
  minute → the `anger` need spikes → **Angry** mood: stomping, refusing to
  follow, fleeing the cursor, slanted-brow face. Anger cools off in ~2 minutes.
* **Window platforms** — EWMH enumeration (`_NET_CLIENT_LIST_STACKING` +
  geometry + `_GTK_FRAME_EXTENTS` to strip CSD shadow margins). Each window's
  *visible* top-edge segments (occlusion-clipped against higher windows) become
  walkable floors in `World::platforms`; physics lands on the nearest support
  below the feet, so the pet can fly up past a window and land on it, ride it
  down when you drag it, and fall when you close it. The focused window's top
  is a **perch** target — the pet flies up and sits there watching you work
  (skipped for maximised windows, where it would sit off-screen).
* **Notifications** — two detectors: X11 windows of type `NOTIFICATION`
  (dunst-style daemons, with real position) and a `dbus-monitor` child filtered
  to `org.freedesktop.Notifications.Notify` (catches GNOME's in-shell banners;
  only the app *name* is read, never the content). Either one triggers
  **Investigate**: excited fly-over, hovering under the popup with a "!" emote
  and an *"Ooh! Slack!"* banner.

Privacy stance: window *geometry and types* only — never titles, never text,
no keylogging. All of it stays on the machine.

## Custom pets (`companion-convert` + `pack.rs`)

Any image becomes a pet without touching code or recompiling:

```
avatar.svg/png ─► companion-convert ─► pets/<name>/{sprite.rgba, preview.png, pet.toml}
                                                    │
config: pet="<name>" ─► pack.rs ─► Gpu.set_pet() ─► procedural-motion render path
```

* **Two-binary split keeps the pet lean.** Image/SVG decoders (`image`, `resvg`)
  are *only* linked into `companion-convert`. The resident `companion` binary reads
  `sprite.rgba` — an 8-byte header (`w,h`) + straight-alpha RGBA — with no decoder, so
  it stays ~6 MB and starts instantly. (Verified: `nm` finds zero `resvg`/`image`
  symbols in the pet binary.)
* **The converter** rasterises SVG (via resvg/tiny-skia), trims the transparent
  margin, bottom-anchors the figure (so it stands on the ground), squares the canvas
  (so it can tilt without clipping), and records `fig_height`/`bottom_margin` so the
  engine can place and scale it precisely.
* **One static image, made alive.** Since a downloaded mascot has a single pose, the
  body is animated *procedurally* in `app.rs::pet_motion`: vertical bob, squash-and-
  stretch, a walk-rock, a velocity-based flight lean, and facing-flip (real quad
  rotation added to the vertex shader). A small **mood emote** (💤/✨/tear/…) is
  floated above the head so a face that can't change still reads its mood.

## Performance techniques

* Event-driven loop with a **capped clock**; `target_fps()` drops to **8 FPS** when
  the pet is resting and silent.
* Sim computed once, not per window.
* Persistent instance/uniform buffers written with `write_buffer` (no per-frame
  allocations); a handful of instances per frame.
* `LowPower` adapter; `Fifo` present mode (vsync, no busy-spin).
* Release profile: LTO, `panic=abort`, stripped → 6 MB binary.

## Privacy & security

* No network code, no files written except reading an optional `config.toml`.
* "Activity" is inferred purely from **cursor movement** — no keylogging, no window
  titles, no screen capture.
* X11 connection is read-only (`QueryPointer`).

## Accessibility

`reduced_motion` removes flight and keeps gentle walks; `scale` enlarges the pet;
reminders are fully tunable or disable-able. (Planned: high-contrast outline,
silent/pause/work modes — see roadmap.)

---

## Roadmap / future expansion

The current build is the **MVP**: a living, multi-monitor, cursor-aware, mood-driven
pet with wellness banners. Natural next milestones, each isolated to a module:

1. **Native probes** for Win32 (`GetCursorPos`/`EnumWindows`) and macOS
   (`NSEvent`/`CGWindowList`) in `platform.rs` → full cross-platform parity for
   cursor, grab, and window platforms. *(medium)*
2. ~~**Interaction**: pick up / throw the creature.~~ ✅ Done — grab & throw with
   tumble physics and an anger response (see *Desktop awareness* above). Next:
   petting (hold still over it) to build bond.
3. **Personality traits** (playful/shy/lazy…) as slow-moving weights biasing the
   brain menu, so two installs diverge over time. *(`brain.rs` + new `personality.rs`)*
4. **Persistence** (optional SQLite) for bond/personality/stats across restarts —
   currently intentionally omitted to stay zero-state. *(new `save.rs`)*
5. **Daily routines & seasonal events** driven by wall-clock/date.
6. ~~**Mascot packs**: drop-in custom pets from any image.~~ ✅ Done — see
   *Custom pets* above. Next: multi-frame packs (hand-drawn walk/fly frames in a
   sheet) for users who want true animation, and behaviour/sound in the pack TOML.
7. **Audio** (`rodio`) for soft chirps tied to moods.
8. **Settings app**: a small `egui` window reusing `config.rs`.

The architecture keeps simulation, rendering, and OS glue decoupled, so each of the
above lands without touching the others.
