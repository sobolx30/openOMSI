//! The bus radio as live internet radio. OMSI itself plays no music: the buses' radios
//! only set variables that plugins turned into sound - `Snd_Radio` (the cassette player of
//! the stock buses and many mods: 1 while it plays) and the Sound Extension radios'
//! `SndExt_Radio` (the station button pressed, 0 = off) with `SndVol_Radio` (the volume
//! knob). Here those variables tune in a list of internet stations, streamed while they
//! play (see `omsi_audio::radio`); Shift+R steps through the list.
//!
//! The stations are kept in `~/.openomsi/radio.cfg`, one `name = address` per line
//! (MP3, AAC or Ogg streams, or .m3u/.pls playlists), with `volume = 0..1`; the file is
//! written with a default list the first time. A map can bring stations of its own: a
//! `radio.cfg` of the same kind beside its global.cfg, whose stations come before the
//! player's (its `volume` is not read).

use glam::{DVec3, Vec3};
use omsi_audio::fx::RadioFx;
use omsi_audio::stream::{StreamBuf, StreamControl};
use omsi_audio::{AudioEngine, VoiceId, VoiceParams};
use std::path::PathBuf;
use std::sync::Arc;

const DEFAULT_STATIONS: &[(&str, &str)] = &[
    ("radioeins", "https://dispatcher.rndfnk.com/rbb/radioeins/live/mp3/mid"),
    ("Berlin 88.8", "https://dispatcher.rndfnk.com/rbb/rbb888/live/mp3/mid"),
    ("Deutschlandfunk", "https://st01.sslstream.dlf.de/dlf/01/128/mp3/stream.mp3"),
    ("SWR3", "https://liveradio.swr.de/sw282p3/swr3/play.mp3"),
    ("Европа Плюс", "https://ep256.hostingradio.ru:8052/europaplus256.mp3"),
    ("Русское Радио", "https://rusradio.hostingradio.ru/rusradio128.mp3"),
    ("Ретро FM", "https://retro.hostingradio.ru:8043/retro256.mp3"),
    ("Наше Радио", "https://nashe1.hostingradio.ru:80/nashe-128.mp3"),
    ("Radio Paradise", "https://stream.radioparadise.com/mp3-128"),
];

/// `radio.cfg` as it is written the first time: what it is for, the volume, the default list.
fn default_text() -> String {
    let mut text = String::from(
        "# Internet radio for the buses' radios: one station per line, `name = address`\n\
         # (an MP3, AAC or Ogg stream, or an .m3u/.pls playlist). A radio's station\n\
         # button n plays the n-th station, a cassette player the first; Shift+R in the\n\
         # game steps through the list. volume = 0..1.\n\
         volume = 0.7\n\
         # (The optional radio effects - speaker, hiss, lost reception - are in the game's\n\
         # Settings -> Sound.)\n",
    );
    for (n, u) in DEFAULT_STATIONS {
        text.push_str(&format!("{n} = {u}\n"));
    }
    text
}

/// The names of the settings in a radio.cfg (not stations).
fn is_setting(name: &str) -> bool {
    ["volume", "effects", "fx_speaker", "fx_noise", "fx_dropouts"].iter().any(|k| name.eq_ignore_ascii_case(k))
}

/// The station lines of a radio.cfg as they stand: (name, all behind the `=` - the address
/// and any frequencies after it).
fn station_lines(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(|l| l.trim().trim_start_matches('\u{feff}'))
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(n, v)| (n.trim().to_string(), v.trim().to_string()))
        .filter(|(n, v)| !is_setting(n) && !v.split('|').next().unwrap_or("").trim().is_empty())
        .collect()
}

