//! [ROLLBACK units-80] Metric or imperial figures on the game's own displays (the information
//! bar, the touch panel's speed, the navigator, the names over the other players): speed in
//! km/h or mph, distances in m / km or yd / mi, the odometer in km or mi. The `imperial_units`
//! setting; kept in one place for the displays to ask, not passed to each of them.

use std::sync::atomic::{AtomicBool, Ordering};

static IMPERIAL: AtomicBool = AtomicBool::new(false);

/// km/h to mph, m to yards and to miles.
const MPH: f32 = 0.621_371_2;
const YARD: f64 = 1.093_613_3;
const MILE: f64 = 0.000_621_371_2;

pub fn set_imperial(on: bool) {
    IMPERIAL.store(on, Ordering::Relaxed);
}

pub fn imperial() -> bool {
    IMPERIAL.load(Ordering::Relaxed)
}

/// A speed given in km/h, in the unit shown.
pub fn speed_value(kmh: f32) -> f32 {
    if imperial() {
        kmh * MPH
    } else {
        kmh
    }
}

/// The speed unit: `metric` (the language's own writing of km/h) or "mph".
pub fn speed_unit(metric: &'static str) -> &'static str {
    if imperial() {
        "mph"
    } else {
        metric
    }
}

/// "50 km/h" / "31 mph".
pub fn speed_text(kmh: f32) -> String {
    format!("{:.0} {}", speed_value(kmh), speed_unit("km/h"))
}

fn round_to(v: f64, step: f64) -> f64 {
    if step > 0.0 {
        ((v / step).round() * step).max(step)
    } else {
        v
    }
}

/// A distance in metres: "230 m" / "1.2 km", or "250 yd" / "0.7 mi". Short distances are rounded to
/// `step` of the unit shown (0: not rounded).
pub fn distance(m: f64, step: f64) -> String {
    if imperial() {
        let yd = m * YARD;
        if yd >= 1000.0 {
            format!("{:.1} mi", m * MILE)
        } else {
            format!("{:.0} yd", round_to(yd, step))
        }
    } else if m >= 1000.0 {
        format!("{:.1} km", m / 1000.0)
    } else {
        format!("{:.0} m", round_to(m, step))
    }
}

/// `n` with a space between the thousands: 1433243 is "1 433 243".
pub fn grouped(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (k, c) in digits.chars().enumerate() {
        if k > 0 && (digits.len() - k) % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    if n < 0 {
        out.insert(0, '-');
    }
    out
}

/// The odometer as the dials write it (the tenth cut off, not rounded) from the bus's whole
/// kilometres and the metres beyond: "1 433 243.3 km" / "890 534.6 mi".
pub fn odometer(km: i64, m: i64) -> String {
    let m = m.clamp(0, 999);
    if imperial() {
        let miles = km as f64 * 0.621_371_192 + m as f64 * 0.000_621_371_192;
        let tenths = (miles * 10.0 + 1e-6).floor() as i64;
        format!("{}.{} mi", grouped(tenths / 10), tenths % 10)
    } else {
        format!("{}.{} km", grouped(km), m / 100)
    }
}

/// The scale bar's lengths (metres, text), shortest first.
pub fn scale_steps() -> &'static [(f64, &'static str)] {
    if imperial() {
        &[
            (9.144, "10 yd"),
            (18.288, "20 yd"),
            (45.72, "50 yd"),
            (91.44, "100 yd"),
            (182.88, "200 yd"),
            (457.2, "500 yd"),
            (804.672, "0.5 mi"),
            (1609.344, "1 mi"),
            (3218.688, "2 mi"),
            (8046.72, "5 mi"),
        ]
    } else {
        &[
            (10.0, "10 m"),
            (20.0, "20 m"),
            (50.0, "50 m"),
            (100.0, "100 m"),
            (200.0, "200 m"),
            (500.0, "500 m"),
            (1000.0, "1 km"),
            (2000.0, "2 km"),
            (5000.0, "5 km"),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_are_set_apart() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1000), "1 000");
        assert_eq!(grouped(1433243), "1 433 243");
        assert_eq!(grouped(-12345), "-12 345");
    }

    #[test]
    fn the_odometer_cuts_the_tenth_off() {
        set_imperial(false);
        assert_eq!(odometer(1433243, 399), "1 433 243.3 km");
        assert_eq!(odometer(123468, 850), "123 468.8 km");
        assert_eq!(odometer(84894, 0), "84 894.0 km");
    }
}
