//! The battery, from /sys/class/power_supply: the charge, whether it's
//! charging, and the time left at the current draw (ADR-0024). Plain file
//! reads, cheap enough to repeat every few seconds; laptops with two
//! batteries count as one.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charge {
    Charging,
    Discharging,
    Full,
    /// Plugged in but holding (a charge threshold, or warm).
    NotCharging,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Battery {
    /// 0 to 100.
    pub percent: u8,
    pub charge: Charge,
    /// Minutes until empty (discharging) or full (charging), when the
    /// battery reports its power draw.
    pub minutes: Option<u32>,
}

impl Battery {
    /// Low enough to warn about: 10% or under, and not charging.
    pub fn low(&self) -> bool {
        self.percent <= 10 && self.charge == Charge::Discharging
    }

    /// "65% · 4 h 10 min left", for a tooltip.
    pub fn describe(&self) -> String {
        let time = self.minutes.map(|m| {
            if m >= 60 {
                format!("{} h {} min", m / 60, m % 60)
            } else {
                format!("{m} min")
            }
        });
        match (self.charge, time) {
            (Charge::Discharging, Some(t)) => format!("{}% · {t} left", self.percent),
            (Charge::Charging, Some(t)) => format!("Charging {}% · {t} to full", self.percent),
            (Charge::Charging, None) => format!("Charging {}%", self.percent),
            (Charge::Full, _) => "Fully charged".into(),
            (Charge::NotCharging, _) => format!("{}% · plugged in, not charging", self.percent),
            (Charge::Discharging, None) => format!("{}%", self.percent),
        }
    }
}

/// One battery's readings, in µWh and µW (or µAh and µA; the ratios are
/// the same).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Reading {
    pub now: f64,
    pub full: f64,
    pub rate: f64,
    pub status: Option<Charge>,
}

fn status(text: &str) -> Option<Charge> {
    match text.trim() {
        "Charging" => Some(Charge::Charging),
        "Discharging" => Some(Charge::Discharging),
        "Full" => Some(Charge::Full),
        "Not charging" => Some(Charge::NotCharging),
        _ => None,
    }
}

fn number(dir: &Path, name: &str) -> Option<f64> {
    std::fs::read_to_string(dir.join(name))
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn read_one(dir: &Path) -> Option<Reading> {
    let (now, full, rate) = match number(dir, "energy_now") {
        Some(now) => (
            now,
            number(dir, "energy_full")?,
            number(dir, "power_now").unwrap_or(0.0),
        ),
        None => (
            number(dir, "charge_now")?,
            number(dir, "charge_full")?,
            number(dir, "current_now").unwrap_or(0.0),
        ),
    };
    let status = std::fs::read_to_string(dir.join("status"))
        .ok()
        .and_then(|s| status(&s));
    Some(Reading {
        now,
        full,
        rate: rate.abs(),
        status,
    })
}

/// The batteries combined, or `None` on a machine without one.
pub fn read() -> Option<Battery> {
    let readings: Vec<Reading> = std::fs::read_dir("/sys/class/power_supply")
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            std::fs::read_to_string(p.join("type")).is_ok_and(|t| t.trim() == "Battery")
                // Mice and headsets report batteries too; only the
                // machine's own power them.
                && std::fs::read_to_string(p.join("scope")).map_or(true, |s| s.trim() != "Device")
        })
        .filter_map(|p| read_one(&p))
        .collect();
    combine(&readings)
}

/// Several batteries as one: summed charge, and the most telling status
/// (charging over discharging over the rest).
pub fn combine(readings: &[Reading]) -> Option<Battery> {
    let full: f64 = readings.iter().map(|r| r.full).sum();
    if full <= 0.0 {
        return None;
    }
    let now: f64 = readings.iter().map(|r| r.now).sum();
    let rate: f64 = readings.iter().map(|r| r.rate).sum();
    let has = |c: Charge| readings.iter().any(|r| r.status == Some(c));
    let charge = if has(Charge::Charging) {
        Charge::Charging
    } else if has(Charge::Discharging) {
        Charge::Discharging
    } else if has(Charge::NotCharging) {
        Charge::NotCharging
    } else {
        Charge::Full
    };
    let percent = ((now / full) * 100.0).round().clamp(0.0, 100.0) as u8;
    let hours = match charge {
        Charge::Discharging if rate > 0.0 => Some(now / rate),
        Charge::Charging if rate > 0.0 => Some((full - now).max(0.0) / rate),
        _ => None,
    };
    Some(Battery {
        percent,
        charge,
        // Above two days the draw is too small to mean anything.
        minutes: hours
            .filter(|h| *h < 48.0)
            .map(|h| (h * 60.0).round() as u32),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(now: f64, full: f64, rate: f64, status: Charge) -> Reading {
        Reading {
            now,
            full,
            rate,
            status: Some(status),
        }
    }

    #[test]
    fn nifls_battery() {
        let b = combine(&[reading(
            29_560_000.0,
            45_670_000.0,
            7_066_000.0,
            Charge::Discharging,
        )])
        .unwrap();
        assert_eq!(b.percent, 65);
        assert_eq!(b.minutes, Some(251));
        assert_eq!(b.describe(), "65% · 4 h 11 min left");
        assert!(!b.low());
    }

    #[test]
    fn charging_counts_time_to_full() {
        let b = combine(&[reading(40.0, 100.0, 30.0, Charge::Charging)]).unwrap();
        assert_eq!(b.minutes, Some(120));
        assert_eq!(b.describe(), "Charging 40% · 2 h 0 min to full");
    }

    #[test]
    fn two_batteries_are_one_and_charging_wins() {
        let b = combine(&[
            reading(10.0, 50.0, 0.0, Charge::Full),
            reading(40.0, 50.0, 5.0, Charge::Charging),
        ])
        .unwrap();
        assert_eq!(b.percent, 50);
        assert_eq!(b.charge, Charge::Charging);
    }

    #[test]
    fn low_only_when_discharging() {
        let low = combine(&[reading(8.0, 100.0, 10.0, Charge::Discharging)]).unwrap();
        assert!(low.low());
        let charging = combine(&[reading(8.0, 100.0, 10.0, Charge::Charging)]).unwrap();
        assert!(!charging.low());
    }

    #[test]
    fn no_battery_no_reading() {
        assert_eq!(combine(&[]), None);
    }
}