/// `text` (a radio.cfg) with `list` for its stations: its comments and volume stay as they
/// are, the stations stand where its first one stood (after the rest when it had none). A
/// station without an address is left out, and a name loses what would end it or make the
/// line a comment (`=`, a line break, a leading `#`).
fn with_stations(text: &str, list: &[(String, String)]) -> String {
    let mut out = String::new();
    let mut placed = false;
    let put = |out: &mut String| {
        for (k, (name, value)) in list.iter().enumerate() {
            let value = value.replace(['\r', '\n'], " ");
            if value.split('|').next().unwrap_or("").trim().is_empty() {
                continue;
            }
            let name = name.replace(['=', '\r', '\n'], " ");
            let name = name.trim().trim_start_matches('#').trim();
            let name = if name.is_empty() { format!("Station {}", k + 1) } else { name.to_string() };
            out.push_str(&format!("{name} = {}\n", value.trim()));
        }
    };
    for line in text.lines() {
        if !station_lines(line).is_empty() {
            if !placed {
                put(&mut out);
                placed = true;
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    if !placed {
        put(&mut out);
    }
    out
}

/// The player's own stations (`radio.cfg`; the default list while there is none), as the
/// launcher's Sound settings show them to edit (#857).
pub(crate) fn own_stations() -> Vec<(String, String)> {
    station_lines(&config_path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_else(default_text))
}

/// Keep `list` as the player's own stations in `radio.cfg` (see `with_stations`): the game
/// tunes in to them when it starts.
pub(crate) fn save_stations(list: &[(String, String)]) -> std::io::Result<()> {
    let Some(p) = config_path() else { return Err(std::io::Error::other("no home folder")) };
    let text = std::fs::read_to_string(&p).unwrap_or_else(|_| default_text());
    std::fs::create_dir_all(p.parent().unwrap_or(&p))?;
    std::fs::write(&p, with_stations(&text, list))
}

fn config_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".openomsi").join("radio.cfg"))
}

/// The stream addresses an OMSI radio plugin keeps in its text files under `plugins`
/// (SuperRadio's `.opl` and its lists; whatever the layout, a line with an http(s) address is
/// a station, named by the text before the address or else by its host): "openOMSI does not
/// load the stations I defined in the .opl".
fn plugin_stations(dir: &std::path::Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut files = Vec::new();
    let mut walk = vec![(dir.to_path_buf(), 0)];
    while let Some((d, depth)) = walk.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && depth < 2 {
                walk.push((p, depth + 1));
            } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| matches!(x.to_ascii_lowercase().as_str(), "opl" | "cfg" | "ini" | "txt" | "m3u" | "pls")) {
                files.push(p);
            }
        }
    }
    files.sort();
    for f in files {
        let Ok(bytes) = std::fs::read(&f) else { continue };
        if bytes.len() > 1 << 20 {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        for line in text.lines() {
            let Some(at) = line.find("http://").or_else(|| line.find("https://")) else { continue };
            let url: String = line[at..].chars().take_while(|c| !c.is_whitespace() && !matches!(c, '"' | '\'' | ';' | ',' | '|')).collect();
            let before = line[..at].trim().trim_end_matches(['=', ':', '|', ',', ';', '"', '\'', '\t']).trim();
            let before = before.trim_start_matches(|c: char| c.is_ascii_digit() || matches!(c, '.' | ')' | '-' | ' '));
            let name = if before.is_empty() || before.len() > 60 {
                url.split('/').nth(2).unwrap_or(&url).to_string()
            } else {
                before.to_string()
            };
            if url.len() > 12 && !out.iter().any(|(_, u): &(String, String)| u == &url) {
                out.push((name, url));
            }
        }
    }
    out
}

/// A frequency a station is on (as a radio's display writes it: `94.6`), and where: a
/// place on the map in the game's own metres - x east, y north, as the game's log gives a
/// bus's position - near which that frequency is the one on air. Without a place it is
/// the station's frequency everywhere.
#[derive(Clone, Debug, PartialEq)]
struct Frequency {
    mhz: String,
    at: Option<(f64, f64)>,
}

