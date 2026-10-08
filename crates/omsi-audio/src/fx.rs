//! The colour of a car radio, for the ones who want it: a small speaker that shines in the
//! middle and has no bass, the hiss and thin, mono sound of a weak FM signal, and now and
//! then a dropout. It works on the internet radio's frames in the mixer, after the stream is
//! read and before the voice's gain and filter, and costs a handful of one-pole filters per
//! frame. Everything is off (and nothing is touched) while all three amounts are 0.

use std::f32::consts::PI;

/// How much of each effect, 0..1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RadioFx {
    /// The small speaker: highs and lows cut, the middle pushed, a little mono, a little
    /// overdrive.
    pub speaker: f32,
    /// The FM signal's weakness: hiss, crackle, the high end and the stereo fading in and
    /// out as the reception wanders.
    pub noise: f32,
    /// How often the sound breaks up: short mute with static, a deep fade.
    pub dropouts: f32,
}

impl RadioFx {
    pub const OFF: RadioFx = RadioFx { speaker: 0.0, noise: 0.0, dropouts: 0.0 };

    /// A noticeable but gentle colour.
    pub fn light() -> RadioFx {
        RadioFx { speaker: 0.5, noise: 0.25, dropouts: 0.15 }
    }

    /// A cheap radio in a bad spot.
    pub fn strong() -> RadioFx {
        RadioFx { speaker: 0.9, noise: 0.65, dropouts: 0.5 }
    }

    pub fn is_off(&self) -> bool {
        self.speaker <= 0.001 && self.noise <= 0.001 && self.dropouts <= 0.001
    }

