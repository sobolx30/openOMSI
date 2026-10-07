//! Precipitation: rain streaks / snow flakes falling through the world around the camera,
//! drawn through the corona sprite pipeline (cone value -2 marks a streak, -3 a snow flake),
//! and the picture the film on the glass wears in a snow weather.
//!
//! The particles live in the world, not on the camera: driving on, the flakes stay where they
//! are and go by (the box round the camera only decides which of them are drawn, and a
//! particle leaving it comes in again on the other side).

use glam::{DVec3, Vec3};
use omsi_render::{Corona, LightMode, Scene};

/// Half the side of the box of precipitation round the camera (m).
const HALF: f64 = 50.0;
/// The box's height range relative to the camera (m).
const Z_LO: f64 = -8.0;
const Z_SPAN: f64 = 32.0;

/// A light that may fall on the flakes: what `Rain::tick` keeps of a `PointLight`.
struct Lamp {
    pos: DVec3,
    radius: f32,
    core: f32,
    color: Vec3,
    intensity: f32,
    /// A spot's axis (zero: a point light) and the cosines of its inner and outer cone.
    dir: Vec3,
    cone: [f32; 2],
    /// A vehicle's lamp or other `LightMode::Enhanced`: its strength is in candelas, not the
    /// map lamp's classic reach.
    physical: bool,
}

impl Lamp {
    /// The light this lamp throws on a flake at `p` (linear rgb, about 1 = a flake lit white).
    fn on(&self, p: DVec3) -> Vec3 {
        let d = (self.pos - p).as_vec3();
        // (the square first: most flakes are out of most lamps' reach, and none costs a root)
        let d2 = d.length_squared();
        if d2 >= self.radius * self.radius {
            return Vec3::ZERO;
        }
        let dist = d2.sqrt();
        let window = (1.0 - dist / self.radius).clamp(0.0, 1.0);
        let att = if self.physical {
            // inverse square from the lamp's core
            let c = self.core.max(0.1);
            self.intensity * (c * c) / (dist * dist).max(c * c) * window * 0.5
        } else {
            // the classic picture's map lamp: full within an eighth of the reach, then
            // inverse square, cut off at the reach (as the shader's `point_lights`)
            let r0 = self.radius * 0.125;
            ((r0 * r0) / (dist * dist).max(0.01)).min(1.0) * window * 3.75 * self.intensity
        };
        let mut k = att;
        if self.dir.length_squared() > 0.25 {
            let c = (-d / dist.max(0.01)).dot(self.dir.normalize());
            let t = ((c - self.cone[1]) / (self.cone[0] - self.cone[1]).max(1e-3)).clamp(0.0, 1.0);
            k *= t * t * (3.0 - 2.0 * t);
        }
        self.color * k.min(2.0)
    }
}

struct Particle {
    /// World position.
    pos: DVec3,
    /// 0..1: its size and how fast it falls (a big flake falls faster)
    k: f32,
    /// Its own phase of the sway.
    phase: f32,
    /// What the lamps throw on it (kept between frames: a quarter of the flakes are lit anew
    /// each frame).
    lit: Vec3,
}

fn smooth(a: f64, b: f64, x: f64) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    (t * t * (3.0 - 2.0 * t)) as f32
}

/// Wrap `v` into `lo..lo + span`: a step of one span is a compare and a subtraction (the
/// flakes only ever leave the box by a little), anything farther the remainder.
fn wrap(v: f64, lo: f64, span: f64) -> f64 {
    if v < lo {
        if v >= lo - span { v + span } else { lo + (v - lo).rem_euclid(span) }
    } else if v >= lo + span {
        if v < lo + 2.0 * span { v - span } else { lo + (v - lo).rem_euclid(span) }
    } else {
        v
    }
}

pub struct Rain {
    particles: Vec<Particle>,
    /// How many of them have been placed in the world (the rest hold offsets from the camera
    /// to be turned into world positions at the next tick).
    seeded: usize,
    kind: i32,
    rate: f32,
    rng: u64,
    time: f32,
    frame: u32,
    /// The light the flakes are lit by (linear rgb, 1 = a bright overcast day).
    light: Vec3,
}