/// `94.6`, `94.6 MHz` or `94.6 @ 25400, 990`.
fn parse_frequency(text: &str) -> Option<Frequency> {
    let (mhz, at) = match text.split_once('@') {
        Some((m, p)) => (m, Some(p)),
        None => (text, None),
    };
    let mhz = mhz.trim().to_ascii_lowercase();
    let mhz: f32 = mhz.trim_end_matches("mhz").trim().replace(',', ".").parse().ok().filter(|v: &f32| *v > 0.0 && v.is_finite())?;
    let at = match at {
        Some(p) => {
            let (x, y) = p.split_once(',')?;
            Some((x.trim().parse::<f64>().ok()?, y.trim().parse::<f64>().ok()?))
        }
        None => None,
    };
    Some(Frequency { mhz: format!("{mhz:.1}"), at })
}

/// The frequencies of the stations that name any, by address (lower case).
type Frequencies = std::collections::HashMap<String, Vec<Frequency>>;

/// Which of a station's frequencies is on air at (x, y): that of the nearest place, and
/// the one without a place where no place is given.
fn frequency_at(on: &[Frequency], x: f64, y: f64) -> Option<&str> {
    let near = on
        .iter()
        .filter_map(|f| f.at.map(|(fx, fy)| ((fx - x).powi(2) + (fy - y).powi(2), f)))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, f)| f);
    near.or_else(|| on.iter().find(|f| f.at.is_none())).map(|f| f.mhz.as_str())
}

/// What a radio.cfg says: its stations, its volume where one is set, and the frequencies
/// of the stations that name any (by address, lower case). A line is `name = address`,
/// and behind the address `| frequency` or `| frequency @ x, y` as often as the station
/// has frequencies along the map.
fn parse_stations(text: &str) -> (Vec<(String, String)>, Option<f32>, Frequencies) {
    let (mut stations, mut volume, mut frequencies) = (Vec::new(), None, Frequencies::new());
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else { continue };
        let (name, value) = (name.trim(), value.trim());
        if name.eq_ignore_ascii_case("volume") {
            volume = value.parse::<f32>().ok().map(|v| v.clamp(0.0, 1.0)).or(volume);
            continue;
        }
        if is_setting(name) {
            continue;
        }
        let mut parts = value.split('|');
        let url = parts.next().unwrap_or("").trim();
        if url.is_empty() {
            continue;
        }
        let on: Vec<Frequency> = parts.filter_map(parse_frequency).collect();
        if !on.is_empty() {
            frequencies.insert(url.to_ascii_lowercase(), on);
        }
        stations.push((name.to_string(), url.to_string()));
    }
    (stations, volume, frequencies)
}

/// A map's own stations: `radio.cfg` beside its global.cfg, written like the player's -
/// the stations a bus hears where the map plays, and the frequencies they are on there.
/// Its volume line counts for nothing: how loud the radio is stays the player's business.
fn map_stations(map_cfg: &std::path::Path) -> (Vec<(String, String)>, Frequencies) {
    let Some(dir) = map_cfg.parent() else { return Default::default() };
    let Ok(bytes) = omsi_cfg::vfs::read(&dir.join("radio.cfg")) else { return Default::default() };
    let (stations, _, frequencies) = parse_stations(&omsi_cfg::codepage::decode(&bytes));
    (stations, frequencies)
}

pub struct Radio {
    /// What the buttons play: the map's stations, then the player's.
    stations: Vec<(String, String)>,
    /// The player's own (radio.cfg and the radio plugins' lists).
    own: Vec<(String, String)>,
    /// The frequencies the stations are on: the player's file's, and the map's over them.
    frequencies: Frequencies,
    own_frequencies: Frequencies,
    /// The map the list was made for, and whether the list changed while a station played.
    map: String,
    relisted: bool,
    volume: f32,
    /// Shift+R: how far the list is turned from the bus's own station numbers.
    offset: usize,
    playing: Option<Playing>,
    /// The smoothed place of the sound: 0..1 how near the head is to the radio, and the
    /// balance -1..1 (see `update`).
    near: f32,
    bal: f32,
}

