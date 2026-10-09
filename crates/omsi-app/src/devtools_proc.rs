//! The game's side of the developer tools (Settings → General → *Enable Developer Tools*):
//! starts the tools window as a process of its own - this program again, told by the
//! `OMSI_DEVTOOLS_WINDOW` variable to be the tools window (see `launcher::devtools`) - keeps
//! it for the session, talks to it, and draws what it asks for over the picture.
//!
//! The talk is lines of text with tab-separated fields over the new process's standard input
//! and output (never blocking the game: each pipe has a thread of its own):
//!
//! * window → game: `hello` (the window is up: send everything), `view <key> <0|1>` (an
//!   overlay on or off), `watch|unwatch <v|s> <name>` (a variable of the player's vehicle to
//!   show: `v` numeric, `s` string), `set <v|s> <name> <value>` (write it once),
//!   `hold <v|s> <name> <value>` (write it every frame) and `release <v|s> <name>`;
//! * game → window: `view <key> <0|1>`, `vehicle <name>`, `names_begin` / `name <v|s> <name>` /
//!   `names_end` (all the variables of the player's vehicle), `watched <v|s> <name>`,
//!   `held <v|s> <name> <value>` and `val <v|s> <name> <value>` (the watched ones, five times
//!   a second).
//!
//! The game holds the window's end of the pipes: when the game ends, even killed, the window
//! reads the end of its input and closes, so no tools window outlives its session.

use crate::ui::Mark;
use glam::{DVec3, Mat4, Vec3};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};

/// The overlays the tools window switches (the keys are the protocol's).
pub(crate) const VIEW_KEYS: [&str; 12] = [
    "seats", "standing", "paths", "pathlabels", "devices", "boxes", "axles", "cog", "lights", "mirrors", "viewsdriver", "viewspax",
];

#[derive(Default, Clone, Copy)]
pub(crate) struct DevView {
    pub seats: bool,
    pub standing: bool,
    pub paths: bool,
    pub boxes: bool,
    pub axles: bool,
    pub lights: bool,
    pub pathlabels: bool,
    pub devices: bool,
    pub cog: bool,
    pub mirrors: bool,
    pub viewsdriver: bool,
    pub viewspax: bool,
}

impl DevView {
    pub fn any(&self) -> bool {
        self.seats || self.standing || self.paths || self.boxes || self.axles || self.lights || self.pathlabels || self.devices || self.cog || self.mirrors || self.viewsdriver || self.viewspax
    }

    fn set(&mut self, key: &str, on: bool) {
        match key {
            "seats" => self.seats = on,
            "standing" => self.standing = on,
            "paths" => self.paths = on,
            "boxes" => self.boxes = on,
            "axles" => self.axles = on,
            "lights" => self.lights = on,
            "pathlabels" => self.pathlabels = on,
            "devices" => self.devices = on,
            "cog" => self.cog = on,
            "mirrors" => self.mirrors = on,
            "viewsdriver" => self.viewsdriver = on,
            "viewspax" => self.viewspax = on,
            _ => {}
        }
    }

    fn get(&self, key: &str) -> bool {
        match key {
            "seats" => self.seats,
            "standing" => self.standing,
            "paths" => self.paths,
            "boxes" => self.boxes,
            "axles" => self.axles,
            "lights" => self.lights,
            "pathlabels" => self.pathlabels,
            "devices" => self.devices,
            "cog" => self.cog,
            "mirrors" => self.mirrors,
            "viewsdriver" => self.viewsdriver,
            "viewspax" => self.viewspax,
            _ => false,
        }
    }
}

/// A vehicle file the window can have read again while playing (see `app_events`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileKind {
    Sound,
    Bus,
    Model,
    Constants,
}

