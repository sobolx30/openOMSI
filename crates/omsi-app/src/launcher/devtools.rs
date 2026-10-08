//! The developer tools window: a process of its own (this program started with
//! `OMSI_DEVTOOLS_WINDOW=1` by the game, see `crate::devtools_proc`) that opens beside the game
//! when Settings → General → *Enable Developer Tools* is on. Closing the window only closes
//! it; Ctrl+Shift+Backspace in the game starts it again.
//!
//! The game keeps both ends of this process's standard input and output: lines of text go
//! both ways (the protocol is described in `devtools_proc`). When the game ends (or is
//! killed) the input ends, this side reads the end of it and the window closes too, so no
//! tools window is left behind by a crashed session.
//!
//! The tools: switches for what the game draws over the picture (the places, the
//! passengers' paths, the vehicle's bounding box), and the variables of the player's vehicle -
//! numbers and strings - to watch, to write once or hold, and to keep as named lists (the
//! names only, never the values).

use super::ui::{ButtonKind, Modifiers};
use super::*;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// The overlays, as `devtools_proc::VIEW_KEYS` names them.
const VIEW_KEYS: [&str; 8] = ["seats", "standing", "paths", "boxes", "axles", "lights", "pathlabels", "devices"];

/// Tell the game something (one line).
fn send_line(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

/// Run the tools window until it is closed or the game that started it is gone.
pub fn run(instance: wgpu::Instance) -> anyhow::Result<()> {
    let game_gone = Arc::new(AtomicBool::new(false));
    let inbox: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    {
        // (a thread that reads what the game says, and sees the end of the pipe)
        let flag = game_gone.clone();
        let inbox = inbox.clone();
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            for line in stdin.lock().lines() {
                match line {
                    Ok(l) => {
                        if let Ok(mut q) = inbox.lock() {
                            q.push(l);
                        }
                    }
                    Err(_) => break,
                }
            }
            flag.store(true, Ordering::Relaxed);
        });
    }
    let event_loop = EventLoop::new()?;
    let mut app = DevTools {
        instance,
        window: None,
        surface: None,
        renderer: None,
        gpu: None,
        ui: Ui::new(),
        last: Instant::now(),
        focused: true,
        game_gone,
        inbox,
        modifiers: Modifiers::default(),
        tools: Tools::new(),
    };
    send_line("hello");
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// One watched variable.
struct Watch {
    is_str: bool,
    name: String,
    /// What the game says it is now.
    value: String,
    /// What is typed into its field, to write.
    edit: String,
    seeded: bool,
    /// Written every frame by the game.
    held: bool,
}

/// The tools' state, apart from the window.
struct Tools {
    /// The overlays on (by `VIEW_KEYS`).
    view: [bool; 8],
    /// The list of switches is open.
    overlays_open: bool,
    vehicle: Option<String>,
    /// The vehicle's variables: numbers, strings.
    names: [Vec<String>; 2],
    incoming: [Vec<String>; 2],
    watched: Vec<Watch>,
    /// 0 numbers, 1 strings.
    tab: usize,
    filter: String,
    lists: Vec<(String, Vec<(bool, String)>)>,
    list_name: String,
}

enum Act {
    Add(bool, String),
    Remove(usize),
    Set(usize),
    Hold(usize),
    Save,
    Load(usize),
    Delete(usize),
}

fn kind(is_str: bool) -> &'static str {
    if is_str {
        "s"
    } else {
        "v"
    }
}

fn clean(s: &str) -> String {
    s.chars().map(|c| if c == '\t' || c == '\n' || c == '\r' { ' ' } else { c }).collect()
}

/// Where the named lists are kept: the settings' folder.
fn lists_path() -> Option<std::path::PathBuf> {
    crate::settings::Settings::path().and_then(|p| p.parent().map(|d| d.join("devtools_lists.txt")))
}

