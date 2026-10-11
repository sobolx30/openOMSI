//! [ROLLBACK ailist-73] The AI list page: the buses a map's depots have on the road, from its
//! `ailists.cfg` - which types of bus there are, how many of each, and which are switched off.
//! The file is edited as text (`omsi_map::ailists::edit`); one in the content folder is changed
//! in place (the original kept once as `ailists.cfg.orig`), a map of the original installation
//! gets a copy in the content folder, which the game reads before the original.

use super::drive;
use super::theme::*;
use super::ui::{ButtonKind, Ui};
use super::Launcher;
use glam::Vec2;
use omsi_launcher_lib as core;
use omsi_map::ailists::edit;
use omsi_ui::paint::Align;
use omsi_ui::{Rect, Weight};
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct AiView {
    pub map: usize,
    /// The map the file was read for (its `global.cfg`).
    loaded: Option<String>,
    map_dir: PathBuf,
    bytes: Vec<u8>,
    /// The depot group shown.
    depot: usize,
    /// [ROLLBACK ailist-75] The bus to add as Drive picks one: manufacturer, model (an index
    /// in the manufacturer's list), livery (0 the bus's own), fleet number and plate - and the
    /// bus file those were last filled in for.
    maker: usize,
    model: usize,
    paint: usize,
    number: String,
    plate: String,
    picked: String,
    makers: Vec<drive::BusManufacturer>,
    makers_n: usize,
    /// Changed and not saved yet.
    dirty: bool,
    /// The reset button was pressed once.
    reset_armed: bool,
}

/// The map's `ailists.cfg` as the game reads it (a copy in the content folder first).
fn source(map_dir: &Path) -> PathBuf {
    omsi_cfg::resolve_path(map_dir, "ailists.cfg")
}

/// Where the edited file goes: in place when the map lies unpacked in the content folder (its
/// file kept once as `.orig`), else a copy in the content folder's `maps/<folder>`.
fn save_target(map_dir: &Path) -> Result<PathBuf, String> {
    let content = core::content_dir().ok_or("no content folder")?;
    let src = source(map_dir);
    if src.starts_with(&content) && omsi_cfg::vfs::archive_of(&src).is_none() {
        let orig = PathBuf::from(format!("{}.orig", src.display()));
        if src.is_file() && !orig.exists() {
            std::fs::copy(&src, &orig).map_err(|e| e.to_string())?;
        }
        return Ok(src);
    }
    let folder = map_dir.file_name().map(|f| f.to_string_lossy().into_owned()).ok_or("no map folder")?;
    let dir = content.join("maps").join(folder);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("ailists.cfg"))
}

/// The map's own list back: the copy in the content folder deleted, or the `.orig` put back.
fn reset(map_dir: &Path) -> Result<String, String> {
    let content = core::content_dir().ok_or("no content folder")?;
    let folder = map_dir.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let copy = content.join("maps").join(&folder).join("ailists.cfg");
    if !map_dir.starts_with(&content) && copy.is_file() {
        std::fs::remove_file(&copy).map_err(|e| e.to_string())?;
        return Ok(format!("The AI list of {folder} is the map's own again (the edited copy was removed)"));
    }
    let src = source(map_dir);
    let orig = PathBuf::from(format!("{}.orig", src.display()));
    if orig.is_file() {
        std::fs::copy(&orig, &src).map_err(|e| e.to_string())?;
        std::fs::remove_file(&orig).map_err(|e| e.to_string())?;
        return Ok(format!("The AI list of {folder} put back as it was"));
    }
    Err(format!("The AI list of {folder} has not been changed here"))
}

fn norm(s: &str) -> String {
    s.replace('\\', "/").trim().to_lowercase()
}

/// The entry lines the depot's `_2` blocks of vehicle file `file` already have.
fn entries_of(bytes: &[u8], depot: &str, file: &str) -> Vec<String> {
    edit::typ_blocks(bytes).into_iter().filter(|b| b.depot == depot && b.v2 && norm(&b.file) == norm(file)).flat_map(|b| b.entries).collect()
}