#[derive(Default)]
pub(crate) struct DevToolsProc {
    child: Option<Child>,
    /// Lines for the window (a thread writes them to its input) and from it.
    tx: Option<Sender<String>>,
    rx: Option<Receiver<String>>,
    pub view: DevView,
    /// The variables shown in the window: (a string?, name).
    watch: Vec<(bool, String)>,
    /// Variables written every frame: (a string?, name, value).
    holds: Vec<(bool, String, String)>,
    /// The window said `hello`: it gets everything again.
    resync: bool,
    /// The vehicle type whose variable names the window has (its address; 0: none).
    names_of: usize,
    snapshot_t: f32,
    /// What the window asked to be read again, not yet done (see `take_reloads`).
    reloads: Vec<crate::humans::CabinReload>,
    file_reloads: Vec<FileKind>,
}

impl DevToolsProc {
    /// Open the tools window, unless one is running. A window the user has closed is replaced.
    pub fn start(&mut self) {
        if let Some(c) = self.child.as_mut() {
            match c.try_wait() {
                Ok(None) => return,
                _ => self.stop(),
            }
        }
        let exe = match std::env::current_exe() {
            Ok(e) => e,
            Err(e) => {
                log::warn!("developer tools: cannot find the program to start: {e}");
                return;
            }
        };
        let spawned = Command::new(exe)
            .env("OMSI_DEVTOOLS_WINDOW", "1")
            .env("OMSI_DEVTOOLS_GAME_PID", std::process::id().to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        match spawned {
            Ok(mut c) => {
                if let Some(mut stdin) = c.stdin.take() {
                    let (tx, lines) = channel::<String>();
                    std::thread::spawn(move || {
                        while let Ok(first) = lines.recv() {
                            let mut ok = writeln!(stdin, "{first}").is_ok();
                            while ok {
                                match lines.try_recv() {
                                    Ok(l) => ok = writeln!(stdin, "{l}").is_ok(),
                                    Err(_) => break,
                                }
                            }
                            if !ok || stdin.flush().is_err() {
                                break;
                            }
                        }
                        // (the pipe closes with `stdin`: the window sees the end)
                    });
                    self.tx = Some(tx);
                }
                if let Some(stdout) = c.stdout.take() {
                    let (tx, rx) = channel::<String>();
                    std::thread::spawn(move || {
                        for line in BufReader::new(stdout).lines() {
                            match line {
                                Ok(l) => {
                                    if tx.send(l).is_err() {
                                        break;
                                    }
                                }
                                Err(_) => break,
                            }
                        }
                    });
                    self.rx = Some(rx);
                }
                self.child = Some(c);
                self.resync = true;
                self.names_of = 0;
                log::info!("developer tools: window started");
            }
            Err(e) => log::warn!("developer tools: cannot start the window: {e}"),
        }
    }

    /// Close the tools window (if there is one).
    pub fn stop(&mut self) {
        self.tx = None;
        self.rx = None;
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// Close the window and open a fresh one (which gets the state again).
    pub fn restart(&mut self) {
        self.stop();
        self.start();
    }

    /// The reloads the window asked for since the last call.
    pub(crate) fn take_reloads(&mut self) -> Vec<crate::humans::CabinReload> {
        std::mem::take(&mut self.reloads)
    }

    /// The vehicle files the window asked to be read again since the last call.
    pub(crate) fn take_file_reloads(&mut self) -> Vec<FileKind> {
        std::mem::take(&mut self.file_reloads)
    }

    /// Tell the window how a reload went.
    pub(crate) fn note(&self, text: &str) {
        self.send(format!("note\t{}", clean(text)));
    }

    fn send(&self, line: String) {
        if let Some(tx) = self.tx.as_ref() {
            let _ = tx.send(line);
        }
    }
}

impl Drop for DevToolsProc {
    fn drop(&mut self) {
        self.stop();
    }
}

/// A text for one field of a line: no tab or line break in it.
fn clean(s: &str) -> String {
    s.chars().map(|c| if c == '\t' || c == '\n' || c == '\r' { ' ' } else { c }).collect()
}

/// A mirror camera as one line for the window: x;y;z;dist;fov;yaw;pitch;extra (`-`: none).
fn mirror_to_text(c: &omsi_vehicle::Camera) -> String {
    let extra = c.extra.map(|e| format!("{e}")).unwrap_or_else(|| "-".to_string());
    format!("{};{};{};{};{};{};{};{}", c.pos[0], c.pos[1], c.pos[2], c.dist, c.fov, c.yaw, c.pitch, extra)
}

/// The camera a window line (see [`mirror_to_text`]) sets, made from `base` (which says whether
/// there is an extra value and whether the camera is a static one); `None` for a line that
/// is no good - then nothing changes.
fn mirror_from_text(text: &str, base: &omsi_vehicle::Camera) -> Option<omsi_vehicle::Camera> {
    let f: Vec<&str> = text.split(';').collect();
    if f.len() < 8 {
        return None;
    }
    let mut n = [0.0_f32; 7];
    for (k, slot) in n.iter_mut().enumerate() {
        let x: f32 = f[k].trim().replace(',', ".").parse().ok()?;
        if !x.is_finite() {
            return None;
        }
        *slot = x;
    }
    let extra = match base.extra {
        Some(_) => {
            let x: f32 = f[7].trim().replace(',', ".").parse().ok()?;
            if !x.is_finite() {
                return None;
            }
            Some(x)
        }
        None => None,
    };
    Some(omsi_vehicle::Camera { pos: [n[0], n[1], n[2]], dist: n[3], fov: n[4].clamp(0.0, 170.0), yaw: n[5], pitch: n[6].clamp(-89.0, 89.0), extra, fixed: base.fixed })
}

/// Write `value` into variable `name` of the vehicle (numbers also with a comma).
fn apply(p: &mut crate::player::Player, is_str: bool, name: &str, value: &str) {
    let v = &mut p.vehicle;
    if is_str {
        if let Some(i) = v.ty.program.str_var(name) {
            if let Some(slot) = v.state.str_vars.get_mut(i as usize) {
                *slot = value.to_string();
            }
        }
    } else if let Ok(x) = value.trim().replace(',', ".").parse::<f32>() {
        v.set_var(name, x);
    }
}

/// Once a frame: what the window said, what the game answers.
pub(crate) fn tick(dev: &mut DevToolsProc, mut player: Option<&mut crate::player::Player>, dt: f32) {
    let Some(rx) = dev.rx.as_ref() else { return };
    let lines: Vec<String> = rx.try_iter().collect();
    for line in lines {
        let f: Vec<&str> = line.splitn(4, '\t').collect();
        let (is_str, name) = (f.get(1).is_some_and(|k| *k == "s"), f.get(2).copied().unwrap_or(""));
        match f[0] {
            "hello" => dev.resync = true,
            "view" if f.len() >= 3 => dev.view.set(f[1], f[2] == "1"),
            "watch" if f.len() >= 3 => {
                let e = (is_str, name.to_string());
                if !dev.watch.contains(&e) {
                    dev.watch.push(e);
                }
            }
            "unwatch" if f.len() >= 3 => {
                dev.watch.retain(|w| !(w.0 == is_str && w.1 == name));
                dev.holds.retain(|h| !(h.0 == is_str && h.1 == name));
            }
            "set" if f.len() >= 4 => {
                if let Some(p) = player.as_deref_mut() {
                    apply(p, is_str, name, f[3]);
                }
            }
            "hold" if f.len() >= 4 => {
                if let Some(p) = player.as_deref_mut() {
                    apply(p, is_str, name, f[3]);
                }
                dev.holds.retain(|h| !(h.0 == is_str && h.1 == name));
                dev.holds.push((is_str, name.to_string(), f[3].to_string()));
            }
            "reload" if f.len() >= 2 => match f[1] {
                "cabin" => dev.reloads.push(crate::humans::CabinReload::Cabin),
                "paths" => dev.reloads.push(crate::humans::CabinReload::Paths),
                "sound" => dev.file_reloads.push(FileKind::Sound),
                "bus" => dev.file_reloads.push(FileKind::Bus),
                "model" => dev.file_reloads.push(FileKind::Model),
                "consts" => dev.file_reloads.push(FileKind::Constants),
                _ => {}
            },
            "mirrorset" if f.len() >= 3 => {
                if let (Some(p), Ok(i)) = (player.as_deref_mut(), f[1].parse::<usize>()) {
                    let base = p.all_mirror_cams();
                    if let Some(c) = base.get(i).and_then(|b| mirror_from_text(f[2], b)) {
                        p.mirror_dev.resize(base.len(), None);
                        p.mirror_dev[i] = Some(c);
                        p.mirror_dev_dirty.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            }
            "mirrorreset" if f.len() >= 2 => {
                if let (Some(p), Ok(i)) = (player.as_deref_mut(), f[1].parse::<usize>()) {
                    if let Some(slot) = p.mirror_dev.get_mut(i) {
                        *slot = None;
                        p.mirror_dev_dirty.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            }
            "release" if f.len() >= 3 => dev.holds.retain(|h| !(h.0 == is_str && h.1 == name)),
            _ => {}
        }
    }
    if dev.resync {
        dev.resync = false;
        dev.names_of = 0;
        for key in VIEW_KEYS {
            dev.send(format!("view\t{key}\t{}", dev.view.get(key) as u8));
        }
        for (s, n) in &dev.watch {
            dev.send(format!("watched\t{}\t{}", if *s { "s" } else { "v" }, clean(n)));
        }
        for (s, n, v) in &dev.holds {
            dev.send(format!("held\t{}\t{}\t{}", if *s { "s" } else { "v" }, clean(n), clean(v)));
        }
    }
    let Some(p) = player else { return };
    for (s, n, v) in &dev.holds {
        apply(p, *s, n, v);
    }
    // the names of the vehicle's variables, again when the vehicle is another
    let id = std::sync::Arc::as_ptr(&p.vehicle.ty) as *const u8 as usize;
    if dev.names_of != id {
        dev.names_of = id;
        let dir = p.vehicle.ty.def.path.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        dev.send(format!("vehicle\t{}", clean(&dir)));
        dev.send("names_begin".into());
        for n in &p.vehicle.ty.program.var_names {
            dev.send(format!("name\tv\t{}", clean(n)));
        }
        for n in &p.vehicle.ty.program.str_var_names {
            dev.send(format!("name\ts\t{}", clean(n)));
        }
        dev.send("names_end".into());
        // the vehicle's mirrors as the files give them, and the ones set by hand
        dev.send("mirrors_begin".into());
        for (i, c) in p.all_mirror_cams().iter().enumerate() {
            dev.send(format!("mirror\t{i}\t{}\t{}", c.fixed as u8, mirror_to_text(c)));
            if let Some(d) = p.mirror_dev.get(i).and_then(|d| d.as_ref()) {
                dev.send(format!("mirrordev\t{i}\t{}", mirror_to_text(d)));
            }
        }
        dev.send("mirrors_end".into());
    }
    // the watched values, five times a second
    dev.snapshot_t -= dt;
    if dev.snapshot_t <= 0.0 {
        dev.snapshot_t = 0.2;
        for (s, n) in &dev.watch {
            let value = if *s {
                clean(&p.vehicle.str_var(n))
            } else {
                match p.vehicle.var(n) {
                    Some(x) => format!("{x}"),
                    None => "?".to_string(),
                }
            };
            dev.send(format!("val\t{}\t{}\t{}", if *s { "s" } else { "v" }, clean(n), value));
        }
    }
}

/// World points to screen pixels the way the name tags above the other players' buses do
/// (one view, or the views of a triple-screen rig).
struct Projector {
    views: Vec<(Mat4, f32, f32)>,
    origin: DVec3,
    limit: f32,
    hud_x: f32,
}

impl Projector {
    fn new(cam: &omsi_render::Camera, width: f32, height: f32, rig: Option<&omsi_render::TripleScreen>, hud_x: f32) -> Projector {
        let views = if let Some(rig) = rig {
            rig.views(cam, width as u32, height as u32)
                .iter()
                .map(|v| {
                    let vp = v.projection * Mat4::look_to_rh(Vec3::ZERO, v.camera.forward(), v.camera.up());
                    (vp, v.viewport[0] as f32, v.viewport[2] as f32)
                })
                .collect()
        } else {
            vec![(cam.view_proj(width / height.max(1.0), cam.position), 0.0, width)]
        };
        Projector { views, origin: cam.position, limit: if rig.is_some() { 1.0 } else { 3.0 }, hud_x }
    }

    /// Where `p` is on the screen, none behind the camera (or far outside the picture).
    fn point(&self, p: DVec3) -> Option<(f32, f32, bool)> {
        let rel = (p - self.origin).as_vec3().extend(1.0);
        let mut first = None;
        for (vp, offset, panel_width) in &self.views {
            let c = *vp * rel;
            if c.w <= 0.05 {
                continue;
            }
            let (x, y) = (c.x / c.w, c.y / c.w);
            if x.abs() > self.limit || y.abs() > self.limit {
                continue;
            }
            let on = x.abs() <= 1.0 && y.abs() <= 1.0;
            let pos = (offset + (x + 1.0) * 0.5 * panel_width - self.hud_x, y);
            if on {
                return Some((pos.0, pos.1, true));
            }
            first.get_or_insert((pos.0, pos.1, false));
        }
        first
    }
}

/// The dots and lines the window asked for, over the picture of this frame.
#[allow(clippy::too_many_arguments)]
pub(crate) fn marks(
    view: &DevView,
    humans: Option<&mut crate::humans::Humans>,
    player: &crate::player::Player,
    cam: &omsi_render::Camera,
    width: f32,
    height: f32,
    rig: Option<&omsi_render::TripleScreen>,
    hud_x: f32,
) -> Vec<Mark> {
    let proj = Projector::new(cam, width, height, rig, hud_x);
    let mut out: Vec<Mark> = Vec::new();
    // (the projector gives the height as -1..1 up: to pixels down)
    let screen = |p: DVec3| proj.point(p).map(|(x, y, on)| ((x, (0.5 - y * 0.5) * height), on));
    let line = |out: &mut Vec<Mark>, a: DVec3, b: DVec3, rgba: [u8; 4], size: f32| {
        if let (Some((pa, on_a)), Some((pb, on_b))) = (screen(a), screen(b)) {
            if on_a || on_b {
                out.push(Mark { p: pa, to: Some(pb), rgba, size, label: None });
            }
        }
    };
    let dot = |out: &mut Vec<Mark>, a: DVec3, rgba: [u8; 4], size: f32| {
        if let Some((pa, true)) = screen(a) {
            out.push(Mark { p: pa, to: None, rgba, size, label: None });
        }
    };
    if view.seats || view.standing || view.paths || view.pathlabels || view.devices {
        if let Some(c) = humans.and_then(|h| h.debug_cabin(&player.vehicle)) {
            if view.paths {
                for (a, b) in &c.links {
                    if let (Some(pa), Some(pb)) = (c.points.get(*a), c.points.get(*b)) {
                        line(&mut out, *pa, *pb, [70, 235, 110, 255], 1.6);
                    }
                }
            }
            if view.paths || view.pathlabels {
                for (i, p) in c.points.iter().enumerate() {
                    let role = c.roles.get(i).cloned().unwrap_or_default();
                    // what the point is for, and the colour of the strongest use
                    let mut tags: Vec<String> = Vec::new();
                    let mut rgba = [70, 235, 110, 255];
                    let mut size = 2.5;
                    for (n, no_sale, button) in &role.entries {
                        tags.push(format!(
                            "entry {n}{}{}",
                            if *no_sale { " {noticketsale}" } else { "" },
                            if *button { " {withbutton}" } else { "" }
                        ));
                        rgba = [255, 255, 255, 255];
                        size = 6.0;
                    }
                    for n in &role.exits {
                        tags.push(format!("exit {n}"));
                        rgba = if role.entries.is_empty() { [175, 110, 255, 255] } else { [255, 200, 255, 255] };
                        size = 6.0;
                    }
                    if role.link_next {
                        tags.push("linkToNextVeh".into());
                        rgba = [0, 255, 210, 255];
                        size = 6.0;
                    }
                    if role.link_prev {
                        tags.push("linkToPrevVeh".into());
                        rgba = [0, 255, 210, 255];
                        size = 6.0;
                    }
                    if role.sale {
                        tags.push("ticket sale".into());
                        rgba = [255, 120, 200, 255];
                        size = 6.0;
                    }
                    if role.stamper {
                        tags.push("stamper".into());
                        rgba = [200, 255, 80, 255];
                        size = 5.0;
                    }
                    if view.pathlabels {
                        if let Some((pm, true)) = screen(*p) {
                            let text = if tags.is_empty() { format!("{i}") } else { format!("{i}: {}", tags.join(", ")) };
                            out.push(Mark { p: pm, to: None, rgba, size, label: Some(text) });
                        }
                    } else {
                        dot(&mut out, *p, rgba, size);
                    }
                }
            }
            if view.devices {
                for (p, kind, note) in &c.devices {
                    let rgba = match *kind {
                        "ticket_sale" => [255, 215, 0, 255],
                        "stamper" => [255, 140, 255, 255],
                        k if k.contains("money") => [120, 255, 200, 255],
                        _ => [255, 170, 120, 255],
                    };
                    if let Some((pm, true)) = screen(*p) {
                        let text = if note.is_empty() { format!("[{kind}]") } else { format!("[{kind}] {note}") };
                        out.push(Mark { p: pm, to: None, rgba, size: 5.0, label: Some(text) });
                    }
                }
            }
            for (p, seated) in &c.seats {
                if *seated && view.seats {
                    dot(&mut out, *p, [255, 165, 20, 255], 5.0);
                } else if !*seated && view.standing {
                    dot(&mut out, *p, [255, 40, 40, 255], 5.0);
                }
            }
        }
    }
    // --- the vehicle and the sections hung on it, each in its own frame
    struct Part<'a> {
        pos: DVec3,
        rot: Mat4,
        ty: &'a omsi_sim::VehicleType,
        is_main: bool,
        /// A mesh's transform (by its index in the model), and a `.bus` camera in the world.
        mesh_xf: Box<dyn Fn(usize) -> Mat4 + 'a>,
        cam: Box<dyn Fn(&omsi_vehicle::Camera) -> (DVec3, f32, f32, f32) + 'a>,
    }
    let v = &player.vehicle;
    let mut parts: Vec<Part> = vec![Part {
        pos: v.position,
        rot: v.body_rotation(),
        ty: &*v.ty,
        is_main: true,
        mesh_xf: Box::new(move |def_index: usize| match v.ty.meshes.iter().position(|m| m.def_index == def_index) {
            Some(i) => v.mesh_local_transform(i),
            None => v.body_rotation(),
        }),
        cam: Box::new(move |c: &omsi_vehicle::Camera| v.camera_world_full(c)),
    }];
    for t in &v.trailers {
        parts.push(Part {
            pos: t.position,
            rot: t.body_rotation(),
            ty: &*t.ty,
            is_main: false,
            mesh_xf: Box::new(move |def_index: usize| match t.ty.meshes.iter().position(|m| m.def_index == def_index) {
                Some(i) => t.mesh_local_transform(i),
                None => t.body_rotation(),
            }),
            cam: Box::new(move |c: &omsi_vehicle::Camera| t.camera_world_full(c)),
        });
    }
    // a view direction (yaw from +y towards +x, pitch up) in a frame
    let dir_of = |yaw: f32, pitch: f32| {
        let (sy, cy) = yaw.to_radians().sin_cos();
        let (sp, cp) = pitch.to_radians().sin_cos();
        Vec3::new(sy * cp, cy * cp, sp)
    };
    let label_at = |out: &mut Vec<Mark>, at: DVec3, rgba: [u8; 4], size: f32, text: String| {
        if let Some((pm, true)) = screen(at) {
            out.push(Mark { p: pm, to: None, rgba, size, label: Some(text) });
        }
    };

    for part in &parts {
        let (pos, rot, ty) = (part.pos, part.rot, part.ty);
        let world = |l: Vec3| pos + rot.transform_point3(l).as_dvec3();
        if view.boxes {
            // `[boundingbox]` is w l h and the centre x y z
            if let Some(bb) = ty.def.bounding_box {
                let (hw, hl, hh) = (bb[0] * 0.5, bb[1] * 0.5, bb[2] * 0.5);
                let mut c = [DVec3::ZERO; 8];
                for (k, slot) in c.iter_mut().enumerate() {
                    let (sx, sy, sz) = (if k & 1 == 0 { -1.0 } else { 1.0 }, if k & 2 == 0 { -1.0 } else { 1.0 }, if k & 4 == 0 { -1.0 } else { 1.0 });
                    *slot = world(Vec3::new(bb[3] + sx * hw, bb[4] + sy * hl, bb[5] + sz * hh));
                }
                for k in 0..8usize {
                    for bit in [1usize, 2, 4] {
                        if k & bit == 0 {
                            line(&mut out, c[k], c[k | bit], [255, 70, 255, 255], 1.8);
                        }
                    }
                }
            }
        }
        if view.axles {
            // Each axle: `[axle]`'s track is `max width` over the outer edges of the tyres and
            // `min width` over the inner ones (the suspension probes at their middle), so the
            // tyre stands between min/2 and max/2 from the middle. A ring of the wheel's
            // diameter at both edges, a dot at the wheel's centre (a radius above the
            // ground), a white dot where it meets the ground, a line across.
            for (n, a) in ty.def.axles.iter().enumerate() {
                let r = (a.wheel_diameter * 0.5).max(0.1);
                let outer = a.max_width * 0.5;
                let inner = if a.min_width > 0.0 && a.min_width < a.max_width { a.min_width * 0.5 } else { outer };
                let centre = (outer + inner) * 0.5;
                let blue = [60, 200, 255, 255];
                line(&mut out, world(Vec3::new(-centre, a.long, r)), world(Vec3::new(centre, a.long, r)), blue, 1.6);
                for side in [-1.0f32, 1.0] {
                    let edges: &[f32] = if outer - inner > 0.005 { &[outer, inner] } else { &[outer] };
                    for &x in edges {
                        let mut last = None;
                        for step in 0..=20 {
                            let ang = step as f32 / 20.0 * std::f32::consts::TAU;
                            let q = world(Vec3::new(side * x, a.long + r * ang.sin(), r - r * ang.cos()));
                            if let Some(prev) = last {
                                line(&mut out, prev, q, [60, 200, 255, 200], 1.2);
                            }
                            last = Some(q);
                        }
                    }
                    if edges.len() == 2 {
                        // the tyre's width: across at the top, bottom, front and back
                        for (dy, dz) in [(0.0f32, 0.0f32), (r, r), (0.0, 2.0 * r), (-r, r)] {
                            line(&mut out, world(Vec3::new(side * outer, a.long + dy, dz)), world(Vec3::new(side * inner, a.long + dy, dz)), [60, 200, 255, 200], 1.2);
                        }
                    }
                    dot(&mut out, world(Vec3::new(side * centre, a.long, r)), blue, 4.0);
                    dot(&mut out, world(Vec3::new(side * centre, a.long, 0.0)), [255, 255, 255, 255], 2.5);
                }
                let name = format!(
                    "axle {n}{}: dia {:.2} m, tyre {:.0} cm wide (track {:.2}..{:.2} m)",
                    if a.driven { " (driven)" } else { "" },
                    a.wheel_diameter,
                    (outer - inner) * 100.0,
                    a.min_width,
                    a.max_width
                );
                label_at(&mut out, world(Vec3::new(0.0, a.long, r)), blue, 4.0, name);
            }
        }
        if view.cog {
            // where the physics puts the centre of gravity (`[cog]` x and y, the height of
            // `[schwerpunkt]`, else the `[cog]`'s, else 1 m), and its place on the ground
            let (cx, cy) = ty.def.cog.map(|c| (c[0], c[1])).unwrap_or((0.0, 0.0));
            let cz = if ty.def.cog_height > 0.0 { ty.def.cog_height } else { ty.def.cog.map(|c| c[2]).filter(|z| *z > 0.0).unwrap_or(1.0) };
            line(&mut out, world(Vec3::new(cx, cy, cz)), world(Vec3::new(cx, cy, 0.0)), [255, 255, 255, 200], 1.2);
            dot(&mut out, world(Vec3::new(cx, cy, 0.0)), [255, 255, 255, 255], 2.5);
            label_at(&mut out, world(Vec3::new(cx, cy, cz)), [255, 60, 60, 255], 7.0, format!("center of gravity ({cx:.2}, {cy:.2}, {cz:.2})"));
        }
        if view.lights {
            // `[interiorlight]`: the position, lit (the variable at 0.5 or more) bright
            for (n, l) in ty.model.interior_lights.iter().enumerate() {
                let lit = if part.is_main {
                    l.variable.trim().parse::<f32>().ok().or_else(|| player.vehicle.var(&l.variable)).map(|x| x >= 0.5)
                } else {
                    None
                };
                let rgba = match lit {
                    Some(true) => [255, 240, 80, 255],
                    Some(false) => [150, 135, 60, 255],
                    None => [210, 190, 70, 255],
                };
                label_at(&mut out, world(Vec3::from(l.pos)), rgba, 5.0, format!("{n}: {}", l.variable));
            }
        }
        if view.viewsdriver || view.viewspax {
            // the cameras of the `.bus` (`[add_camera_driver]` and `[add_camera_pax]`): the
            // eye and where it looks
            let mut show = |cams: &[omsi_vehicle::Camera], what: &str, rgba: [u8; 4], std: Option<usize>| {
                for (n, c) in cams.iter().enumerate() {
                    let (eye, yaw, pitch, _) = (part.cam)(c);
                    let to = eye + (dir_of(yaw, pitch) * 1.0).as_dvec3();
                    line(&mut out, eye, to, rgba, 1.6);
                    let std = if std == Some(n) { " [standard]" } else { "" };
                    label_at(&mut out, eye, rgba, 5.0, format!("{what} view {n} (fov {:.0}){std}", c.fov));
                }
            };
            if view.viewsdriver {
                show(&ty.def.cameras_driver, "driver", [255, 255, 255, 255], part.is_main.then_some(ty.def.camera_std));
            }
            if view.viewspax {
                show(&ty.def.cameras_pax, "pax", [200, 160, 255, 255], None);
            }
        }
    }

    // the mirrors of the front part (`[add_camera_reflexion]`): the mirror and a bar for
    // the way the file turns it
    if view.mirrors {
        for (n, c) in v.ty.def.cameras_reflexion.iter().enumerate() {
            // (a mirror set by hand in the developer tools is shown as set)
            let c = player.mirror_dev.get(n).and_then(|d| d.as_ref()).unwrap_or(c);
            let (at, yaw, pitch, _) = v.camera_world_full(c);
            let blue = [150, 220, 255, 255];
            line(&mut out, at, at + (dir_of(yaw, pitch) * 0.6).as_dvec3(), [255, 255, 255, 255], 2.4);
            label_at(&mut out, at, blue, 5.0, format!("mirror {n}{}", if c.fixed { " (static)" } else { "" }));
        }
    }
    out
}
