//! The quick menu (Alt, as in OMSI 2): a grid of tiles in the lower right corner of the picture
//! for what is wanted often - placing, swapping and removing a vehicle, going to a start point,
//! the line and tour, the destination display, repairing, refuelling and washing, the route
//! arrows, the time, the weather and the game controllers - without the way through the Esc
//! menu's pages.
//!
//! It is an overlay: the game goes on under it (nothing is paused), the keys stay the game's
//! and a click on a tile is the only thing it takes from the mouse. Alt tapped alone (down and up
//! with nothing else pressed or clicked between) shows and hides it; Esc hides it. Every tile
//! does what its line in the game menu does (`App::page_action`); the tiles that choose
//! something (a vehicle, a line, a destination, the weather ...) open the game menu on that list
//! straight away, without its pause, and the menu closes when the list is done (`session`).
//! While a timetable is active the line-and-tour tile is the one that ends it: a click asks (a
//! window over the picture) and then does what "Free drive" in the list of lines does.
//!
//! It is drawn with `omsi-ui` over the game's picture the way the touch controls are
//! (`touch.rs`): Material icons, in physical pixels scaled by the screen's density.

use crate::app::App;
use glam::Vec2;
use omsi_ui::paint::Align;
use omsi_ui::{Atlas, Color, Draw, Fonts, Gpu, Layer, Painter, Rect, Weight};
use winit::keyboard::KeyCode;

/// Tiles in a row.
const COLS: usize = 4;

const PANEL: Color = Color::rgba(14, 16, 20, 0.84);
const TILE: Color = Color::rgba(40, 44, 50, 0.92);
const TILE_HOVER: Color = Color::rgba(78, 84, 92, 0.96);
const TILE_ON: Color = Color::rgba(244, 127, 48, 0.96);
const TILE_OFF: Color = Color::rgba(40, 44, 50, 0.45);
// (the tile that ends the timetable is red: it does not choose, it takes away)
const TILE_CANCEL: Color = Color::rgba(92, 40, 38, 0.94);
const TILE_CANCEL_HOVER: Color = Color::rgba(138, 58, 52, 0.97);
const BADGE_CANCEL: Color = Color::rgba(214, 64, 52, 1.0);
const BTN_RED: Color = Color::rgba(168, 50, 44, 0.97);
const BTN_RED_HOVER: Color = Color::rgba(204, 66, 58, 1.0);
const VEIL: Color = Color::rgba(0, 0, 0, 0.38);
const EDGE: Color = Color::rgba(244, 127, 48, 0.9);
const BADGE: Color = Color::rgba(14, 16, 20, 0.95);
const TEXT: Color = Color::rgba(240, 240, 240, 1.0);
const TEXT_OFF: Color = Color::rgba(240, 240, 240, 0.30);
const TEXT_ON: Color = Color::rgba(20, 20, 20, 1.0);
const DIM: Color = Color::rgba(240, 240, 240, 0.60);

/// One tile: what it does (`id`, see `App::quick_do`), its icon, a small second icon in its
/// corner (what is done to the vehicle), and the words shown over the menu while the mouse is on it.
struct Def {
    id: &'static str,
    icon: &'static str,
    badge: &'static str,
    label: &'static str,
    tip: &'static str,
}

/// What a tile shows now: its own, or - the line-and-tour tile while a timetable is active - the
/// one that ends the timetable.
struct Look {
    icon: &'static str,
    badge: &'static str,
    label: &'static str,
    tip: &'static str,
    cancel: bool,
}

static TILES: [Def; 12] = [
    Def { id: "place", icon: "directions_bus", badge: "add", label: "Place a vehicle", tip: "Place a vehicle of your choice" },
    Def { id: "swap", icon: "directions_bus", badge: "sync_alt", label: "Swap for another vehicle", tip: "Put another vehicle in this one's place and drive it" },
    Def { id: "remove", icon: "directions_bus", badge: "close", label: "Remove this vehicle", tip: "Removes the current vehicle" },
    Def { id: "tplist", icon: "location_on", badge: "", label: "Teleport to a start point", tip: "Teleport to a starting point on the map" },
    Def { id: "duty", icon: "departure_board", badge: "", label: "Line and tour", tip: "Choose the line, tour and stop to drive from" },
    Def { id: "dest", icon: "display_settings", badge: "", label: "Destination display", tip: "Change the current destination" },
    Def { id: "repair", icon: "construction", badge: "", label: "Repair", tip: "Repairs the current vehicle" },
    Def { id: "washfuel", icon: "water_drop", badge: "", label: "Refuel and wash", tip: "Fills the tank and cleans the vehicle at a petrol station or wash yard" },
    Def { id: "arrows", icon: "navigation", badge: "", label: "Route arrows (as in OMSI 2)", tip: "Shows OMSI 2's route arrows over the road" },
    Def { id: "time", icon: "schedule", badge: "", label: "Time", tip: "Change the time of day" },
    Def { id: "weather", icon: "partly_cloudy_day", badge: "", label: "Weather", tip: "Choose the weather" },
    Def { id: "controllers", icon: "gamepad", badge: "", label: "Game controllers", tip: "Configure wheels, pedals, gamepads and force feedback" },
];

