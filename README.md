# patina-shell

Desktop pieces in [egui](https://github.com/emilk/egui): a bar, a launcher, a menu or a lock
screen, each an egui UI on a wlr-layer-shell surface, drawn in software. It's the base of the
[Patina](https://github.com/Sector-F-Labs/patina) desktop's shell pieces and of
[patina-launcher](https://github.com/Sector-F-Labs/patina-launcher), and works on any
compositor with layer shell: sway, Hyprland, river, KDE, COSMIC, Patina's own.

eframe only makes ordinary windows; a desktop piece needs a surface anchored to an edge, on a
layer above or below the windows, taking the keyboard or not. That's what this crate does.

```rust
use patina_shell::{Anchor, Host, KeyboardInteractivity, Layer, Options, Shell};

struct Hello;

impl Shell for Hello {
    fn setup(&mut self, ctx: &egui::Context) {}
    fn ui(&mut self, ui: &mut egui::Ui, _output: &str) {
        ui.heading("Hello");
    }
}

let options = Options {
    namespace: "hello".into(),
    layer: Layer::Overlay,
    anchor: Anchor::empty(),          // none centres it
    size: (360, 140),
    exclusive_zone: 0,
    margin: (0, 0, 0, 0),
    keyboard: KeyboardInteractivity::Exclusive,
    per_output: false,
    toplevels: false,
    click_through: false,
    lock: false,
    window_fallback: true,
};
let (host, event_loop) = Host::connect(options, Hello)?;
host.run(event_loop)?;
```

`cargo run --example hello` shows it (`make example`).

## What's in it

- **`Host`** (`host.rs`): makes the surfaces from `Options` (anchor, size, layer, margins,
  exclusive zone, keyboard, one per output or one, click-through) and drives them: outputs,
  fractional scale (`scale.rs`, wp_fractional_scale and wp_viewporter), pointer, keyboard and
  touch into egui (each finger an egui touch, the first one egui's pointer too, so taps press), frame callbacks, repaints when egui asks. A shell can size its surfaces to what it
  shows (`Shell::wanted_size`), and learn its output's size (`output_size`) to know where on the
  screen something centred is; `set_anchor`, `set_margin`, `set_size` and `set_exclusive_zone`
  move a surface while it runs.
  - With `lock`, ext-session-lock surfaces instead, one per output, for a lock screen that
    unlocks when the shell finishes.
  - With `window_fallback`, a compositor without layer shell (GNOME) gets an ordinary window
    instead: borderless, maximised and transparent, with the shell centred in it, and a click
    around it or closing the window ending it, as wofi or Spotlight behave.
- **`raster`**: a small CPU renderer for egui's meshes into wl_shm buffers. Desktop pieces are
  small or rarely change, and this needs no GPU, EGL or GL driver, so it works in VMs and on
  software-rendered compositors alike.
- **`piece`**: one running copy per Wayland display. A second run tells the first a word (to
  close, to flash) over a socket in `XDG_RUNTIME_DIR` and exits, so one key or button toggles a
  menu. `opening` places a piece at the bar's button that opened it (`PATINA_OPEN_AT`,
  `edge,x,y[,far]`), turned towards the side of the screen with more room, or on the bar's
  edge without it.
- **`bar`**: the Patina bar's conventions every piece follows: its skin and edge from
  `PATINA_SKIN` and `PATINA_BAR_EDGE`, or from `~/.config/patina-bar/config.toml`, and writing
  `skin` and `wallpaper` there.
- **`toplevels`**: the open windows through wlr-foreign-toplevel-management, for a window list.

Skins are [Patina](https://github.com/Sector-F-Labs/patina)'s, so pieces look like Patina apps.

## Use

Not on crates.io; depend on it from GitHub. It uses Patina's `develop` branch, so depend on
that too, for the skins to be the same crate:

```toml
patina-shell = { git = "https://github.com/Sector-F-Labs/patina-shell" }
patina-ui = { git = "https://github.com/Sector-F-Labs/patina", branch = "develop" }
egui = "0.35"
```

## Develop

`make check`: tests and clippy. BSD-3-Clause.
