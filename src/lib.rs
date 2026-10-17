//! Desktop pieces in egui: a bar, a launcher, a menu or a lock screen are
//! egui UIs on wlr-layer-shell surfaces, drawn in software. This crate is
//! what they share: the host that makes and drives the surfaces
//! ([`Host`], ADR-0003 and ADR-0011 of the Patina desktop), the software
//! painter ([`raster`], its ADR-0002), one running copy per display
//! ([`piece`]), the Patina bar's skin and edge ([`bar`]), and the open
//! windows ([`toplevels`]).

pub mod bar;
pub mod host;
pub mod piece;
pub mod raster;
mod scale;
pub mod toplevels;

pub use host::{Anchor, Host, KeyboardInteractivity, Layer, Options, Shell, output_size};
pub use smithay_client_toolkit::reexports::calloop;
pub use toplevels::{Toplevel, ToplevelRequest};