pub(crate) struct QuickMenu {
    /// The menu is showing.
    pub(crate) open: bool,
    /// Alt went down with nothing else held and nothing has happened since: letting go of it now
    /// shows or hides the menu (a key, a click or the wheel in between make it a modifier again).
    pub(crate) alt_armed: bool,
    /// The game menu was opened from here, on a list, without pausing: it closes when no list shows.
    pub(crate) session: bool,
    /// The question "end the timetable?" is shown over the menu.
    pub(crate) confirm: bool,
    /// The press of a click that a tile (or the question) took; its release is taken too.
    consumed: bool,
    pressed: Option<usize>,
    hover: Option<usize>,
    rects: Vec<Rect>,
    panel: Rect,
    /// The question's two buttons (yes, no) as last laid out, and the one a press began on.
    yes: Rect,
    no: Rect,
    confirm_pressed: Option<bool>,
    gpu: Option<(Gpu, wgpu::TextureFormat)>,
    fonts: Option<Fonts>,
    atlas: Atlas,
    painter: Painter,
}

impl QuickMenu {
    pub(crate) fn new() -> QuickMenu {
        QuickMenu {
            open: false,
            alt_armed: false,
            session: false,
            confirm: false,
            consumed: false,
            pressed: None,
            hover: None,
            rects: Vec::new(),
            panel: Rect::new(0.0, 0.0, 0.0, 0.0),
            yes: Rect::new(0.0, 0.0, 0.0, 0.0),
            no: Rect::new(0.0, 0.0, 0.0, 0.0),
            confirm_pressed: None,
            gpu: None,
            fonts: None,
            atlas: Atlas::new(1024),
            painter: Painter::new(),
        }
    }

    /// The window's graphics went away: the picture is made again with the next frame.
    pub(crate) fn drop_gpu(&mut self) {
        self.gpu = None;
        self.atlas = Atlas::new(1024);
    }

    fn tile_at(&self, p: Vec2) -> Option<usize> {
        self.rects.iter().position(|r| r.contains(p))
    }

    /// Draw the menu painted last over the frame (`view`, `w` x `h`).
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
        let mut enc = r.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("quick menu") });
        gpu.render(&r.device, &r.queue, &mut enc, view, (w, h), None, &layers, &draws);
        r.queue.submit([enc.finish()]);
    }
}

fn is_alt(code: KeyCode) -> bool {
    matches!(code, KeyCode::AltLeft | KeyCode::AltRight)
}

impl App {
    /// Whether the menu may be shown now: a world is loaded and nothing else holds the keys.
    fn quick_menu_allowed(&self) -> bool {
        self.world.is_some() && self.menu.is_none() && self.game_menu.is_none() && self.chooser.is_none() && self.list_kind.is_none() && self.editor.is_none() && !self.input_away
    }

    /// A key of the window, seen before the game's own handling of it. Alt tapped alone shows and
    /// hides the menu; Esc hides it. True when the key is taken here.
    pub(crate) fn quick_menu_key(&mut self, code: KeyCode, pressed: bool, repeat: bool) -> bool {
        if pressed && !repeat {
            // (the question asked: Enter or Y ends the timetable, Esc or N keeps it)
            if self.quick.confirm {
                match code {
                    KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::KeyY => {
                        self.quick_cancel_duty();
                        return true;
                    }
                    KeyCode::Escape | KeyCode::KeyN => {
                        self.quick.confirm = false;
                        return true;
                    }
                    _ => {}
                }
            }
            if code == KeyCode::Escape && self.quick.open {
                self.quick.open = false;
                return true;
            }
            // (armed only by an Alt that went down with no other key held: `self.keys` has it already)
            self.quick.alt_armed = is_alt(code) && self.keys.iter().all(|k| is_alt(*k));
        } else if !pressed && is_alt(code) {
            let still = self.keys.contains(&KeyCode::AltLeft) || self.keys.contains(&KeyCode::AltRight);
            if self.quick.alt_armed && !still {
                self.quick.alt_armed = false;
                self.quick.confirm = false;
                if self.quick.open {
                    self.quick.open = false;
                } else if self.quick_menu_allowed() {
                    self.quick.open = true;
                    self.quick.pressed = None;
                    self.quick.consumed = false;
                }
            }
        }
        false
    }

