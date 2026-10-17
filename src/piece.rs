//! What every shell piece the bar opens (launcher, volume menu, Wi-Fi menu)
//! does alike: run once per display, with a second run telling the first
//! (to close, or to flash) over a socket in the runtime directory; and the
//! bar's skin and edge, so it matches the bar and opens from it.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use patina::skin::{self, Skin};

use crate::Anchor;

/// The socket of piece `name` on this Wayland display.
fn socket_path(name: &str) -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from);
    let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into());
    dir.join(format!("{name}-{display}.sock"))
}

/// One running copy of a piece.
pub struct Single {
    path: PathBuf,
}

impl Single {
    /// If `name` is already running, sends it `word` and returns `None`:
    /// this run is done. Otherwise takes the socket and calls `heard` with
    /// every word later runs send, on a thread of its own.
    pub fn start(
        name: &str,
        word: &str,
        heard: impl Fn(String) -> bool + Send + 'static,
    ) -> Result<Option<Self>, String> {
        let path = socket_path(name);
        if let Ok(mut open) = UnixStream::connect(&path) {
            let _ = open.write_all(word.as_bytes());
            return Ok(None);
        }
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        std::thread::Builder::new()
            .name("socket".into())
            .spawn(move || {
                for mut stream in listener.incoming().flatten() {
                    let mut word = String::new();
                    let _ = stream.read_to_string(&mut word);
                    if !heard(word) {
                        return;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Some(Self { path }))
    }
}

impl Drop for Single {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The edge the bar is on, which the piece opens from.
pub fn bar_edge() -> Anchor {
    if crate::bar::at_top() {
        Anchor::TOP
    } else {
        Anchor::BOTTOM
    }
}

/// A layer surface's place: anchor, margins (top, right, bottom, left) and
/// exclusive zone.
pub type Place = (Anchor, (i32, i32, i32, i32), i32);

/// Where a piece `size` big goes, with `gap` around it: just past the
/// button that opened it, when the bar said where that is
/// (`PATINA_OPEN_AT`), turned towards the side of the screen with more
/// room; otherwise on the bar's edge, at `side` (left or right). Its
/// anchor, margins (top, right, bottom, left) and exclusive zone.
pub fn opening(size: (u32, u32), gap: i32, side: Anchor) -> Place {
    std::env::var("PATINA_OPEN_AT")
        .ok()
        .and_then(|at| place(&at, size, gap))
        .unwrap_or((bar_edge() | side, (gap, gap, gap, gap), 0))
}

/// `PATINA_OPEN_AT`, `edge,x,y[,far]` (as patina-menu's `--at` and
/// `--edge` take them, and how far the button is from the screen's far
/// side along the edge), as a place for a piece `size` big.
fn place(at: &str, (w, h): (u32, u32), gap: i32) -> Option<Place> {
    let mut parts = at.split(',');
    let edge = parts.next()?;
    let mut number = || parts.next().and_then(|n| n.trim().parse::<f32>().ok());
    let (x, y, far) = (number()?, number()?, number());
    // Centred on the button, but never off the screen: measured from the
    // nearer end, kept the gap in from it.
    let along = |from: f32, half: f32| ((from - half).round() as i32).max(gap);
    let (mut margin, mut anchor) = ((0, 0, 0, 0), Anchor::empty());
    match edge {
        "top" | "bottom" => {
            let half = w as f32 / 2.0;
            if let Some(far) = far.filter(|&f| f < x) {
                anchor |= Anchor::RIGHT;
                margin.1 = along(far, half);
            } else {
                anchor |= Anchor::LEFT;
                margin.3 = along(x, half);
            }
            if edge == "top" {
                anchor |= Anchor::TOP;
                margin.0 = y.round() as i32;
            } else {
                anchor |= Anchor::BOTTOM;
                margin.2 = y.round() as i32;
            }
        }
        "left" | "right" => {
            let half = h as f32 / 2.0;
            if let Some(far) = far.filter(|&f| f < y) {
                anchor |= Anchor::BOTTOM;
                margin.2 = along(far, half);
            } else {
                anchor |= Anchor::TOP;
                margin.0 = along(y, half);
            }
            if edge == "left" {
                anchor |= Anchor::LEFT;
                margin.3 = x.round() as i32;
            } else {
                anchor |= Anchor::RIGHT;
                margin.1 = x.round() as i32;
            }
        }
        _ => return None,
    }
    // Margins from the screen's edges, not from what the bar keeps.
    Some((anchor, margin, -1))
}

/// The bar's skin, from Patina's built-ins, the bar's skins folder and the
/// piece's own, or Default.
pub fn skin(app: &str) -> Skin {
    let Some(name) = crate::bar::skin() else {
        return Skin::default_skin();
    };
    let dirs: Vec<_> = skin::app_dir("patina-bar")
        .into_iter()
        .chain(skin::app_dir(app))
        .collect();
    let (skins, _) = skin::load_all(skin::BUILT_IN, &dirs);
    skins
        .into_iter()
        .find(|s| s.name == name)
        .unwrap_or_else(Skin::default_skin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_piece_opens_at_its_button_towards_the_room() {
        // Near the right of a bottom bar: measured from the right.
        assert_eq!(
            place("bottom,1800,53,120", (300, 400), 8),
            Some((Anchor::BOTTOM | Anchor::RIGHT, (0, 8, 53, 0), -1)),
            "kept the gap in from the edge"
        );
        assert_eq!(
            place("bottom,700,53,1220", (300, 400), 8),
            Some((Anchor::BOTTOM | Anchor::LEFT, (0, 0, 53, 550), -1))
        );
        assert_eq!(
            place("top,700,53", (300, 400), 8),
            Some((Anchor::TOP | Anchor::LEFT, (53, 0, 0, 550), -1)),
            "without the far side, from the left"
        );
        // Beside a column on the left, level with the button.
        assert_eq!(
            place("left,53,500,580", (300, 400), 8),
            Some((Anchor::LEFT | Anchor::TOP, (300, 0, 0, 53), -1))
        );
        assert_eq!(place("sideways,1,2", (300, 400), 8), None);
        assert_eq!(place("bottom,x,2", (300, 400), 8), None);
    }
}