    pub fn clamped(self) -> RadioFx {
        let c = |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
        RadioFx { speaker: c(self.speaker), noise: c(self.noise), dropouts: c(self.dropouts) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Event {
    None,
    /// Sound gone for a moment, static in its place.
    Mute,
    /// The last few hundredths of a second repeated.
    /// A deep fade with the reception worsening, and back.
    Dip,
}

/// The state of the effects of one stream.
pub struct FxState {
    rate: f32,
    rng: u32,
    // coefficients, set by `prepare`
    fx: RadioFx,
    a_hp: f32,
    a_lp1: f32,
    a_pres_lo: f32,
    a_pres_hi: f32,
    a_fm_lp: f32,
    a_click: f32,
    // the reception wandering
    quality: f32,
    target: f32,
    retarget_in: f32,
    // filters, per channel
    fm_lp: [f32; 2],
    hp: [f32; 2],
    lp1: [f32; 2],
    lp2: [f32; 2],
    pres_lo: [f32; 2],
    pres_hi: [f32; 2],
    prev_white: [f32; 2],
    click: f32,
    // dropouts
    event: Event,
    event_left: f32,
    next_in: f32,
    gain: f32,
    gain_target: f32,
    dip: f32,
}

impl Default for FxState {
    fn default() -> Self {
        FxState {
            rate: 0.0,
            rng: 0x9E37_79B9,
            fx: RadioFx::OFF,
            a_hp: 0.0,
            a_lp1: 1.0,
            a_pres_lo: 0.0,
            a_pres_hi: 0.0,
            a_fm_lp: 0.0,
            a_click: 0.0,
            quality: 1.0,
            target: 1.0,
            retarget_in: 0.5,
            fm_lp: [0.0; 2],
            hp: [0.0; 2],
            lp1: [0.0; 2],
            lp2: [0.0; 2],
            pres_lo: [0.0; 2],
            pres_hi: [0.0; 2],
            prev_white: [0.0; 2],
            click: 0.0,
            event: Event::None,
            event_left: 0.0,
            next_in: 8.0,
            gain: 1.0,
            gain_target: 1.0,
            dip: 0.0,
        }
    }
}

/// The coefficient of a one-pole low-pass at `hz`.
fn pole(hz: f32, rate: f32) -> f32 {
    1.0 - (-2.0 * PI * hz.min(rate * 0.45) / rate).exp()
}

impl FxState {
    fn rnd(&mut self) -> f32 {
        // xorshift32
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    fn white(&mut self) -> f32 {
        self.rnd() * 2.0 - 1.0
    }

    /// Set the amounts and the device rate for the next block of frames.
    pub fn prepare(&mut self, fx: RadioFx, rate: f32) {
        let fx = fx.clamped();
        if rate != self.rate {
            self.rate = rate;
        }
        if fx == self.fx && self.a_hp > 0.0 {
            return;
        }
        self.fx = fx;
        let s = fx.speaker;
        // highs cut from 16 kHz to 4 kHz, lows from 40 Hz to 300 Hz
        let lp_hz = 16_000.0 * (4_000.0f32 / 16_000.0).powf(s);
        let hp_hz = 40.0 * (300.0f32 / 40.0).powf(s);
        self.a_lp1 = pole(lp_hz, rate);
        self.a_hp = pole(hp_hz, rate);
        // the presence band
        self.a_pres_lo = pole(800.0, rate);
        self.a_pres_hi = pole(2_800.0, rate);
        self.a_fm_lp = pole(3_500.0, rate);
        self.a_click = (-1.0 / (0.0012 * rate)).exp();
    }

    /// A moment of static: what is heard while the stream has nothing yet (a station being
    /// tuned in, a connection lost). The speaker still colours it.
    pub fn stall(&mut self) -> (f32, f32) {
        let level = 0.02 + 0.04 * self.fx.noise;
        let (a, b) = (self.white() * level, self.white() * level);
        self.speaker(a, b)
    }

    /// One frame through the radio.
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let dt = 1.0 / self.rate.max(1.0);
        let fx = self.fx;
        let (mut l, mut r) = (l, r);

        // --- the reception: a quality that wanders, worse the more noise is asked for
        if fx.noise > 0.001 {
            self.retarget_in -= dt;
            if self.retarget_in <= 0.0 {
                self.retarget_in = 0.4 + 1.6 * self.rnd();
                let u = self.rnd();
                self.target = 1.0 - fx.noise * 0.9 * u * u;
            }
            self.quality += (self.target - self.quality) * (dt / 1.2);
        } else {
            self.quality = 1.0;
        }
        let weak = ((1.0 - self.quality) + self.dip * 0.5).clamp(0.0, 1.0);

        if fx.noise > 0.001 || self.dip > 0.0 {
            // stereo falls to mono and the highs go as the signal weakens
            let blend = (weak * 1.6).min(1.0);
            let m = (l + r) * 0.5;
            l += (m - l) * blend;
            r += (m - r) * blend;
            let cut = (weak * 1.5).min(1.0);
            self.fm_lp[0] += (l - self.fm_lp[0]) * self.a_fm_lp;
            self.fm_lp[1] += (r - self.fm_lp[1]) * self.a_fm_lp;
            l += (self.fm_lp[0] - l) * cut;
            r += (self.fm_lp[1] - r) * cut;
            // hiss, rising with the frequency as FM noise does
            let hiss = 0.003 * fx.noise + 0.06 * weak;
            for c in 0..2 {
                let w = self.white();
                let n = (w - self.prev_white[c]) * 0.5;
                self.prev_white[c] = w;
                if c == 0 {
                    l += n * hiss;
                } else {
                    r += n * hiss;
                }
            }
            // crackle
            let rate_hz = 0.2 * fx.noise + 3.0 * weak;
            if self.rnd() < rate_hz * dt {
                let sign = if self.rnd() < 0.5 { -1.0 } else { 1.0 };
                self.click = sign * (0.12 + 0.4 * self.rnd());
            }
            if self.click.abs() > 1e-4 {
                l += self.click;
                r += self.click * 0.8;
                self.click *= self.a_click;
            }
        }

        // --- dropouts
        if fx.dropouts > 0.001 {
            if self.event == Event::None {
                self.next_in -= dt;
                if self.next_in <= 0.0 {
                    let kind = self.rnd();
                    if kind < 0.55 {
                        self.event = Event::Mute;
                        self.event_left = 0.06 + 0.3 * self.rnd();
                        self.gain_target = 0.0;
                    } else {
                        self.event = Event::Dip;
                        self.event_left = 0.5 + 0.9 * self.rnd();
                        self.gain_target = 0.15;
                    }
                }
            } else {
                self.event_left -= dt;
                if self.event_left <= 0.0 {
                    self.event = Event::None;
                    self.gain_target = 1.0;
                    // from a few seconds to minutes, the more the bigger the amount
                    self.next_in = (150.0 + (14.0 - 150.0) * fx.dropouts) * (0.4 + 1.2 * self.rnd());
                }
            }
            // the gain follows quickly down, a little slower up
            let k = if self.gain_target < self.gain { dt / 0.003 } else { dt / 0.02 };
            self.gain += (self.gain_target - self.gain) * k.min(1.0);
            let dip_target = if self.event == Event::Dip { 1.0 } else { 0.0 };
            self.dip += (dip_target - self.dip) * (dt / 0.3).min(1.0);
            l *= self.gain;
            r *= self.gain;
            // static where the sound was
            let hole = 1.0 - self.gain;
            if hole > 0.01 {
                let a = self.white() * 0.05 * hole;
                let b = self.white() * 0.05 * hole;
                l += a;
                r += b;
            }
        } else {
            self.gain = 1.0;
            self.gain_target = 1.0;
            self.event = Event::None;
            self.dip = 0.0;
        }

        self.speaker(l, r)
    }

    /// The small speaker.
    fn speaker(&mut self, l: f32, r: f32) -> (f32, f32) {
        let s = self.fx.speaker;
        if s <= 0.001 {
            return (l, r);
        }
        let m = (l + r) * 0.5;
        let mix = 0.7 * s;
        let mut x = [l + (m - l) * mix, r + (m - r) * mix];
        for c in 0..2 {
            // lows out
            self.hp[c] += (x[c] - self.hp[c]) * self.a_hp;
            let mut y = x[c] - self.hp[c];
            // highs out, twice
            self.lp1[c] += (y - self.lp1[c]) * self.a_lp1;
            self.lp2[c] += (self.lp1[c] - self.lp2[c]) * self.a_lp1;
            y = self.lp2[c];
            // the middle pushed
            self.pres_lo[c] += (y - self.pres_lo[c]) * self.a_pres_lo;
            self.pres_hi[c] += (y - self.pres_hi[c]) * self.a_pres_hi;
            y += (self.pres_hi[c] - self.pres_lo[c]) * 0.9 * s;
            // a little overdrive
            let drive = 1.0 + 1.5 * s;
            x[c] = (y * drive).tanh() / drive * (1.0 + 0.35 * s);
        }
        (x[0], x[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(fx: RadioFx, seconds: f32, input: impl Fn(usize) -> f32) -> Vec<(f32, f32)> {
        let rate = 48_000.0;
        let mut s = FxState::default();
        s.prepare(fx, rate);
        (0..(rate * seconds) as usize)
            .map(|i| {
                let v = input(i);
                s.process(v, v)
            })
            .collect()
    }

    #[test]
    fn off_changes_nothing_and_is_recognised() {
        assert!(RadioFx::OFF.is_off());
        assert!(!RadioFx::light().is_off());
        let out = run(RadioFx::OFF, 0.1, |i| (i as f32 * 0.01).sin() * 0.5);
        for (i, (l, r)) in out.iter().enumerate() {
            let v = (i as f32 * 0.01).sin() * 0.5;
            assert!((l - v).abs() < 1e-6 && (r - v).abs() < 1e-6);
        }
    }

    #[test]
    fn a_small_speaker_keeps_the_middle_and_loses_the_lows_and_highs() {
        let rate = 48_000.0f32;
        let power = |hz: f32| {
            let out = run(RadioFx { speaker: 1.0, noise: 0.0, dropouts: 0.0 }, 0.5, |i| (2.0 * PI * hz * i as f32 / rate).sin() * 0.3);
            let tail = &out[out.len() / 2..];
            (tail.iter().map(|(l, _)| l * l).sum::<f32>() / tail.len() as f32).sqrt()
        };
        let (low, mid, high) = (power(60.0), power(1500.0), power(12_000.0));
        assert!(mid > low * 3.0, "mid {mid} low {low}");
        assert!(mid > high * 3.0, "mid {mid} high {high}");
    }

    #[test]
    fn noise_adds_hiss_to_silence_and_stays_finite() {
        let out = run(RadioFx { speaker: 0.0, noise: 1.0, dropouts: 0.0 }, 2.0, |_| 0.0);
        let rms = (out.iter().map(|(l, _)| l * l).sum::<f32>() / out.len() as f32).sqrt();
        assert!(rms > 1e-4 && rms < 0.2, "{rms}");
        assert!(out.iter().all(|(l, r)| l.is_finite() && r.is_finite() && l.abs() < 2.0 && r.abs() < 2.0));
    }

    #[test]
    fn dropouts_come_and_pass() {
        // a steady tone: with the amount at 1 it must break up (a quiet stretch) within a minute
        // and be back to full afterwards
        let rate = 48_000.0f32;
        let mut s = FxState::default();
        let fx = RadioFx { speaker: 0.0, noise: 0.0, dropouts: 1.0 };
        s.prepare(fx, rate);
        let (mut quiet, mut loud_after) = (false, false);
        for i in 0..(rate * 60.0) as usize {
            let v = (2.0 * PI * 440.0 * i as f32 / rate).sin() * 0.5;
            let (l, _) = s.process(v, v);
            if l.abs() < 0.02 && v.abs() > 0.4 {
                quiet = true;
            }
            if quiet && l.abs() > 0.45 {
                loud_after = true;
            }
        }
        assert!(quiet && loud_after);
    }

    #[test]
    fn the_stall_is_static_not_silence() {
        let mut s = FxState::default();
        s.prepare(RadioFx::light(), 48_000.0);
        let e: f32 = (0..4800).map(|_| s.stall().0.powi(2)).sum();
        assert!(e > 0.0);
    }
}