/// The boxes a vehicle keeps the weather out of, as `Rain::tick` takes them: its own
/// `[boundingbox]` and each coupled part's (an articulated bus's rear section is a part of
/// its own, with its own box: without it, it rained in the rear saloon, #777).
pub fn vehicle_boxes(v: &omsi_sim::VehicleInstance) -> Vec<(DVec3, f64, [f32; 6])> {
    let front = v.ty.def.bounding_box.map(|bb| (v.position, v.heading, bb));
    let parts = v.trailers.iter().filter_map(|t| {
        // a part coupled the other way round stands turned about its own origin
        let heading = if t.reversed { t.heading + 180.0 } else { t.heading };
        t.ty.def.bounding_box.map(|bb| (t.position, heading, bb))
    });
    front.into_iter().chain(parts).collect()
}

/// The light a flake is lit by, from the day's ambient, sky and sun light: a night is dark
/// (a faint trace is left so that the snow never vanishes outright), a bright day white.
pub fn flake_light(d: &omsi_sim::Daylight) -> Vec3 {
    let sun = d.sun_color * d.sun_dir.z.max(0.0) * 0.35;
    let l = (d.ambient + d.sky * 0.5 + sun) * 0.9;
    l.clamp(Vec3::splat(0.05), Vec3::splat(1.1))
}

impl Rain {
    pub fn new() -> Rain {
        Rain {
            particles: Vec::new(),
            seeded: 0,
            kind: 0,
            rate: 0.0,
            rng: 0xABCDEF12345,
            time: 0.0,
            frame: 0,
            light: Vec3::new(0.8, 0.8, 0.85),
        }
    }

    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    /// The light the flakes are drawn in (`flake_light`).
    pub fn set_light(&mut self, light: Vec3) {
        self.light = light;
    }

    /// `kind`: 0 none, 1 rain, 2 snow; `rate` 0..1.
    pub fn set(&mut self, kind: i32, rate: f32) {
        self.kind = kind;
        self.rate = rate.clamp(0.0, 1.0);
        let n = if kind == 0 {
            0
        } else {
            (1500.0 + 12000.0 * self.rate) as usize
        };
        while self.particles.len() < n {
            let pos = DVec3::new(
                (self.rand() as f64 * 2.0 - 1.0) * HALF,
                (self.rand() as f64 * 2.0 - 1.0) * HALF,
                Z_LO + self.rand() as f64 * Z_SPAN,
            );
            let (k, phase) = (self.rand(), self.rand() * std::f32::consts::TAU);
            self.particles.push(Particle { pos, k, phase, lit: Vec3::ZERO });
        }
        self.particles.truncate(n);
        self.seeded = self.seeded.min(n);
    }

