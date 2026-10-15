//! The screen's backlight (ADR-0020): read from /sys/class/backlight, which
//! anyone may read, and set through logind's SetBrightness, which lets the
//! active session change it without root, as GNOME does. The compositor's
//! brightness keys step it; the volume menu's flash shows it and its slider
//! sets it.

use std::path::{Path, PathBuf};

/// The backlight to change: firmware ones first (they know the panel
/// best), then platform, then raw (the GPU's, like intel_backlight).
fn device() -> Option<PathBuf> {
    let rank = |path: &Path| match std::fs::read_to_string(path.join("type"))
        .as_deref()
        .map(str::trim)
    {
        Ok("firmware") => 0,
        Ok("platform") => 1,
        _ => 2,
    };
    std::fs::read_dir("/sys/class/backlight")
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .min_by_key(|p| rank(p))
}

fn read_number(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// The backlight's device, and its brightness now and at most.
fn read() -> Result<(PathBuf, u32, u32), String> {
    let device = device().ok_or("no backlight")?;
    let max = read_number(&device.join("max_brightness")).ok_or("no max_brightness")?;
    let now = read_number(&device.join("brightness")).ok_or("no brightness")?;
    Ok((device, now, max))
}

/// The brightness, 0 to 1; `None` without a backlight (a desktop monitor).
pub fn level() -> Option<f32> {
    let (_, now, max) = read().ok()?;
    (max > 0).then(|| now as f32 / max as f32)
}

/// Steps the backlight by `percent` of its range, never down to black.
pub fn step(percent: i64) -> Result<(), String> {
    let (device, now, max) = read()?;
    write(&device, stepped(now, max, percent))
}

/// Sets the backlight to `level` (0 to 1) of its range, never black.
pub fn set(level: f32) -> Result<(), String> {
    let (device, _, max) = read()?;
    write(&device, at_level(level, max))
}

fn write(device: &Path, value: u32) -> Result<(), String> {
    let name = device
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("odd backlight name")?;
    let system = zbus::blocking::Connection::system().map_err(|e| e.to_string())?;
    let session = zbus::blocking::Proxy::new(
        &system,
        "org.freedesktop.login1",
        "/org/freedesktop/login1/session/auto",
        "org.freedesktop.login1.Session",
    )
    .map_err(|e| e.to_string())?;
    session
        .call::<_, _, ()>("SetBrightness", &("backlight", name, value))
        .map_err(|e| format!("logind SetBrightness: {e}"))
}

/// The lowest brightness: 1% of the range, and at least 1, never black.
fn floor(max: u32) -> i64 {
    (max as i64 / 100).max(1)
}

/// `now` moved by `percent` of `max`, kept between the floor and `max`.
pub fn stepped(now: u32, max: u32, percent: i64) -> u32 {
    let step = (max as i64 * percent.abs() / 100).max(1) * percent.signum();
    (now as i64 + step).clamp(floor(max), max as i64) as u32
}

/// `level` (0 to 1) of `max`, kept between the floor and `max`.
fn at_level(level: f32, max: u32) -> u32 {
    ((level.clamp(0.0, 1.0) as f64 * max as f64).round() as i64).clamp(floor(max), max as i64)
        as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_steps_by_percent_of_the_range() {
        // nifl's intel_backlight: max 19393.
        assert_eq!(stepped(5953, 19393, 5), 5953 + 969);
        assert_eq!(stepped(5953, 19393, -5), 5953 - 969);
    }

    #[test]
    fn brightness_stops_at_the_ends_and_never_goes_black() {
        assert_eq!(stepped(19000, 19393, 5), 19393);
        assert_eq!(stepped(300, 19393, -5), 193);
        assert_eq!(stepped(193, 19393, -5), 193);
        // A tiny range still moves, and still keeps a floor of 1.
        assert_eq!(stepped(3, 10, 5), 4);
        assert_eq!(stepped(1, 10, -5), 1);
    }

    #[test]
    fn a_level_is_a_share_of_the_range_never_black() {
        assert_eq!(at_level(0.5, 19393), 9697);
        assert_eq!(at_level(1.0, 19393), 19393);
        assert_eq!(at_level(0.0, 19393), 193);
        assert_eq!(at_level(-1.0, 10), 1);
    }
}
