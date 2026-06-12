# 🐾 Companion

A tiny living pixel creature that walks, flies and feels at home on your desktop.

Not an assistant. Not a chatbot. Just a little friend that happens to live inside
your computer — it wanders your screens, follows your cursor, naps when you're away,
gets bouncy when you're busy, and gently reminds you to drink water.

* **Extremely lightweight** — pure native Rust, no Electron, no Chromium, no webview.
* **Offline-first** — zero cloud, zero telemetry, nothing ever leaves your machine.
* **No AI** — moods come from a small needs-simulation, not a model.

Measured on an Intel UHD 730 iGPU across **two monitors**: ~2.5% CPU while actively
animating, dropping to a near-idle 8 FPS clock when the pet rests; ~95 MB RAM; 6 MB binary.

---

## Quick start

```bash
cargo run --release
```

That's it. A transparent, always-on-top, click-through pet appears on your screen.
It never steals focus or blocks your clicks — you work *through* it.

To tweak it, copy [`config.toml`](config.toml) next to the binary and edit. Every
option is optional; delete the file for defaults.

### Install (Linux)

```bash
./install.sh          # builds release + installs to ~/.local/bin + a .desktop entry
```

Run it any time with `companion`, or from your application launcher.

---

## What it does

| Feature | How |
|---|---|
| **Lives on the desktop** | One transparent overlay window per monitor, always-on-top, click-through. |
| **Moods** | happy · energetic · neutral · sleepy · sad · crying · **angry** — driven by an internal needs sim (energy / social / fun / bond / anger). Each mood changes the face, speed and behaviour. |
| **Follows your cursor** | Reads the global pointer (X11) and walks/runs toward it — or *away* if you set `flee_cursor`. |
| **Real flight physics** | Gravity, wing-flap impulses, drag, soft bounces. It arcs and flutters rather than gliding robotically. |
| **Multi-monitor** | Roams across all screens in one virtual space, and peeks toward the screen your cursor is on. |
| **Gentle reminders** | Speech-bubble banners — *"Psst… drink some water!"*, *"Eyes tired? Look far away."*, *"You've been at it a while. Stretch?"* — paced so it never nags. |
| **Pick it up & throw it** | Click-grab the pet, drag it, fling it — real ballistic tumble with spin and bounces (*"Wheee!"*). Leave it in mid-air and it flutters off somewhere on its own. Throw it around too much and it gets **angry** — stomps off, refuses to follow, glares. |
| **Climbs your windows** | Open windows' top edges are platforms: it lands on your browser, patrols along your editor, perches on the active window to watch you work — and falls off when you move or close the window. |
| **Chases notifications** | When a desktop notification pops up it zooms over all excited (*"Ooh! Slack!"*) and hovers under it to see what's going on. |
| **Visits every screen** | Deliberately travels between monitors — sometimes walking across, sometimes a full cross-screen flight. |
| **Sleeps when *you're* away** | Naps only while you're idle, wakes up when you come back (*"Welcome back!"*). Never randomly nods off mid-play. |

The built-in creature is **drawn procedurally in code** — no image files to ship,
and recolouring it is a single config value. Or bring your own pet ⬇️

---

## 🎨 Bring your own pet

Found a mascot you love? Turn any image into a living pet in two commands:

```bash
companion-convert avatars/thor.svg     # SVG, PNG, JPG, GIF or WEBP
# → prints:  add  pet = "thor"  to config.toml, then run  companion
```

The converter auto-trims the transparent margin, bottom-anchors the figure so it
stands on the ground, and writes a `pets/thor/` pack (a raw sprite + `pet.toml`).
Set `pet = "thor"` in [`config.toml`](config.toml), run `companion`, and Thor now
walks, flies and follows your cursor.

**How a single image comes alive:** a downloaded mascot is one static pose, so the
engine animates it *procedurally* — bob, squash-and-stretch, a walk-rock, a flight
lean, facing-flip — and floats a small **mood emote** above it (💤 asleep, ✨ when
energetic, a tear when sad). No frame-by-frame art needed.