    /// Move the particles (in the world) and push the ones in the box round the camera as sprites.
    /// `inside`: the buses near the camera as (origin, heading in degrees, `[boundingbox]`),
    /// the player's first; no drop or flake is drawn within any, so that it does not rain
    /// in a cab (only the player's own had been left dry: riding in another player's bus or
    /// a timetable bus it snowed in the saloon).
    pub fn tick(
        &mut self,
        dt: f32,
        camera: DVec3,
        wind: Vec3,
        scene: &mut Scene,
        inside: &[(DVec3, f64, [f32; 6])],
    ) {
        if self.particles.is_empty() {
            return;
        }
        // the newly made ones were offsets from the camera: set them down in the world
        for p in self.particles[self.seeded..].iter_mut() {
            p.pos += camera;
        }
        self.seeded = self.particles.len();
        self.time += dt;
        let buses: Vec<(DVec3, f64, [f32; 6])> = inside.iter().filter(|b| (b.0 - camera).length() < 40.0).map(|&(o, h, bb)| (o, h.to_radians(), bb)).collect();
        let in_one = |p: DVec3, (o, h, bb): (DVec3, f64, [f32; 6])| -> bool {
            let d = p - o;
            let (sh, ch) = (h.sin(), h.cos());
            let x = d.x * ch - d.y * sh;
            let y = d.x * sh + d.y * ch;
            let z = d.z;
            // a flat 0.3 m margin left flakes rendering on the inside of the glass near a
            // window sill (the ledge below a side window sits close to the body's outer
            // envelope, closer than the margin covered): widened so the whole cabin,
            // ledges included, sits inside the excluded box.
            (x - bb[3] as f64).abs() < bb[0] as f64 * 0.5 + 0.6
                && (y - bb[4] as f64).abs() < bb[1] as f64 * 0.5 + 0.6
                && (z - bb[5] as f64).abs() < bb[2] as f64 * 0.5 + 0.6
        };
        let in_bus = |p: DVec3| buses.iter().any(|b| in_one(p, *b));
        let snow = self.kind == 2;
        let t = self.time;
        // the lamps in reach of the box (the map's street lights, the vehicles' lights): they
        // light the flakes that pass them
        let mut lamps: Vec<Lamp> = Vec::new();
        if snow {
            for l in scene.lights.iter() {
                // (a lamp is registered twice, for the classic and the enhanced picture:
                // the classic duplicate of a vehicle's lamp is left out)
                if l.mode == LightMode::Vanilla || l.intensity <= 0.01 {
                    continue;
                }
                let reach = l.radius.min(80.0);
                if (l.position - camera).length() > reach as f64 + 2.0 * HALF {
                    continue;
                }
                lamps.push(Lamp {
                    pos: l.position,
                    radius: reach,
                    core: l.core,
                    color: Vec3::from_array(l.color),
                    intensity: l.intensity,
                    dir: l.direction,
                    cone: l.cone,
                    physical: l.mode == LightMode::Enhanced,
                });
            }
            // (the nearest two dozen: a town's worth of lamps would be too many to try on every flake)
            if lamps.len() > 24 {
                lamps.sort_by(|a, b| (a.pos - camera).length_squared().total_cmp(&(b.pos - camera).length_squared()));
                lamps.truncate(24);
            }
        }
        let light = self.light;
        let frame = self.frame;
        self.frame = self.frame.wrapping_add(1);
        let mut excluded = 0usize;
        for (i, p) in self.particles.iter_mut().enumerate() {
            if snow {
                // a flake drifts: it falls at its own speed and sways to and fro, the
                // wind taking it on
                let fall = 0.9 + 0.9 * p.k as f64;
                let sway_x = ((t * 1.1 + p.phase).sin() * 0.45) as f64;
                let sway_y = ((t * 0.8 + p.phase * 1.7).cos() * 0.35) as f64;
                p.pos.z -= fall * dt as f64;
                p.pos.x += (wind.x as f64 + sway_x) * dt as f64;
                p.pos.y += (wind.y as f64 + sway_y) * dt as f64;
            } else {
                p.pos.z -= 9.0 * dt as f64;
                p.pos.x += wind.x as f64 * dt as f64;
                p.pos.y += wind.y as f64 * dt as f64;
            }
            // leaving the box round the camera, it comes in at the other side
            p.pos.x = camera.x + wrap(p.pos.x - camera.x, -HALF, 2.0 * HALF);
            p.pos.y = camera.y + wrap(p.pos.y - camera.y, -HALF, 2.0 * HALF);
            p.pos.z = camera.z + wrap(p.pos.z - camera.z, Z_LO, Z_SPAN);
            let w = p.pos;
            // at the box's rim a particle fades out, so that none pops in or out of sight
            let d = w - camera;
            let edge = (HALF - d.x.abs()).min(HALF - d.y.abs());
            let top = (Z_LO + Z_SPAN) - d.z;
            let mut fade = (((edge.min(top)) / 14.0).clamp(0.0, 1.0)) as f32;
            if fade <= 0.0 {
                continue;
            }
            let dist = d.length();
            // a far flake is under a pixel: of every four, one is drawn out to the rim (a
            // little bigger and brighter for the three that are not), another to 45 m, two
            // only to 28 m, each fading out over the last stretch, so that a deep snowfall
            // costs what a thin one near the lens does
            let mut grow = 1.0f32;
            if snow {
                match i & 3 {
                    0 => grow = 1.0 + 0.7 * smooth(18.0, 50.0, dist),
                    2 => fade *= 1.0 - smooth(35.0, 45.0, dist),
                    _ => fade *= 1.0 - smooth(18.0, 28.0, dist),
                }
                if fade <= 0.01 {
                    continue;
                }
            }
            if in_bus(w) {
                excluded += 1;
                continue;
            }
            if snow {
                // the lamps' light on a flake changes slowly: a quarter of the flakes are
                // lit anew each frame
                if !lamps.is_empty() && (i as u32).wrapping_add(frame) % 4 == 0 {
                    let mut sum = Vec3::ZERO;
                    for l in &lamps {
                        sum += l.on(w);
                    }
                    p.lit = sum;
                }
                let spin = p.phase + t * (p.k - 0.5) * 1.5;
                // close to the lens a flake dissolves rather than flaring up large
                let near = ((dist as f32 - 0.4) / 1.2).clamp(0.0, 1.0);
                scene.coronas.push(Corona {
                    position: w,
                    size: (0.02 + 0.03 * p.k) * grow,
                    // its own turn (and a slow tumble), and which of the four shapes it is
                    up: Vec3::new(spin.cos(), spin.sin(), 0.0),
                    flags: ((p.phase * 997.0) as u32 % 4 * 16) as u8,
                    color: (light + p.lit).min(Vec3::splat(1.6)).to_array(),
                    brightness: (0.55 + 0.45 * p.k) * fade * near * grow,
                    direction: Vec3::ZERO,
                    cone_cos: -3.0,
                    ..Default::default()
                });
            } else {
                scene.coronas.push(Corona {
                    position: w,
                    size: 0.05,
                    color: [0.75, 0.8, 0.9],
                    brightness: 0.35 * fade,
                    direction: Vec3::ZERO,
                    cone_cos: -2.0,
                    ..Default::default()
                });
            }
        }
        if omsi_cfg::env::var_os("OMSI_DEBUG_RAIN").is_some() {
            log::info!(
                "rain: {} particles, {excluded} inside the buses (boxes {:?})",
                self.particles.len(),
                buses.iter().map(|b| (b.0, b.1.to_degrees(), b.2)).collect::<Vec<_>>()
            );
        }
    }
}

