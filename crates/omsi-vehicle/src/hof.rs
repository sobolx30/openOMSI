//! `.hof` depot files: termini, bus stop display strings, IBIS info system.

use omsi_cfg::CfgFile;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Terminus {
    pub code: i32,
    /// Texture change id.
    pub texture_id: String,
    /// Bus stop where everybody leaves (None for `_allexit`).
    pub terminus_stop: Option<String>,
    pub all_exit: bool,
    pub strings: Vec<String>,
}

impl Terminus {
    /// Name used for destination text while preserving the original HOF string indices.
    pub fn display_name(&self) -> String {
        let first = self.strings.iter().find(|s| !s.trim().is_empty()).map(String::as_str);
        if first.is_some_and(|s| is_destination_image_path(s)) {
            return self.texture_id.trim().to_string();
        }
        first.map(str::trim).filter(|s| !s.is_empty()).unwrap_or_else(|| self.texture_id.trim()).to_string()
    }

    /// Name used by the destination menu: the identifier on the second `[addterminus]` line.
    pub fn menu_name(&self) -> String {
        let first = self.strings.iter().find(|s| !s.trim().is_empty()).map(String::as_str).unwrap_or("").trim();
        if !first.is_empty() && (first.eq_ignore_ascii_case("no") || is_destination_image_path(first) || has_route_label(&self.texture_id)) {
            let id = self.texture_id.trim();
            if !id.is_empty() {
                return id.to_string();
            }
        }
        self.display_name()
    }
}

fn is_destination_image_path(s: &str) -> bool {
    let s = s.trim().to_ascii_lowercase();
    s.ends_with(".bmp") || s.ends_with(".tga") || s.ends_with(".png")
}