> Tip: a **transparent PNG** or an **SVG** gives the cleanest result (the pet's edges
> blend into the desktop). A picture on a solid background will look like a sticker.
> The heavy image/SVG code lives only in `companion-convert` — the resident pet
> binary never links it, so it stays ~6 MB and starts instantly.

---

## Platforms

* **Linux (X11 / XWayland)** — fully working today: transparency, click-through,
  global cursor, multi-monitor.
* **Wayland (native)** — the window appears, but a pure-Wayland session forbids
  reading the global cursor and absolute positioning; the app forces the X11
  backend so it runs through XWayland, where everything works. (Ubuntu/GNOME ✅)
* **Windows / macOS** — the engine is platform-agnostic Rust; the global-cursor
  probe is X11-only and falls back to autonomous roaming. Native cursor probes for
  Win32/AppKit are the obvious next step (see [ARCHITECTURE.md](ARCHITECTURE.md)).

---

## Configuration

See [`config.toml`](config.toml). Highlights:

```toml
name = "Pico"
scale = 3.0                 # bigger pet
body_color = [255, 176, 84] # recolour the whole creature
follow_cursor = true
flee_cursor = false         # shy mode
reduced_motion = false      # accessibility: no flight, gentle walks
[reminders]
water_minutes = 45
```

---

## 📦 Sharing it (build bundles for Windows / macOS / Linux)

### From this Linux machine, right now

```bash
sudo apt install gcc-mingw-w64-x86-64      # one-time, for the Windows cross-build
./dist.sh
```

That produces two ready-to-share files in `dist/`:

| File | Give it to | They run |
|---|---|---|
| `companion-vX.Y.Z-linux-x86_64.tar.gz` | Linux friends | extract → `./install.sh` (or just `./companion`) |
| `companion-vX.Y.Z-windows-x86_64.zip` | Windows friends | extract → double-click `companion.exe` |

No installer, no runtime, no dependencies — single native binaries (~5 MB).

### macOS (needs Apple hardware — use GitHub Actions)

Apple's SDK license blocks cross-building macOS apps from Linux, so the repo
ships a release workflow ([.github/workflows/release.yml](.github/workflows/release.yml))
that builds **all four bundles** (Linux, Windows, macOS arm64 + Intel `.app`) on
GitHub's machines:

```bash
git init && git add . && git commit -m "companion v0.1.0"
gh repo create companion --public --source . --push    # or add a remote manually
git tag v0.1.0
git push origin v0.1.0          # <- this triggers the builds
```

A few minutes later the GitHub *Releases* page has all four artifacts attached.
The macOS `.app` is ad-hoc signed — recipients right-click → *Open* the first
time (standard for unsigned indie apps; a paid Apple Developer ID removes that).

### Honest platform status

| | runs | cursor-follow | grab/throw | climb windows | notifications |
|---|---|---|---|---|---|
| Linux X11/XWayland | ✅ | ✅ | ✅ | ✅ | ✅ |
| Windows | ✅ | ✅ | ✅ | ⏳ | ⏳ |
| macOS | ✅ | ⏳ | ⏳ | ⏳ | ⏳ |

Windows uses its own presentation path (a small `UpdateLayeredWindow` surface —
DWM rejects per-pixel-alpha GPU swapchains, which is also why there's no
fullscreen overlay there) plus native cursor/button probes, so following and
grab-and-throw work natively. Window-climbing/notifications on Windows
(`EnumWindows`/toast listeners) and the macOS probes (`NSEvent`/`CGWindowList`)
are the top roadmap items and slot into [platform.rs](src/platform.rs) without
touching the engine.

## How it's built

```
winit ──► transparent click-through overlay windows (one per monitor)
wgpu  ──► instanced textured quads, premultiplied alpha → composited onto desktop
x11rb ──► global cursor position
（pure-Rust simulation: emotion · needs · behaviour · physics · procedural sprites）
```

A full tour — module map, the emotion/behaviour algorithms, the multi-monitor
coordinate model, the render pipeline, and the roadmap — is in
[ARCHITECTURE.md](ARCHITECTURE.md).

## License

MIT.
