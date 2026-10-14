//! Fractional scaling for the host's surfaces: wp_fractional_scale_v1 says
//! the exact scale (in 120ths), and wp_viewporter maps the buffer drawn at
//! that scale back to the surface's logical size. Where the compositor has
//! neither, the host keeps to integer buffer scales.

use smithay_client_toolkit::{
    dispatch2::Dispatch2,
    reexports::{
        client::{Connection, Proxy, QueueHandle},
        protocols::wp::fractional_scale::v1::client::wp_fractional_scale_v1::{
            self, WpFractionalScaleV1,
        },
    },
};

use crate::host::{Host, Shell};

/// User data for objects with no events: the managers and viewports.
pub(crate) struct NoEvents;

impl<I: Proxy, S: Shell> Dispatch2<I, Host<S>> for NoEvents {
    fn event(&self, _: &mut Host<S>, _: &I, _: I::Event, _: &Connection, _: &QueueHandle<Host<S>>) {
    }
}

/// User data for a surface's fractional-scale object.
pub(crate) struct FractionalData;

impl<S: Shell> Dispatch2<WpFractionalScaleV1, Host<S>> for FractionalData {
    fn event(
        &self,
        host: &mut Host<S>,
        proxy: &WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _: &Connection,
        _: &QueueHandle<Host<S>>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            host.set_scale_120(proxy, scale);
        }
    }
}

/// The buffer size, in pixels, for a surface `logical` points across at
/// `scale`, as the protocol asks: rounded half away from zero.
pub fn pixels(logical: u32, scale: f32) -> i32 {
    (logical as f32 * scale).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffers_are_the_logical_size_times_the_scale_rounded() {
        assert_eq!(pixels(41, 1.5), 62);
        assert_eq!(pixels(1280, 1.5), 1920);
        assert_eq!(pixels(41, 1.25), 51);
        assert_eq!(pixels(41, 1.0), 41);
    }
}
