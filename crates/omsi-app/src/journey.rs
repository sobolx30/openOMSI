//! The journey's timetable (#800, #1061): the stops of the duty's trips with their planned
//! and actual arrival and departure and how far off those were, as the logbooks of the
//! virtual bus companies keep them. It is written to the content folder's `Journeys` every
//! time the bus reaches or leaves a stop, so it is there however the session ends.

use crate::career::{EARLY_DEPARTURE, LATE_ARRIVAL};
use crate::schedule::{PlannedTrip, PlayerDuty};
use std::path::PathBuf;

/// One stop of a trip in the log.
struct Row {
    trip: usize,
    stop: usize,
    name: String,
    /// Planned arrival and departure, and the actual ones (s of the duty's day).
    plan: (f64, f64),
    arrived: Option<f64>,
    left: Option<f64>,
    /// The bus's odometer (km) when it arrived here, or left when it never arrived.
    odo: Option<f64>,
    /// The trip begins here (no arrival to tell) / ends here (no departure).
    first: bool,
    last: bool,
}

/// The width of the rules of a table.
const RULE: usize = 120;
/// What stands where a stop the bus never served has its real times.
const DASHES: &str = "---";

/// One stop of a trip as a table shows it.
pub(crate) struct RowView {
    pub(crate) name: String,
    /// Arrival (plan, real, difference), then departure (plan, real, difference).
    pub(crate) cells: [String; 6],
    /// The odometer (km), blank when the bus was not there.
    pub(crate) odo: String,
    pub(crate) status: &'static str,
    /// The stop was served (the bus arrived or left), arrived late, left early, was missed.
    pub(crate) served: bool,
    pub(crate) late: bool,
    pub(crate) early: bool,
    pub(crate) missed: bool,
}

/// A trip's table with its words, for the window after the trip and for its export.
pub(crate) struct TripSummary {
    /// The lines above the trips (driver, map, bus, date; line and tour).
    pub(crate) head: String,
    /// The trip's title (number, line, terminus, departure).
    pub(crate) title: String,
    pub(crate) rows: Vec<RowView>,
    pub(crate) footer: String,
    /// All of it as plain text: what is copied and saved.
    pub(crate) text: String,
}

/// The table's head line and one of its lines, in the fixed columns of the log.
fn table_header() -> String {
    format!("{:<30}  {:>9} {:>9} {:>10}   {:>9} {:>9} {:>10}   {:>9}   Status", "Stop", "Arr. plan", "Arr. real", "Diff", "Dep. plan", "Dep. real", "Diff", "Odo km")
}

fn table_row(r: &RowView) -> String {
    let name: String = r.name.chars().take(30).collect();
    let c = &r.cells;
    format!("{name:<30}  {:>9} {:>9} {:>10}   {:>9} {:>9} {:>10}   {:>9}   {}", c[0], c[1], c[2], c[3], c[4], c[5], r.odo, r.status).trim_end().to_string()
}

pub(crate) struct Journey {
    /// The duty it is the log of: line, tour and where its trips begin in the tour.
    key: (String, String, usize),
    /// The lines above the trips (driver, map, bus, date; line and tour).
    head: String,
    /// Each trip's title.
    trips: Vec<String>,
    rows: Vec<Row>,
    dir: PathBuf,
    /// The file, chosen when there is first something to write.
    path: Option<PathBuf>,
}

impl Journey {
    /// The log of the duty `key` (line, tour, where its `trips` begin in the tour), nothing
    /// driven yet; `head` comes first in it.
    fn new(key: (String, String, usize), trips_planned: &[PlannedTrip], head: &str, dir: PathBuf) -> Journey {
        let mut trips = Vec::new();
        let mut rows = Vec::new();
        for (t, trip) in trips_planned.iter().enumerate() {
            let line = if trip.line.trim().is_empty() { key.0.trim() } else { trip.line.trim() };
            trips.push(format!("Trip {}  ·  {} to {}  ·  {}", key.2 + t + 1, line, trip.terminus.trim(), hms(trip.departure)));
            let n = trip.stops.len();
            // (the stations a depot run passes are not stops)
            for (k, s) in trip.stops.iter().enumerate().filter(|(_, s)| s.stops) {
                rows.push(Row { trip: t, stop: k, name: s.name.trim().to_string(), plan: (s.arr, s.dep), arrived: None, left: None, odo: None, first: k == 0, last: k + 1 == n });
            }
        }
        let head = format!("{}\nLine {}, tour {}", head.trim_end(), key.0.trim(), key.1.trim());
        Journey { key, head, trips, rows, dir, path: None }
    }

