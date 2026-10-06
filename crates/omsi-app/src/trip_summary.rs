//! The summary of a trip (as OMSI 2 shows it when a course is over): a window over the picture
//! with the line and the tour's number and the stops of the trip - for each the planned and the
//! real arrival and departure, how far off those were and the bus's odometer - with the stops
//! the bus never served marked "---". It can be copied whole or saved as a text file.
//!
//! It comes up by itself when the bus has stood at the last stop of a trip with a passenger door
//! open (the tour's next trip begins at that moment, see `PlayerDuty::update`), unless the
//! setting `trip_summary` is off. The game goes on under it: nothing is paused, and only the keys
//! of the window are taken (Esc or Enter closes it, Ctrl+C copies, Ctrl+E saves, Page Up, Page
//! Down, Home and End scroll). The data is the journey log's (`journey.rs`).
//!
//! It is drawn with `omsi-ui` over the game's picture the way the touch controls and the quick
//! menu are (`quick_menu.rs`).

use crate::app::App;
use crate::journey::TripSummary;
use glam::Vec2;
use omsi_ui::paint::Align;
use omsi_ui::{Atlas, Color, Draw, Fonts, Gpu, Layer, Painter, Rect, Weight};
use winit::keyboard::KeyCode;

const PANEL: Color = Color::rgba(14, 16, 20, 0.95);
const EDGE: Color = Color::rgba(244, 127, 48, 0.9);
const BTN: Color = Color::rgba(40, 44, 50, 0.96);
const BTN_HOVER: Color = Color::rgba(78, 84, 92, 0.98);
const BTN_MAIN: Color = Color::rgba(244, 127, 48, 0.96);
const BTN_MAIN_HOVER: Color = Color::rgba(255, 150, 80, 1.0);
const TEXT: Color = Color::rgba(240, 240, 240, 1.0);
const TEXT_ON: Color = Color::rgba(20, 20, 20, 1.0);
const DIM: Color = Color::rgba(240, 240, 240, 0.58);
const WARN: Color = Color::rgba(240, 170, 50, 1.0);
const OK: Color = Color::rgba(104, 190, 118, 1.0);
const BAND: Color = Color::rgba(255, 255, 255, 0.045);
const VEIL: Color = Color::rgba(0, 0, 0, 0.32);

/// The table's columns: the head of each (translated when drawn).
const HEADS: [&str; 9] = ["Stop", "Arr. plan", "Arr. real", "Diff", "Dep. plan", "Dep. real", "Diff", "Odo km", "Status"];

pub(crate) struct SummaryWindow {
    pub(crate) open: bool,
    summary: Option<TripSummary>,
    /// The trip's number in the duty (the log's), for the file's name.
    trip: usize,
    /// The first row shown.
    scroll: usize,
    /// The trip shown last (line, tour, where the duty's trips begin, trip): a trip is shown once.
    shown: Option<(String, String, usize, usize)>,
    /// What the last click on Copy or Save did.
    note: Option<String>,
    panel: Rect,
    buttons: [Rect; 3],
    rows_fit: usize,
    hover: Option<usize>,
    pressed: Option<usize>,
    consumed: bool,
    gpu: Option<(Gpu, wgpu::TextureFormat)>,
    fonts: Option<Fonts>,
    atlas: Atlas,
    painter: Painter,
}

impl SummaryWindow {
    pub(crate) fn new() -> SummaryWindow {
        let none = Rect::new(0.0, 0.0, 0.0, 0.0);
        SummaryWindow {
            open: false,
            summary: None,
            trip: 0,
            scroll: 0,
            shown: None,
            note: None,
            panel: none,
            buttons: [none; 3],
            rows_fit: 1,
            hover: None,
            pressed: None,
            consumed: false,
            gpu: None,
            fonts: None,
            atlas: Atlas::new(1024),
            painter: Painter::new(),
        }
    }

    /// Show `summary`, the one of trip `trip` of the duty.
    fn show(&mut self, summary: TripSummary, trip: usize) {
        self.summary = Some(summary);
        self.trip = trip;
        self.scroll = 0;
        self.note = None;
        self.pressed = None;
        self.consumed = false;
        self.open = true;
    }

