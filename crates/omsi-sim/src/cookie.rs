//! The lamps of `[spotlight_cookie]`: where they shine, how they turn and how they come on.
//!
//! A `[spotlight_cookie]` is one lamp, or - unless its flag says 1 - a lamp and its twin on the
//! other side of the vehicle. The twin's position and direction are mirrored across the
//! vehicle's axis; the beam picture and the angle offsets are not, so an asymmetric beam is
//! asymmetric the same way on both sides and both lamps turn the same way. A vehicle that
//! wants its two modules apart (cornering lights, say) declares each as a lamp of its own
//! with the flag 1: each has its own switching variable, its own offsets and its own picture.

use glam::{Quat, Vec3};
use omsi_model::SpotlightCookie;

/// How far (in degrees) a script may turn a lamp with its offset variables.
pub const MAX_OFFSET_DEG: f32 = 90.0;

/// A lamp's brightness `b` one step of `dt` seconds on towards `target` with the lamp's time
/// constant: 63 % of the way in that time when it is switched on, down to 27 % of its brightness
/// when it is switched off (as a `[light_enh_2]`'s `timeconst`, see `VehicleInstance::light_fade`);
/// at once when the time constant is 0.
pub fn fade_step(b: f32, target: f32, dt: f32, time_const: f32) -> f32 {
    let target = if target.is_finite() { target } else { 0.0 };
    let b = if b.is_finite() { b } else { target };
    if time_const <= 0.001 {
        target
    } else {
        let rate = if target > b { 1.0 } else { 1.31 } / time_const;
        b + (target - b) * (1.0 - (-dt * rate).exp())
    }
}

/// A direction in the vehicle's frame (x right, y forward, z up) turned `h_deg` degrees to the
/// right and then `v_deg` degrees up, about the vehicle's up axis and the lamp's own right axis.
pub fn aim(dir: Vec3, h_deg: f32, v_deg: f32) -> Vec3 {
    let d = dir.normalize_or_zero();
    if d == Vec3::ZERO {
        return d;
    }
    let clamp = |a: f32| if a.is_finite() { a.clamp(-MAX_OFFSET_DEG, MAX_OFFSET_DEG) } else { 0.0 };
    let (h, v) = (clamp(h_deg).to_radians(), clamp(v_deg).to_radians());
    let yawed = Quat::from_rotation_z(-h) * d;
    let right = yawed.cross(Vec3::Z);
    let right = if right.length_squared() > 1e-8 { right.normalize() } else { Vec3::X };
    (Quat::from_axis_angle(right, v) * yawed).normalize()
}