    /// The bus arrived at stop `at` (trip, stop) `late` s after its time (negative: early).
    /// True when that is news.
    fn arrived(&mut self, at: (usize, usize), late: f64) -> bool {
        match self.row(at) {
            Some(r) if r.arrived.is_none() => {
                r.arrived = Some(r.plan.0 + late);
                true
            }
            _ => false,
        }
    }

    /// The bus left stop `at`, having arrived and left `late` s after its times.
    fn left(&mut self, at: (usize, usize), late: (f64, f64)) -> bool {
        let Some(r) = self.row(at) else { return false };
        r.arrived.get_or_insert(r.plan.0 + late.0);
        r.left = Some(r.plan.1 + late.1);
        true
    }

    /// The odometer read `km` when the bus was at stop `at` (the first reading counts).
    fn stamp(&mut self, at: (usize, usize), km: f64) {
        if let Some(r) = self.row(at) {
            r.odo.get_or_insert(km);
        }
    }

    fn row(&mut self, (trip, stop): (usize, usize)) -> Option<&mut Row> {
        self.rows.iter_mut().find(|r| r.trip == trip && r.stop == stop)
    }

    /// The stops of trip `t` as a table shows them. `finished`: the trip is over, so every stop the
    /// bus did not serve was missed ("---"); while it is under way only those the bus has driven
    /// past are.
    fn trip_rows(&self, t: usize, finished: bool) -> Vec<RowView> {
        let rows: Vec<&Row> = self.rows.iter().filter(|r| r.trip == t).collect();
        let none = || [String::new(), String::new(), String::new()];
        let missed_cells = |plan: f64| [hms(plan), DASHES.to_string(), DASHES.to_string()];
        let mut out = Vec::new();
        for (i, r) in rows.iter().enumerate() {
            let served = r.arrived.is_some() || r.left.is_some();
            let later = rows[i + 1..].iter().any(|x| x.arrived.is_some() || x.left.is_some());
            let missed = !served && (finished || later);
            // (the first stop has no arrival to tell, the last no departure)
            let arr = if r.first { none() } else if missed { missed_cells(r.plan.0) } else { times(r.plan.0, r.arrived) };
            let dep = if r.last { none() } else if missed { missed_cells(r.plan.1) } else { times(r.plan.1, r.left) };
            let late = !r.first && r.arrived.is_some_and(|a| (a - r.plan.0).round() > LATE_ARRIVAL);
            let early = !r.last && r.left.is_some_and(|l| (l - r.plan.1).round() < EARLY_DEPARTURE);
            let status = if served {
                match (late, early) {
                    (true, true) => "late, left early",
                    (true, false) => "late",
                    (false, true) => "left early",
                    _ => "OK",
                }
            } else if missed {
                "missed"
            } else {
                ""
            };
            let [a0, a1, a2] = arr;
            let [d0, d1, d2] = dep;
            out.push(RowView { name: r.name.clone(), cells: [a0, a1, a2, d0, d1, d2], odo: r.odo.map(|k| format!("{k:.1}")).unwrap_or_default(), status, served, late, early, missed });
        }
        out
    }

    /// The footer under a trip's table: the stops served, the late ones, the early departures and
    /// the distance the odometer says the bus drove between its first and last reading.
    fn trip_footer(&self, t: usize, rows: &[RowView]) -> String {
        let served = rows.iter().filter(|r| r.served).count();
        let late = rows.iter().filter(|r| r.late).count();
        let early = rows.iter().filter(|r| r.early).count();
        let mut out = format!("Stops served: {served} of {}, late: {late}, left early: {early}", rows.len());
        let kms: Vec<f64> = self.rows.iter().filter(|r| r.trip == t).filter_map(|r| r.odo).collect();
        if let (Some(a), Some(b)) = (kms.first(), kms.last()) {
            if b >= a {
                out.push_str(&format!(", distance: {:.1} km", b - a));
            }
        }
        out
    }