    /// The left button over the picture: a click on the menu's tiles is theirs (true: taken).
    /// A click anywhere else goes on to the game as it always did.
    pub(crate) fn quick_menu_click(&mut self, pressed: bool) -> bool {
        if pressed {
            self.quick.alt_armed = false;
        }
        if !self.quick.open {
            return false;
        }
        let p = Vec2::new(self.cursor.0, self.cursor.1);
        if self.quick.confirm {
            // (the question asked: its buttons are all the mouse does now; any other click is
            // taken too, so that a click meant for it does not reach the game)
            let at = |q: &QuickMenu| if q.yes.contains(p) { Some(true) } else if q.no.contains(p) { Some(false) } else { None };
            if pressed {
                self.quick.consumed = true;
                self.quick.confirm_pressed = at(&self.quick);
                return true;
            }
            if !self.quick.consumed {
                return false;
            }
            self.quick.consumed = false;
            let began = self.quick.confirm_pressed.take();
            if began.is_some() && began == at(&self.quick) {
                if began == Some(true) {
                    self.quick_cancel_duty();
                } else {
                    self.quick.confirm = false;
                }
            }
            return true;
        }
        if pressed {
            if self.quick.panel.contains(p) {
                self.quick.consumed = true;
                self.quick.pressed = self.quick.tile_at(p);
                return true;
            }
            return false;
        }
        if !self.quick.consumed {
            return false;
        }
        self.quick.consumed = false;
        if let Some(i) = self.quick.pressed.take() {
            if self.quick.tile_at(p) == Some(i) {
                self.quick_do(i);
            }
        }
        true
    }

    /// Once a frame: the game menu opened from here closes when its list is done.
    pub(crate) fn quick_menu_frame(&mut self) {
        if self.game_menu.is_none() {
            self.quick.session = false;
        } else if self.quick.session && self.list_kind.is_none() && self.chooser.is_none() {
            self.quick.session = false;
            self.close_game_menu();
        }
    }

    /// Whether the tile `id` has nothing to act on now (shown faded, a click does nothing).
    fn quick_off(&self, id: &str) -> bool {
        let has = self.player.is_some();
        match id {
            "swap" | "remove" | "dest" | "repair" | "washfuel" => !has,
            "tplist" => !has || self.navigator.is_none() || crate::input_script::on_server(&self.args),
            "time" => self.lan.as_ref().is_some_and(|l| l.role == omsi_net::Role::Client),
            _ => false,
        }
    }

    /// The game menu, opened on a list from here: not paused (as in a LAN session), the keys
    /// the menu's own while it is open.
    fn quick_open_session(&mut self) {
        self.menu_prev_pause = self.paused;
        self.release_vehicle_keys();
        self.game_menu = Some(0);
        self.menu_top = None;
        self.menu_kbd = true;
        self.menu_drag = None;
        self.quick.session = true;
    }

    /// The timetable ends: free drive, as "Free drive" in the list of lines does it (no duty, the
    /// timetable's answers to the bus empty again).
    fn quick_cancel_duty(&mut self) {
        self.quick.confirm = false;
        let _ = crate::game_lists::run(self, &crate::game_lists::ListKind::Lines, "free");
    }

    /// What tile `def` shows now.
    fn quick_look(&self, def: &Def) -> Look {
        if def.id == "duty" && self.duty.is_some() {
            Look { icon: "departure_board", badge: "close", label: "Cancel the timetable", tip: "Ends the active timetable and returns to free driving", cancel: true }
        } else {
            Look { icon: def.icon, badge: def.badge, label: def.label, tip: def.tip, cancel: false }
        }
    }