    /// The window's graphics went away: the picture is made again with the next frame.
    pub(crate) fn drop_gpu(&mut self) {
        self.gpu = None;
        self.atlas = Atlas::new(1024);
    }

    fn button_at(&self, p: Vec2) -> Option<usize> {
        self.buttons.iter().position(|r| r.contains(p))
    }

    fn scroll_by(&mut self, rows: i64) {
        let n = self.summary.as_ref().map(|s| s.rows.len()).unwrap_or(0);
        let max = n.saturating_sub(self.rows_fit.max(1)) as i64;
        self.scroll = (self.scroll as i64 + rows).clamp(0, max) as usize;
    }

    /// Draw the window painted last over the frame (`view`, `w` x `h`).
    pub(crate) fn render(&mut self, r: &omsi_render::Renderer, view: &wgpu::TextureView, w: u32, h: u32) {
        if !self.open || self.painter.is_empty() {
            return;
        }
        let format = r.format();
        if self.gpu.as_ref().map(|g| g.1 != format).unwrap_or(true) {
            self.gpu = Some((Gpu::new(&r.device, format, 1, self.atlas.size), format));
            self.atlas.mark_all_dirty();
        }
        let Some((gpu, _)) = self.gpu.as_mut() else { return };
        gpu.upload(&r.device, &r.queue, 0, &self.painter.verts);
        gpu.upload_atlas(&r.queue, &mut self.atlas);
        let layers = [Layer::flat([0.0, 0.0, w as f32, h as f32], 0.0, 1.0)];
        let draws = [Draw { buffer: 0, range: 0..self.painter.len(), layer: 0, texture: 0 }];
        let mut enc = r.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("trip summary") });
        gpu.render(&r.device, &r.queue, &mut enc, view, (w, h), None, &layers, &draws);
        r.queue.submit([enc.finish()]);
    }
}

impl App {
    /// A trip of the duty is over (`trip`: its number in the duty): its summary comes up, when the
    /// player wants them (the setting `trip_summary`), once, and only for a trip the bus drove to
    /// its end - not one given up on the way or never begun.
    pub(crate) fn trip_summary_open(&mut self, trip: usize) {
        if !self.settings.trip_summary {
            return;
        }
        let (Some(duty), Some(journey)) = (self.duty.as_ref(), self.journey.as_ref()) else { return };
        let key = (duty.line.clone(), duty.tour.clone(), duty.first_trip, trip);
        if self.summary.shown.as_ref() == Some(&key) {
            return;
        }
        let Some(s) = journey.trip_summary(trip, true) else { return };
        if !s.rows.last().is_some_and(|r| r.served) || !s.rows.iter().any(|r| r.served) {
            return;
        }
        self.summary.shown = Some(key);
        self.summary.show(s, trip);
    }

    /// A key of the window, seen before the game's own handling of it. True when it is taken.
    pub(crate) fn summary_key(&mut self, code: KeyCode, pressed: bool, repeat: bool) -> bool {
        if !self.summary.open || !pressed {
            return false;
        }
        let ctrl = self.keys.contains(&KeyCode::ControlLeft) || self.keys.contains(&KeyCode::ControlRight);
        let fit = self.summary.rows_fit.max(1) as i64;
        match code {
            KeyCode::Escape | KeyCode::Enter | KeyCode::NumpadEnter => {
                if !repeat {
                    self.summary.open = false;
                }
                true
            }
            KeyCode::KeyC if ctrl => {
                if !repeat {
                    self.summary_copy();
                }
                true
            }
            KeyCode::KeyE if ctrl => {
                if !repeat {
                    self.summary_save();
                }
                true
            }
            KeyCode::PageDown => {
                self.summary.scroll_by(fit);
                true
            }
            KeyCode::PageUp => {
                self.summary.scroll_by(-fit);
                true
            }
            KeyCode::Home => {
                self.summary.scroll_by(i64::MIN / 2);
                true
            }
            KeyCode::End => {
                self.summary.scroll_by(i64::MAX / 2);
                true
            }
            _ => false,
        }
    }