/// A text for a field of an entry line: no tabs and no line breaks.
fn field(s: &str) -> String {
    s.chars().map(|c| if c == '\t' || c == '\r' || c == '\n' { ' ' } else { c }).collect::<String>().trim().to_string()
}

pub fn draw(l: &mut Launcher, area: Rect) {
    let body = l.page_title(area, "AI list", "The buses a map's depots put on the road (ailists.cfg): which types, how many of each. Saved in the content folder; OMSI 2's own files stay as they are.");
    let maps: Vec<(String, String)> = l.state.maps.iter().map(|m| (m.friendly.clone(), m.file.clone())).collect();
    if maps.is_empty() {
        l.ui.paragraph("No maps found.", Vec2::new(body.x, body.y), body.w, 14.0, Weight::Regular, TEXT_DIM);
        return;
    }
    {
        let av = &mut l.pages.ai;
        if av.loaded.is_none() {
            if let Some(k) = maps.iter().position(|m| m.1 == l.state.choice.map) {
                av.map = k;
            }
        }
        av.map = av.map.min(maps.len() - 1);
        if av.loaded.as_deref() != Some(maps[av.map].1.as_str()) {
            let file = &maps[av.map].1;
            let root = PathBuf::from(&l.state.config.root);
            let global = core::content_dir().map(|c| c.join(file)).filter(|p| p.is_file()).unwrap_or_else(|| omsi_cfg::resolve_path(&root, file));
            av.map_dir = global.parent().map(Path::to_path_buf).unwrap_or_default();
            av.bytes = omsi_cfg::vfs::read(&source(&av.map_dir)).unwrap_or_default();
            av.loaded = Some(file.clone());
            av.depot = 0;
            av.picked.clear();
            av.dirty = false;
            av.reset_armed = false;
        }
        // the manufacturers, as Drive lists them (again when the bus list changed)
        let n = l.state.vehicles.len();
        if av.makers_n != n || (av.makers.is_empty() && n > 0) {
            av.makers = drive::build_bus_manufacturers(&l.state.vehicles, None, &std::collections::HashSet::new());
            av.makers_n = n;
            // (the first time: on the bus the Drive page has chosen)
            if let Some((mk, md)) = av.makers.iter().enumerate().find_map(|(i, m)| m.variants.iter().position(|v| v.file == l.state.choice.bus).map(|k| (i, k))) {
                av.maker = mk;
                av.model = md;
            }
            av.picked.clear();
        }
    }
    // the page: the picking column on the left, the bus and the depot's buses on the right
    let col1 = (body.w * 0.36).clamp(300.0, 420.0);
    let left = Rect::new(body.x, body.y, col1, body.h);
    let right = Rect::new(left.right() + GAP * 2.0, body.y, body.right() - left.right() - GAP * 2.0, body.h);
    let ph = (right.h * 0.42).clamp(170.0, 380.0);
    let view = Rect::new(right.x, right.y, right.w, ph);
    let list_r = Rect::new(right.x, right.y + ph + GAP, right.w, (right.h - ph - GAP).max(120.0));
    // [ROLLBACK ailist-76] (the picture is not a panel: `preview` marks its rect as interface,
    // and the mouse dragged on interface does not turn the bus)
    let over = l.ui.over_ui;
    l.preview(view);
    l.ui.over_ui = over;
    l.ui.p().rounded_border(view, RADIUS, 1.0, EDGE);

    let av = &mut l.pages.ai;
    let vehicles: Vec<(String, String)> = l.state.vehicles.iter().map(|v| (v.name.clone(), v.file.clone())).collect();
    let depots = edit::depots(&av.bytes);
    av.depot = av.depot.min(depots.len().saturating_sub(1));
    let depot = depots.get(av.depot).cloned().unwrap_or_default();
    av.maker = av.maker.min(av.makers.len().saturating_sub(1));
    if let Some(m) = av.makers.get(av.maker) {
        av.model = av.model.min(m.variants.len().saturating_sub(1));
    }
    // the bus chosen, and its numbers and plate filled in once for it
    let file = av.makers.get(av.maker).and_then(|m| m.variants.get(av.model)).map(|v| v.file.clone()).unwrap_or_default();
    let veh = l.state.vehicles.iter().find(|v| v.file == file).cloned();
    if !file.is_empty() && av.picked != file {
        av.picked = file.clone();
        av.paint = 0;
        let line = edit::next_entry(&entries_of(&av.bytes, &depot, &file), veh.as_ref().map(|v| v.numbers.as_slice()).unwrap_or(&[]));
        let mut f = line.split('\t');
        av.number = f.next().unwrap_or("").to_string();
        av.plate = f.next().unwrap_or("").to_string();
    }
    let paints: Vec<String> = veh.as_ref().map(|v| std::iter::once(drive::default_livery_label(v).to_string()).chain(v.paints.iter().cloned()).collect()).unwrap_or_default();
    av.paint = av.paint.min(paints.len().saturating_sub(1));
    let paint_name = if av.paint == 0 { String::new() } else { paints.get(av.paint).cloned().unwrap_or_default() };
    l.preview_other = if file.is_empty() { None } else { Some((file.clone(), paint_name.clone())) };

    let mut status: Option<(String, bool)> = None;
    let ui: &mut Ui = &mut l.ui;

    // --- the map, the depot, the bus to add, save and reset
    ui.panel(left);
    let inner = ui.heading(Rect::new(left.x + 18.0, left.y + 14.0, left.w - 36.0, left.h - 28.0), "Map", Some("map"));
    let names: Vec<String> = maps.iter().map(|m| m.0.clone()).collect();
    let mut m = av.map;
    if ui.select("ai-map", Rect::new(inner.x, inner.y, inner.w, ROW), &mut m, &names) {
        av.map = m;
        av.loaded = None;
        return;
    }
    let by = inner.bottom() - ROW;
    let mid = Rect::new(inner.x - 4.0, inner.y + ROW + 10.0, inner.w + 8.0, (by - ROW - 8.0 - 10.0 - (inner.y + ROW + 10.0)).max(80.0));
    let mut add = false;
    if depots.is_empty() {
        ui.paragraph("This map has no depot group in its ailists.cfg: no timetable buses of its own.", Vec2::new(inner.x, mid.y), inner.w, 13.0, Weight::Regular, TEXT_DIM);
    } else {
        let (mut d, mut mk, mut md, mut pt) = (av.depot, av.maker, av.model, av.paint);
        let (mut number, mut plate) = (av.number.clone(), av.plate.clone());
        let makers = &av.makers;
        let maker_names: Vec<String> = makers.iter().map(|m| m.name.clone()).collect();
        let model_names: Vec<String> = makers.get(mk).map(|m| m.variants.iter().map(|v| format!("{} · {}", v.variant, drive::liveries_text(v.paints))).collect()).unwrap_or_default();
        let numbers: Vec<(String, String)> = veh.as_ref().map(|v| v.numbers.clone()).unwrap_or_default();
        let number_names: Vec<String> = numbers.iter().map(|(n, p)| if p.trim().is_empty() { n.clone() } else { format!("{n}  ({})", p.trim()) }).collect();
        let mut num_sel = numbers.iter().position(|(n, _)| *n == number).unwrap_or(0);
        let mut picked_num = false;
        let mut reseed = false;
        ui.scroll_area("ai-left", mid, &mut |ui, v| {
            let x = v.x + 4.0;
            let w = v.w - 16.0;
            let mut y = v.y;
            ui.text_in("Depot", Rect::new(x, y, w, 18.0), 11.5, Weight::Bold, TEXT_DIM, Align::Left);
            y += 20.0;
            if ui.select("ai-depot", Rect::new(x, y, w, ROW), &mut d, &depots) {
                reseed = true;
            }
            y += ROW + 16.0;
            ui.text_in("Add a bus", Rect::new(x, y, w, 20.0), 12.5, Weight::Bold, TEXT, Align::Left);
            y += 26.0;
            if makers.is_empty() {
                ui.paragraph("No buses found.", Vec2::new(x, y), w, 13.0, Weight::Regular, TEXT_DIM);
                return y - v.y + 30.0;
            }
            ui.text_in("Manufacturer", Rect::new(x, y, w, 18.0), 11.5, Weight::Bold, TEXT_DIM, Align::Left);
            y += 20.0;
            if ui.select("ai-maker", Rect::new(x, y, w, ROW), &mut mk, &maker_names) {
                md = 0;
            }
            y += ROW + 10.0;
            ui.text_in("Model", Rect::new(x, y, w, 18.0), 11.5, Weight::Bold, TEXT_DIM, Align::Left);
            y += 20.0;
            ui.select("ai-model", Rect::new(x, y, w, ROW), &mut md, &model_names);
            y += ROW + 10.0;
            if !paints.is_empty() {
                ui.text_in("Livery", Rect::new(x, y, w, 18.0), 11.5, Weight::Bold, TEXT_DIM, Align::Left);
                if paints.len() > 1 {
                    ui.text_in(&format!("{} / {}", pt + 1, paints.len()), Rect::new(x + w - 70.0, y, 70.0, 18.0), 11.5, Weight::Regular, TEXT_DIM, Align::Right);
                }
                y += 20.0;
                let arrows = paints.len() > 1;
                let sel = Rect::new(x, y, w - if arrows { 88.0 } else { 0.0 }, ROW);
                ui.select("ai-paint", sel, &mut pt, &paints);
                if arrows {
                    let prev = Rect::new(sel.right() + 8.0, y, ROW, ROW);
                    let next = Rect::new(prev.right() + 8.0, y, ROW, ROW);
                    if ui.button("ai-paint-prev", prev, "", Some("chevron_left"), ButtonKind::Normal) {
                        pt = (pt + paints.len() - 1) % paints.len();
                    }
                    if ui.button("ai-paint-next", next, "", Some("chevron_right"), ButtonKind::Normal) {
                        pt = (pt + 1) % paints.len();
                    }
                }
                y += ROW + 10.0;
            }
            ui.text_in("Fleet number", Rect::new(x, y, w, 18.0), 11.5, Weight::Bold, TEXT_DIM, Align::Left);
            y += 20.0;
            ui.text_input("ai-number", Rect::new(x, y, w, ROW), &mut number, "e.g. 101", Some("tag"));
            y += ROW + 6.0;
            if !number_names.is_empty() {
                if ui.select("ai-number-list", Rect::new(x, y, w, ROW), &mut num_sel, &number_names) {
                    picked_num = true;
                }
                y += ROW + 6.0;
            }
            ui.text_in("Number plate", Rect::new(x, y, w, 18.0), 11.5, Weight::Bold, TEXT_DIM, Align::Left);
            y += 20.0;
            ui.text_input("ai-plate", Rect::new(x, y, w, ROW), &mut plate, "Automatic", Some("badge"));
            y += ROW + 14.0;
            if ui.button("ai-add", Rect::new(x, y, w, ROW), "Add to the depot", Some("add"), ButtonKind::Primary) {
                add = true;
            }
            y += ROW + 8.0;
            y - v.y
        });
        if picked_num {
            if let Some((n, p)) = numbers.get(num_sel) {
                number = n.trim().to_string();
                plate = p.trim().to_string();
            }
        }
        if mk != av.maker {
            md = 0;
        }
        if reseed || md != av.model || mk != av.maker {
            av.picked.clear();
        }
        av.depot = d;
        av.maker = mk;
        av.model = md;
        av.paint = pt;
        av.number = number;
        av.plate = plate;
        if add {
            let num = field(&av.number);
            let used = entries_of(&av.bytes, &depot, &file);
            if file.is_empty() {
                status = Some(("Choose a bus first".into(), true));
            } else if num.is_empty() {
                status = Some(("Give the bus a fleet number".into(), true));
            } else if used.iter().any(|e| e.split('\t').next().unwrap_or("").trim().eq_ignore_ascii_case(&num)) {
                status = Some((format!("Fleet number {num} is already in the depot for this bus: choose another"), true));
            } else {
                let line = edit::entry_line_paint(&num, &field(&av.plate), &field(&paint_name));
                let name = veh.as_ref().map(|v| core::display_bus_name(&v.name)).unwrap_or_default();
                let head = edit::typ_blocks(&av.bytes).into_iter().find(|b| b.depot == depot && b.v2 && norm(&b.file) == norm(&file)).map(|b| b.head);
                av.bytes = match head {
                    Some(h) => edit::add_entry(&av.bytes, h, &line),
                    None => edit::add_block(&av.bytes, &depot, &file.replace('/', "\\"), &[line]),
                };
                av.dirty = true;
                av.picked.clear();
                status = Some((format!("{name} {num} added to {depot}: save to keep it"), false));
            }
        }
    }
    let ui: &mut Ui = &mut l.ui;
    if ui.button("ai-save", Rect::new(inner.x, by, inner.w, ROW), if av.dirty { "Save" } else { "Saved" }, Some("save"), if av.dirty { ButtonKind::Primary } else { ButtonKind::Normal }) && av.dirty {
        let saved = save_target(&av.map_dir).and_then(|p| std::fs::write(&p, &av.bytes).map(|_| p).map_err(|e| e.to_string()));
        match saved {
            Ok(_) => {
                av.dirty = false;
                omsi_cfg::content_changed();
                status = Some(("AI list saved: it counts the next time a game starts".into(), false));
            }
            Err(e) => status = Some((format!("Not saved: {e}"), true)),
        }
    }
    let rr = Rect::new(inner.x, by - ROW - 8.0, inner.w, ROW);
    if ui.button("ai-reset", rr, if av.reset_armed { "Press again to reset" } else { "Reset AI list" }, Some("restart_alt"), ButtonKind::Normal) {
        if av.reset_armed {
            av.reset_armed = false;
            match reset(&av.map_dir) {
                Ok(m) => {
                    omsi_cfg::content_changed();
                    av.loaded = None;
                    l.state.set_status(m, false);
                    return;
                }
                Err(e) => status = Some((e, true)),
            }
        } else {
            av.reset_armed = true;
            status = Some(("Press \"Reset AI list\" again to put the map's own list back (the changes made here are lost)".into(), false));
        }
    }

    // --- the bus types of the depot
    ui.panel(list_r);
    let title = if depot.is_empty() { "Depot".to_string() } else { depot.clone() };
    let inner = ui.heading(Rect::new(list_r.x + 18.0, list_r.y + 14.0, list_r.w - 36.0, list_r.h - 28.0), &format!("Buses of {title}"), Some("directions_bus"));
    let blocks: Vec<edit::TypBlock> = edit::typ_blocks(&av.bytes).into_iter().filter(|b| b.depot == depot).collect();
    let rows: Vec<(edit::TypBlock, String)> = blocks
        .into_iter()
        .map(|b| {
            let name = vehicles.iter().find(|v| norm(&v.1) == norm(&b.file) || norm(&b.file).ends_with(&norm(&v.1))).map(|v| v.0.clone()).unwrap_or_else(|| b.file.clone());
            (b, name)
        })
        .collect();
    if rows.is_empty() {
        ui.paragraph("Nothing here yet: choose a bus on the left and add it.", Vec2::new(inner.x, inner.y), inner.w, 13.0, Weight::Regular, TEXT_DIM);
    }
    // (an action: block head, what)
    let mut act: Option<(usize, u8)> = None;
    let mut show: Option<String> = None;
    const ROW_H: f32 = 56.0;
    ui.scroll_area("ai-blocks", Rect::new(inner.x - 6.0, inner.y, inner.w + 12.0, inner.h), &mut |ui, v| {
        for (i, (b, name)) in rows.iter().enumerate() {
            let r = Rect::new(v.x + 6.0, v.y + i as f32 * (ROW_H + 4.0), v.w - 16.0, ROW_H);
            ui.p().rounded(r, 6.0, SELECTED);
            let mut on = b.enabled;
            if ui.toggle(&format!("ai-on-{i}-{}", b.head), Rect::new(r.x + 10.0, r.y + 4.0, r.w - 190.0, 28.0), &mut on, name) {
                act = Some((b.head, if on { 1 } else { 0 }));
            }
            // (the fleet numbers it has, as many as fit)
            let mut nums = b.entries.iter().map(|e| {
                let f: Vec<&str> = e.split('\t').collect();
                let n = f.first().map(|s| s.trim()).unwrap_or("");
                match f.get(2).map(|s| s.trim()).filter(|s| !s.is_empty()) {
                    Some(p) => format!("{n} ({p})"),
                    None => n.to_string(),
                }
            }).collect::<Vec<_>>().join(", ");
            if nums.chars().count() > 90 {
                nums = nums.chars().take(89).collect::<String>() + "…";
            }
            ui.text_in(&nums, Rect::new(r.x + 12.0, r.y + 34.0, r.w - 24.0, 16.0), 11.0, Weight::Regular, TEXT_DIM, Align::Left);
            let (hv, _, clicked) = ui.interact(super::ui::id_of(&format!("ai-show-{i}-{}", b.head)), Rect::new(r.right() - 170.0, r.y, 100.0, 30.0));
            if clicked {
                show = Some(b.file.clone());
            }
            ui.text_in(&format!("{} buses", b.entries.len()), Rect::new(r.right() - 170.0, r.y, 110.0, 30.0), 12.0, Weight::Regular, if hv { TEXT } else { TEXT_DIM }, Align::Right);
            if ui.icon_button(&format!("ai-minus-{i}-{}", b.head), Vec2::new(r.right() - 40.0, r.y + 16.0), 14.0, "remove", "One bus fewer") {
                act = Some((b.head, 2));
            }
            if ui.icon_button(&format!("ai-plus-{i}-{}", b.head), Vec2::new(r.right() - 14.0, r.y + 16.0), 14.0, "add", "One bus more") {
                act = Some((b.head, 3));
            }
        }
        rows.len() as f32 * (ROW_H + 4.0)
    });
    if let Some(f) = show {
        // (a click on a type's count: its bus in the picture and the picker)
        if let Some(v) = l.state.vehicles.iter().find(|v| norm(&v.file) == norm(&f)) {
            if let Some((mk, md)) = av.makers.iter().enumerate().find_map(|(i, m)| m.variants.iter().position(|x| x.file == v.file).map(|k| (i, k))) {
                av.maker = mk;
                av.model = md;
                av.picked.clear();
            }
        }
    }
    if let Some((head, what)) = act {
        match what {
            0 | 1 => av.bytes = edit::set_enabled(&av.bytes, head, what == 1),
            2 => av.bytes = edit::remove_last_entry(&av.bytes, head),
            _ => {
                if let Some((b, _)) = rows.iter().find(|(b, _)| b.head == head) {
                    let offered = l.state.vehicles.iter().find(|v| norm(&v.file) == norm(&b.file)).map(|v| v.numbers.clone()).unwrap_or_default();
                    let line = edit::next_entry_like(&b.entries, &offered);
                    av.bytes = edit::add_entry(&av.bytes, head, &line);
                }
            }
        }
        av.dirty = true;
        av.picked.clear();
    }
    if let Some((s, err)) = status {
        l.state.set_status(s, err);
    }
}