/// The named lists: `list <name>` and under it `v <name>` / `s <name>` lines.
fn load_lists() -> Vec<(String, Vec<(bool, String)>)> {
    let mut out: Vec<(String, Vec<(bool, String)>)> = Vec::new();
    let Some(text) = lists_path().and_then(|p| std::fs::read_to_string(p).ok()) else { return out };
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("list\t") {
            out.push((name.to_string(), Vec::new()));
        } else if let (Some(entry), Some(last)) = (line.split_once('\t'), out.last_mut()) {
            match entry.0 {
                "v" => last.1.push((false, entry.1.to_string())),
                "s" => last.1.push((true, entry.1.to_string())),
                _ => {}
            }
        }
    }
    out
}

fn save_lists(lists: &[(String, Vec<(bool, String)>)]) {
    let Some(path) = lists_path() else { return };
    let mut text = String::new();
    for (name, entries) in lists {
        text.push_str(&format!("list\t{}\n", clean(name)));
        for (is_str, n) in entries {
            text.push_str(&format!("{}\t{}\n", kind(*is_str), clean(n)));
        }
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, text);
}

impl Tools {
    fn new() -> Tools {
        Tools {
            view: [false; 8],
            overlays_open: true,
            vehicle: None,
            names: [Vec::new(), Vec::new()],
            incoming: [Vec::new(), Vec::new()],
            watched: Vec::new(),
            tab: 0,
            filter: String::new(),
            lists: load_lists(),
            list_name: String::new(),
        }
    }

    fn entry(&mut self, is_str: bool, name: &str) -> &mut Watch {
        let at = match self.watched.iter().position(|w| w.is_str == is_str && w.name == name) {
            Some(i) => i,
            None => {
                self.watched.push(Watch { is_str, name: name.to_string(), value: String::new(), edit: String::new(), seeded: false, held: false });
                self.watched.len() - 1
            }
        };
        &mut self.watched[at]
    }

    /// One line from the game.
    fn handle(&mut self, line: &str) {
        let f: Vec<&str> = line.splitn(4, '\t').collect();
        let is_str = f.get(1).is_some_and(|k| *k == "s");
        match f[0] {
            "view" if f.len() >= 3 => {
                if let Some(i) = VIEW_KEYS.iter().position(|k| *k == f[1]) {
                    self.view[i] = f[2] == "1";
                }
            }
            "vehicle" => self.vehicle = Some(f.get(1).copied().unwrap_or("").to_string()).filter(|v| !v.is_empty()),
            "names_begin" => self.incoming = [Vec::new(), Vec::new()],
            "name" if f.len() >= 3 => self.incoming[is_str as usize].push(f[2].to_string()),
            "names_end" => {
                let mut got = std::mem::take(&mut self.incoming);
                for list in &mut got {
                    list.sort_by_key(|n| n.to_lowercase());
                    list.dedup_by_key(|n| n.to_lowercase());
                }
                self.names = got;
            }
            "watched" if f.len() >= 3 => {
                self.entry(is_str, f[2]);
            }
            "held" if f.len() >= 4 => {
                let w = self.entry(is_str, f[2]);
                w.held = true;
                w.edit = f[3].to_string();
                w.seeded = true;
            }
            "val" if f.len() >= 4 => {
                if let Some(w) = self.watched.iter_mut().find(|w| w.is_str == is_str && w.name == f[2]) {
                    w.value = f[3].to_string();
                    if !w.seeded {
                        w.edit = w.value.clone();
                        w.seeded = true;
                    }
                }
            }
            _ => {}
        }
    }

