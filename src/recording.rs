//! Nagrywanie przejść: filtr odczytów GPS, statystyki (dystans, podejścia, czas ruchu, prędkości),
//! zapis bieżącego nagrania odporny na zamknięcie programu, zapisane przejścia i eksport
//! (GPX z czasem, FIT jako aktywność – do zegarków i serwisów Garmin, Suunto, Coros, Strava).
//!
//! Nagranie na dysku:
//!  - `nagrania/biezace.json` – stan bieżącego nagrania (nazwa, początek, przerwy),
//!  - `nagrania/biezace.csv` – surowe odczyty pisane przez stronę systemową (patrz [`crate::gps`]);
//!    po ponownym uruchomieniu punkty odtwarzają się z niego tym samym filtrem,
//!  - `nagrania/<id>.json` – zapisane przejścia (lista z podsumowaniem – w bibliotece).

use serde::{Deserialize, Serialize};

use crate::geo::haversine;
use crate::gps::Fix;

/// Punkt nagrania.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Point {
    pub t: f64,
    pub lon: f64,
    pub lat: f64,
    /// Wysokość do statystyk: z modelu terenu (dokładniejsza od GPS), inaczej z odbiornika.
    pub ele: f32,
    /// Wysokość z odbiornika.
    pub gps_ele: Option<f32>,
    pub acc: f32,
    pub speed: Option<f32>,
    /// Odcinek: rośnie po każdej przerwie (między odcinkami nie liczy się dystansu).
    pub seg: u32,
}

/// Podsumowanie przejścia.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stats {
    /// Dystans (m).
    pub dist: f64,
    pub up: f64,
    pub down: f64,
    /// Czas od startu do ostatniego punktu bez przerw (s).
    pub duration: f64,
    /// Czas w ruchu (s) – postoje nie są liczone.
    pub moving: f64,
    /// Największa prędkość (m/s, uśredniona z co najmniej 10 s).
    pub max_speed: f64,
    pub min_ele: f64,
    pub max_ele: f64,
}

impl Stats {
    /// Średnia prędkość w ruchu (m/s).
    pub fn avg_speed(&self) -> f64 {
        if self.moving > 1.0 {
            self.dist / self.moving
        } else {
            0.0
        }
    }
}

/// Próg wysokości dla podejść i zejść (m): drobne wahania pomiaru nie sumują się w setki metrów.
const ELE_STEP: f64 = 4.0;
/// Najmniejsza prędkość odcinka uznawanego za ruch (m/s ≈ 1 km/h).
const MOVING: f64 = 0.28;
/// Przerwa w odczytach, po której odcinek nie liczy się do czasu ruchu (s).
const GAP: f64 = 120.0;

/// Statystyki z punktów (po kolei).
pub fn stats(points: &[Point]) -> Stats {
    let mut s = Stats { min_ele: f64::INFINITY, max_ele: f64::NEG_INFINITY, ..Default::default() };
    if points.is_empty() {
        return Stats::default();
    }
    let mut anchor = points[0].ele as f64;
    let mut seg_start = 0usize;
    for (i, p) in points.iter().enumerate() {
        let e = p.ele as f64;
        s.min_ele = s.min_ele.min(e);
        s.max_ele = s.max_ele.max(e);
        if i == 0 {
            continue;
        }
        let a = &points[i - 1];
        if a.seg != p.seg {
            // po przerwie: nowy odcinek, wysokość odniesienia od nowa
            anchor = e;
            seg_start = i;
            continue;
        }
        let dt = (p.t - a.t).max(0.0);
        let d = haversine(a.lon, a.lat, p.lon, p.lat);
        s.dist += d;
        s.duration += dt.min(GAP * 10.0);
        if dt > 0.0 && dt <= GAP && d / dt >= MOVING {
            s.moving += dt;
        }
        // podejścia z histerezą
        if e - anchor >= ELE_STEP {
            s.up += e - anchor;
            anchor = e;
        } else if anchor - e >= ELE_STEP {
            s.down += anchor - e;
            anchor = e;
        }
        // prędkość z okna co najmniej 10 s (pojedyncze skoki pozycji jej nie zawyżają)
        let mut j = i;
        while j > seg_start && p.t - points[j].t < 10.0 {
            j -= 1;
        }
        let win = p.t - points[j].t;
        if j < i && win >= 10.0 && win <= 60.0 {
            let mut wd = 0.0;
            for k in j + 1..=i {
                wd += haversine(points[k - 1].lon, points[k - 1].lat, points[k].lon, points[k].lat);
            }
            s.max_speed = s.max_speed.max(wd / win);
        }
    }
    if !s.min_ele.is_finite() {
        s.min_ele = 0.0;
        s.max_ele = 0.0;
    }
    s
}