fn has_route_label(s: &str) -> bool {
    let Some((prefix, _)) = s.trim().split_once(':') else { return false };
    !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit() || c.is_ascii_alphabetic())
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct BusStop {
    pub ident: String,
    pub strings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct InfoTrip {
    pub code: String,
    pub name: String,
    pub route: String,
    pub line: String,
    pub extra: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Hof {
    pub path: PathBuf,
    pub name: String,
    pub service_trip: String,
    pub global_strings: Vec<String>,
    pub string_count_terminus: usize,
    pub string_count_busstop: usize,
    pub termini: Vec<Terminus>,
    pub bus_stops: Vec<BusStop>,
    pub info_trips: Vec<InfoTrip>,
    pub info_busstop_lists: Vec<Vec<String>>,
    pub info_busstops: Vec<Vec<String>>,
    /// Where the virtual termini begin (see [`Hof::with_custom_terminus`]); None: the file
    /// has none. Never read from a file: they exist only in memory.
    pub custom_from: Option<usize>,
    /// Which of the two virtual termini was filled last (0 or 1).
    pub custom_last: usize,
}

impl Hof {
    pub fn load(path: &Path) -> Result<Hof, omsi_cfg::CfgError> {
        let f = CfgFile::read(path)?;
        Ok(Self::parse(&f))
    }

    /// Only the `[name]` of a depot file ("" when it has none), kept for the session: the
    /// searches by name below read every depot file of every vehicle folder, and parsing
    /// each whole (termini, stops, the IVU trips) made a big installation's start take
    /// minutes.
    pub fn read_name(path: &Path) -> Option<String> {
        type Names = std::collections::HashMap<PathBuf, Option<String>>;
        static CACHE: std::sync::OnceLock<std::sync::Mutex<(u64, Names)>> = std::sync::OnceLock::new();
        let cache = CACHE.get_or_init(Default::default);
        let generation = omsi_cfg::content_generation();
        {
            let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
            if c.0 != generation {
                *c = (generation, Names::new());
            }
            if let Some(n) = c.1.get(path) {
                return n.clone();
            }
        }
        let name = CfgFile::read(path).ok().map(|f| {
            let mut r = f.reader().with_rule(omsi_cfg::KeywordRule::TrimEnd);
            while let Some(k) = r.next_keyword() {
                if k == "name" {
                    return r.str().to_string();
                }
            }
            String::new()
        });
        cache.lock().unwrap_or_else(|e| e.into_inner()).1.insert(path.to_path_buf(), name.clone());
        name
    }

    pub fn parse(f: &CfgFile) -> Hof {
        let mut h = Hof { path: f.path.clone(), string_count_terminus: 0, string_count_busstop: 0, ..Default::default() };
        // `stringcount_terminus` / `stringcount_busstop` are bare (unbracketed) directives.
        for (i, l) in f.lines.iter().enumerate() {
            let w = l.trim_end();
            if w.eq_ignore_ascii_case("stringcount_terminus") {
                h.string_count_terminus = f.lines.get(i + 1).map(|s| omsi_cfg::parse_i64(s).max(0) as usize).unwrap_or(0);
            } else if w.eq_ignore_ascii_case("stringcount_busstop") {
                h.string_count_busstop = f.lines.get(i + 1).map(|s| omsi_cfg::parse_i64(s).max(0) as usize).unwrap_or(0);
            }
        }
        // the stock depot files are spreadsheet exports: every line ends in tabs, which the
        // original cuts off (with spaces and quotes) before looking at it
        let mut r = f.reader().with_rule(omsi_cfg::KeywordRule::TrimEnd);
        while let Some(k) = r.next_keyword() {
            match k.as_str() {
                "name" => h.name = r.str().to_string(),
                "servicetrip" => h.service_trip = r.str().to_string(),
                "global_strings" => {
                    let n = r.usize();
                    h.global_strings = (0..n).map(|_| r.str().to_string()).collect();
                }
                "addterminus" | "addterminus_allexit" => {
                    // Despite the SDK comment, no shipped file carries a separate terminus
                    // station line: every record is code, ident, then stringcount strings
                    // (verified over all stock .hof files). The ident doubles as the station.
                    let all_exit = k.ends_with("allexit");
                    let code = r.i32();
                    let texture_id = r.str().to_string();
                    let terminus_stop = if all_exit { None } else { Some(texture_id.clone()) };
                    let strings: Vec<String> = (0..h.string_count_terminus).map(|_| r.str().to_string()).collect();
                    h.termini.push(Terminus { code, texture_id, terminus_stop, all_exit, strings });
                }
                "addterminus_list" => {
                    // One row per terminus, tab separated: a flag column (`{ALLEX}` or
                    // empty), the code, the ident and the display strings - every one of the
                    // 3 549 rows of the stock and installed depot files has the flag column,
                    // empty ones included. Reading the code from the flag column gave every
                    // terminus without `{ALLEX}` the code 0 and its code as ident: typed
                    // destination codes were "wrong", a route found no terminus and the
                    // displays fell back to the first (empty) entry.
                    for l in r.until("[end]") {
                        if l.trim().is_empty() {
                            continue;
                        }
                        let cols: Vec<&str> = l.split('\t').collect();
                        let all_exit = cols[0].trim().eq_ignore_ascii_case("{ALLEX}");
                        let code = omsi_cfg::parse_i32(cols.get(1).unwrap_or(&"0"));
                        let texture_id = cols.get(2).unwrap_or(&"").trim().to_string();
                        let terminus_stop = if all_exit { None } else { Some(texture_id.clone()) };
                        let mut strings: Vec<String> = cols.iter().skip(3).map(|s| s.to_string()).collect();
                        if h.string_count_terminus > 0 {
                            strings.resize(h.string_count_terminus, String::new());
                        }
                        h.termini.push(Terminus { code, texture_id, terminus_stop, all_exit, strings });
                    }
                }
                "addbusstop" => {
                    let ident = r.str().to_string();
                    let strings = (0..h.string_count_busstop).map(|_| r.str().to_string()).collect();
                    h.bus_stops.push(BusStop { ident, strings });
                }
                "addbusstop_list" => {
                    for l in r.until("[end]") {
                        if l.trim().is_empty() {
                            continue;
                        }
                        let mut cols = l.split('\t');
                        let ident = cols.next().unwrap_or("").trim().to_string();
                        h.bus_stops.push(BusStop { ident, strings: cols.map(|s| s.to_string()).collect() });
                    }
                }
                "infosystem_trip" => {
                    let code = r.str().to_string();
                    let name = r.str().to_string();
                    let route = r.str().to_string();
                    let line = r.str().to_string();
                    h.info_trips.push(InfoTrip { code, name, route, line, extra: Vec::new() });
                    // every trip has a stop list, empty until one follows (THof.LoadFromFile
                    // 0x7ea142), so that the lists stay in step with the trips
                    h.info_busstop_lists.push(Vec::new());
                }
                "infosystem_busstop_list" => {
                    // the list of the trip read last (0x7ea16f: DynArrayHigh of the trips);
                    // pushed as one more list, a trip without one (the IVU data routes of
                    // some depot files) gave every later trip the stops of the one before
                    let n = r.usize();
                    let list: Vec<String> = (0..n).map(|_| r.str().to_string()).collect();
                    if let Some(last) = h.info_busstop_lists.last_mut() {
                        *last = list;
                    }
                }
                "infosystem_busstop" => h.info_busstops.push((0..3).map(|_| r.str().to_string()).collect()),
                _ => {}
            }
        }
        h
    }

    /// The termini the file itself lists (without the virtual ones of
    /// [`Hof::with_custom_terminus`]): what a destination menu offers.
    pub fn real_termini(&self) -> &[Terminus] {
        let end = self.custom_from.unwrap_or(self.termini.len()).min(self.termini.len());
        &self.termini[..end]
    }

    /// A copy of this depot file with a destination of the driver's own text, and its
    /// index: a virtual terminus the bus scripts find by `GetTerminusString` /
    /// `GetTerminusCode` / `GetTerminusIndex` like any other of the file (the file on disk is
    /// not touched). `top` is string 1 (the upper line), `bottom` string 2 (the lower one);
    /// string 0 and the rest stay empty.
    ///
    /// Two virtual termini (the codes after the highest of the file, one and two above it)
    /// are filled in turn: scripts refresh a display when the terminus index or code they
    /// last saw changes, so a second custom text must not come under the code of the first.
    /// Each call fills the one not shown now and answers its index.
    pub fn with_custom_terminus(&self, top: &str, bottom: &str) -> (Hof, usize) {
        let mut h = self.clone();
        let slots_there = h.custom_from.is_some_and(|f| h.termini.len() >= f + 2);
        if !slots_there {
            // (a file replaced or edited since leaves no stale slots: start from its own termini)
            let from = h.custom_from.filter(|f| *f <= h.termini.len()).unwrap_or(h.termini.len());
            h.termini.truncate(from);
            let base = h.termini.iter().map(|t| t.code).max().unwrap_or(0).max(0);
            for k in 1..=2 {
                h.termini.push(Terminus { code: base.saturating_add(k), ..Default::default() });
            }
            h.custom_from = Some(from);
            h.custom_last = 1;
        }
        let from = h.custom_from.unwrap_or(0);
        let slot = 1 - h.custom_last.min(1);
        let count = h.string_count_terminus.max(3);
        // As in the depot files (code, ident, then: a full text, the upper line, the lower
        // line, and what other displays take - pictures, signs): strings 1 and 2 are the
        // two lines and every other string is empty.
        let mut strings = vec![String::new(); count];
        strings[1] = top.to_string();
        strings[2] = bottom.to_string();
        let ident = if top.trim().is_empty() { bottom.trim() } else { top.trim() }.to_string();
        let t = &mut h.termini[from + slot];
        t.texture_id = ident.clone();
        t.terminus_stop = Some(ident);
        t.all_exit = false;
        t.strings = strings;
        h.custom_last = slot;
        (h, from + slot)
    }

    pub fn terminus_by_code(&self, code: i32) -> Option<&Terminus> {
        self.termini.iter().find(|t| t.code == code)
    }
}

/// The `.hof` files available to a vehicle. Files next to the vehicle come first, merged
/// over all content roots (a mod's depot beside the installation's; archives read in place
/// too), followed by the shared top-level `HOFs/` folder. A vehicle-local file hides a
/// shared file of the same name; within each group files are sorted by name.
pub fn depot_files(dir: &Path) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    let mut local: Vec<PathBuf> = Vec::new();
    for d in omsi_cfg::mirrored_dirs(dir) {
        for p in omsi_cfg::vfs::read_dir_paths(&d) {
            if !p.extension().map(|e| e.eq_ignore_ascii_case("hof")).unwrap_or(false) {
                continue;
            }
            if seen.insert(p.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase()) {
                local.push(p);
            }
        }
    }
    local.sort_by_key(|f| f.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase());

    let mut shared: Vec<PathBuf> = omsi_cfg::read_dir_merged("HOFs")
        .into_iter()
        .filter(|p| p.extension().map(|e| e.eq_ignore_ascii_case("hof")).unwrap_or(false))
        .filter(|p| seen.insert(p.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase()))
        .collect();
    shared.sort_by_key(|f| f.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase());

    local.extend(shared);
    local
}

/// The depot file of `dir` called `name`: by its file name (without `.hof`) or its `[name]`.
pub fn depot_in(dir: &Path, name: &str) -> Option<Hof> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let files = depot_files(dir);
    if let Some(f) = files.iter().find(|f| f.file_stem().map(|s| s.to_string_lossy().trim().eq_ignore_ascii_case(name)).unwrap_or(false)) {
        if let Ok(h) = Hof::load(f) {
            return Some(h);
        }
    }
    files
        .iter()
        .filter(|f| Hof::read_name(f).is_some_and(|n| n.trim().eq_ignore_ascii_case(name)))
        .find_map(|f| Hof::load(f).ok())
}