    /// The interface; what the game should hear goes into `out`.
    fn draw(&mut self, ui: &mut Ui, size: Vec2, out: &mut Vec<String>) {
        let pad = 20.0;
        let w = (size.x - 2.0 * pad).max(200.0);
        ui.text("Developer tools", Vec2::new(pad, 38.0), 22.0, Weight::Bold, TEXT, Align::Left);
        let status = match &self.vehicle {
            Some(v) => format!("Vehicle: {v}"),
            None => "No vehicle in the game yet.".to_string(),
        };
        ui.text(&status, Vec2::new(pad, 60.0), 13.0, Weight::Regular, TEXT_DIM, Align::Left);
        let mut y = 84.0;

        // --- what the game draws over the picture: a list that folds up
        let head = Rect::new(pad, y, w, 28.0);
        if ui.row("dt-overlays-head", head, false) {
            self.overlays_open = !self.overlays_open;
        }
        let c = Vec2::new(pad + 10.0, y + 14.0);
        if self.overlays_open {
            ui.p().line(Vec2::new(c.x - 4.0, c.y - 2.0), Vec2::new(c.x, c.y + 2.0), 1.6, TEXT_SOFT);
            ui.p().line(Vec2::new(c.x, c.y + 2.0), Vec2::new(c.x + 4.0, c.y - 2.0), 1.6, TEXT_SOFT);
        } else {
            ui.p().line(Vec2::new(c.x - 2.0, c.y - 4.0), Vec2::new(c.x + 2.0, c.y), 1.6, TEXT_SOFT);
            ui.p().line(Vec2::new(c.x + 2.0, c.y), Vec2::new(c.x - 2.0, c.y + 4.0), 1.6, TEXT_SOFT);
        }
        let shown = self.view.iter().filter(|v| **v).count();
        let title = if self.overlays_open || shown == 0 { "Over the picture in the game".to_string() } else { format!("Over the picture in the game ({shown} on)") };
        ui.text(&title, Vec2::new(pad + 26.0, y + 19.0), 15.0, Weight::Bold, TEXT_SOFT, Align::Left);
        y += 30.0;
        if self.overlays_open {
            let labels = [
                "Seated places (orange dots)",
                "Standing places (red dots)",
                "Paths of the passengers inside the vehicle",
                "Path point numbers and uses (entry, exit, links, sale, stamper)",
                "Ticket sale, stamper, money and change points",
                "Bounding box of the vehicle",
                "Axles and wheels (blue, with the axle's number)",
                "Interior lights (yellow, with number and variable)",
            ];
            // (the switch's place in `VIEW_KEYS` for each line)
            let keys = [0usize, 1, 2, 6, 7, 3, 4, 5];
            let two = w >= 760.0;
            let col = if two { (w - 20.0) * 0.5 } else { w.min(520.0) };
            for (k, label) in labels.iter().enumerate() {
                let (cx, row) = if two { (pad + (k % 2) as f32 * (col + 20.0), k / 2) } else { (pad, k) };
                let r = Rect::new(cx, y + row as f32 * 30.0, col, 28.0);
                let at = keys[k];
                let mut on = self.view[at];
                if ui.toggle(&format!("dt-view-{at}"), r, &mut on, label) {
                    self.view[at] = on;
                    out.push(format!("view\t{}\t{}", VIEW_KEYS[at], on as u8));
                }
            }
            let rows = if two { labels.len().div_ceil(2) } else { labels.len() };
            y += rows as f32 * 30.0;
        }
        y += 12.0;

        // --- the variables
        ui.text("Variables of the vehicle", Vec2::new(pad, y + 12.0), 15.0, Weight::Bold, TEXT_SOFT, Align::Left);
        y += 22.0;
        let mut tab = self.tab;
        if ui.segmented("dt-tab", Rect::new(pad, y, 220.0_f32.min(w), 28.0), &mut tab, &["Numbers", "Strings"]) {
            self.tab = tab;
        }
        y += 38.0;
        let gap = 16.0;
        let cw = ((w - gap) * 0.5).max(150.0);
        let height = (size.y - pad - y).max(120.0);
        let left = Rect::new(pad, y, cw, height);
        let right = Rect::new(pad + cw + gap, y, cw, height);
        let mut acts: Vec<Act> = Vec::new();
        let tab = self.tab;

        // left: all the names, searched
        ui.text_input("dt-filter", Rect::new(left.x, left.y, left.w, 30.0), &mut self.filter, "Search by name", None);
        let list_rect = Rect::new(left.x, left.y + 38.0, left.w, left.h - 38.0);
        if self.names[tab].is_empty() {
            ui.paragraph("The variables appear here once a vehicle is loaded in the game.", Vec2::new(list_rect.x + 6.0, list_rect.y), list_rect.w - 12.0, 13.0, Weight::Regular, TEXT_DIM);
        } else {
            let q = self.filter.to_lowercase();
            let shown: Vec<&String> = self.names[tab].iter().filter(|n| q.is_empty() || n.to_lowercase().contains(&q)).take(800).collect();
            let watched = &self.watched;
            let acts_ref = &mut acts;
            ui.scroll_area("dt-names", list_rect, &mut |ui, r| {
                let mut yy = r.y;
                for n in &shown {
                    let row = Rect::new(r.x, yy, r.w - 8.0, 24.0);
                    yy += 24.0;
                    if !ui.rect_visible(row) {
                        continue;
                    }
                    let on = watched.iter().any(|w| w.is_str == (tab == 1) && &w.name == *n);
                    if ui.row(&format!("dt-n-{tab}-{n}"), row, on) && !on {
                        acts_ref.push(Act::Add(tab == 1, (*n).clone()));
                    }
                    ui.text_in(n, Rect::new(row.x + 8.0, row.y, row.w - 16.0, row.h), 12.5, Weight::Regular, if on { ACCENT } else { TEXT_SOFT }, Align::Left);
                }
                yy - r.y
            });
        }

        // right: the lists kept under a name, the watched variables
        ui.text("Watched (click a name on the left to add)", Vec2::new(right.x, right.y + 14.0), 13.0, Weight::Bold, TEXT_SOFT, Align::Left);
        let mut ry = right.y + 24.0;
        ui.text_input("dt-list-name", Rect::new(right.x, ry, right.w - 92.0, 28.0), &mut self.list_name, "Name for the list", None);
        if ui.button("dt-list-save", Rect::new(right.x + right.w - 86.0, ry, 86.0, 28.0), "Save list", None, ButtonKind::Normal) {
            acts.push(Act::Save);
        }
        ry += 34.0;
        if !self.lists.is_empty() {
            let rows = self.lists.len().min(4);
            let lr = Rect::new(right.x, ry, right.w, rows as f32 * 28.0);
            let lists = &self.lists;
            let acts_ref = &mut acts;
            ui.scroll_area("dt-lists", lr, &mut |ui, r| {
                let mut yy = r.y;
                for (k, (name, entries)) in lists.iter().enumerate() {
                    let row = Rect::new(r.x, yy, r.w - 8.0, 28.0);
                    yy += 28.0;
                    if !ui.rect_visible(row) {
                        continue;
                    }
                    ui.text_in(&format!("{name}  ({})", entries.len()), Rect::new(row.x + 4.0, row.y, row.w - 100.0, row.h), 13.0, Weight::Regular, TEXT, Align::Left);
                    if ui.button(&format!("dt-ll-{k}"), Rect::new(row.right() - 94.0, row.y + 1.0, 60.0, 26.0), "Load", None, ButtonKind::Normal) {
                        acts_ref.push(Act::Load(k));
                    }
                    if ui.button(&format!("dt-ld-{k}"), Rect::new(row.right() - 30.0, row.y + 1.0, 30.0, 26.0), "×", None, ButtonKind::Ghost) {
                        acts_ref.push(Act::Delete(k));
                    }
                }
                yy - r.y
            });
            ry += lr.h + 8.0;
        }
        let wr = Rect::new(right.x, ry, right.w, (right.bottom() - ry).max(60.0));
        if self.watched.is_empty() {
            ui.paragraph("Nothing watched yet.", Vec2::new(wr.x + 4.0, wr.y), wr.w - 8.0, 13.0, Weight::Regular, TEXT_DIM);
        } else {
            let watched = &mut self.watched;
            let acts_ref = &mut acts;
            ui.scroll_area("dt-watched", wr, &mut |ui, r| {
                let mut yy = r.y;
                for (k, wt) in watched.iter_mut().enumerate() {
                    let item = Rect::new(r.x, yy, r.w - 8.0, 62.0);
                    yy += 66.0;
                    if !ui.rect_visible(item) {
                        continue;
                    }
                    let label = format!("{}{}", wt.name, if wt.is_str { "  (string)" } else { "" });
                    ui.text_in(&label, Rect::new(item.x + 2.0, item.y, item.w * 0.5, 22.0), 13.0, Weight::Bold, TEXT, Align::Left);
                    ui.text_in(&wt.value, Rect::new(item.x + item.w * 0.5, item.y, item.w * 0.5 - 2.0, 22.0), 13.0, Weight::Regular, ACCENT, Align::Right);
                    let by = item.y + 26.0;
                    let field_w = (item.w - 4.0 * 3.0 - 52.0 - 56.0 - 30.0).max(60.0);
                    ui.text_input(&format!("dt-e-{k}-{}", wt.name), Rect::new(item.x, by, field_w, 28.0), &mut wt.edit, "new value", None);
                    let mut bx = item.x + field_w + 4.0;
                    if ui.button(&format!("dt-set-{k}-{}", wt.name), Rect::new(bx, by, 52.0, 28.0), "Set", None, ButtonKind::Normal) {
                        acts_ref.push(Act::Set(k));
                    }
                    bx += 56.0;
                    if ui.button(&format!("dt-hold-{k}-{}", wt.name), Rect::new(bx, by, 52.0, 28.0), "Hold", None, if wt.held { ButtonKind::Primary } else { ButtonKind::Normal }) {
                        acts_ref.push(Act::Hold(k));
                    }
                    bx += 56.0;
                    if ui.button(&format!("dt-rm-{k}-{}", wt.name), Rect::new(bx, by, 30.0, 28.0), "×", None, ButtonKind::Ghost) {
                        acts_ref.push(Act::Remove(k));
                    }
                }
                yy - r.y
            });
        }

        // what was clicked
        for act in acts {
            match act {
                Act::Add(is_str, name) => {
                    self.entry(is_str, &name);
                    out.push(format!("watch\t{}\t{}", kind(is_str), name));
                }
                Act::Remove(k) => {
                    if k < self.watched.len() {
                        let w = self.watched.remove(k);
                        out.push(format!("unwatch\t{}\t{}", kind(w.is_str), w.name));
                    }
                }
                Act::Set(k) => {
                    if let Some(w) = self.watched.get(k) {
                        out.push(format!("set\t{}\t{}\t{}", kind(w.is_str), w.name, clean(&w.edit)));
                    }
                }
                Act::Hold(k) => {
                    if let Some(w) = self.watched.get_mut(k) {
                        w.held = !w.held;
                        if w.held {
                            out.push(format!("hold\t{}\t{}\t{}", kind(w.is_str), w.name, clean(&w.edit)));
                        } else {
                            out.push(format!("release\t{}\t{}", kind(w.is_str), w.name));
                        }
                    }
                }
                Act::Save => {
                    let name = clean(self.list_name.trim());
                    if !name.is_empty() {
                        let entries: Vec<(bool, String)> = self.watched.iter().map(|w| (w.is_str, w.name.clone())).collect();
                        match self.lists.iter_mut().find(|l| l.0 == name) {
                            Some(l) => l.1 = entries,
                            None => self.lists.push((name, entries)),
                        }
                        save_lists(&self.lists);
                    }
                }
                Act::Load(k) => {
                    if let Some((_, entries)) = self.lists.get(k).cloned() {
                        for w in std::mem::take(&mut self.watched) {
                            out.push(format!("unwatch\t{}\t{}", kind(w.is_str), w.name));
                        }
                        for (is_str, n) in entries {
                            self.entry(is_str, &n);
                            out.push(format!("watch\t{}\t{}", kind(is_str), n));
                        }
                    }
                }
                Act::Delete(k) => {
                    if k < self.lists.len() {
                        self.lists.remove(k);
                        save_lists(&self.lists);
                    }
                }
            }
        }
        let _ = size;
    }
}