struct Playing {
    station: usize,
    buf: Arc<StreamBuf>,
    voice: VoiceId,
    /// The status last shown on screen (the song, "no signal" ...).
    shown: String,
    /// The line a text display runs through (see `Radio::display_text`), and since when.
    line: (String, std::time::Instant),
}

/// Where the radio sits against the driver's camera, in the bus's frame (metres: right,
/// forward, up): the dashboard, a little to the right of the driver and low.
const RADIO_OFFSET: Vec3 = Vec3::new(0.3, 0.5, -0.25);
/// Inside the cab the radio is at full volume within this distance of the head (metres).
const ROOM_RANGE: f32 = 2.5;
/// A radio this far to the side (metres) pulls the sound as far as `MAX_BALANCE`.
const BALANCE_SPAN: f32 = 1.5;
/// How far to the side the sound sits at most (1 = fully one ear's side; the far ear is
/// turned down by 40 % of this, see the mixer).
const MAX_BALANCE: f32 = 0.5;

/// Characters in a line of a radio's text display (the "Magnitola" radio of P3ta's SOR
/// buses and its kin: two lines of ten).
const DISPLAY_WIDTH: usize = 10;

/// A station's name and its song as one line a simple text display can show: plain Latin
/// letters (its fonts have little else), no `@` (the display's line break).
fn display_line(name: &str, status: &str) -> String {
    let waiting = status.is_empty() || status.ends_with('…') || status.eq_ignore_ascii_case(name);
    let text = if waiting { name.to_string() } else { format!("{name} - {status}") };
    let mut out = String::new();
    for c in text.chars() {
        let plain: &str = match c {
            '@' => " ",
            c if c.is_ascii() && !c.is_ascii_control() => {
                out.push(c);
                continue;
            }
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ą' => "a",
            'Á' | 'À' | 'Â' | 'Ä' | 'Ã' | 'Å' | 'Ą' => "A",
            'č' | 'ç' | 'ć' => "c",
            'Č' | 'Ç' | 'Ć' => "C",
            'ď' => "d",
            'Ď' => "D",
            'é' | 'ě' | 'è' | 'ê' | 'ë' | 'ę' => "e",
            'É' | 'Ě' | 'È' | 'Ê' | 'Ë' | 'Ę' => "E",
            'í' | 'ì' | 'î' | 'ï' => "i",
            'Í' | 'Ì' | 'Î' | 'Ï' => "I",
            'ľ' | 'ĺ' | 'ł' => "l",
            'Ľ' | 'Ĺ' | 'Ł' => "L",
            'ň' | 'ñ' | 'ń' => "n",
            'Ň' | 'Ñ' | 'Ń' => "N",
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ő' | 'ø' => "o",
            'Ó' | 'Ò' | 'Ô' | 'Ö' | 'Õ' | 'Ő' | 'Ø' => "O",
            'ř' | 'ŕ' => "r",
            'Ř' | 'Ŕ' => "R",
            'š' | 'ś' => "s",
            'Š' | 'Ś' => "S",
            'ß' => "ss",
            'ť' => "t",
            'Ť' => "T",
            'ú' | 'ù' | 'û' | 'ü' | 'ů' | 'ű' => "u",
            'Ú' | 'Ù' | 'Û' | 'Ü' | 'Ů' | 'Ű' => "U",
            'ý' | 'ÿ' => "y",
            'Ý' => "Y",
            'ž' | 'ź' | 'ż' => "z",
            'Ž' | 'Ź' | 'Ż' => "Z",
            '–' | '—' => "-",
            '’' | '‘' => "'",
            '…' => "...",
            _ => "",
        };
        out.push_str(plain);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `width` characters of `text` at `seconds` after it came up: all of a text that fits,
/// else the text running through from right to left, four characters a second after a
/// moment at its beginning, with a gap before it comes round again.
fn marquee(text: &str, width: usize, seconds: f32) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width {
        return format!("{text:<width$}");
    }
    let round = chars.len() + 3;
    let at = ((seconds - 1.5).max(0.0) * 4.0) as usize % round;
    (0..width).map(|i| chars.get((at + i) % round).copied().unwrap_or(' ')).collect()
}

impl Radio {
    pub fn load(omsi_root: &std::path::Path) -> Radio {
        let mut stations: Vec<(String, String)>;
        let mut volume = 0.7f32;
        let mut frequencies = Frequencies::new();
        let path = config_path();
        match path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()) {
            Some(text) => {
                let (list, vol, on) = parse_stations(&text);
                stations = list;
                volume = vol.unwrap_or(volume);
                frequencies = on;
            }
            None => {
                stations = DEFAULT_STATIONS.iter().map(|(n, u)| (n.to_string(), u.to_string())).collect();
                if let Some(p) = &path {
                    let _ = std::fs::create_dir_all(p.parent().unwrap_or(p));
                    let _ = std::fs::write(p, default_text());
                }
            }
        }
        // the stations of OMSI's radio plugins (SuperRadio and the like), after the own ones
        let own = stations.len();
        for (name, url) in plugin_stations(&omsi_root.join("plugins")) {
            if !stations.iter().any(|(_, u)| u.eq_ignore_ascii_case(&url)) {
                stations.push((name, url));
            }
        }
        if stations.len() > own {
            log::info!("radio: {} stations of the OMSI radio plugins added", stations.len() - own);
        }
        if !stations.is_empty() {
            log::info!("radio: {} stations", stations.len());
        }
        Radio { stations: stations.clone(), own: stations, frequencies: frequencies.clone(), own_frequencies: frequencies, map: String::new(), relisted: false, volume, offset: 0, playing: None, near: 1.0, bal: 0.0 }
    }

    /// The frequency the station that plays is on where the bus is (`94.6 MHz`), for a
    /// radio's display: of the places the station's frequencies are given for, the nearest
    /// to (x, y) counts; a frequency without a place is the station's everywhere else.
    /// None for a station without frequencies - the display then keeps what its script
    /// writes.
    pub fn frequency(&self, x: f64, y: f64) -> Option<String> {
        let p = self.playing.as_ref()?;
        let on = self.frequencies.get(&self.stations.get(p.station)?.1.to_ascii_lowercase())?;
        Some(format!("{} MHz", frequency_at(on, x, y)?))
    }

    /// The map that plays (its global.cfg under `root`): its radio.cfg's stations come
    /// first, on the first station buttons, and the player's own follow - less those the
    /// map names too.
    pub fn set_map(&mut self, root: &std::path::Path, map: &str) {
        if self.map == map {
            return;
        }
        self.map = map.to_string();
        let (mut list, on) = map_stations(&omsi_cfg::resolve_path(root, map));
        if !list.is_empty() {
            log::info!("radio: {} stations of the map, {} with their frequencies", list.len(), on.len());
        }
        self.frequencies = self.own_frequencies.clone();
        self.frequencies.extend(on);
        for s in &self.own {
            if !list.iter().any(|(_, u)| u.eq_ignore_ascii_case(&s.1)) {
                list.push(s.clone());
            }
        }
        if list != self.stations {
            self.stations = list;
            self.offset = 0;
            self.relisted = true;
        }
    }

    /// The station the bus's radio is tuned to, None while it is off.
    fn wanted(&self, v: &omsi_sim::VehicleInstance) -> Option<usize> {
        if self.stations.is_empty() {
            return None;
        }
        let button = match v.var("SndExt_Radio") {
            Some(x) if x >= 0.5 => Some(x.round() as usize - 1),
            _ => None,
        };
        let button = button.or_else(|| v.var("Snd_Radio").filter(|x| *x >= 0.5).map(|_| 0));
        button.map(|b| (b + self.offset) % self.stations.len())
    }

    /// Follow the player's bus radio; a text for the screen when the station or its song
    /// changes.
    ///
    /// The sound comes from the cab: a point beside and ahead of the driver's camera. Inside
    /// it is not spatial - it only gets a little quieter as the head moves away from the radio
    /// and sits a little to the side it is on. Both follow the *bus's* axes (not where the
    /// head looks) and are smoothed over about half a second, so turning the head or switching
    /// the cab views never swings the sound from one ear to the other. Outside it is a
    /// voice at that point, muffled by the bodywork. `ear` is the camera in the world, `dt`
    /// the frame time, `fx` the effects of the Sound settings.
    pub fn update(&mut self, audio: &AudioEngine, v: &omsi_sim::VehicleInstance, inside: bool, ear: Option<DVec3>, dt: f32, fx: RadioFx) -> Option<String> {
        let wanted = self.wanted(v);
        // (another map's list: the same button is another station now)
        if std::mem::take(&mut self.relisted) {
            self.stop(audio);
        }
        if self.playing.as_ref().map(|p| p.station) != wanted {
            self.stop(audio);
            if let Some(station) = wanted {
                let (name, url) = &self.stations[station];
                log::info!("radio: station {} {name} ({url})", station + 1);
                let buf = omsi_audio::radio::open(url);
                let voice = audio.play_stream(buf.clone(), VoiceParams { gain: 0.0, ..Default::default() });
                self.playing = Some(Playing { station, buf, voice, shown: String::new(), line: (String::new(), std::time::Instant::now()) });
            }
        }
        let p = self.playing.as_mut()?;
        // the volume knob, where the radio has one (0..1, some go to 2)
        let knob = v.var("SndVol_Radio").map(|x| x.clamp(0.0, 2.0)).unwrap_or(1.0);
        let gain = self.volume * knob;
        // the radio: by the driver's camera (right, forward, down), else in the front of the cab
        let def = &v.ty.def;
        let n = def.cameras_driver.len().max(1);
        let seat = def.cameras_driver.get(def.camera_std % n).or(def.cameras_driver.first()).map(|c| Vec3::new(c.pos[0], c.pos[1], c.pos[2]));
        let local = seat.map(|s| s + RADIO_OFFSET).unwrap_or(Vec3::new(0.0, 0.0, 1.5));
        let rot = v.body_rotation();
        let source = v.position + rot.transform_vector3(local).as_dvec3();
        let right = rot.transform_vector3(Vec3::X).as_dvec3();
        let (mut near_goal, mut bal_goal) = (1.0f32, 0.0f32);
        if let Some(ear) = ear.filter(|_| inside) {
            let d = source - ear;
            near_goal = (ROOM_RANGE / (d.length() as f32).max(ROOM_RANGE)).clamp(0.12, 1.0);
            bal_goal = (d.dot(right) as f32 / BALANCE_SPAN).clamp(-1.0, 1.0) * MAX_BALANCE;
        }
        // (about half a second to settle; a jump of the view moves it gently)
        let k = 1.0 - (-dt.clamp(0.0, 0.25) / 0.55).exp();
        self.near += (near_goal - self.near) * k;
        self.bal += (bal_goal - self.bal) * k;
        // heard through the bodywork from outside
        let params = if inside {
            VoiceParams { gain: gain * self.near, ..Default::default() }
        } else {
            VoiceParams { gain, position: Some(source.as_vec3()), range: 4.0, lowpass_hz: 700.0, ..Default::default() }
        };
        audio.set_params(p.voice, params);
        p.buf.set_control(StreamControl { balance: if inside { self.bal } else { 0.0 }, fx: fx.clamped() });
        let status = p.buf.status();
        if status != p.shown && !status.is_empty() && status != "connecting …" {
            p.shown = status.clone();
            let line = format!("Radio {}: {} - {status}", p.station + 1, self.stations[p.station].0);
            log::info!("{line}");
            return Some(line);
        }
        None
    }

    /// The station that plays and its stream title, for the scripts' `GetRadioName` and
    /// `GetRadioSong`: the name the station gives itself (else the one in the list), and the
    /// song or whatever text it sends. Both empty while the radio is off.
    pub fn now_playing(&self) -> (String, String) {
        let Some(p) = self.playing.as_ref() else { return Default::default() };
        let (name, song) = p.buf.info();
        let name = if name.trim().is_empty() { self.stations.get(p.station).map(|s| s.0.clone()).unwrap_or_default() } else { name };
        (name, song)
    }

    /// What a radio with a text display shows now (`VehicleInstance::radio_text`): the
    /// station and its song, running through the line where they do not fit. Empty while
    /// the radio is off; None without stations (the scripts' own texts then stay).
    pub fn display_text(&mut self) -> Option<String> {
        if self.stations.is_empty() {
            return None;
        }
        let Some(p) = self.playing.as_mut() else { return Some(String::new()) };
        let line = display_line(&self.stations[p.station].0, &p.buf.status());
        if p.line.0 != line {
            p.line = (line, std::time::Instant::now());
        }
        Some(marquee(&p.line.0, DISPLAY_WIDTH, p.line.1.elapsed().as_secs_f32()))
    }

    /// Shift+R: the next station of the list on every button.
    pub fn next_station(&mut self) -> String {
        if self.stations.is_empty() {
            return "No radio stations (see ~/.openomsi/radio.cfg)".into();
        }
        self.offset = (self.offset + 1) % self.stations.len();
        match &self.playing {
            Some(p) => {
                let next = (p.station + 1) % self.stations.len();
                format!("Radio {}: {}", next + 1, self.stations[next].0)
            }
            None => "The bus radio is off (switch it on in the cockpit)".into(),
        }
    }

    pub fn stop(&mut self, audio: &AudioEngine) {
        if let Some(p) = self.playing.take() {
            p.buf.close();
            audio.stop(p.voice);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_station_list_edited_keeps_the_files_comments_volume_and_frequencies() {
        let text = "# my stations\nvolume = 0.4\nOne = https://a.example/one.mp3 | 94.6 @ 100, 200\n# a note\nTwo = https://b.example/two\n";
        let list = station_lines(text);
        assert_eq!(list, vec![("One".to_string(), "https://a.example/one.mp3 | 94.6 @ 100, 200".to_string()), ("Two".to_string(), "https://b.example/two".to_string())]);
        // the second removed, two added (one of them still without an address)
        let edited = vec![list[0].clone(), ("#Jazz = FM".to_string(), "https://c.example/jazz.m3u".to_string()), ("Later".to_string(), String::new())];
        let out = with_stations(text, &edited);
        assert_eq!(out, "# my stations\nvolume = 0.4\nOne = https://a.example/one.mp3 | 94.6 @ 100, 200\nJazz   FM = https://c.example/jazz.m3u\n# a note\n");
        let (stations, volume, frequencies) = parse_stations(&out);
        assert_eq!(stations.iter().map(|s| s.0.as_str()).collect::<Vec<_>>(), ["One", "Jazz   FM"]);
        assert_eq!(volume, Some(0.4));
        assert_eq!(frequencies.len(), 1);
        // a file without stations gets them after what it has
        assert_eq!(with_stations("volume = 1\n", &edited[..1]), "volume = 1\nOne = https://a.example/one.mp3 | 94.6 @ 100, 200\n");
        assert_eq!(station_lines(&default_text()).len(), DEFAULT_STATIONS.len());
    }

    #[test]
    fn the_old_effect_lines_of_a_radio_cfg_are_no_stations() {
        let text = "effects = light\nfx_noise = 0.3\nOne = http://a.example/x.mp3\n";
        assert_eq!(station_lines(text).len(), 1);
        assert_eq!(parse_stations(text).0.len(), 1);
        assert!(station_lines(&default_text()).iter().all(|(n, _)| !is_setting(n)));
    }

    #[test]
    fn a_text_display_gets_plain_letters_and_no_line_break() {
        assert_eq!(display_line("Evropa 2", ""), "Evropa 2");
        assert_eq!(display_line("Evropa 2", "buffering …"), "Evropa 2");
        assert_eq!(display_line("Evropa 2", "evropa 2"), "Evropa 2");
        assert_eq!(display_line("Český rozhlas", "Dvořák – Žalm č. 23"), "Cesky rozhlas - Dvorak - Zalm c. 23");
        assert_eq!(display_line("me@radio", "Наше Радио"), "me radio -");
    }

    #[test]
    fn a_maps_stations_come_first_and_its_volume_counts_for_nothing() {
        let dir = std::env::temp_dir().join(format!("omsi_map_radio_{}", std::process::id()));
        let map = dir.join("maps").join("Mesto");
        std::fs::create_dir_all(&map).unwrap();
        std::fs::write(map.join("radio.cfg"), "# the town's stations\nvolume = 0.1\nMestske radio = http://example.org/mesto.mp3\nSecond = http://example.org/own.mp3\n").unwrap();
        let own = vec![("Mine".to_string(), "http://example.org/mine.mp3".to_string()), ("Own".to_string(), "http://EXAMPLE.org/own.mp3".to_string())];
        let mut r = Radio { stations: own.clone(), own, frequencies: Default::default(), own_frequencies: Default::default(), map: String::new(), relisted: false, volume: 0.7, offset: 2, playing: None, near: 1.0, bal: 0.0 };
        r.set_map(&dir, "maps/Mesto/global.cfg");
        let names: Vec<&str> = r.stations.iter().map(|s| s.0.as_str()).collect();
        assert_eq!(names, ["Mestske radio", "Second", "Mine"]);
        assert_eq!((r.volume, r.offset, r.relisted), (0.7, 0, true));
        // a map without a radio.cfg: the player's own again
        r.set_map(&dir, "maps/Jinde/global.cfg");
        assert_eq!(r.stations.iter().map(|s| s.0.as_str()).collect::<Vec<_>>(), ["Mine", "Own"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stations_frequencies_and_their_places() {
        let (stations, volume, on) = parse_stations(
            "volume = 0.5\nZurnal = http://example.org/Z.mp3 | 94.6 @ 25400, 990 | 90.9 MHz @ 2300,-720\nKiss = http://example.org/k.mp3 | 98,1\nPlain = http://example.org/p.mp3\nBad = http://example.org/b.mp3 | here @ 1,2 | 0\n",
        );
        assert_eq!(stations.iter().map(|s| s.1.as_str()).collect::<Vec<_>>(), ["http://example.org/Z.mp3", "http://example.org/k.mp3", "http://example.org/p.mp3", "http://example.org/b.mp3"]);
        assert_eq!(volume, Some(0.5));
        assert_eq!(on["http://example.org/z.mp3"], [Frequency { mhz: "94.6".into(), at: Some((25400.0, 990.0)) }, Frequency { mhz: "90.9".into(), at: Some((2300.0, -720.0)) }]);
        assert_eq!(on["http://example.org/k.mp3"], [Frequency { mhz: "98.1".into(), at: None }]);
        assert!(!on.contains_key("http://example.org/p.mp3") && !on.contains_key("http://example.org/b.mp3"));
        // the nearest place counts
        assert_eq!(frequency_at(&on["http://example.org/z.mp3"], 24000.0, 0.0), Some("94.6"));
        assert_eq!(frequency_at(&on["http://example.org/z.mp3"], 5000.0, 0.0), Some("90.9"));
        assert_eq!(frequency_at(&on["http://example.org/k.mp3"], 5000.0, 0.0), Some("98.1"));
    }

    #[test]
    fn a_long_line_runs_through_ten_characters() {
        assert_eq!(marquee("KISS", 10, 5.0), "KISS      ");
        let line = "Evropa 2 - Song";
        assert_eq!(marquee(line, 10, 0.0), "Evropa 2 -");
        assert_eq!(marquee(line, 10, 1.5), "Evropa 2 -");
        assert_eq!(marquee(line, 10, 2.0), "ropa 2 - S");
        // round again after the text and its gap: 18 characters, four a second
        assert_eq!(marquee(line, 10, 1.5 + 14.0 / 4.0), "g   Evropa");
        assert_eq!(marquee(line, 10, 1.5 + 18.0 / 4.0), "Evropa 2 -");
    }
}