/// The picture the windscreen and window layers wear while it snows.
///
/// Every bus carries the same rain film: a mesh in front of the glass textured with
/// `regen.tga` (running drops) and faded in by `[alphascale] Rain_Window_*_Wetness`, which
/// `rain.osc` fills from `PrecipRate` - and `rain.osc` never asks what is falling, so in a
/// snowstorm the original shows raindrops on every pane, in the cab and along the saloon.
/// OMSI's own way out is the seasonal texture folder (`texture\WinterSnow\`), which no stock
/// vehicle fills in, so openOMSI builds the winter picture itself: crystals settled on
/// the glass, thickest along the rim where a pane collects them, made from the game's own
/// `Texture\snowflake.tga` where it is there and from soft specks of its own where it is not.
pub fn snow_on_glass(root: &std::path::Path) -> omsi_texture::Image {
    // the same grain as the rain film it stands in for (`regen.tga`, 1024 x 1024 with
    // drops a dozen pixels across and a mean alpha of 7 %)
    const SIDE: usize = 512;
    // white where it is clear as well, so that the smaller mip levels stay white specks
    // and do not grey towards the black of transparent texels
    let mut rgba = [255u8, 255, 255, 0].repeat(SIDE * SIDE);
    let flake =
        omsi_texture::decode_file(&omsi_cfg::resolve_path(root, "Texture\\snowflake.tga")).ok();
    let mut rng = 0x9E37_79B9_7F4A_7C15u64;
    let mut rand = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng >> 40) as f32 / (1u64 << 24) as f32
    };
    // a pane holds more snow at its rim than in the middle, where the airstream and the
    // wipers take it away
    let edge = |x: f32, y: f32| {
        let d = (x - 0.5).abs().max((y - 0.5).abs()) * 2.0;
        0.35 + 0.65 * d * d
    };
    for _ in 0..1800 {
        let (cx, cy) = (rand() * SIDE as f32, rand() * SIDE as f32);
        if rand() > edge(cx / SIDE as f32, cy / SIDE as f32) {
            continue;
        }
        let r = 1.5 + rand() * 3.5;
        let strength = 0.35 + 0.65 * rand();
        let lo = |v: f32| (v - r).max(0.0) as usize;
        let hi = |v: f32| ((v + r) as usize).min(SIDE - 1);
        for y in lo(cy)..=hi(cy) {
            for x in lo(cx)..=hi(cx) {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                let d = (dx * dx + dy * dy).sqrt() / r;
                if d > 1.0 {
                    continue;
                }
                // the flake's own picture where the game has one, else a soft round speck
                let a = match &flake {
                    Some(f) => {
                        let u = ((dx / r * 0.5 + 0.5) * (f.width - 1) as f32) as usize;
                        let v = ((dy / r * 0.5 + 0.5) * (f.height - 1) as f32) as usize;
                        let p = (v * f.width as usize + u) * 4;
                        // the stock flake is white on black; its alpha channel, or its
                        // brightness where it has none
                        (f.rgba[p + 3] as f32 / 255.0).max(f.rgba[p] as f32 / 255.0)
                    }
                    None => (1.0 - d * d).powf(1.5),
                };
                let a = (a * strength * 255.0) as u32;
                let p = (y * SIDE + x) * 4;
                rgba[p] = 255;
                rgba[p + 1] = 255;
                rgba[p + 2] = 255;
                rgba[p + 3] = rgba[p + 3].max(a.min(255) as u8);
            }
        }
    }
    omsi_texture::Image {
        width: SIDE as u32,
        height: SIDE as u32,
        rgba,
        has_alpha: true,
    }
}