    /// Do what tile `i` stands for.
    fn quick_do(&mut self, i: usize) {
        let Some(def) = TILES.get(i) else { return };
        if self.quick_off(def.id) {
            return;
        }
        match def.id {
            // what is chosen from a list: the menu goes, the list comes, the game runs on
            "place" | "swap" | "dest" | "tplist" => {
                self.quick.open = false;
                self.quick_open_session();
                self.page_action(def.id);
            }
            "duty" => {
                // a timetable is active: this tile ends it - after a question
                if self.duty.is_some() {
                    self.quick.confirm = true;
                    self.quick.confirm_pressed = None;
                    return;
                }
                self.quick.open = false;
                self.quick_open_session();
                self.open_list(crate::game_lists::ListKind::Lines);
            }
            "time" | "weather" => {
                self.quick.open = false;
                self.quick_open_session();
                let tab = crate::game_lists::world_tab(self, def.label);
                self.open_list(crate::game_lists::ListKind::World(tab));
            }
            "controllers" => {
                self.quick.open = false;
                self.quick_open_session();
                self.open_list(crate::game_lists::ListKind::ControllerDevices(0));
            }
            // at once, and the menu stays for the next one (`page_action` closes the game menu
            // and gives the pause back: it is what it was, so nothing changes)
            "remove" | "repair" => {
                self.menu_prev_pause = self.paused;
                self.page_action(def.id);
            }
            "washfuel" => self.run_service("washfuel"),
            "arrows" => {
                crate::game_lists::flip_switch(self, "nav_arrows");
            }
            _ => {}
        }
    }