/// The words of a depot or map name that tell one place from another: four letters or
/// more, not a year or a number, not a word every depot file has ("Linie 20", "Hof").
fn place_words(s: &str) -> Vec<String> {
    const COMMON: [&str; 14] = ["linie", "line", "lines", "depot", "omsi", "maps", "version", "final", "neue", "update", "addon", "fixed", "standard", "default"];
    s.split(|c: char| !c.is_alphanumeric())
        .map(|w| w.to_lowercase())
        .filter(|w| w.chars().count() >= 4 && !w.chars().all(|c| c.is_ascii_digit()) && !COMMON.contains(&w.as_str()))
        .collect()
}

/// Of `names` (the depot files a bus has), the one that belongs to the place `hints` name
/// (the map's depot names, its title, its folder): the one sharing the most of their
/// words, the longer words counting more; the first of equals. None when none shares one.
///
/// OMSI asks the driver which of the bus's depot files to use; taking the first of them
/// when none is called exactly as the map wants put a bus on Hamburg's Linie 20 with the
/// Grundorf depot of its folder - no line and no destination its IBIS knew (#896).
pub fn closest_name(names: &[&str], hints: &[&str]) -> Option<usize> {
    let wanted: Vec<String> = hints.iter().flat_map(|h| place_words(h)).collect();
    let mut best: Option<(usize, usize)> = None;
    for (i, n) in names.iter().enumerate() {
        let mut words = place_words(n);
        words.dedup();
        let score: usize = words.iter().filter(|w| wanted.contains(w)).map(|w| w.chars().count()).sum();
        if score > 0 && best.is_none_or(|(_, b)| score > b) {
            best = Some((i, score));
        }
    }
    best.map(|(i, _)| i)
}