struct DevTools {
    instance: wgpu::Instance,
    window: Option<Arc<Window>>,
    surface: Option<SurfaceState<'static>>,
    renderer: Option<Renderer>,
    gpu: Option<omsi_ui::Gpu>,
    ui: Ui,
    last: Instant,
    focused: bool,
    /// The game's end of our standard input closed.
    game_gone: Arc<AtomicBool>,
    /// What the game said, not yet handled.
    inbox: Arc<Mutex<Vec<String>>>,
    modifiers: Modifiers,
    tools: Tools,
}

impl DevTools {
    /// Physical pixels per interface pixel.
    fn scale(&self) -> f32 {
        self.window.as_ref().map(|w| w.scale_factor() as f32).unwrap_or(1.0).max(0.5)
    }

    fn make_surface(&mut self, event_loop: &ActiveEventLoop) {
        let Some(window) = self.window.clone() else { return };
        if self.renderer.is_none() {
            let settings = crate::settings::Settings::load();
            let renderer = match crate::startup::window_renderer(&mut self.instance, &window, showroom_options(&settings)) {
                Ok(r) => r,
                Err(e) => {
                    log::error!("developer tools: cannot draw on this computer: {e:#}");
                    event_loop.exit();
                    return;
                }
            };
            self.gpu = Some(omsi_ui::Gpu::new(&renderer.device, renderer.format(), 4, self.ui.atlas.size));
            self.renderer = Some(renderer);
        }
        let Some(renderer) = self.renderer.as_ref() else { return };
        let size = window.inner_size();
        self.surface = SurfaceState::new_with(&self.instance, window.clone(), renderer, size.width.max(1), size.height.max(1), true).ok();
        self.last = Instant::now();
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        if self.surface.is_none() {
            self.make_surface(event_loop);
        }
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f32().min(0.1);
        self.last = now;
        let (Some(window), Some(_)) = (self.window.clone(), self.surface.as_ref()) else { return };
        let scale = self.scale();
        let phys = window.inner_size();
        let (pw, ph) = (phys.width.max(1), phys.height.max(1));
        let size = Vec2::new(pw as f32, ph as f32) / scale;

        // what the game said
        let lines: Vec<String> = self.inbox.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
        for l in &lines {
            self.tools.handle(l);
        }

        let mut out: Vec<String> = Vec::new();
        self.ui.begin(size, scale, dt);
        self.tools.draw(&mut self.ui, size, &mut out);
        window.set_cursor(self.ui.cursor);
        let (layers, verts, ranges) = self.ui.finish();
        for l in &out {
            send_line(l);
        }

        let Some(renderer) = self.renderer.as_ref() else { return };
        let draws: Vec<Draw> = ranges.iter().enumerate().map(|(k, (r, tex))| Draw { buffer: 0, range: r.clone(), layer: k, texture: *tex }).collect();
        let bg = wgpu::Color { r: 0.0056, g: 0.0056, b: 0.0056, a: 1.0 };
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.upload(&renderer.device, &renderer.queue, 0, &verts);
            gpu.upload_atlas(&renderer.queue, &mut self.ui.atlas);
        }
        let Some(surface) = self.surface.as_mut() else { return };
        let frame = match surface.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                surface.resize(renderer, pw, ph);
                return;
            }
            _ => return,
        };
        let view = frame.texture.create_view(&Default::default());
        if let Some(gpu) = self.gpu.as_mut() {
            let mut enc = renderer.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("developer tools") });
            gpu.render(&renderer.device, &renderer.queue, &mut enc, &view, (pw, ph), Some(bg), &layers, &draws);
            renderer.queue.submit([enc.finish()]);
        }
        window.pre_present_notify();
        frame.present();
    }
}

