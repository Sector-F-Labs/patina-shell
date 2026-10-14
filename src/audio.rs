//! The default output's volume, through PipeWire's wpctl (installed with
//! PipeWire, Ubuntu's sound system), for the bar's speaker and the volume
//! menu. The calls wait for wpctl, so run them off the UI thread.

use std::process::{Command, Stdio};

/// The default output's level, 0.0 to 1.0 (wpctl allows more; the slider
/// stops at 1), and whether it's muted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Volume {
    pub level: f32,
    pub muted: bool,
}

const SINK: &str = "@DEFAULT_AUDIO_SINK@";

/// Reads `wpctl get-volume`'s "Volume: 0.57 [MUTED]".
pub fn parse(text: &str) -> Option<Volume> {
    let rest = text.trim().strip_prefix("Volume:")?.trim();
    let level = rest.split_whitespace().next()?.parse().ok()?;
    Some(Volume {
        level,
        muted: rest.contains("[MUTED]"),
    })
}

pub fn get() -> Option<Volume> {
    let out = Command::new("wpctl")
        .args(["get-volume", SINK])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    parse(&String::from_utf8_lossy(&out.stdout))
}

pub fn set_level(level: f32) {
    let level = format!("{:.3}", level.clamp(0.0, 1.0));
    run(&["set-volume", SINK, &level]);
}

pub fn set_muted(muted: bool) {
    run(&["set-mute", SINK, if muted { "1" } else { "0" }]);
}

fn run(args: &[&str]) {
    if let Err(e) = Command::new("wpctl")
        .args(args)
        .stdin(Stdio::null())
        .status()
    {
        eprintln!("patina-volume: wpctl: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_wpctl() {
        assert_eq!(
            parse("Volume: 0.57\n"),
            Some(Volume {
                level: 0.57,
                muted: false
            })
        );
        assert_eq!(
            parse("Volume: 0.57 [MUTED]\n"),
            Some(Volume {
                level: 0.57,
                muted: true
            })
        );
        assert_eq!(
            parse("Volume: 1.20"),
            Some(Volume {
                level: 1.2,
                muted: false
            })
        );
        assert_eq!(parse("Translate ID error: '@DEFAULT_AUDIO_SINK@'"), None);
        assert_eq!(parse(""), None);
    }
}