/// The depot file of `dir` that belongs to the place `hints` name (see [`closest_name`]),
/// by its file name or its `[name]`.
pub fn depot_like(dir: &Path, hints: &[&str]) -> Option<Hof> {
    let files = depot_files(dir);
    let names: Vec<String> = files
        .iter()
        .map(|f| {
            let stem = f.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            match Hof::read_name(f) {
                Some(n) => format!("{stem} {n}"),
                None => stem,
            }
        })
        .collect();
    let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    closest_name(&refs, hints).and_then(|i| Hof::load(&files[i]).ok())
}

/// The depot file called `name` in the shared `HOFs/` folder or any vehicle folder of any content root (`Vehicles/*/`).
///
/// A depot file belongs to a map, not to a bus model: it lists the map's termini, stops and
/// IBIS codes. A mod bus brings only the depot of the map it was made on (the O530 Citaro
/// pack has Grundorf.hof alone), and on another map every code of the timetable was then
/// unknown to its IBIS - no line and no destination on any display. OMSI players copy the
/// map's .hof into such a folder; this finds the copy that is already installed with
/// another bus.
pub fn depot_anywhere(name: &str) -> Option<Hof> {
    // (asked for every type of AI bus without the map's depot: the file found is kept for
    // the session, and a big installation's thousands of folders are gone through once)
    type Found = std::collections::HashMap<String, Option<PathBuf>>;
    static FOUND: std::sync::OnceLock<std::sync::Mutex<(u64, Found)>> = std::sync::OnceLock::new();
    let found = FOUND.get_or_init(Default::default);
    let key = name.trim().to_ascii_lowercase();
    let generation = omsi_cfg::content_generation();
    let known = {
        let mut f = found.lock().unwrap_or_else(|e| e.into_inner());
        if f.0 != generation {
            *f = (generation, Found::new());
        }
        f.1.get(&key).cloned()
    };
    if let Some(path) = known {
        return path.and_then(|p| Hof::load(&p).ok());
    }
    // every vehicle folder once over all roots (depot_in looks at each root's copy)
    let mut dirs: Vec<PathBuf> = omsi_cfg::read_dir_merged("Vehicles").into_iter().filter(|d| omsi_cfg::vfs::is_dir(d)).collect();
    dirs.sort_by_key(|d| d.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase());
    let h = dirs.iter().find_map(|d| depot_in(d, name));
    found.lock().unwrap_or_else(|e| e.into_inner()).1.insert(key, h.as_ref().map(|h| h.path.clone()));
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #896: the bus's own depot of the map's place, not the first of its folder.
    #[test]
    fn closest_depot_name_is_the_maps_place() {
        let names = ["Grundorf", "Hamburg Linie 20", "Spandau 2019"];
        assert_eq!(closest_name(&names, &["Hamburg_Linie_20", "Linie 20"]), Some(1));
        assert_eq!(closest_name(&names, &["Spandau 1986"]), Some(2));
        assert_eq!(closest_name(&names, &["Berlin-Spandau"]), Some(2));
        assert_eq!(closest_name(&names, &["Thüringer Wald"]), None);
        assert_eq!(closest_name(&["Berlin X10", "Spandau"], &["Berlin-Spandau"]), Some(1));
        assert_eq!(closest_name(&["Linie 20"], &["Linie 7"]), None);
    }

    #[test]
    fn shared_depots_are_available_but_vehicle_copy_wins() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("openomsi-shared-hof-{stamp}"));
        let bus = root.join("Vehicles/TestBus");
        let shared = root.join("HOFs");
        std::fs::create_dir_all(&bus).unwrap();
        std::fs::create_dir_all(&shared).unwrap();

        let local_name = format!("local-{stamp}.hof");
        let shared_name = format!("shared-{stamp}.hof");
        let same_name = format!("same-{stamp}.hof");
        std::fs::write(bus.join(&local_name), "[name]\nLocal\n").unwrap();
        std::fs::write(shared.join(&shared_name), "[name]\nShared\n").unwrap();
        std::fs::write(bus.join(&same_name), "[name]\nLocal duplicate\n").unwrap();
        std::fs::write(shared.join(&same_name), "[name]\nShared duplicate\n").unwrap();

        omsi_cfg::add_content_root(root.clone());
        let files = depot_files(&bus);
        let named = |p: &Path, name: &str| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n == name);
        let local_i = files.iter().position(|p| named(p, &local_name)).unwrap();
        let shared_i = files.iter().position(|p| named(p, &shared_name)).unwrap();
        let duplicate = files.iter().find(|p| named(p, &same_name)).unwrap();
        assert!(local_i < shared_i);
        assert_eq!(duplicate, &bus.join(&same_name));
        assert_eq!(depot_in(&bus, "Shared").map(|h| h.name), Some("Shared".into()));

        omsi_cfg::remove_content_root(&root);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn custom_terminus_alternates_two_virtual_slots() {
        let text = "stringcount_terminus\n3\n[addterminus_list]\n\t10\tA\tAAA\tTOP\tAAA\n\t282\tB\tBBB\t\t\n[end]\n";
        let h = Hof::parse(&CfgFile::from_str("t.hof", text));
        assert_eq!(h.termini.len(), 2);
        let (h1, i1) = h.with_custom_terminus("Upper", "Lower");
        assert_eq!((i1, h1.termini[i1].code), (2, 283));
        assert_eq!(h1.termini[i1].strings, vec!["", "Upper", "Lower"]);
        assert_eq!(h1.real_termini().len(), 2);
        // the next text goes under the other code, the first one stays as it was
        let (h2, i2) = h1.with_custom_terminus("X", "Y");
        assert_eq!((i2, h2.termini[i2].code), (3, 284));
        assert_eq!(h2.termini[i1].strings[1], "Upper");
        // and the third back under the first slot: never the same code twice in a row
        let (h3, i3) = h2.with_custom_terminus("P", "Q");
        assert_eq!(i3, i1);
        assert_eq!(h3.termini.len(), 4);
        assert_eq!(h3.termini[i3].strings, vec!["", "P", "Q"]);
        // the file's own data is untouched
        assert_eq!(h3.real_termini(), h.termini.as_slice());
    }

    #[test]
    fn terminus_list_columns() {
        let text = "stringcount_terminus\r\n3\r\n\r\n[addterminus_list]\r\n{ALLEX}\t13\tBetriebsfahrt\tBETRIEBSFAHRT\t\tBETRIEBSFAHRT\t\t\r\n\t282\tU Ruhleben\tRUHLEBEN\tU-BAHNHOF\tRUHLEBEN  \t\t\t\r\n[end]\r\n";
        let h = Hof::parse(&CfgFile::from_str("test.hof", text));
        assert_eq!(h.termini.len(), 2);
        assert_eq!((h.termini[0].code, h.termini[0].texture_id.as_str(), h.termini[0].all_exit), (13, "Betriebsfahrt", true));
        assert_eq!(h.termini[0].strings, vec!["BETRIEBSFAHRT", "", "BETRIEBSFAHRT"]);
        assert_eq!((h.termini[1].code, h.termini[1].texture_id.as_str(), h.termini[1].all_exit), (282, "U Ruhleben", false));
        assert_eq!(h.termini[1].terminus_stop.as_deref(), Some("U Ruhleben"));
        assert_eq!(h.termini[1].strings, vec!["RUHLEBEN", "U-BAHNHOF", "RUHLEBEN  "]);
        assert_eq!(h.terminus_by_code(282).map(|t| t.texture_id.as_str()), Some("U Ruhleben"));
    }

    #[test]
    fn legacy_hof_destination_name_falls_back_to_ident() {
        let text = "stringcount_terminus\n6\n[addterminus]\n71910\n71 Eden Tunnel\n\n\n\n\nLegacyRoute\\71Y_1.bmp\n71Y\n";
        let h = Hof::parse(&CfgFile::from_str("legacy.hof", text));
        let t = h.terminus_by_code(71910).unwrap();
        assert_eq!(t.strings[0], "");
        assert_eq!(t.strings[4], "LegacyRoute\\71Y_1.bmp");
        assert_eq!(t.strings[5], "71Y");
        assert_eq!(t.display_name(), "71 Eden Tunnel");
        assert_eq!(t.menu_name(), "71 Eden Tunnel");
    }

    #[test]
    fn normal_hof_menu_name_keeps_display_text() {
        let t = Terminus { texture_id: "910".into(), strings: vec!["AEC".into()], ..Default::default() };
        assert_eq!(t.menu_name(), "AEC");
    }

    #[test]
    fn route_label_menu_name_uses_the_ident() {
        let t = Terminus { texture_id: "66: South Valley Railway Station Circular".into(), strings: vec!["S.VALLEY STN CIR".into()], ..Default::default() };
        assert_eq!(t.menu_name(), "66: South Valley Railway Station Circular");
    }

    /// #667: a trip without a stop list (an IVU data route) keeps the lists of the trips
    /// after it on their own trips.
    #[test]
    fn stop_lists_belong_to_the_trip_before_them() {
        let text = "[infosystem_trip]\r\n45581\r\nZOB-HOHENECK\r\n81\r\n455\r\n\r\n\
            [infosystem_busstop_list]\r\n2\r\nZOB\r\nHoheneck\r\n\r\n\
            [infosystem_trip]\r\n455900\r\nIVU\r\n81\r\n455\r\n\r\n\
            [infosystem_trip]\r\n45503\r\nHBF-BERGERFUERTH\r\n3\r\n455\r\n\r\n\
            [infosystem_busstop_list]\r\n3\r\nHauptbahnhof\r\nMarkt\r\nBergerfuerth\r\n";
        let h = Hof::parse(&CfgFile::from_str("test.hof", text));
        assert_eq!(h.info_trips.len(), 3);
        assert_eq!(h.info_busstop_lists.len(), 3);
        assert_eq!(h.info_busstop_lists[0], vec!["ZOB", "Hoheneck"]);
        assert!(h.info_busstop_lists[1].is_empty());
        assert_eq!(h.info_busstop_lists[2], vec!["Hauptbahnhof", "Markt", "Bergerfuerth"]);
    }
}