    /// Lay the menu out for a picture of `w` x `h` physical pixels and paint it (drawn by
    /// `QuickMenu::render` once the game's picture is in the frame).
    pub(crate) fn quick_prepare(&mut self, w: u32, h: u32) {
        if !self.quick.open || self.game_menu.is_some() {
            self.quick.open = self.quick.open && self.game_menu.is_none();
            self.quick.confirm = false;
            self.quick.painter.clear();
            self.quick.rects.clear();
            self.quick.hover = None;
            return;
        }
        let (wf, hf) = (w as f32, h as f32);
        let dpi = self.window.as_ref().map(|win| win.scale_factor() as f32).unwrap_or(1.0).max(0.5);
        let u = dpi * ((hf / dpi) / 1000.0).clamp(0.8, 1.5);
        let (tile, gap, pad) = (62.0 * u, 8.0 * u, 18.0 * u);
        let rows = TILES.len().div_ceil(COLS);
        let pw = COLS as f32 * tile + (COLS as f32 + 1.0) * gap;
        let ph = rows as f32 * tile + (rows as f32 + 1.0) * gap;
        let panel = Rect::new(wf - pad - pw, hf - pad - ph, pw, ph);
        let rects: Vec<Rect> = (0..TILES.len())
            .map(|k| Rect::new(panel.x + gap + (k % COLS) as f32 * (tile + gap), panel.y + gap + (k / COLS) as f32 * (tile + gap), tile, tile))
            .collect();
        let cursor = Vec2::new(self.cursor.0, self.cursor.1);
        let off: Vec<bool> = TILES.iter().map(|t| self.quick_off(t.id)).collect();
        let lit: Vec<bool> = TILES.iter().map(|t| t.id == "arrows" && crate::game_lists::switch_on(self, "nav_arrows")).collect();
        let looks: Vec<Look> = TILES.iter().map(|t| self.quick_look(t)).collect();
        let asking = self.quick.confirm;
        let q = &mut self.quick;
        q.panel = panel;
        q.rects = rects.clone();
        q.hover = q.tile_at(cursor);
        let hover = q.hover;
        let fonts = q.fonts.get_or_insert_with(Fonts::new);
        let pt = &mut q.painter;
        pt.clear();
        q.atlas.begin_frame();
        let atlas = &mut q.atlas;
        pt.rounded(panel, 14.0 * u, PANEL);
        for i in 0..TILES.len() {
            let r = rects[i];
            let look = &looks[i];
            let bg = if lit[i] {
                TILE_ON
            } else if off[i] {
                TILE_OFF
            } else if look.cancel {
                if hover == Some(i) { TILE_CANCEL_HOVER } else { TILE_CANCEL }
            } else if hover == Some(i) {
                TILE_HOVER
            } else {
                TILE
            };
            let fg = if lit[i] {
                TEXT_ON
            } else if off[i] {
                TEXT_OFF
            } else {
                TEXT
            };
            pt.rounded(r, 10.0 * u, bg);
            if hover == Some(i) && !off[i] {
                pt.rounded_border(r, 10.0 * u, 2.0 * u, EDGE);
            }
            pt.icon(atlas, look.icon, r.center(), 34.0 * u, fg);
            if !look.badge.is_empty() {
                let b = Vec2::new(r.right() - 14.0 * u, r.bottom() - 14.0 * u);
                pt.circle(b, 10.0 * u, if look.cancel { BADGE_CANCEL } else { BADGE });
                pt.icon(atlas, look.badge, b, 15.0 * u, if off[i] { TEXT_OFF } else { TEXT });
            }
        }
        // what the tile under the mouse is, over the menu
        if let (Some(i), false) = (hover, asking) {
            let look = &looks[i];
            let label = omsi_ui::tr(look.label).to_string();
            let tip = omsi_ui::tr(look.tip).to_string();
            let (ls, ts) = (14.0 * u, 12.0 * u);
            let tw = fonts.width(&label, ls, Weight::Bold).max(fonts.width(&tip, ts, Weight::Regular));
            let (bw, bh) = (tw + 28.0 * u, 52.0 * u);
            let br = Rect::new(panel.right() - bw, panel.y - bh - 8.0 * u, bw, bh);
            pt.rounded(br, 10.0 * u, PANEL);
            pt.text_in(atlas, fonts, &label, ls, Weight::Bold, Rect::new(br.x, br.y + 6.0 * u, br.w, 22.0 * u), Align::Center, TEXT);
            pt.text_in(atlas, fonts, &tip, ts, Weight::Regular, Rect::new(br.x, br.y + 28.0 * u, br.w, 18.0 * u), Align::Center, DIM);
        }
        // the question "end the timetable?" over everything, the picture dimmed behind it
        if asking {
            let title = omsi_ui::tr("You have an active timetable").to_string();
            let body = omsi_ui::tr("Do you want to cancel it and drive without one?").to_string();
            let yes_text = omsi_ui::tr("Cancel the timetable").to_string();
            let no_text = omsi_ui::tr("Keep it").to_string();
            let (ts, bs, gs) = (17.0 * u, 13.0 * u, 14.0 * u);
            let tw = fonts.width(&title, ts, Weight::Bold).max(fonts.width(&body, bs, Weight::Regular));
            let btw = fonts.width(&yes_text, gs, Weight::Bold).max(fonts.width(&no_text, gs, Weight::Bold)) + 36.0 * u;
            let pw2 = tw.max(btw * 2.0 + 12.0 * u).max(320.0 * u) + 48.0 * u;
            let ph2 = 150.0 * u;
            let ask = Rect::new((wf - pw2) * 0.5, (hf - ph2) * 0.5, pw2, ph2);
            let bw = (pw2 - 48.0 * u - 12.0 * u) * 0.5;
            let by = ask.bottom() - 24.0 * u - 40.0 * u;
            let yes = Rect::new(ask.x + 24.0 * u, by, bw, 40.0 * u);
            let no = Rect::new(yes.right() + 12.0 * u, by, bw, 40.0 * u);
            q.yes = yes;
            q.no = no;
            let on = if yes.contains(cursor) { Some(true) } else if no.contains(cursor) { Some(false) } else { None };
            pt.rect(Rect::new(0.0, 0.0, wf, hf), VEIL);
            pt.rounded(ask, 14.0 * u, PANEL);
            pt.rounded_border(ask, 14.0 * u, 2.0 * u, EDGE);
            pt.text_in(atlas, fonts, &title, ts, Weight::Bold, Rect::new(ask.x, ask.y + 18.0 * u, ask.w, 26.0 * u), Align::Center, TEXT);
            pt.text_in(atlas, fonts, &body, bs, Weight::Regular, Rect::new(ask.x, ask.y + 50.0 * u, ask.w, 22.0 * u), Align::Center, DIM);
            pt.rounded(yes, 10.0 * u, if on == Some(true) { BTN_RED_HOVER } else { BTN_RED });
            pt.text_in(atlas, fonts, &yes_text, gs, Weight::Bold, yes, Align::Center, TEXT);
            pt.rounded(no, 10.0 * u, if on == Some(false) { TILE_HOVER } else { TILE });
            pt.text_in(atlas, fonts, &no_text, gs, Weight::Bold, no, Align::Center, TEXT);
        }
    }
}