    /// Trip `t` as a summary: for the window after the trip and its export. `finished`: see
    /// `trip_rows`. None for a trip the log does not have.
    pub(crate) fn trip_summary(&self, t: usize, finished: bool) -> Option<TripSummary> {
        let title = self.trips.get(t)?.clone();
        let rows = self.trip_rows(t, finished);
        if rows.is_empty() {
            return None;
        }
        let footer = self.trip_footer(t, &rows);
        let rule = "-".repeat(RULE);
        let mut text = format!("openOMSI trip summary\n{}\n\n{title}\n{rule}\n{}\n{rule}\n", self.head, table_header());
        for r in &rows {
            text.push_str(&table_row(r));
            text.push('\n');
        }
        text.push_str(&format!("{rule}\n{footer}\n"));
        Some(TripSummary { head: self.head.clone(), title, rows, footer, text })
    }

    /// Write `text` (trip `t`'s summary) to the log's folder as a file of its own, named by the real
    /// date and time and the line, tour and trip; the path it was written to.
    pub(crate) fn save_trip_summary(&self, t: usize, text: &str) -> Result<PathBuf, String> {
        let (y, mo, d, h, mi) = omsi_launcher_lib::local_now().unwrap_or((1970, 1, 1, 0, 0));
        let clean = |s: &str| s.trim().chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect::<String>();
        let stem = format!("{y:04}-{mo:02}-{d:02} {h:02}-{mi:02} summary line {} tour {} trip {}", clean(&self.key.0), clean(&self.key.1), self.key.2 + t + 1);
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("{}: {e}", self.dir.display()))?;
        // (another file of the same minute keeps its own)
        let path = (1..100).map(|n| self.dir.join(if n == 1 { format!("{stem}.txt") } else { format!("{stem} ({n}).txt") })).find(|p| !p.exists()).ok_or_else(|| "too many files of the same name".to_string())?;
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    }

    /// The log as it is written.
    fn text(&self) -> String {
        let rule = "-".repeat(RULE);
        let mut out = format!("openOMSI journey log\n{}\n", self.head);
        let (mut served, mut late, mut early) = (0, 0, 0);
        for (t, title) in self.trips.iter().enumerate() {
            // (a trip is in it once the bus has reached one of its stops: a tour has dozens)
            if self.rows.iter().filter(|r| r.trip == t).all(|r| r.arrived.is_none() && r.left.is_none()) {
                continue;
            }
            out.push_str(&format!("\n{title}\n{rule}\n{}\n{rule}\n", table_header()));
            for r in self.trip_rows(t, false) {
                served += r.served as usize;
                late += r.late as usize;
                early += r.early as usize;
                out.push_str(&table_row(&r));
                out.push('\n');
            }
        }
        out.push_str(&format!("{rule}\nStops served: {served}, late: {late}, left early: {early}\n"));
        out
    }

    /// Write the log (its file is named by the real date and time it began, and the line and tour).
    fn write(&mut self) {
        if self.path.is_none() {
            let (y, mo, d, h, mi) = omsi_launcher_lib::local_now().unwrap_or((1970, 1, 1, 0, 0));
            let clean = |s: &str| s.trim().chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect::<String>();
            let stem = format!("{y:04}-{mo:02}-{d:02} {h:02}-{mi:02} line {} tour {}", clean(&self.key.0), clean(&self.key.1));
            if let Err(e) = std::fs::create_dir_all(&self.dir) {
                log::warn!("journey log: {}: {e}", self.dir.display());
                return;
            }
            // (another session begun in the same minute keeps its own)
            let path = (1..100).map(|n| self.dir.join(if n == 1 { format!("{stem}.txt") } else { format!("{stem} ({n}).txt") })).find(|p| !p.exists());
            let Some(path) = path else { return };
            log::info!("journey log: {}", path.display());
            self.path = Some(path);
        }
        if let Some(p) = self.path.as_ref() {
            if let Err(e) = std::fs::write(p, self.text()) {
                log::warn!("journey log: {}: {e}", p.display());
            }
        }
    }
}

