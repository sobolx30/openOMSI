//! The editor of the driver's view and the mirrors' angles: a translucent panel on the right
//! of the screen with sliders, so that the picture stays in sight while it is adjusted.
//!
//! Opened from the game menu (Esc -> Camera... -> *Edit the driver's view and mirror angles*);
//! Esc or *Done* closes it. What is set is kept for the vehicle alone: the eye's shift and the
//! extra turn of the driver's view in `driverview.cfg` (`settings::driver_view`), the turn of
//! each mirror in `mirrors.cfg` (`settings::mirror_offsets`, the same turn Ctrl+Alt+arrows
//! makes). Another vehicle is not touched.

use crate::player::Player;
use crate::ui::{EditorRow, EditorView};
use crate::App;

const FWD: u8 = 1;
const UP: u8 = 2;
const SIDE: u8 = 3;
const YAW: u8 = 4;
const PITCH: u8 = 5;
const FOV: u8 = 6;
const MIRROR: u8 = 20;
const M_YAW: u8 = 21;
const M_PITCH: u8 = 22;
const RESET_VIEW: u8 = 30;
const RESET_MIRROR: u8 = 31;
const DONE: u8 = 32;
const PLACE_YES: u8 = 40;
const PLACE_NO: u8 = 41;

/// (smallest, largest, step) of a slider.
fn spec(id: u8) -> Option<(f32, f32, f32)> {
    Some(match id {
        FWD | UP | SIDE => (-1.0, 1.0, 0.005),
        YAW => (-90.0, 90.0, 1.0),
        PITCH => (-60.0, 60.0, 1.0),
        FOV => (-40.0, 40.0, 1.0),
        M_YAW => (-45.0, 45.0, 0.5),
        M_PITCH => (-30.0, 30.0, 0.5),
        _ => return None,
    })
}

#[derive(Default)]
pub(crate) struct ViewEditor {
    pub open: bool,
    /// The mirror the two lower sliders turn.
    pub mirror: usize,
    /// The slider being dragged.
    drag: Option<u8>,
    /// A place on the ground clicked in the map camera (F4), waiting for the answer to
    /// "move the vehicle here?".
    pub place: Option<glam::DVec3>,
}

impl ViewEditor {
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    fn mirrors(p: &Player) -> usize {
        p.vehicle.ty.def.cameras_reflexion.len()
    }

    fn value(&self, id: u8, p: &Player) -> f32 {
        let o = p.mirror_offsets.get(self.mirror).copied().unwrap_or([0.0; 2]);
        match id {
            FWD => p.view_adj[1],
            UP => p.view_adj[2],
            SIDE => p.view_adj[0],
            YAW => p.view_adj[3],
            PITCH => p.view_adj[4],
            FOV => p.view_adj[5],
            M_YAW => o[0],
            M_PITCH => o[1],
            _ => 0.0,
        }
    }

    fn set(&mut self, id: u8, v: f32, p: &mut Player) {
        match id {
            FWD => p.view_adj[1] = v,
            UP => p.view_adj[2] = v,
            SIDE => p.view_adj[0] = v,
            YAW => p.view_adj[3] = v,
            PITCH => p.view_adj[4] = v,
            FOV => p.view_adj[5] = v,
            M_YAW | M_PITCH => {
                let n = Self::mirrors(p);
                if p.mirror_offsets.len() < n {
                    p.mirror_offsets.resize(n, [0.0; 2]);
                }
                if let Some(o) = p.mirror_offsets.get_mut(self.mirror) {
                    o[(id == M_PITCH) as usize] = v;
                    p.mirrors_dirty = true;
                }
            }
            _ => {}
        }
    }