impl ApplicationHandler for DevTools {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("openOMSI Developer Tools")
            .with_window_icon(crate::startup::window_icon())
            .with_inner_size(winit::dpi::LogicalSize::new(940.0, 720.0))
            .with_min_inner_size(winit::dpi::LogicalSize::new(560.0, 420.0));
        match event_loop.create_window(attrs) {
            Ok(w) => self.window = Some(Arc::new(w)),
            Err(e) => {
                log::error!("developer tools: cannot open the window: {e}");
                event_loop.exit();
                return;
            }
        }
        self.make_surface(event_loop);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let scale = self.scale();
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Focused(f) => self.focused = f,
            WindowEvent::Resized(s) => {
                if let (Some(sf), Some(r)) = (self.surface.as_mut(), self.renderer.as_ref()) {
                    sf.resize(r, s.width.max(1), s.height.max(1));
                }
            }
            WindowEvent::ModifiersChanged(m) => {
                self.modifiers.told(m.state());
                self.modifiers.apply(&mut self.ui.input);
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.ui.input.mouse = Vec2::new(position.x as f32, position.y as f32) / scale;
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                let down = state == ElementState::Pressed;
                if down {
                    self.ui.input.pressed = true;
                } else {
                    self.ui.input.released = true;
                }
                self.ui.input.down = down;
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let d = match delta {
                    MouseScrollDelta::LineDelta(x, y) => Vec2::new(x, y),
                    MouseScrollDelta::PixelDelta(p) => Vec2::new(p.x as f32, p.y as f32) / 40.0,
                };
                self.ui.input.wheel += d;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if self.modifiers.key(code, event.state == ElementState::Pressed) {
                        self.modifiers.apply(&mut self.ui.input);
                    }
                }
                if event.state != ElementState::Pressed {
                    return;
                }
                let cmd = self.modifiers.command();
                if let PhysicalKey::Code(code) = event.physical_key {
                    self.ui.input.raw_key = Some(code);
                    let k = match code {
                        KeyCode::ArrowLeft => Some(Key::Left),
                        KeyCode::ArrowRight => Some(Key::Right),
                        KeyCode::ArrowUp => Some(Key::Up),
                        KeyCode::ArrowDown => Some(Key::Down),
                        KeyCode::Home => Some(Key::Home),
                        KeyCode::End => Some(Key::End),
                        KeyCode::Backspace => Some(Key::Backspace),
                        KeyCode::Delete => Some(Key::Delete),
                        KeyCode::Enter | KeyCode::NumpadEnter => Some(Key::Enter),
                        KeyCode::Escape => Some(Key::Escape),
                        KeyCode::Tab => Some(Key::Tab),
                        KeyCode::KeyA if cmd => Some(Key::SelectAll),
                        _ => None,
                    };
                    if let Some(k) = k {
                        self.ui.input.keys.push(k);
                    }
                }
                if !cmd {
                    if let Some(t) = event.text.as_ref() {
                        self.ui.input.text.push_str(t);
                    }
                }
            }
            WindowEvent::RedrawRequested => self.frame(event_loop),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.game_gone.load(Ordering::Relaxed) {
            event_loop.exit();
            return;
        }
        // 20 frames a second are plenty for tools (10 without the keyboard)
        let interval = if self.focused { 0.05 } else { 0.1 };
        let since = self.last.elapsed().as_secs_f32();
        if since < interval {
            event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(self.last + std::time::Duration::from_secs_f32(interval)));
            return;
        }
        event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }
}