/// Keep `slot` the log of `duty` and note in it what the duty's update did, written at once
/// when that changed it: the bus left the stop it was due at before the update, `due`,
/// `served` seconds late (arrival, departure: what `PlayerDuty::update` returned), and
/// arrived at the one it is due at now (`PlayerDuty::arrived`); `odo` is the bus's odometer
/// (km, see `odometer_km`), noted at the stop in either case. `head` (driver, map, bus,
/// date) is asked for when a log begins.
pub(crate) fn note(slot: &mut Option<Journey>, duty: &PlayerDuty, due: (usize, usize), served: Option<(f64, f64)>, odo: Option<f64>, root: &std::path::Path, head: impl FnOnce() -> String) {
    // (every frame: the key is compared, not made)
    if slot.as_ref().is_none_or(|j| (j.key.0.as_str(), j.key.1.as_str(), j.key.2) != (duty.line.as_str(), duty.tour.as_str(), duty.first_trip)) {
        let dir = crate::startup::content_dir().unwrap_or_else(|| root.to_path_buf()).join("Journeys");
        *slot = Some(Journey::new((duty.line.clone(), duty.tour.clone(), duty.first_trip), &duty.trips, &head(), dir));
    }
    let Some(j) = slot.as_mut() else { return };
    let left = served.is_some_and(|s| j.left(due, s));
    let here = (duty.trip_index, duty.next_stop);
    let arrived = duty.arrived().is_some_and(|late| j.arrived(here, late));
    // (the odometer is read where the news is: at the stop the bus arrived at or left)
    if let Some(km) = odo {
        if left {
            j.stamp(due, km);
        }
        if arrived {
            j.stamp(here, km);
        }
    }
    if left || arrived {
        j.write();
    }
}

/// The log's first line: the driver, the map, the bus and the game's date.
pub(crate) fn head(career: &crate::career::Career, map: &str, bus: &omsi_sim::VehicleInstance, clock: &omsi_sim::SimClock) -> String {
    let driver = career.driver.as_ref().map(|d| d.name.clone()).or_else(|| career.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()));
    let (day, month) = clock.day_month();
    let bus = format!("{} {}", bus.ty.def.manufacturer.trim(), bus.ty.def.type_name.trim());
    let mut parts: Vec<String> = driver.map(|d| format!("Driver {}", d.trim())).into_iter().collect();
    parts.extend([map.trim().to_string(), bus.trim().to_string(), format!("{:04}-{month:02}-{day:02}", clock.year)]);
    parts.join("  ·  ")
}

/// The bus's odometer in kilometres (`kmcounter_km` whole kilometres, `kmcounter_m` the metres of
/// the fraction, as the original keeps them), None for a bus that has none.
pub(crate) fn odometer_km(bus: &omsi_sim::VehicleInstance) -> Option<f64> {
    let km = bus.var("kmcounter_km")?;
    Some(km as f64 + bus.var("kmcounter_m").unwrap_or(0.0) as f64 / 1000.0)
}