    /// The panel's rows for this vehicle as it is now.
    pub fn view(&self, p: &Player) -> EditorView {
        let cm = |v: f32| format!("{:+.1} cm", v * 100.0);
        let slider = |id: u8, label: &str, text: String| {
            let (lo, hi, _) = spec(id).unwrap_or((0.0, 1.0, 1.0));
            EditorRow::Slider { id, label: label.to_string(), text, frac: (self.value(id, p) - lo) / (hi - lo) }
        };
        let mut rows = vec![
            EditorRow::Header("Driver's view of this vehicle".into()),
            slider(FWD, "Eye forward and back", cm(self.value(FWD, p))),
            slider(UP, "Eye up and down", cm(self.value(UP, p))),
            slider(SIDE, "Eye left and right", cm(self.value(SIDE, p))),
            slider(YAW, "View turn", format!("{:+.0}°", self.value(YAW, p))),
            slider(PITCH, "View tilt", format!("{:+.0}°", self.value(PITCH, p))),
            slider(FOV, "Field of view", format!("{:+.0}°", self.value(FOV, p))),
        ];
        let n = Self::mirrors(p);
        if n > 0 {
            let at = self.mirror % n;
            let x = p.vehicle.ty.def.cameras_reflexion.get(at).map(|c| c.pos[0]).unwrap_or(0.0);
            let side = if x > 0.3 { " (right)" } else if x < -0.3 { " (left)" } else { "" };
            rows.push(EditorRow::Header("Mirror angles of this vehicle".into()));
            rows.push(EditorRow::Picker { id: MIRROR, label: "Mirror".into(), text: format!("{} / {}{}", at + 1, n, side) });
            rows.push(slider(M_YAW, "Turn across", format!("{:+.1}°", self.value(M_YAW, p))));
            rows.push(slider(M_PITCH, "Turn up and down", format!("{:+.1}°", self.value(M_PITCH, p))));
        }
        let mut buttons = vec![(RESET_VIEW, "Reset view".to_string())];
        if n > 0 {
            buttons.push((RESET_MIRROR, "Reset mirror".to_string()));
        }
        buttons.push((DONE, "Done".to_string()));
        rows.push(EditorRow::Buttons(buttons));
        EditorView { title: "View editor".into(), rows, centered: false }
    }
}

impl App {
    /// The editor is open and can be worked (not behind the game menu).
    pub(crate) fn view_editor_active(&self) -> bool {
        self.view_editor.open && self.game_menu.is_none() && self.player.is_some()
    }

    /// The question "move the vehicle here?" is up.
    pub(crate) fn place_question_active(&self) -> bool {
        self.view_editor.place.is_some() && self.game_menu.is_none() && self.player.is_some() && self.view == "free"
    }

    /// The panel's rows for the frame being drawn.
    pub(crate) fn view_editor_frame(&self) -> Option<EditorView> {
        if self.place_question_active() {
            return Some(EditorView {
                title: "Move the vehicle".into(),
                rows: vec![
                    EditorRow::Header("Do you want to move the vehicle here?".into()),
                    EditorRow::Buttons(vec![(PLACE_YES, "Yes, move it".to_string()), (PLACE_NO, "No".to_string())]),
                ],
                centered: true,
            });
        }
        if !self.view_editor_active() {
            return None;
        }
        self.player.as_ref().map(|p| self.view_editor.view(p))
    }

    fn view_editor_inside(&self) -> bool {
        let (x, y) = self.cursor;
        self.ui.as_ref().and_then(|u| u.edit_panel).is_some_and(|r| x >= r[0] && x <= r[2] && y >= r[1] && y <= r[3])
    }

    /// The cursor is over the panel (the wheel must not zoom the view there).
    pub(crate) fn view_editor_over(&self) -> bool {
        (self.view_editor_active() || self.place_question_active()) && self.view_editor_inside()
    }

    /// A click on the ground in the map camera: ask whether the vehicle should be moved
    /// there (as Omsi's map view does). Anywhere, not only on a street.
    pub(crate) fn ask_place_vehicle(&mut self, at: glam::DVec3) {
        if self.lan.as_ref().is_some_and(|l| l.role == omsi_net::Role::Client) {
            self.service_msg = Some(("In a LAN session only the host moves vehicles on the map".into(), 4.0));
            return;
        }
        self.view_editor.place = Some(at);
    }

    /// Esc, or a click beside the question: no.
    pub(crate) fn cancel_place_question(&mut self) -> bool {
        self.view_editor.place.take().is_some()
    }

    /// Enter: yes.
    pub(crate) fn accept_place_question(&mut self) -> bool {
        match self.view_editor.place.take() {
            Some(at) => {
                if self.place_question_ok() {
                    let heading = self.player.as_ref().map(|p| p.vehicle.heading).unwrap_or(0.0);
                    crate::admin::teleport(self, at, heading);
                    self.service_msg = Some(("The vehicle stands where you clicked".into(), 3.0));
                }
                true
            }
            None => false,
        }
    }

    fn place_question_ok(&self) -> bool {
        self.player.is_some() && self.game_menu.is_none()
    }

