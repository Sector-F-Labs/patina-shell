# patina-shell

The library every shell piece is built on: the bar, the launcher, the menus, the lock screen
and the wallpaper. It isn't a program and has no place on screen of its own; it's what makes an
egui UI into a piece of the desktop.

## What it gives the pieces

- **A layer-shell host for egui** (`host.rs`, ADR-0003, ADR-0011). eframe only makes ordinary
  windows; desktop pieces need wlr-layer-shell surfaces anchored to an edge, on a layer, with
  or without the keyboard. A piece implements `Shell` (its UI) and passes `Options` (anchor,
  size, layer, keyboard, one surface per output or one, click-through, lock); `Host::connect`
  and `Host::run` do the rest. With `Options::lock` the host draws on ext-session-lock surfaces
  instead, for patina-lock.
- **A software painter** (`raster.rs`, ADR-0002): egui's meshes drawn on the CPU into wl_shm
  buffers. Pieces are small or rarely change, and this needs no GPU or GL driver, so they work
  in VMs and on any compositor. `scale.rs` draws at the output's exact fractional scale.
- **One copy per display** (`piece.rs`): `Single::start` makes a second run of a piece tell the
  first over a socket in `XDG_RUNTIME_DIR` (`<name>-<display>.sock`) and exit, so one button
  toggles a menu and a key keeps a flash up.
- **The bar's look, for every piece** (`bar.rs`, `piece.rs`): the skin and the edge the bar is
  on, from `PATINA_SKIN` and `PATINA_BAR_EDGE` (which the bar sets when it opens a piece) or the
  bar's config file (when the compositor does). `set_skin` and `set_wallpaper` write the bar's
  config for patina-menu; the bar and wallpaper follow the file.
- **What the tray and menus read and change:** `audio.rs` (volume through `wpctl`), `network.rs`
  (Wi-Fi through NetworkManager on the system bus, with zbus, ADR-0022), `power.rs` (the battery
  from sysfs, ADR-0024), `backlight.rs` (sysfs to read, logind's SetBrightness to set, ADR-0020;
  the compositor uses it too).
- **Shared drawing:** `icons.rs` (small icons drawn from shapes in skin colours), `logo.rs` (the
  Sector F flask and Ubuntu Mono, for the launcher button and the default wallpaper),
  `toplevels.rs` (the client side of wlr-foreign-toplevel-management, for the bar's tabs).

## Trying it

`cargo run -p patina-shell --example wifi` prints what `network.rs` reads from NetworkManager.

The Linux-only parts (`backlight.rs` through logind, `network.rs`, `audio.rs`) build everywhere
and do nothing where their service is missing.