    /// The mouse wheel over the window scrolls its table (true: taken).
    pub(crate) fn summary_wheel(&mut self, amount: f32) -> bool {
        if !self.summary.open || !self.summary.panel.contains(Vec2::new(self.cursor.0, self.cursor.1)) {
            return false;
        }
        let rows = (-amount * 3.0).round() as i64;
        self.summary.scroll_by(rows);
        true
    }

    /// The left button over the picture: a click in the window is its own (true: taken); a click
    /// on one of its buttons does what it says when the button is let go over it.
    pub(crate) fn summary_click(&mut self, pressed: bool) -> bool {
        if !self.summary.open {
            return false;
        }
        let p = Vec2::new(self.cursor.0, self.cursor.1);
        if pressed {
            if self.summary.panel.contains(p) {
                self.summary.consumed = true;
                self.summary.pressed = self.summary.button_at(p);
                return true;
            }
            return false;
        }
        if !self.summary.consumed {
            return false;
        }
        self.summary.consumed = false;
        if let Some(i) = self.summary.pressed.take() {
            if self.summary.button_at(p) == Some(i) {
                match i {
                    0 => self.summary_copy(),
                    1 => self.summary_save(),
                    _ => self.summary.open = false,
                }
            }
        }
        true
    }

    /// The whole summary on the clipboard.
    fn summary_copy(&mut self) {
        let Some(text) = self.summary.summary.as_ref().map(|s| s.text.clone()) else { return };
        #[cfg(not(target_os = "android"))]
        {
            thread_local! {
                // (kept alive: on X11 the text is gone when the clipboard is dropped)
                static CLIPBOARD: std::cell::RefCell<Option<arboard::Clipboard>> = const { std::cell::RefCell::new(None) };
            }
            let ok = CLIPBOARD.with(|c| {
                let mut c = c.borrow_mut();
                if c.is_none() {
                    *c = arboard::Clipboard::new().ok();
                }
                c.as_mut().is_some_and(|cb| cb.set_text(text.clone()).is_ok())
            });
            self.summary.note = Some(omsi_ui::tr(if ok { "Copied to the clipboard" } else { "The clipboard is not available" }).to_string());
        }
        #[cfg(target_os = "android")]
        {
            let _ = text;
            self.summary.note = Some(omsi_ui::tr("The clipboard is not available").to_string());
        }
    }