/// Ustawienia filtra odczytów.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Filter {
    /// Odczyty mniej dokładne niż tyle metrów są pomijane.
    pub max_acc: f32,
}

impl Default for Filter {
    fn default() -> Self {
        Filter { max_acc: 30.0 }
    }
}

/// Stan bieżącego nagrania na dysku (`biezace.json`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub id: u64,
    pub name: String,
    pub start: f64,
    /// Przerwy: początek i koniec (koniec `None` – trwa).
    pub pauses: Vec<(f64, Option<f64>)>,
    pub filter: Filter,
}

impl State {
    pub fn paused(&self) -> bool {
        self.pauses.last().is_some_and(|p| p.1.is_none())
    }

    fn paused_at(&self, t: f64) -> bool {
        self.pauses.iter().any(|&(a, b)| t >= a && b.map_or(true, |b| t < b))
    }

    /// Numer odcinka w chwili `t`: liczba przerw zakończonych przed nią.
    fn seg_at(&self, t: f64) -> u32 {
        self.pauses.iter().filter(|p| p.1.is_some_and(|b| b <= t)).count() as u32
    }

    /// Łączny czas przerw do chwili `now` (s).
    pub fn paused_time(&self, now: f64) -> f64 {
        self.pauses.iter().map(|&(a, b)| (b.unwrap_or(now) - a).max(0.0)).sum()
    }
}

fn state_path() -> std::path::PathBuf {
    crate::gps::dir().join("biezace.json")
}

/// Trwające nagranie.
pub struct Recorder {
    pub state: State,
    pub points: Vec<Point>,
    pub stats: Stats,
    /// Ostatni przyjęty odczyt (do filtra).
    last: Option<Fix>,
    /// Statystyki do przeliczenia (nowe punkty).
    dirty: bool,
    stats_at: std::time::Instant,
}

impl Recorder {
    pub fn new(id: u64, name: String, filter: Filter) -> Recorder {
        let state = State { id, name, start: crate::gps::now(), pauses: Vec::new(), filter };
        let _ = std::fs::create_dir_all(crate::gps::dir());
        // nowy dziennik
        let _ = std::fs::remove_file(crate::gps::journal_path());
        let r = Recorder { state, points: Vec::new(), stats: Stats::default(), last: None, dirty: false, stats_at: std::time::Instant::now() };
        r.save_state();
        r
    }

    /// Nagranie przerwane zamknięciem programu: stan i punkty z dziennika.
    pub fn recover(ele: &dyn Fn(f64, f64) -> Option<f32>) -> Option<Recorder> {
        let state: State = serde_json::from_slice(&std::fs::read(state_path()).ok()?).ok()?;
        let mut r = Recorder { state, points: Vec::new(), stats: Stats::default(), last: None, dirty: true, stats_at: std::time::Instant::now() };
        for f in crate::gps::read_journal(&crate::gps::journal_path()) {
            r.push(&f, ele);
        }
        // czas, gdy program (i nagrywanie) nie działał – jako przerwa, nie jako marsz
        let now = crate::gps::now();
        let last_t = r.points.last().map(|p| p.t).unwrap_or(r.state.start);
        if !r.state.paused() && now - last_t > 90.0 {
            r.state.pauses.push((last_t + 1.0, Some(now)));
            r.last = None;
            r.save_state();
        }
        r.refresh(true);
        Some(r)
    }

