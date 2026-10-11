//! `ailists.cfg`, `unsched_vehgroups.txt`, `unsched_trafficdens.txt`, `parklist_p.txt`,
//! `humans.txt`, `drivers.txt`, `registrations.txt`.

use omsi_cfg::CfgFile;
use std::path::PathBuf;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AiVehicleEntry {
    pub file: String,
    /// Weight (`[aigroup_2]`) or number/registration (`[aigroup_depot_typgroup_2]`).
    pub weight: f32,
    pub number: Option<String>,
    pub registration: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AiGroup {
    pub name: String,
    pub vehicles: Vec<AiVehicleEntry>,
    /// Depot groups: HOF name.
    pub hof: Option<String>,
    pub is_depot: bool,
    /// Depot type groups: vehicle file + list of numbers/registrations.
    pub typgroups: Vec<AiTypGroup>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AiTypGroup {
    pub file: String,
    pub entries: Vec<DepotEntry>,
}

/// One vehicle of a depot's type group: an `[aigroup_depot_typgroup_2]` line is its fleet
/// number, plate, repaint and first and last day (YYYYMMDD), tab-separated (Omsi.exe
/// 0x780a58 splits it at each tab with 0x7ef900, empty fields kept); an
/// `[aigroup_depot_typgroup]` line is the number alone, which also names the repaint.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DepotEntry {
    pub number: String,
    pub registration: String,
    pub paint: String,
    pub from: Option<i32>,
    pub to: Option<i32>,
}

impl DepotEntry {
    /// A `[aigroup_depot_typgroup_2]` line.
    pub fn parse(line: &str) -> DepotEntry {
        let mut f = line.split('\t');
        let mut next = || f.next().unwrap_or("").to_string();
        let (number, registration, paint, from, to) = (next(), next(), next(), next(), next());
        let date = |s: &str| s.trim().parse::<i32>().ok();
        DepotEntry {
            number: number.trim().to_string(),
            registration,
            paint,
            from: date(&from),
            to: date(&to),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AiLists {
    pub groups: Vec<AiGroup>,
    /// The default group (index into `groups`): the `[ailist]` header's second number, the
    /// "NotInGroup" group it makes for -1, else the first group (Omsi.exe 0x78093c sets 0,
    /// 0x780a58 reads the header). A map without `unsched_vehgroups.txt` takes its random
    /// traffic from this group alone (0x785f98: the one group it makes then has no name,
    /// and a nameless group is the default group).
    pub default_group: usize,
}

impl AiLists {
    pub fn load(path: &Path) -> Result<AiLists, omsi_cfg::CfgError> {
        let f = CfgFile::read(path)?;
        Ok(Self::parse(&f))
    }

    pub fn parse(f: &CfgFile) -> AiLists {
        let mut a = AiLists::default();
        // the original lower-cases every line of this file before looking at it
        let mut r = f.reader().with_rule(omsi_cfg::KeywordRule::AnyCase);
        // the old format's list (`[ailist]`): (default group, vehicles)
        let mut legacy: Option<(i32, Vec<String>)> = None;
        while let Some(k) = r.next_keyword() {
            match k.as_str() {
                // the original: a line it passes over, the default group's index, a
                // count and that many vehicle files
                "ailist" => {
                    let _ = r.str();
                    let default = r.str().trim().parse::<i32>().unwrap_or(-1);
                    let n = r.str().trim().parse::<usize>().unwrap_or(0).min(10_000);
                    let files: Vec<String> = (0..n).map(|_| r.str().trim().to_string()).filter(|f| !f.is_empty()).collect();
                    legacy = Some((default, files));
                }
                "aigroup" | "aigroup_2" => {
                    let name = r.str().to_string();
                    // second header line: the depot (hof) name, empty for plain car groups
                    let hof = r.str().trim().to_string();
                    // (a map that leaves the depot line out altogether starts the list right
                    // there: Novi Sad's "Trucks" group - that line is a vehicle, not a depot)
                    let lower = hof.to_ascii_lowercase();
                    let vehicle = lower.contains(".bus") || lower.contains(".ovh") || lower.contains(".sco");
                    let mut g = AiGroup { name, hof: if hof.is_empty() || vehicle { None } else { Some(hof.clone()) }, ..Default::default() };
                    if vehicle {
                        let (file, w) = match hof.split_once('\t') {
                            Some((f, w)) => (f.trim().to_string(), omsi_cfg::parse_f32(w)),
                            None => (hof.trim().to_string(), 1.0),
                        };
                        g.vehicles.push(AiVehicleEntry { file, weight: w, number: None, registration: None });
                    }
                    for l in r.until("[end]") {
                        let l = l.trim_end();
                        if l.trim().is_empty() {
                            continue;
                        }
                        let (file, w) = match l.split_once('\t') {
                            Some((f, w)) => (f.trim().to_string(), omsi_cfg::parse_f32(w)),
                            None => (l.trim().to_string(), 1.0),
                        };
                        g.vehicles.push(AiVehicleEntry { file, weight: w, number: None, registration: None });
                    }
                    a.groups.push(g);
                }
                "aigroup_depot" => {
                    let name = r.str().to_string();
                    let hof = r.str().to_string();
                    a.groups.push(AiGroup { name, hof: Some(hof), is_depot: true, ..Default::default() });
                }
                "aigroup_depot_typgroup" | "aigroup_depot_typgroup_2" => {
                    let file = r.str().to_string();
                    let mut tg = AiTypGroup { file, entries: Vec::new() };
                    let v2 = k == "aigroup_depot_typgroup_2";
                    for l in r.until("[end]") {
                        if l.trim().is_empty() {
                            continue;
                        }
                        tg.entries.push(if v2 {
                            DepotEntry::parse(l)
                        } else {
                            DepotEntry { number: l.trim().to_string(), paint: l.trim().to_string(), ..Default::default() }
                        });
                    }
                    if let Some(g) = a.groups.last_mut() {
                        g.typgroups.push(tg);
                    }
                }
                _ => {}
            }
        }
        // no default group given: every listed vehicle that is in no group makes the group
        // "NotInGroup" (weight 1), as OMSI does after reading the file
        if let Some((default, files)) = legacy {
            if default < 0 || default as usize >= a.groups.len() {
                let grouped: std::collections::HashSet<String> = a.groups.iter().flat_map(|g| g.vehicles.iter().map(|v| v.file.to_ascii_lowercase())).collect();
                let vehicles: Vec<AiVehicleEntry> = files
                    .into_iter()
                    .filter(|f| !grouped.contains(&f.to_ascii_lowercase()))
                    .map(|file| AiVehicleEntry { file, weight: 1.0, number: None, registration: None })
                    .collect();
                if !vehicles.is_empty() {
                    a.default_group = a.groups.len();
                    a.groups.push(AiGroup { name: "NotInGroup".into(), vehicles, ..Default::default() });
                }
            } else {
                a.default_group = default as usize;
            }
        }
        a
    }
}

/// A plain list file (one entry per line, blank lines ignored).
pub fn load_list(path: &Path) -> Vec<String> {
    match CfgFile::read(path) {
        Ok(f) => f.lines.iter().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect(),
        Err(_) => Vec::new(),
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct UnschedGroup {
    pub name: String,
    pub factor: f32,
    /// (day-of-week mask, list of (hour, density))
    pub densities: Vec<(i32, Vec<(f32, f32)>)>,
}

/// `unsched_trafficdens.txt`
pub fn parse_unsched_trafficdens(f: &CfgFile) -> Vec<UnschedGroup> {
    let mut out: Vec<UnschedGroup> = Vec::new();
    let mut r = f.reader();
    while let Some(k) = r.next_keyword() {
        match k.as_str() {
            "group" => {
                let name = r.str().to_string();
                let factor = r.f32();
                out.push(UnschedGroup { name, factor, densities: Vec::new() });
            }
            "set_day_of_week" => {
                let d = r.i32();
                if let Some(g) = out.last_mut() {
                    g.densities.push((d, Vec::new()));
                }
            }
            "trafficdensity" => {
                let t = r.f32();
                let d = r.f32();
                if let Some(g) = out.last_mut() {
                    if g.densities.is_empty() {
                        g.densities.push((0, Vec::new()));
                    }
                    g.densities.last_mut().unwrap().1.push((t, d));
                }
            }
            _ => {}
        }
    }
    out
}

/// `unsched_vehgroups.txt`: (group name, default density class)
pub fn parse_unsched_vehgroups(f: &CfgFile) -> Vec<(String, i32)> {
    let mut out = Vec::new();
    let mut r = f.reader();
    while let Some(k) = r.next_keyword() {
        if k == "group" {
            let n = r.str().to_string();
            let d = r.i32();
            out.push((n, d));
        }
    }
    out
}

/// Chrono `Chrono.cfg`
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChronoCfg {
    pub start_date: i32,
    pub end_date: i32,
    pub ticket_pack: Option<String>,
    pub money_system: Option<String>,
    pub deactivate_lines: Vec<String>,
}

pub fn parse_chrono_cfg(f: &CfgFile) -> ChronoCfg {
    let mut c = ChronoCfg::default();
    let mut r = f.reader();
    while let Some(k) = r.next_keyword() {
        match k.as_str() {
            "startdate" => c.start_date = r.i32(),
            "enddate" => c.end_date = r.i32(),
            "ticketpack" => c.ticket_pack = Some(r.str().to_string()),
            "moneysystem" => c.money_system = Some(r.str().to_string()),
            // a count, then that many line names (the original: StrToInt, then ReadLn
            // n times). The count is not a line: read as one it took line "1" or "2" off.
            "deactivate_lines" => {
                let n = r.i32().max(0);
                c.deactivate_lines = (0..n).map(|_| r.str().trim().to_string()).filter(|s| !s.is_empty()).collect();
            }
            _ => {}
        }
    }
    c
}

impl ChronoCfg {
    /// Is the scenario in force on `date` (YYYYMMDD)? As the original: a scenario
    /// without any date never is; `[startdate]` and `[enddate]` both count as in force.
    pub fn active_on(&self, date: i32) -> bool {
        (self.start_date != 0 || self.end_date != 0) && (self.start_date == 0 || date >= self.start_date) && (self.end_date == 0 || date <= self.end_date)
    }
}

/// The `Chrono.cfg` files under a map's `Chrono` folder in the game's order (OMSI
/// the original): depth first, a folder's subfolders before its own files, names in the
/// order Windows lists them (case-insensitive). A later scenario overrides an earlier one.
fn chrono_cfgs(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries = omsi_cfg::vfs::list_dir(dir).unwrap_or_default();
    entries.sort_by_key(|(n, _)| n.to_string_lossy().to_uppercase());
    for (n, is_dir) in &entries {
        if *is_dir {
            chrono_cfgs(&dir.join(n), out);
        }
    }
    for (n, is_dir) in &entries {
        if !*is_dir && n.to_string_lossy().eq_ignore_ascii_case("Chrono.cfg") {
            out.push(dir.join(n));
        }
    }
}

/// Chrono folders of a map that are active on `date` (YYYYMMDD), in the game's order.
pub fn active_chrono_dirs(map_dir: &Path, date: i32) -> Vec<PathBuf> {
    let mut cfgs = Vec::new();
    chrono_cfgs(&omsi_cfg::resolve_path(map_dir, "Chrono"), &mut cfgs);
    cfgs.into_iter()
        .filter(|p| CfgFile::read(p).map(|f| parse_chrono_cfg(&f).active_on(date)).unwrap_or(false))
        .filter_map(|p| p.parent().map(|d| d.to_path_buf()))
        .collect()
}

/// The lines the given (active) chrono folders take off the timetable (`[deactivate_lines]`),
/// each with the folder that does it, in folder order. A scenario takes a line off only for
/// the folders before it (the map's own TTData and earlier scenarios): a later one may bring
/// the line back (see `TimetableData::load_with_chrono`).
pub fn chrono_deactivated_lines(chrono_dirs: &[PathBuf]) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for d in chrono_dirs {
        let cfg = Some(omsi_cfg::resolve_path(d, "Chrono.cfg")).filter(|p| omsi_cfg::vfs::is_file(p));
        if let Some(f) = cfg.and_then(|p| CfgFile::read(&p).ok()) {
            out.extend(parse_chrono_cfg(&f).deactivate_lines.into_iter().map(|l| (l, d.clone())));
        }
    }
    out
}

/// The map's `ailists.cfg` with the updates of the given (active) chrono folders
/// (`ailists_#upd.cfg`, else a full `ailists.cfg` of the scenario): a group of the same name
/// gets the new vehicles, and a depot the depot file (`.hof`) the scenario names - Berlin's
/// buses change to "Spandau 1994" on 29 May 1994.
pub fn ailists_with_chrono(map_dir: &Path, chrono_dirs: &[PathBuf]) -> AiLists {
    let mut ailists = AiLists::load(&omsi_cfg::resolve_path(map_dir, "ailists.cfg")).unwrap_or_default();
    for c in chrono_dirs {
        for name in ["ailists_#upd.cfg", "ailists.cfg"] {
            if let Ok(extra) = AiLists::load(&c.join(name)) {
                for g in extra.groups {
                    match ailists.groups.iter_mut().find(|x| x.name.eq_ignore_ascii_case(&g.name) && x.is_depot == g.is_depot) {
                        Some(base) => {
                            base.vehicles.extend(g.vehicles);
                            base.typgroups.extend(g.typgroups);
                            if g.hof.is_some() {
                                base.hof = g.hof;
                            }
                        }
                        None => ailists.groups.push(g),
                    }
                }
                break;
            }
        }
    }
    ailists
}

/// The depot file (`.hof` name) the map's own buses use on `date` (YYYYMMDD): the first
/// depot's, with the chrono scenarios of that date.
pub fn depot_hof_on(map_dir: &Path, date: i32) -> Option<String> {
    let l = ailists_with_chrono(map_dir, &active_chrono_dirs(map_dir, date));
    l.groups.iter().filter(|g| g.is_depot).chain(l.groups.iter()).find_map(|g| g.hof.clone())
}

/// A date as the game and the launcher write it (`YYYY-MM-DD`) as the chrono's `YYYYMMDD`.
pub fn date_code(date: &str) -> Option<i32> {
    let v: Vec<i32> = date.split('-').filter_map(|x| x.trim().parse().ok()).collect();
    match v[..] {
        [y, m, d] if (1..=12).contains(&m) && (1..=31).contains(&d) => Some(y * 10000 + m * 100 + d),
        _ => None,
    }
}

/// Whether a depot vehicle is in service on `date` (YYYYMMDD): from its first day, if it has
/// one, to its last, if it has one (0x781a15, 0x781a47).
pub fn typgroup_entry_valid(entry: &DepotEntry, date: i32) -> bool {
    entry.from.is_none_or(|f| f <= date) && entry.to.is_none_or(|t| t >= date)
}

/// `signalroutes.cfg` (unit `mc_fahrstrasse`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SignalRoute {
    pub kind: i32,
    pub signal: (i64, i32),
    pub entries: Vec<[i64; 4]>,
    pub dist_signal: Option<i64>,
    pub next_signal: Option<i64>,
    pub speed_limit: Option<f32>,
}

pub fn parse_signalroutes(f: &CfgFile) -> Vec<SignalRoute> {
    let mut out: Vec<SignalRoute> = Vec::new();
    let mut r = f.reader();
    while let Some(k) = r.next_keyword() {
        match k.as_str() {
            "signalroute" => out.push(SignalRoute { kind: r.i32(), ..Default::default() }),
            "signal" => {
                let a = r.i64();
                let b = r.i32();
                if let Some(s) = out.last_mut() {
                    s.signal = (a, b);
                }
            }
            "entry" => {
                let e = [r.i64(), r.i64(), r.i64(), r.i64()];
                if let Some(s) = out.last_mut() {
                    s.entries.push(e);
                }
            }
            "distsignal" => {
                let v = r.i64();
                if let Some(s) = out.last_mut() {
                    s.dist_signal = Some(v);
                }
            }
            "nextsignal" => {
                let v = r.i64();
                if let Some(s) = out.last_mut() {
                    s.next_signal = Some(v);
                }
            }
            "speedlimit" => {
                let v = r.f32();
                if let Some(s) = out.last_mut() {
                    s.speed_limit = Some(v);
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_codes() {
        assert_eq!(date_code("2026-09-17"), Some(20260917));
        assert_eq!(date_code("1989-5-30"), Some(19890530));
        assert_eq!(date_code("1989-13-30"), None);
        assert_eq!(date_code(""), None);
    }

    /// Spandau's 1991 timetable change takes line "5 & 5N" off: on any later date the chrono
    /// that does it is active and names the line; before it, nothing takes the line off.
    #[test]
    fn spandau_takes_line_5_off_in_1991() {
        let root = std::env::var_os("OMSI_ROOT").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("../../../OMSI 2 Original"));
        let map = root.join("maps/Berlin-Spandau");
        if !map.join("Chrono").is_dir() {
            eprintln!("skipped: no {}", map.display());
            return;
        }
        let later = chrono_deactivated_lines(&active_chrono_dirs(&map, 20260917));
        let by = later.iter().find(|(l, _)| l == "5 & 5N").map(|(_, d)| d.file_name().unwrap().to_string_lossy().into_owned());
        assert_eq!(by.as_deref(), Some("1000_FPW_19910602"), "{later:?}");
        let before = chrono_deactivated_lines(&active_chrono_dirs(&map, 19890530));
        assert!(!before.iter().any(|(l, _)| l == "5 & 5N"), "{before:?}");
        // the count before the names is no line
        assert!(!later.iter().any(|(l, _)| l == "1" || l == "17" || l == "2"), "{later:?}");
    }

    #[test]
    fn chrono_dates_are_inclusive_and_need_one() {
        let c = |s, e| ChronoCfg { start_date: s, end_date: e, ..Default::default() };
        assert!(!c(0, 0).active_on(19900101));
        assert!(c(19900910, 19900923).active_on(19900923));
        assert!(!c(19900910, 19900923).active_on(19900924));
        assert!(c(0, 19870430).active_on(19800101));
        assert!(c(19870501, 0).active_on(20260101));
    }
}

#[cfg(test)]
mod legacy_tests {
    #[test]
    fn the_old_ailist_makes_a_group_of_its_own() {
        let f = omsi_cfg::CfgFile::from_str("ailists.cfg", "[ailist]\n0\n-1\n2\nvehicles\\A\\a.bus\nvehicles\\B\\b.ovh\n");
        let a = super::AiLists::parse(&f);
        assert_eq!(a.groups.len(), 1);
        assert_eq!(a.groups[0].name, "NotInGroup");
        assert_eq!(a.groups[0].vehicles.len(), 2);
        assert_eq!(a.default_group, 0);
    }

    #[test]
    fn the_default_group_is_the_first_unless_the_header_names_one() {
        let groups = "[aigroup_2]\nNormalCars\n\nvehicles\\A\\a.bus\t7\n[end]\n[aigroup_2]\nAmbulance\n\nvehicles\\B\\b.ovh\n[end]\n";
        let a = super::AiLists::parse(&omsi_cfg::CfgFile::from_str("ailists.cfg", groups));
        assert_eq!(a.default_group, 0);
        let named = format!("[ailist]\n0\n1\n0\n{groups}");
        let a = super::AiLists::parse(&omsi_cfg::CfgFile::from_str("ailists.cfg", &named));
        assert_eq!(a.groups[a.default_group].name, "Ambulance");
    }
}

#[cfg(test)]
mod chrono_hof_tests {
    #[test]
    fn berlin_changes_its_depot_file_with_the_date() {
        let dir = std::path::Path::new("../../../OMSI 2 Original/maps/Berlin-Spandau");
        if !dir.is_dir() {
            return;
        }
        let at = |d| super::depot_hof_on(dir, d).unwrap_or_default();
        assert_eq!(at(19940601), "Spandau 1994");
        assert_ne!(at(19870101), "Spandau 1994");
        assert_eq!(at(19891201), "Spandau 1989-12");
    }
}

/// [ROLLBACK ailist-73] Editing `ailists.cfg` as text: the launcher's AI list page. The file is
/// handled as bytes by lines, and a line it does not change is kept as it was (the file's code
/// page too); a block is switched off by renaming its keyword (`[off_aigroup_depot_typgroup_2]`),
/// which both OMSI 2 and openOMSI pass over like any other keyword they do not know.
pub mod edit {
    /// One `[aigroup_depot_typgroup]` / `_2` block: the buses of one vehicle file a depot has.
    #[derive(Debug, Clone, PartialEq, Default)]
    pub struct TypBlock {
        /// The depot group it follows (`[aigroup_depot]`'s name; empty before any).
        pub depot: String,
        /// The vehicle file as written in the block.
        pub file: String,
        /// The `_2` form (number, plate, repaint and the dates, tab-separated).
        pub v2: bool,
        pub enabled: bool,
        /// The entry lines (fleet numbers), as written.
        pub entries: Vec<String>,
        /// Line indices of its keyword and of its `[end]`.
        pub head: usize,
        pub end: usize,
    }

    fn lines_of(bytes: &[u8]) -> Vec<&[u8]> {
        bytes.split_inclusive(|b| *b == b'\n').collect()
    }

    fn text(line: &[u8]) -> String {
        String::from_utf8_lossy(line).trim().to_string()
    }

    /// The line's keyword (lower case, without the brackets), when it is one.
    fn keyword(line: &[u8]) -> Option<String> {
        let t = text(line).to_ascii_lowercase();
        let t = t.strip_prefix('[')?.strip_suffix(']')?;
        Some(t.to_string())
    }

    /// The names of the depot groups (`[aigroup_depot]`), in file order.
    pub fn depots(bytes: &[u8]) -> Vec<String> {
        let lines = lines_of(bytes);
        (0..lines.len()).filter(|&i| keyword(lines[i]).as_deref() == Some("aigroup_depot")).filter_map(|i| lines.get(i + 1)).map(|l| text(l)).collect()
    }

    /// Every typgroup block of the file, switched off ones too.
    pub fn typ_blocks(bytes: &[u8]) -> Vec<TypBlock> {
        let lines = lines_of(bytes);
        let mut out = Vec::new();
        let mut depot = String::new();
        let mut i = 0;
        while i < lines.len() {
            let Some(k) = keyword(lines[i]) else {
                i += 1;
                continue;
            };
            if k == "aigroup_depot" {
                depot = lines.get(i + 1).map(|l| text(l)).unwrap_or_default();
                i += 3;
                continue;
            }
            let (enabled, base) = match k.strip_prefix("off_") {
                Some(b) => (false, b.to_string()),
                None => (true, k.clone()),
            };
            if base == "aigroup_depot_typgroup" || base == "aigroup_depot_typgroup_2" {
                let file = lines.get(i + 1).map(|l| text(l)).unwrap_or_default();
                let mut entries = Vec::new();
                let mut j = i + 2;
                while j < lines.len() && !text(lines[j]).eq_ignore_ascii_case("[end]") {
                    let l = text(lines[j]);
                    if !l.is_empty() {
                        entries.push(String::from_utf8_lossy(lines[j]).trim_end_matches(['\r', '\n']).to_string());
                    }
                    j += 1;
                }
                out.push(TypBlock { depot: depot.clone(), file, v2: base.ends_with("_2"), enabled, entries, head: i, end: j.min(lines.len().saturating_sub(1)) });
                i = j + 1;
                continue;
            }
            i += 1;
        }
        out
    }

    fn eol(lines: &[&[u8]], at: usize) -> &'static [u8] {
        if lines.get(at).is_some_and(|l| l.ends_with(b"\r\n")) {
            b"\r\n"
        } else {
            b"\n"
        }
    }

    fn join(lines: &[Vec<u8>]) -> Vec<u8> {
        lines.concat()
    }

    /// The file with the block whose keyword is on line `head` switched on or off.
    pub fn set_enabled(bytes: &[u8], head: usize, on: bool) -> Vec<u8> {
        let mut lines: Vec<Vec<u8>> = lines_of(bytes).into_iter().map(|l| l.to_vec()).collect();
        let Some(line) = lines.get_mut(head) else { return bytes.to_vec() };
        let Some(open) = line.iter().position(|b| *b == b'[') else { return bytes.to_vec() };
        let rest = &line[open + 1..];
        let off = rest.len() >= 4 && rest[..4].eq_ignore_ascii_case(b"off_");
        if on && off {
            line.drain(open + 1..open + 5);
        } else if !on && !off {
            for (k, b) in b"off_".iter().enumerate() {
                line.insert(open + 1 + k, *b);
            }
        }
        join(&lines)
    }

    /// One more entry line at the end of the block that starts on line `head`.
    pub fn add_entry(bytes: &[u8], head: usize, entry: &str) -> Vec<u8> {
        let Some(b) = typ_blocks(bytes).into_iter().find(|b| b.head == head) else { return bytes.to_vec() };
        let lines = lines_of(bytes);
        let mut out: Vec<Vec<u8>> = lines.iter().map(|l| l.to_vec()).collect();
        let nl = eol(&lines, head);
        let mut new = entry.as_bytes().to_vec();
        new.extend_from_slice(nl);
        // (an [end] that is the file's last line without a newline keeps its place)
        out.insert(b.end.min(out.len()), new);
        join(&out)
    }

    /// The last entry line of the block that starts on line `head` taken out (at least one is
    /// left: a block without a bus is not a block).
    pub fn remove_last_entry(bytes: &[u8], head: usize) -> Vec<u8> {
        let Some(b) = typ_blocks(bytes).into_iter().find(|b| b.head == head) else { return bytes.to_vec() };
        if b.entries.len() <= 1 {
            return bytes.to_vec();
        }
        let lines = lines_of(bytes);
        let Some(at) = (b.head + 2..b.end).rev().find(|&k| !text(lines[k]).is_empty()) else { return bytes.to_vec() };
        let out: Vec<Vec<u8>> = lines.iter().enumerate().filter(|(k, _)| *k != at).map(|(_, l)| l.to_vec()).collect();
        join(&out)
    }

    /// A new typgroup of vehicle file `file` with `entries`, after the last block of depot group
    /// `depot` (right after the group's own lines when it has none). The file unchanged when
    /// there is no such depot group.
    pub fn add_block(bytes: &[u8], depot: &str, file: &str, entries: &[String]) -> Vec<u8> {
        let lines = lines_of(bytes);
        let Some(d) = (0..lines.len()).find(|&i| keyword(lines[i]).as_deref() == Some("aigroup_depot") && lines.get(i + 1).is_some_and(|l| text(l) == depot)) else { return bytes.to_vec() };
        let blocks = typ_blocks(bytes);
        let after = blocks.iter().filter(|b| b.depot == depot && b.head > d).map(|b| b.end + 1).max().unwrap_or(d + 3).min(lines.len());
        let nl = eol(&lines, d);
        let mut block: Vec<u8> = Vec::new();
        let mut push = |s: &str| {
            block.extend_from_slice(s.as_bytes());
            block.extend_from_slice(nl);
        };
        push("[aigroup_depot_typgroup_2]");
        push(file);
        for e in entries {
            push(e);
        }
        push("[end]");
        let mut out: Vec<Vec<u8>> = lines.iter().map(|l| l.to_vec()).collect();
        // (the line before ends without a newline: give it one)
        if after > 0 && !out[after - 1].ends_with(b"\n") {
            out[after - 1].extend_from_slice(nl);
        }
        out.insert(after, block);
        join(&out)
    }

    /// A `_2` entry line for fleet number `number` with plate `plate`.
    pub fn entry_line(number: &str, plate: &str) -> String {
        entry_line_paint(number, plate, "")
    }

    /// [ROLLBACK ailist-75] The same with the repaint (the paint scheme's name) in the third field.
    pub fn entry_line_paint(number: &str, plate: &str, paint: &str) -> String {
        format!("{number}\t{plate}\t{paint}\t\t")
    }

    /// The first fleet number of `offered` (number, plate) that the block's `entries` do not
    /// have yet as an entry line, else one past the highest number there.
    pub fn next_entry(entries: &[String], offered: &[(String, String)]) -> String {
        let used: Vec<String> = entries.iter().map(|e| e.split('\t').next().unwrap_or("").trim().to_ascii_lowercase()).collect();
        if let Some((n, p)) = offered.iter().find(|(n, _)| !used.contains(&n.trim().to_ascii_lowercase())) {
            return entry_line(n.trim(), p);
        }
        let top = used.iter().filter_map(|u| u.parse::<u32>().ok()).max().unwrap_or(0);
        entry_line(&format!("{:03}", top + 1), "")
    }

    /// [ROLLBACK ailist-77] `next_entry` with the repaint of the block's last entry: one more
    /// bus of a type is painted as the one before it.
    pub fn next_entry_like(entries: &[String], offered: &[(String, String)]) -> String {
        let line = next_entry(entries, offered);
        let paint = entries.iter().rev().find(|e| !e.trim().is_empty()).and_then(|e| e.split('\t').nth(2)).map(str::trim).unwrap_or("");
        if paint.is_empty() {
            return line;
        }
        let mut f: Vec<&str> = line.split('\t').collect();
        if f.len() > 2 {
            f[2] = paint;
        }
        f.join("\t")
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const FILE: &str = "[aigroup_depot_typgroup_2]\r\nvehicles\\a\\a.bus\r\n101\tAB 101\t\t\t\r\n102\tAB 102\t\t\t\r\n[end]\r\n\r\n[aigroup_depot]\r\nBusses\r\nmy.hof\r\n\r\n[aigroup_depot_typgroup_2]\r\nvehicles\\b\\b.bus\r\n201\tCD 201\t\t\t\r\n[end]\r\n";

        #[test]
        fn the_blocks_are_found_with_their_depot() {
            let b = typ_blocks(FILE.as_bytes());
            assert_eq!(b.len(), 2);
            assert_eq!((b[0].depot.as_str(), b[0].entries.len(), b[0].enabled), ("", 2, true));
            assert_eq!((b[1].depot.as_str(), b[1].file.as_str(), b[1].v2), ("Busses", "vehicles\\b\\b.bus", true));
            assert_eq!(depots(FILE.as_bytes()), vec!["Busses".to_string()]);
        }

        #[test]
        fn a_switched_off_block_is_passed_over_by_the_loader_and_comes_back() {
            let off = set_enabled(FILE.as_bytes(), typ_blocks(FILE.as_bytes())[1].head, false);
            let s = String::from_utf8(off.clone()).unwrap();
            assert!(s.contains("[off_aigroup_depot_typgroup_2]\r\n"));
            assert_eq!(typ_blocks(&off)[1].enabled, false);
            let parsed = super::super::AiLists::parse(&omsi_cfg::CfgFile::from_str("ailists.cfg", &s));
            assert_eq!(parsed.groups.iter().map(|g| g.typgroups.len()).sum::<usize>(), 0, "the first block is before any group, the second is off");
            let on = set_enabled(&off, typ_blocks(&off)[1].head, true);
            assert_eq!(on, FILE.as_bytes());
        }

        #[test]
        fn entries_are_added_and_taken_out_at_the_end_of_the_block() {
            let h = typ_blocks(FILE.as_bytes())[1].head;
            let more = add_entry(FILE.as_bytes(), h, &entry_line("202", "CD 202"));
            let b = typ_blocks(&more);
            assert_eq!(b[1].entries, vec!["201\tCD 201\t\t\t".to_string(), "202\tCD 202\t\t\t".to_string()]);
            assert_eq!(remove_last_entry(&more, h), FILE.as_bytes());
            assert_eq!(remove_last_entry(FILE.as_bytes(), h), FILE.as_bytes(), "the last bus stays");
        }

        #[test]
        fn a_bus_type_is_added_after_the_depot_s_blocks() {
            let out = add_block(FILE.as_bytes(), "Busses", "vehicles\\c\\c.bus", &[entry_line("301", "EF 301")]);
            let b = typ_blocks(&out);
            assert_eq!(b.len(), 3);
            assert_eq!((b[2].depot.as_str(), b[2].file.as_str(), b[2].entries.len()), ("Busses", "vehicles\\c\\c.bus", 1));
            assert_eq!(add_block(FILE.as_bytes(), "Nobody", "x.bus", &[]), FILE.as_bytes());
        }

        #[test]
        fn an_entry_can_name_its_repaint() {
            assert_eq!(entry_line_paint("5", "AB 5", "Red"), "5\tAB 5\tRed\t\t");
            let e = crate::DepotEntry::parse(&entry_line_paint("5", "AB 5", "Red"));
            assert_eq!((e.number.as_str(), e.registration.as_str(), e.paint.as_str()), ("5", "AB 5", "Red"));
        }

        #[test]
        fn one_more_bus_keeps_the_repaint_of_the_last() {
            let e = vec!["101\tAB 101\tRed\t\t".to_string()];
            assert_eq!(next_entry_like(&e, &[]), "102\t\tRed\t\t");
            assert_eq!(next_entry_like(&["101".to_string()], &[]), "102\t\t\t\t");
        }

        #[test]
        fn the_next_number_is_one_the_block_lacks() {
            let offered = vec![("101".to_string(), "AB 101".to_string()), ("103".to_string(), "AB 103".to_string())];
            assert_eq!(next_entry(&["101\tAB 101\t\t\t".to_string()], &offered), "103\tAB 103\t\t\t");
            assert_eq!(next_entry(&["101".to_string(), "7".to_string()], &[]), "102\t\t\t\t");
        }
    }
}
