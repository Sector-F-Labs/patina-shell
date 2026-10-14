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

/// The bar's skin, from Patina's built-ins and the piece's own skins
/// folder, or Default.
pub fn skin(app: &str) -> Skin {
    let Some(name) = crate::bar::skin() else {
        return Skin::default_skin();
    };
    let dirs: Vec<_> = skin::app_dir(app).into_iter().collect();
    let (skins, _) = skin::load_all(skin::BUILT_IN, &dirs);
    skins
        .into_iter()
        .find(|s| s.name == name)
        .unwrap_or_else(Skin::default_skin)
}