    /// The summary saved as a text file in the journey log's folder (`Journeys`).
    fn summary_save(&mut self) {
        let Some(text) = self.summary.summary.as_ref().map(|s| s.text.clone()) else { return };
        let Some(journey) = self.journey.as_ref() else { return };
        match journey.save_trip_summary(self.summary.trip, &text) {
            Ok(path) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.service_msg = Some((format!("{} {}", omsi_ui::tr("Trip summary saved:"), path.display()), 8.0));
                self.summary.note = Some(format!("{} {name}", omsi_ui::tr("Saved:")));
            }
            Err(e) => self.summary.note = Some(format!("{}: {e}", omsi_ui::tr("Could not save"))),
        }
    }

    /// Lay the window out for a picture of `w` x `h` physical pixels and paint it (drawn by
    /// `SummaryWindow::render` once the game's picture is in the frame).
    pub(crate) fn summary_prepare(&mut self, w: u32, h: u32) {
        if !self.summary.open || self.summary.summary.is_none() {
            self.summary.open = self.summary.open && self.summary.summary.is_some();
            self.summary.painter.clear();
            return;
        }
        let (wf, hf) = (w as f32, h as f32);
        let dpi = self.window.as_ref().map(|win| win.scale_factor() as f32).unwrap_or(1.0).max(0.5);
        let u = dpi * ((hf / dpi) / 1000.0).clamp(0.8, 1.5);
        let cursor = Vec2::new(self.cursor.0, self.cursor.1);
        let sw = &mut self.summary;
        let Some(summary) = sw.summary.as_ref() else { return };
        let fonts = sw.fonts.get_or_insert_with(Fonts::new);
        let (size, head_size, row_h) = (12.5 * u, 11.5 * u, 22.0 * u);
        let (pad, gap, margin) = (22.0 * u, 16.0 * u, 20.0 * u);

        // the columns: as wide as their widest text
        let heads: Vec<String> = HEADS.iter().map(|h| omsi_ui::tr(h).to_string()).collect();
        let statuses: Vec<String> = summary.rows.iter().map(|r| omsi_ui::tr(r.status).to_string()).collect();
        let cell = |r: &crate::journey::RowView, c: usize| -> String {
            match c {
                0 => r.name.trim().to_string(),
                1..=6 => r.cells[c - 1].clone(),
                7 => r.odo.clone(),
                _ => String::new(),
            }
        };
        let mut cw = [0.0f32; 9];
        for (c, hd) in heads.iter().enumerate() {
            cw[c] = fonts.width(hd, head_size, Weight::Bold);
        }
        for (k, r) in summary.rows.iter().enumerate() {
            for (c, width) in cw.iter_mut().enumerate().take(8) {
                *width = width.max(fonts.width(&cell(r, c), size, Weight::Regular));
            }
            cw[8] = cw[8].max(fonts.width(&statuses[k], size, Weight::Medium));
        }
        cw[0] = cw[0].min(300.0 * u);
        let avail = (wf - 2.0 * margin - 2.0 * pad).max(300.0 * u);
        let total: f32 = cw.iter().sum::<f32>() + gap * 8.0;
        if total > avail {
            cw[0] = (cw[0] - (total - avail)).max(120.0 * u);
        }
        let table_w = cw.iter().sum::<f32>() + gap * 8.0;

        // the head lines over the table, then the table, the footer and the buttons
        let title = omsi_ui::tr("Trip summary").to_string();
        let head_lines: Vec<String> = summary.head.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
        let head_h = 30.0 * u + head_lines.len() as f32 * 18.0 * u + 30.0 * u + row_h;
        let foot_h = 30.0 * u + 62.0 * u;
        let n = summary.rows.len();
        let max_panel_h = (hf - 2.0 * margin).max(240.0 * u);
        let fit = (((max_panel_h - head_h - foot_h - 2.0 * pad) / row_h).floor() as usize).max(3);
        let shown = n.min(fit);
        let panel_w = (table_w + 2.0 * pad).max(560.0 * u).min(wf - 2.0 * margin);
        let panel_h = head_h + shown as f32 * row_h + foot_h + 2.0 * pad;
        let panel = Rect::new((wf - panel_w) * 0.5, (hf - panel_h) * 0.5, panel_w, panel_h);
        sw.rows_fit = fit;
        sw.scroll = sw.scroll.min(n.saturating_sub(shown));
        let first = sw.scroll;
        sw.panel = panel;
        let (bw, bh) = (150.0 * u, 36.0 * u);
        let by = panel.bottom() - pad - bh;
        let b_close = Rect::new(panel.right() - pad - bw, by, bw, bh);
        let b_save = Rect::new(b_close.x - 10.0 * u - bw, by, bw, bh);
        let b_copy = Rect::new(b_save.x - 10.0 * u - bw, by, bw, bh);
        sw.buttons = [b_copy, b_save, b_close];
        // (not `button_at`: the fonts hold a borrow of the window's field, not of all of it)
        let hover = [b_copy, b_save, b_close].iter().position(|r| r.contains(cursor));
        sw.hover = hover;
        let note = sw.note.clone();
        let more = n > shown;

        let pt = &mut sw.painter;
        pt.clear();
        sw.atlas.begin_frame();
        let atlas = &mut sw.atlas;
        pt.rect(Rect::new(0.0, 0.0, wf, hf), VEIL);
        pt.rounded(panel, 14.0 * u, PANEL);
        pt.rounded_border(panel, 14.0 * u, 2.0 * u, EDGE);
        let x0 = panel.x + pad;
        let mut y = panel.y + pad;
        pt.text_in(atlas, fonts, &title, 18.0 * u, Weight::Bold, Rect::new(x0, y, panel.w - 2.0 * pad, 26.0 * u), Align::Left, TEXT);
        y += 30.0 * u;
        for line in &head_lines {
            pt.text_in(atlas, fonts, line, 12.5 * u, Weight::Regular, Rect::new(x0, y, panel.w - 2.0 * pad, 18.0 * u), Align::Left, DIM);
            y += 18.0 * u;
        }
        y += 6.0 * u;
        pt.text_in(atlas, fonts, &summary.title, 14.0 * u, Weight::Bold, Rect::new(x0, y, panel.w - 2.0 * pad, 22.0 * u), Align::Left, TEXT);
        y += 24.0 * u;
        // the column heads, then the rows
        let mut xs = [0.0f32; 9];
        let mut x = x0;
        for c in 0..9 {
            xs[c] = x;
            x += cw[c] + gap;
        }
        let place = |c: usize, y: f32| -> (Rect, Align) {
            let align = if (1..=7).contains(&c) { Align::Right } else { Align::Left };
            (Rect::new(xs[c], y, cw[c], row_h), align)
        };
        for (c, hd) in heads.iter().enumerate() {
            let (r, a) = place(c, y);
            pt.text_in(atlas, fonts, hd, head_size, Weight::Bold, r, a, DIM);
        }
        y += row_h;
        pt.rect(Rect::new(x0, y - 2.0 * u, table_w, 1.0 * u), DIM);
        for (k, r) in summary.rows.iter().enumerate().skip(first).take(shown) {
            if (k - first) % 2 == 1 {
                pt.rect(Rect::new(x0 - 6.0 * u, y, table_w + 12.0 * u, row_h), BAND);
            }
            let base = if r.missed { DIM } else { TEXT };
            for c in 0..9 {
                let text = if c == 8 { statuses[k].clone() } else { cell(r, c) };
                let color = match c {
                    3 if r.late => WARN,
                    6 if r.early => WARN,
                    1 | 2 | 4 | 5 if text == "---" => DIM,
                    8 => match r.status {
                        "OK" => OK,
                        "missed" => DIM,
                        _ => WARN,
                    },
                    _ => base,
                };
                let (cell_rect, a) = place(c, y);
                let text = if c == 0 { fonts.fit(&text, size, Weight::Regular, cell_rect.w).to_string() } else { text };
                pt.text_in(atlas, fonts, &text, size, if c == 0 { Weight::Medium } else { Weight::Regular }, cell_rect, a, color);
            }
            y += row_h;
        }
        if more {
            let s = format!("{}–{} / {}  (PgUp, PgDn)", first + 1, first + shown, n);
            pt.text_in(atlas, fonts, &s, 11.5 * u, Weight::Regular, Rect::new(x0, y, panel.w - 2.0 * pad, 18.0 * u), Align::Right, DIM);
        }
        pt.text_in(atlas, fonts, &summary.footer, 12.5 * u, Weight::Medium, Rect::new(x0, by - 30.0 * u, panel.w - 2.0 * pad, 20.0 * u), Align::Left, TEXT);
        if let Some(n) = note {
            pt.text_in(atlas, fonts, &n, 12.0 * u, Weight::Regular, Rect::new(x0, by, b_copy.x - x0 - 12.0 * u, bh), Align::Left, DIM);
        }
        let labels = [omsi_ui::tr("Copy").to_string(), omsi_ui::tr("Save to file").to_string(), omsi_ui::tr("Close").to_string()];
        for (i, b) in [b_copy, b_save, b_close].iter().enumerate() {
            let main = i == 2;
            let fill = match (main, hover == Some(i)) {
                (true, true) => BTN_MAIN_HOVER,
                (true, false) => BTN_MAIN,
                (false, true) => BTN_HOVER,
                (false, false) => BTN,
            };
            pt.rounded(*b, 10.0 * u, fill);
            pt.text_in(atlas, fonts, &labels[i], 14.0 * u, Weight::Bold, *b, Align::Center, if main { TEXT_ON } else { TEXT });
        }
    }
}