    pub fn save_state(&self) {
        let _ = std::fs::create_dir_all(crate::gps::dir());
        let tmp = state_path().with_extension("json.tmp");
        if std::fs::write(&tmp, serde_json::to_vec(&self.state).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, state_path());
        }
    }

    pub fn paused(&self) -> bool {
        self.state.paused()
    }

    pub fn set_paused(&mut self, on: bool) {
        let now = crate::gps::now();
        if on && !self.paused() {
            self.state.pauses.push((now, None));
        } else if !on && self.paused() {
            if let Some(p) = self.state.pauses.last_mut() {
                p.1 = Some(now);
            }
            // po przerwie nie łączymy linii z ostatnim punktem
            self.last = None;
        }
        self.save_state();
    }

    /// Czas nagrania bez przerw (s) – zegar na ekranie.
    pub fn elapsed(&self) -> f64 {
        let now = crate::gps::now();
        (now - self.state.start - self.state.paused_time(now)).max(0.0)
    }

    /// Odczyt z odbiornika: punkt trafia do nagrania, jeśli przejdzie filtr. Zwraca, czy przyjęty.
    pub fn push(&mut self, f: &Fix, ele: &dyn Fn(f64, f64) -> Option<f32>) -> bool {
        if !f.gnss || f.t < self.state.start - 5.0 || self.state.paused_at(f.t) {
            return false;
        }
        if f.acc > self.state.filter.max_acc {
            return false;
        }
        let seg = self.state.seg_at(f.t);
        if let Some(l) = self.last.filter(|_| self.points.last().is_some_and(|p| p.seg == seg)) {
            let dt = f.t - l.t;
            if dt <= 0.0 {
                return false;
            }
            let d = haversine(l.lon, l.lat, f.lon, f.lat);
            // skok pozycji (odbicia sygnału): szybciej niż 200 km/h
            if d / dt > 55.0 {
                return false;
            }
            // drżenie w miejscu: punkt dopiero po wyraźnym przesunięciu (albo co minutę – czas postoju)
            let need = (f.acc.max(l.acc) as f64 * 0.6).clamp(3.0, 15.0);
            if d < need && dt < 60.0 {
                return false;
            }
        }
        let dem = ele(f.lon, f.lat);
        self.points.push(Point { t: f.t, lon: f.lon, lat: f.lat, ele: dem.or(f.alt).unwrap_or(0.0), gps_ele: f.alt, acc: f.acc, speed: f.speed, seg });
        self.last = Some(*f);
        self.dirty = true;
        true
    }

    /// Przeliczenie statystyk (najwyżej raz na sekundę, chyba że `force`).
    pub fn refresh(&mut self, force: bool) {
        if self.dirty && (force || self.stats_at.elapsed().as_secs_f32() >= 1.0) {
            self.stats = stats(&self.points);
            self.dirty = false;
            self.stats_at = std::time::Instant::now();
        }
    }

    /// Koniec nagrania: zapisane przejście (albo `None`, gdy nic nie nagrano) i sprzątanie stanu.
    pub fn finish(mut self, keep: bool) -> Option<Recording> {
        self.refresh(true);
        let _ = std::fs::remove_file(state_path());
        let _ = std::fs::remove_file(crate::gps::journal_path());
        if !keep || self.points.len() < 2 {
            return None;
        }
        let end = self.points.last().map(|p| p.t).unwrap_or(self.state.start);
        Some(Recording { id: self.state.id, name: self.state.name, start: self.state.start, end, points: self.points, stats: self.stats, source: String::new() })
    }
}

/// Zapisane przejście (plik `nagrania/<id>.json`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Recording {
    pub id: u64,
    pub name: String,
    pub start: f64,
    pub end: f64,
    pub points: Vec<Point>,
    pub stats: Stats,
    /// Skąd pochodzi: puste – nagrane w GRANI, inaczej nazwa wczytanego pliku (zegarek, inna aplikacja).
    pub source: String,
}

