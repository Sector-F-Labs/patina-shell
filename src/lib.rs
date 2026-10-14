//! The Patina desktop's shell pieces (the bar, the launcher) are egui on
//! wlr-layer-shell surfaces. This crate is what they share: the host that
//! makes and drives the surfaces, and the software painter (ADR-0002,
//! ADR-0003).

pub mod audio;
pub mod bar;
pub mod host;
pub mod icons;
pub mod raster;
mod scale;
pub mod toplevels;

pub use host::{Anchor, Host, KeyboardInteractivity, Layer, Options, Shell};
pub use smithay_client_toolkit::reexports::calloop;
pub use toplevels::{Toplevel, ToplevelRequest};