/// The lamps of `sp` in the vehicle's frame - position, direction and share of its light -
/// with its brightness at `k` (0..1) and its offsets at `h_deg` (to the right) and `v_deg` (up):
/// none when it is off, the lamp and its twin unless the flag keeps the one.
pub fn lamps(sp: &SpotlightCookie, k: f32, h_deg: f32, v_deg: f32) -> Vec<(Vec3, Vec3, f32)> {
    let k = if k.is_finite() { k.clamp(0.0, 1.0) } else { 0.0 };
    // (a lamp fading out stays a little while; below this it is dark for all to see)
    if k <= 0.002 {
        return Vec::new();
    }
    let sides: &[f32] = if sp.mirrored { &[1.0, -1.0] } else { &[1.0] };
    sides
        .iter()
        .map(|s| {
            let pos = Vec3::new(sp.position[0] * s, sp.position[1], sp.position[2]);
            let dir = Vec3::new(sp.direction[0] * s, sp.direction[1], sp.direction[2]);
            (pos, aim(dir, h_deg, v_deg), k / sides.len() as f32)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lamp(mirrored: bool) -> SpotlightCookie {
        SpotlightCookie { position: [0.9, 5.9, 0.7], direction: [0.1, 1.0, 0.0], range: 100.0, mirrored, ..Default::default() }
    }

    fn deg(a: Vec3, b: Vec3) -> f32 {
        a.normalize().dot(b.normalize()).clamp(-1.0, 1.0).acos().to_degrees()
    }

    #[test]
    fn a_lamp_without_offsets_keeps_its_direction() {
        let d = aim(Vec3::new(0.0, 1.0, 0.0), 0.0, 0.0);
        assert!((d - Vec3::Y).length() < 1e-6);
        let tilted = Vec3::new(0.1, 1.0, -0.2).normalize();
        assert!((aim(tilted, 0.0, 0.0) - tilted).length() < 1e-6);
    }

    #[test]
    fn the_offsets_turn_the_lamp_right_and_up() {
        let right = aim(Vec3::Y, 10.0, 0.0);
        assert!(right.x > 0.0 && (deg(right, Vec3::Y) - 10.0).abs() < 1e-3 && right.z.abs() < 1e-6);
        let left = aim(Vec3::Y, -10.0, 0.0);
        assert!(left.x < 0.0);
        let up = aim(Vec3::Y, 0.0, 10.0);
        assert!(up.z > 0.0 && (deg(up, Vec3::Y) - 10.0).abs() < 1e-3 && up.x.abs() < 1e-6);
        let down = aim(Vec3::Y, 0.0, -10.0);
        assert!(down.z < 0.0);
        // both at once: 10 degrees up of the direction turned 20 degrees to the right
        let both = aim(Vec3::Y, 20.0, 10.0);
        assert!((both.z - 10.0f32.to_radians().sin()).abs() < 1e-4);
        assert!((both.x.atan2(both.y).to_degrees() - 20.0).abs() < 1e-3);
        // a turn is not more than a lamp can (a script gone wild)
        assert!((deg(aim(Vec3::Y, 500.0, 0.0), Vec3::Y) - MAX_OFFSET_DEG).abs() < 1e-3);
        assert!((aim(Vec3::Y, f32::NAN, f32::INFINITY) - Vec3::Y).length() < 1e-6);
    }

    #[test]
    fn a_twin_is_mirrored_but_turns_the_same_way() {
        let two = lamps(&lamp(true), 1.0, 10.0, 0.0);
        assert_eq!(two.len(), 2);
        assert!((two[0].0.x - 0.9).abs() < 1e-6 && (two[1].0.x + 0.9).abs() < 1e-6);
        assert!((two[0].2 - 0.5).abs() < 1e-6 && (two[1].2 - 0.5).abs() < 1e-6);
        // before the turn the lamps point 5.7 degrees to the right and the left; after it both are 10 degrees further right
        let before = lamps(&lamp(true), 1.0, 0.0, 0.0);
        for i in 0..2 {
            assert!((deg(two[i].1, before[i].1) - 10.0).abs() < 1e-3);
            assert!(two[i].1.x - before[i].1.x > 0.0);
        }
    }

    #[test]
    fn a_module_apart_is_one_lamp_with_all_its_light() {
        let one = lamps(&lamp(false), 0.6, 0.0, 0.0);
        assert_eq!(one.len(), 1);
        assert!((one[0].2 - 0.6).abs() < 1e-6);
        assert!((one[0].0.x - 0.9).abs() < 1e-6);
    }

    #[test]
    fn a_lamp_that_is_off_has_no_light() {
        assert!(lamps(&lamp(true), 0.0, 0.0, 0.0).is_empty());
        assert!(lamps(&lamp(true), 0.001, 0.0, 0.0).is_empty());
        assert!(lamps(&lamp(true), f32::NAN, 0.0, 0.0).is_empty());
        assert_eq!(lamps(&lamp(false), 3.0, 0.0, 0.0)[0].2, 1.0);
    }

    #[test]
    fn the_fade_follows_the_switch_with_its_time_constant() {
        // at once without a time constant
        assert_eq!(fade_step(0.0, 1.0, 0.016, 0.0), 1.0);
        assert_eq!(fade_step(1.0, 0.0, 0.016, 0.0), 0.0);
        // switched on: 63 % of the way after one time constant
        let mut b = 0.0;
        for _ in 0..1000 {
            b = fade_step(b, 1.0, 0.001, 1.0);
        }
        assert!((b - 0.632).abs() < 0.01, "{b}");
        // switched off it has fallen to 27 % of its brightness after one time constant
        let mut b = 1.0;
        for _ in 0..1000 {
            b = fade_step(b, 0.0, 0.001, 1.0);
        }
        assert!((b - 0.27).abs() < 0.01, "{b}");
        // a NaN does not stay
        assert_eq!(fade_step(f32::NAN, 1.0, 0.016, 0.5), 1.0);
        assert_eq!(fade_step(0.5, f32::NAN, 0.016, 0.0), 0.0);
    }
}