fn file(id: u64) -> std::path::PathBuf {
    crate::gps::dir().join(format!("{id}.json"))
}

impl Recording {
    pub fn save(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(crate::gps::dir())?;
        let f = file(self.id);
        let tmp = f.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(self).unwrap_or_default())?;
        std::fs::rename(&tmp, &f)
    }

    pub fn load(id: u64) -> Option<Recording> {
        serde_json::from_slice(&std::fs::read(file(id)).ok()?).ok()
    }

    pub fn delete(id: u64) {
        let _ = std::fs::remove_file(file(id));
    }

    /// Ślad do mapy, profilu i eksportu jako trasa (dł., szer., wys.).
    pub fn track(&self) -> Vec<(f64, f64, f32)> {
        self.points.iter().map(|p| (p.lon, p.lat, p.ele)).collect()
    }

    /// Przejście z wczytanego śladu z czasami (zegarek, inna aplikacja).
    pub fn from_track(id: u64, name: &str, source: &str, pts: &[(f64, f64, f32, f64)]) -> Option<Recording> {
        if pts.len() < 2 {
            return None;
        }
        let points: Vec<Point> = pts.iter().map(|&(lon, lat, ele, t)| Point { t, lon, lat, ele, gps_ele: Some(ele), acc: 5.0, speed: None, seg: 0 }).collect();
        let stats = stats(&points);
        Some(Recording { id, name: name.into(), start: points[0].t, end: points[points.len() - 1].t, points, stats, source: source.into() })
    }

    /// GPX 1.1 z czasem i wysokością punktów (osobny odcinek po każdej przerwie).
    pub fn to_gpx(&self) -> String {
        let mut out = String::new();
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        out.push_str("<gpx version=\"1.1\" creator=\"GRAŃ — atlas szlaków 3D\" xmlns=\"http://www.topografix.com/GPX/1/1\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:schemaLocation=\"http://www.topografix.com/GPX/1/1 http://www.topografix.com/GPX/1/1/gpx.xsd\">\n");
        out.push_str(&format!("  <metadata><name>{}</name><time>{}</time></metadata>\n", crate::gpx::esc(&self.name), crate::gpx::iso(self.start)));
        out.push_str(&format!("  <trk><name>{}</name><type>hiking</type>\n    <trkseg>\n", crate::gpx::esc(&self.name)));
        for (i, p) in self.points.iter().enumerate() {
            if i > 0 && self.points[i - 1].seg != p.seg {
                out.push_str("    </trkseg>\n    <trkseg>\n");
            }
            out.push_str(&format!("      <trkpt lat=\"{:.7}\" lon=\"{:.7}\"><ele>{:.1}</ele><time>{}</time></trkpt>\n", p.lat, p.lon, p.ele, crate::gpx::iso(p.t)));
        }
        out.push_str("    </trkseg>\n  </trk>\n</gpx>\n");
        out
    }

    /// FIT jako aktywność (piesza wycieczka) – do importu w Garmin Connect, Suunto, Coros, Strava.
    pub fn to_fit(&self) -> Vec<u8> {
        crate::fit::export_activity(self)
    }
}

/// Domyślna nazwa nagrania: data i godzina startu (czas lokalny Polski) i najbliższe miejsce.
pub fn default_name(start: f64, near: Option<String>) -> String {
    let local = start + crate::sun::warsaw_offset_min(start) as f64 * 60.0;
    let d = crate::sun::civil_from_days((local / 86_400.0).floor() as i64);
    let secs = local.rem_euclid(86_400.0) as i64;
    let when = format!("{}.{:02}.{} {}:{:02}", d.d, d.m, d.y, secs / 3600, (secs % 3600) / 60);
    match near {
        Some(n) => format!("{n} · {when}"),
        None => tf!("Przejście {}", when),
    }
}