/// The snow-on-glass picture (`snow_on_glass`) on the GPU, with its mip chain: one level
/// alone, its specks of a pixel or two were point-sampled across a whole windscreen and
/// glittered with every move of the head (#946).
pub fn add_snow_on_glass(
    renderer: &omsi_render::Renderer,
    scene: &mut omsi_render::Scene,
    root: &std::path::Path,
) -> omsi_render::TextureId {
    renderer.add_texture(scene, &snow_on_glass(root), true)
}

#[cfg(test)]
mod tests {
    /// The snow on the glass goes up with its mip levels, and it is white throughout (a
    /// level made smaller stays white, only less opaque).
    #[test]
    fn snow_on_glass_has_mip_levels_and_stays_white() {
        let img = super::snow_on_glass(std::path::Path::new("/nonexistent"));
        assert!(img.rgba.chunks_exact(4).all(|p| p[..3] == [255, 255, 255]));
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::NOOP;
        descriptor.backend_options.noop = wgpu::NoopBackendOptions { enable: true };
        let instance = wgpu::Instance::new(descriptor);
        let renderer = pollster::block_on(omsi_render::Renderer::new_with(
            &instance,
            None,
            Some(wgpu::TextureFormat::Rgba8UnormSrgb),
            omsi_render::RenderOptions { msaa: 1, shadow_size: 1024, ..Default::default() },
        ))
        .expect("noop renderer");
        let mut scene = renderer.new_scene();
        let id = super::add_snow_on_glass(&renderer, &mut scene, std::path::Path::new("/nonexistent"));
        assert_eq!(renderer.texture_levels(&scene, id), Some((512, 512, 10)));
    }

    #[test]
    fn snow_on_glass_is_mostly_clear_and_thickest_at_the_rim() {
        // no content root here: the procedural specks
        let img = super::snow_on_glass(std::path::Path::new("/nonexistent"));
        assert_eq!((img.width, img.height), (512, 512));
        let alpha_of = |x: usize, y: usize| img.rgba[(y * 512 + x) * 4 + 3] as u32;
        let (mut middle, mut rim, mut n_middle, mut n_rim) = (0u32, 0u32, 0u32, 0u32);
        for y in 0..512 {
            for x in 0..512 {
                let d = ((x as f32 - 255.5).abs()).max((y as f32 - 255.5).abs()) / 255.5;
                if d < 0.35 {
                    middle += alpha_of(x, y);
                    n_middle += 1;
                } else if d > 0.8 {
                    rim += alpha_of(x, y);
                    n_rim += 1;
                }
            }
        }
        let mean = img.rgba.chunks_exact(4).map(|p| p[3] as u32).sum::<u32>() / (512 * 512);
        assert!(mean < 60, "the glass would be opaque (mean alpha {mean})");
        let (a, b) = (rim as f32 / n_rim as f32, middle as f32 / n_middle as f32);
        assert!(
            a > b * 1.3,
            "the rim should carry more snow than the middle ({a:.1} vs {b:.1})"
        );
    }
}