    /// Set slider `id` to where the cursor is along its track.
    fn view_editor_drag_to(&mut self, id: u8) {
        let Some((lo, hi, step)) = spec(id) else { return };
        let Some(rect) = self.ui.as_ref().and_then(|u| u.edit_hits.iter().find(|h| h.0 == id && h.1 == 0).map(|h| h.2)) else { return };
        let frac = ((self.cursor.0 - rect[0]) / (rect[2] - rect[0]).max(1.0)).clamp(0.0, 1.0);
        let v = (((lo + frac * (hi - lo)) / step).round() * step).clamp(lo, hi);
        // (no "-0")
        let v = if v.abs() < step * 0.25 { 0.0 } else { v };
        let mut editor = std::mem::take(&mut self.view_editor);
        if let Some(p) = self.player.as_mut() {
            editor.set(id, v, p);
        }
        self.view_editor = editor;
    }

    fn view_editor_save(&mut self) {
        if let Some(p) = self.player.as_ref() {
            crate::settings::save_driver_view(&p.vehicle.ty.def.path, &p.view_adj);
        }
    }

    /// Close the panel, keeping what was set.
    pub(crate) fn view_editor_close(&mut self) {
        self.view_editor.open = false;
        self.view_editor.drag = None;
        self.view_editor_save();
    }

    /// The cursor moved: a slider being dragged follows. True when it did.
    pub(crate) fn view_editor_moved(&mut self) -> bool {
        match self.view_editor.drag {
            Some(id) if self.view_editor_active() => {
                self.view_editor_drag_to(id);
                true
            }
            _ => false,
        }
    }

    /// The left button: a click on the panel's controls works them (true: the click is
    /// taken - anywhere on the panel it is, so that it never reaches the cab behind). A
    /// release always ends a drag and keeps what was set.
    pub(crate) fn view_editor_button(&mut self, pressed: bool) -> bool {
        // the question: yes, no, or a click beside it (no); nothing reaches the game behind
        if self.place_question_active() {
            if pressed {
                let (x, y) = self.cursor;
                let hit = self.ui.as_ref().and_then(|u| u.edit_hits.iter().find(|h| x >= h.2[0] && x <= h.2[2] && y >= h.2[1] && y <= h.2[3]).map(|h| h.0));
                match hit {
                    Some(PLACE_YES) => {
                        self.accept_place_question();
                    }
                    Some(_) => {}
                    None => {
                        self.cancel_place_question();
                    }
                }
                if hit == Some(PLACE_NO) {
                    self.cancel_place_question();
                }
            }
            return true;
        }
        if !pressed {
            if self.view_editor.drag.take().is_some() {
                self.view_editor_save();
                return true;
            }
            return false;
        }
        if !self.view_editor_active() || !self.view_editor_inside() {
            return false;
        }
        let (x, y) = self.cursor;
        let hit = self.ui.as_ref().and_then(|u| u.edit_hits.iter().find(|h| x >= h.2[0] && x <= h.2[2] && y >= h.2[1] && y <= h.2[3]).map(|h| (h.0, h.1)));
        let Some((id, part)) = hit else { return true };
        if spec(id).is_some() {
            self.view_editor.drag = Some(id);
            self.view_editor_drag_to(id);
            return true;
        }
        let n = self.player.as_ref().map(|p| ViewEditor::mirrors(p)).unwrap_or(0);
        match id {
            MIRROR if n > 0 => {
                let at = self.view_editor.mirror % n;
                self.view_editor.mirror = if part == 1 { (at + n - 1) % n } else { (at + 1) % n };
            }
            RESET_VIEW => {
                if let Some(p) = self.player.as_mut() {
                    p.view_adj = [0.0; 6];
                }
                self.view_editor_save();
            }
            RESET_MIRROR => {
                let at = self.view_editor.mirror;
                if let Some(p) = self.player.as_mut() {
                    if let Some(o) = p.mirror_offsets.get_mut(at) {
                        *o = [0.0; 2];
                        p.mirrors_dirty = true;
                    }
                }
            }
            DONE => self.view_editor_close(),
            _ => {}
        }
        true
    }

    /// Open the panel (from the game menu).
    pub(crate) fn view_editor_open(&mut self) {
        if self.player.is_none() {
            return;
        }
        self.chooser = None;
        self.admin_list = None;
        self.list_kind = None;
        self.dropdown = None;
        self.menu_edit = None;
        self.view = "driver".into();
        if self.game_menu.is_some() {
            self.close_game_menu();
        }
        self.view_editor.open = true;
        self.view_editor.mirror = 0;
        self.view_editor.drag = None;
    }
}