/// Czas w sekundach → „1:05:09” (zegar nagrania).
pub fn clock(s: f64) -> String {
    let s = s.max(0.0) as i64;
    format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

/// Tempo (min/km) z prędkości (m/s): „14:30 min/km”.
pub fn pace(speed: f64) -> String {
    if speed < 0.1 {
        return "—".into();
    }
    let s = (1000.0 / speed).round() as i64;
    if s >= 3600 {
        return "—".into();
    }
    tf!("{}:{:02} min/km", s / 60, s % 60)
}

/// Prędkość (m/s) → „4,2 km/h”.
pub fn kmh(speed: f64) -> String {
    tf!("{} km/h", crate::fmt::dec(speed * 3.6, 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fix(t: f64, lat: f64, lon: f64, acc: f32) -> Fix {
        Fix { t, lat, lon, alt: Some(1000.0), acc, vacc: None, speed: None, course: None, gnss: true }
    }

    fn rec() -> Recorder {
        Recorder { state: State { start: 0.0, ..Default::default() }, points: Vec::new(), stats: Stats::default(), last: None, dirty: false, stats_at: std::time::Instant::now() }
    }

    #[test]
    fn filter_drops_jitter_jumps_and_bad_accuracy() {
        let mut r = rec();
        let no = |_: f64, _: f64| None;
        assert!(r.push(&fix(1.0, 49.0, 20.0, 5.0), &no));
        // drżenie w miejscu
        assert!(!r.push(&fix(2.0, 49.000_01, 20.0, 5.0), &no));
        // słaba dokładność
        assert!(!r.push(&fix(3.0, 49.001, 20.0, 80.0), &no));
        // skok 10 km w sekundę
        assert!(!r.push(&fix(4.0, 49.1, 20.0, 5.0), &no));
        // zwykły krok ~11 m
        assert!(r.push(&fix(10.0, 49.0001, 20.0, 5.0), &no));
        // odczyt z sieci nie trafia do nagrania
        let mut n = fix(30.0, 49.0005, 20.0, 5.0);
        n.gnss = false;
        assert!(!r.push(&n, &no));
        assert_eq!(r.points.len(), 2);
    }

    #[test]
    fn pauses_split_segments() {
        let mut r = rec();
        r.state.pauses = vec![(100.0, Some(200.0))];
        let no = |_: f64, _: f64| None;
        assert!(r.push(&fix(10.0, 49.0, 20.0, 5.0), &no));
        assert!(r.push(&fix(50.0, 49.001, 20.0, 5.0), &no));
        assert!(!r.push(&fix(150.0, 49.002, 20.0, 5.0), &no), "w przerwie");
        assert!(r.push(&fix(250.0, 49.01, 20.0, 5.0), &no));
        assert_eq!(r.points[2].seg, 1);
        let s = stats(&r.points);
        // dystans tylko w odcinku przed przerwą (~111 m), bez skoku przez przerwę
        assert!(s.dist > 100.0 && s.dist < 120.0, "{}", s.dist);
    }

    #[test]
    fn ascent_ignores_noise() {
        let mut pts = Vec::new();
        for i in 0..200 {
            let noise = if i % 2 == 0 { 1.5 } else { -1.5 };
            pts.push(Point { t: i as f64 * 5.0, lon: 20.0, lat: 49.0 + i as f64 * 0.0001, ele: 1000.0 + i as f32 * 0.5 + noise, ..Default::default() });
        }
        let s = stats(&pts);
        // 200 punktów po 0,5 m = ~100 m podejścia, szum ±1,5 m nie dodaje setek metrów
        assert!(s.up > 85.0 && s.up < 110.0, "{}", s.up);
        assert!(s.down < 10.0);
        assert!(s.moving > 900.0);
        assert!(s.max_speed > 1.5 && s.max_speed < 3.0, "{}", s.max_speed);
    }

    #[test]
    fn formats() {
        assert_eq!(clock(3909.0), "1:05:09");
        assert_eq!(pace(1000.0 / 870.0), "14:30 min/km");
        assert_eq!(pace(0.0), "—");
    }
}