/// A time of day as HH:MM:SS (a duty's times may run past midnight or before it).
fn hms(t: f64) -> String {
    let s = t.round().rem_euclid(86400.0) as i64;
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// Planned, actual and the difference (+ late, − early), blank while it has not happened.
fn times(plan: f64, real: Option<f64>) -> [String; 3] {
    match real {
        Some(r) => {
            let d = (r - plan).round() as i64;
            let a = d.abs();
            [hms(plan), hms(r), format!("{}{:02}:{:02}:{:02}", if d < 0 { "-" } else { "+" }, a / 3600, a / 60 % 60, a % 60)]
        }
        None => [hms(plan), String::new(), String::new()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::PlannedStop;

    fn trip(stops: &[(&str, f64)]) -> PlannedTrip {
        PlannedTrip {
            name: "5_A".into(),
            line: "5".into(),
            terminus: "Muzealna".into(),
            departure: stops[0].1,
            end: stops[stops.len() - 1].1,
            stops: stops.iter().map(|(n, t)| PlannedStop { object_id: 0, name: n.to_string(), arr: *t, dep: *t, position: None, dir: Default::default(), stops: true }).collect(),
        }
    }

    /// A trip driven stop by stop: the first stop tells its departure only, the last its
    /// arrival only, a stop driven past is missed, a late arrival says so - and the file
    /// has it all at every stop.
    #[test]
    fn the_log_has_every_stop_with_its_times() {
        let t0 = 8.0 * 3600.0;
        let trips = [trip(&[("Metro Mlociny", t0), ("Zajezdnia", t0 + 300.0), ("Prozy", t0 + 600.0), ("Muzealna", t0 + 900.0)]), trip(&[("Muzealna", t0 + 1200.0), ("Metro Mlociny", t0 + 2100.0)])];
        let dir = std::env::temp_dir().join(format!("omsi-journey-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut j = Journey::new(("5".into(), "1".into(), 0), &trips, "Driver Test · Grundorf · MAN SD77 · 1989-05-30", dir.clone());
        assert!(j.arrived((0, 0), 20.0));
        assert!(!j.arrived((0, 0), 30.0), "the first arrival counts");
        assert!(j.left((0, 0), (20.0, 57.0)));
        j.write();
        let path = j.path.clone().expect("written");
        assert!(std::fs::read_to_string(&path).unwrap().contains("08:00:57"));
        // four minutes late at the second stop, past the third, early at the last
        assert!(j.arrived((0, 1), 240.0));
        assert!(j.left((0, 1), (240.0, 270.0)));
        assert!(j.arrived((0, 3), -30.0));
        j.write();
        let text = std::fs::read_to_string(&path).unwrap();
        let line = |name: &str| text.lines().find(|l| l.starts_with(name)).unwrap_or_else(|| panic!("no {name} in\n{text}")).to_string();
        assert!(text.contains("Line 5, tour 1") && text.contains("Driver Test") && text.contains("Trip 1  ·  5 to Muzealna  ·  08:00:00"), "{text}");
        assert!(line("Metro Mlociny").contains("08:00:00  08:00:57  +00:00:57"), "{text}");
        assert!(line("Metro Mlociny").ends_with("OK"), "{text}");
        assert!(line("Zajezdnia").contains("08:05:00  08:09:00  +00:04:00"), "{text}");
        assert!(line("Zajezdnia").contains("08:05:00  08:09:30  +00:04:30"), "{text}");
        assert!(line("Zajezdnia").ends_with("late"), "{text}");
        assert!(line("Prozy").ends_with("missed"), "{text}");
        assert!(line("Muzealna").contains("08:15:00  08:14:30  -00:00:30"), "{text}");
        assert!(!text.contains("Trip 2"), "a trip not begun is not in it yet: {text}");
        assert!(text.contains("Stops served: 3, late: 1, left early: 0"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
        // a duty taken up at its second trip: the first is not in it
        let mut j = Journey::new(("5".into(), "1".into(), 0), &trips, "Grundorf", dir.clone());
        assert!(j.arrived((1, 0), -60.0));
        let text = j.text();
        assert!(text.contains("Trip 2  ·  5 to Muzealna  ·  08:20:00") && !text.contains("Trip 1"), "{text}");
    }

    /// A finished trip's summary: a stop the bus never served shows dashes where its real times
    /// would be, the odometer is read where the bus was, and the text has all of it.
    #[test]
    fn a_trips_summary_has_dashes_for_missed_stops_and_the_odometer() {
        let t0 = 8.0 * 3600.0;
        let trips = [trip(&[("A", t0), ("B", t0 + 300.0), ("C", t0 + 600.0), ("D", t0 + 900.0)])];
        let mut j = Journey::new(("5".into(), "1".into(), 0), &trips, "Driver Test", std::env::temp_dir());
        assert!(j.arrived((0, 0), 0.0));
        assert!(j.left((0, 0), (0.0, 10.0)));
        j.stamp((0, 0), 100.0);
        assert!(j.arrived((0, 1), 30.0));
        assert!(j.left((0, 1), (30.0, 40.0)));
        j.stamp((0, 1), 101.2);
        // C is driven past; the bus arrives at D
        assert!(j.arrived((0, 3), 20.0));
        j.stamp((0, 3), 103.5);
        let s = j.trip_summary(0, true).expect("a summary");
        let row = |n: &str| s.rows.iter().find(|r| r.name == n).unwrap();
        assert!(row("C").missed && row("C").cells[1] == "---" && row("C").cells[4] == "---" && row("C").status == "missed");
        assert!(row("C").odo.is_empty());
        assert_eq!(row("B").odo, "101.2");
        assert_eq!(row("D").cells[1], "08:15:20");
        assert!(s.footer.contains("Stops served: 3 of 4") && s.footer.contains("distance: 3.5 km"), "{}", s.footer);
        assert!(s.text.contains("openOMSI trip summary") && s.text.contains("Odo km") && s.text.contains("---"), "{}", s.text);
        // under way, a stop not yet reached is no miss
        let mut j2 = Journey::new(("5".into(), "1".into(), 0), &trips, "Driver Test", std::env::temp_dir());
        j2.arrived((0, 0), 0.0);
        let under_way = j2.trip_summary(0, false).unwrap();
        assert!(under_way.rows.iter().all(|r| !r.missed));
    }
}
