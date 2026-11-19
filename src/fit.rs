//! Eksport trasy jako kursu FIT (Garmin, Suunto, Wahoo, Coros): wiadomości file_id, course, lap,
//! event (start/stop), rekordy śladu z planowanym czasem przejścia (wirtualny partner) i punkty
//! kursu z nazwami (punkty trasy, szczyt). Format binarny według FIT SDK, little-endian.

use crate::graph::Route;

/// Początek czasu FIT: 1989-12-31 00:00:00 UTC.
const FIT_EPOCH: i64 = 631_065_600;

/// Typy podstawowe FIT.
const ENUM: u8 = 0x00;
const UINT8: u8 = 0x02;
const UINT16: u8 = 0x84;
const SINT32: u8 = 0x85;
const UINT32: u8 = 0x86;
const STRING: u8 = 0x07;
const UINT32Z: u8 = 0x8C;

const NAME_LEN: usize = 16;

/// Punkt kursu: nazwa i rodzaj (0 – ogólny, 1 – szczyt, 3 – woda, 4 – jedzenie, 5 – niebezpieczeństwo).
pub struct CoursePoint {
    pub name: String,
    pub lon: f64,
    pub lat: f64,
    pub kind: u8,
}

/// CRC-16 z FIT SDK.
pub fn crc(mut crc: u16, bytes: &[u8]) -> u16 {
    const T: [u16; 16] = [0x0000, 0xCC01, 0xD801, 0x1400, 0xF001, 0x3C00, 0x2800, 0xE401, 0xA001, 0x6C00, 0x7800, 0xB401, 0x5000, 0x9C01, 0x8801, 0x4400];
    for &b in bytes {
        let tmp = T[(crc & 0xF) as usize];
        crc = (crc >> 4) & 0x0FFF;
        crc = crc ^ tmp ^ T[(b & 0xF) as usize];
        let tmp = T[(crc & 0xF) as usize];
        crc = (crc >> 4) & 0x0FFF;
        crc = crc ^ tmp ^ T[((b >> 4) & 0xF) as usize];
    }
    crc
}

fn semicircles(deg: f64) -> i32 {
    (deg * (2_147_483_648.0 / 180.0)).round().clamp(i32::MIN as f64, i32::MAX as f64) as i32
}

/// Nazwa jako pole tekstowe stałej długości (UTF-8, ucięta na granicy znaku, z zerem na końcu).
fn name_field(s: &str) -> [u8; NAME_LEN] {
    let mut out = [0u8; NAME_LEN];
    let mut n = 0;
    for c in s.chars() {
        let mut buf = [0u8; 4];
        let b = c.encode_utf8(&mut buf).as_bytes();
        if n + b.len() > NAME_LEN - 1 {
            break;
        }
        out[n..n + b.len()].copy_from_slice(b);
        n += b.len();
    }
    out
}

struct Writer {
    data: Vec<u8>,
}

impl Writer {
    /// Wiadomość definicji: lokalny typ, numer globalny, pola (numer, rozmiar, typ).
    fn define(&mut self, local: u8, global: u16, fields: &[(u8, u8, u8)]) {
        self.data.push(0x40 | local);
        self.data.push(0); // zarezerwowane
        self.data.push(0); // little-endian
        self.data.extend_from_slice(&global.to_le_bytes());
        self.data.push(fields.len() as u8);
        for &(num, size, ty) in fields {
            self.data.extend_from_slice(&[num, size, ty]);
        }
    }

    fn header(&mut self, local: u8) {
        self.data.push(local);
    }

    fn u8(&mut self, v: u8) {
        self.data.push(v);
    }

    fn u16(&mut self, v: u16) {
        self.data.extend_from_slice(&v.to_le_bytes());
    }

    fn u32(&mut self, v: u32) {
        self.data.extend_from_slice(&v.to_le_bytes());
    }

    fn i32(&mut self, v: i32) {
        self.data.extend_from_slice(&v.to_le_bytes());
    }

    fn bytes(&mut self, v: &[u8]) {
        self.data.extend_from_slice(v);
    }
}

/// Plik FIT z kursem. `start_unix` – planowany początek (sekundy UTC); czasy kolejnych punktów
/// wynikają z modelu czasu przejścia trasy.
pub fn export(route: &Route, name: &str, points: &[CoursePoint], start_unix: i64) -> Vec<u8> {
    let tr = &route.track;
    let n = tr.lon.len();
    let ts0 = (start_unix - FIT_EPOCH).max(0) as u32;
    let ts = |i: usize| ts0 + (tr.t[i].max(0.0) as f64 * 3600.0).round() as u32;
    let dist = |i: usize| (tr.d3[i].max(0.0) as f64 * 100.0).round() as u32;
    let alt = |e: f32| ((e as f64 + 500.0) * 5.0).round().clamp(0.0, 65_534.0) as u16;
    let end_ts = if n > 0 { ts(n - 1) } else { ts0 };
    let total_ms = (end_ts - ts0).saturating_mul(1000);

    let mut w = Writer { data: Vec::with_capacity(64 + n * 17) };
    // file_id
    w.define(0, 0, &[(0, 1, ENUM), (1, 2, UINT16), (2, 2, UINT16), (3, 4, UINT32Z), (4, 4, UINT32)]);
    w.header(0);
    w.u8(6); // kurs
    w.u16(255); // producent: development
    w.u16(1);
    w.u32((ts0 % 0xFFFF_FFF0).max(1));
    w.u32(ts0);
    // course
    w.define(1, 31, &[(4, 1, ENUM), (5, NAME_LEN as u8, STRING)]);
    w.header(1);
    w.u8(17); // turystyka piesza
    w.bytes(&name_field(name));
    // lap z podsumowaniem
    w.define(2, 19, &[
        (253, 4, UINT32),
        (2, 4, UINT32),
        (3, 4, SINT32),
        (4, 4, SINT32),
        (5, 4, SINT32),
        (6, 4, SINT32),
        (7, 4, UINT32),
        (8, 4, UINT32),
        (9, 4, UINT32),
        (21, 2, UINT16),
        (22, 2, UINT16),
    ]);
    w.header(2);
    w.u32(ts0);
    w.u32(ts0);
    let (first, last) = if n > 0 { (0, n - 1) } else { (0, 0) };
    let lat = |i: usize| if n > 0 { semicircles(tr.lat[i]) } else { 0 };
    let lon = |i: usize| if n > 0 { semicircles(tr.lon[i]) } else { 0 };
    w.i32(lat(first));
    w.i32(lon(first));
    w.i32(lat(last));
    w.i32(lon(last));
    w.u32(total_ms);
    w.u32(total_ms);
    w.u32(if n > 0 { dist(n - 1) } else { 0 });
    w.u16(route.stats.up.round().clamp(0.0, 65_000.0) as u16);
    w.u16(route.stats.down.round().clamp(0.0, 65_000.0) as u16);
    // event: start
    w.define(3, 21, &[(253, 4, UINT32), (0, 1, ENUM), (1, 1, ENUM), (4, 1, UINT8)]);
    w.header(3);
    w.u32(ts0);
    w.u8(0); // timer
    w.u8(0); // start
    w.u8(0);
    // rekordy
    w.define(4, 20, &[(253, 4, UINT32), (0, 4, SINT32), (1, 4, SINT32), (2, 2, UINT16), (5, 4, UINT32)]);
    for i in 0..n {
        w.header(4);
        w.u32(ts(i));
        w.i32(semicircles(tr.lat[i]));
        w.i32(semicircles(tr.lon[i]));
        w.u16(alt(tr.e[i]));
        w.u32(dist(i));
    }
    // punkty kursu: najbliższy punkt śladu daje dystans i czas
    if n > 0 && !points.is_empty() {
        w.define(5, 32, &[(254, 2, UINT16), (1, 4, UINT32), (2, 4, SINT32), (3, 4, SINT32), (4, 4, UINT32), (5, 1, ENUM), (6, NAME_LEN as u8, STRING)]);
        let mut placed: Vec<(usize, &CoursePoint)> = points
            .iter()
            .map(|p| {
                let kx = p.lat.to_radians().cos();
                let i = (0..n)
                    .min_by(|&a, &b| {
                        let da = ((tr.lon[a] - p.lon) * kx).powi(2) + (tr.lat[a] - p.lat).powi(2);
                        let db = ((tr.lon[b] - p.lon) * kx).powi(2) + (tr.lat[b] - p.lat).powi(2);
                        da.total_cmp(&db)
                    })
                    .unwrap_or(0);
                (i, p)
            })
            .collect();
        placed.sort_by_key(|(i, _)| *i);
        for (k, (i, p)) in placed.into_iter().enumerate() {
            w.header(5);
            w.u16(k as u16);
            w.u32(ts(i));
            w.i32(semicircles(p.lat));
            w.i32(semicircles(p.lon));
            w.u32(dist(i));
            w.u8(p.kind);
            w.bytes(&name_field(&p.name));
        }
    }
    // event: koniec
    w.header(3);
    w.u32(end_ts);
    w.u8(0);
    w.u8(9); // stop_disable_all
    w.u8(0);

    finish(w)
}

/// Zamknięcie pliku FIT: nagłówek, dane, suma kontrolna.
fn finish(w: Writer) -> Vec<u8> {
    let mut out = Vec::with_capacity(w.data.len() + 16);
    out.push(14);
    out.push(0x20); // protokół 2.0
    out.extend_from_slice(&2132u16.to_le_bytes());
    out.extend_from_slice(&(w.data.len() as u32).to_le_bytes());
    out.extend_from_slice(b".FIT");
    let hcrc = crc(0, &out);
    out.extend_from_slice(&hcrc.to_le_bytes());
    out.extend_from_slice(&w.data);
    let c = crc(0, &out);
    out.extend_from_slice(&c.to_le_bytes());
    out
}

/// Nagrane przejście jako aktywność FIT (piesza wycieczka): file_id, zdarzenia startu i końca
/// (także przerw), rekordy z czasem, pozycją, wysokością, dystansem i prędkością, okrążenie,
/// sesja i aktywność – plik przyjmują Garmin Connect, Suunto, Coros i Strava.
pub fn export_activity(rec: &crate::recording::Recording) -> Vec<u8> {
    let pts = &rec.points;
    let fit_ts = |t: f64| (t.round() as i64 - FIT_EPOCH).max(0) as u32;
    let ts0 = fit_ts(rec.start);
    let end = fit_ts(rec.end.max(rec.start));
    let alt = |e: f32| (((e as f64) + 500.0) * 5.0).round().clamp(0.0, 4_000_000_000.0) as u32;
    let st = &rec.stats;
    let elapsed_ms = ((end - ts0) as f64 * 1000.0) as u32;
    let timer_ms = (st.duration * 1000.0).round().clamp(0.0, u32::MAX as f64) as u32;
    let dist_cm = (st.dist * 100.0).round() as u32;
    let (lat0, lon0) = pts.first().map(|p| (semicircles(p.lat), semicircles(p.lon))).unwrap_or((0, 0));
    let (lat1, lon1) = pts.last().map(|p| (semicircles(p.lat), semicircles(p.lon))).unwrap_or((0, 0));

    let mut w = Writer { data: Vec::with_capacity(128 + pts.len() * 22) };
    // file_id: aktywność
    w.define(0, 0, &[(0, 1, ENUM), (1, 2, UINT16), (2, 2, UINT16), (3, 4, UINT32Z), (4, 4, UINT32)]);
    w.header(0);
    w.u8(4);
    w.u16(255);
    w.u16(1);
    w.u32((ts0 % 0xFFFF_FFF0).max(1));
    w.u32(ts0);
    // zdarzenia: licznik start (0) / stop wszystkich (4)
    w.define(1, 21, &[(253, 4, UINT32), (0, 1, ENUM), (1, 1, ENUM)]);
    let event = |w: &mut Writer, t: u32, kind: u8| {
        w.header(1);
        w.u32(t);
        w.u8(0);
        w.u8(kind);
    };
    event(&mut w, ts0, 0);
    // rekordy: czas, pozycja, wysokość (enhanced), dystans, prędkość
    w.define(2, 20, &[(253, 4, UINT32), (0, 4, SINT32), (1, 4, SINT32), (78, 4, UINT32), (5, 4, UINT32), (6, 2, UINT16)]);
    let mut dist = 0.0f64;
    for (i, p) in pts.iter().enumerate() {
        let mut speed = 0.0;
        if i > 0 {
            let a = &pts[i - 1];
            if a.seg != p.seg {
                // przerwa: stop i start licznika
                event(&mut w, fit_ts(a.t), 4);
                event(&mut w, fit_ts(p.t), 0);
            } else {
                let d = crate::geo::haversine(a.lon, a.lat, p.lon, p.lat);
                dist += d;
                let dt = p.t - a.t;
                if dt > 0.0 {
                    speed = d / dt;
                }
            }
        }
        let speed = p.speed.map(|s| s as f64).unwrap_or(speed);
        w.header(2);
        w.u32(fit_ts(p.t));
        w.i32(semicircles(p.lat));
        w.i32(semicircles(p.lon));
        w.u32(alt(p.ele));
        w.u32((dist * 100.0).round() as u32);
        w.u16((speed * 1000.0).round().clamp(0.0, 65_534.0) as u16);
    }
    event(&mut w, end, 4);
    // okrążenie
    w.define(3, 19, &[(253, 4, UINT32), (0, 1, ENUM), (1, 1, ENUM), (2, 4, UINT32), (3, 4, SINT32), (4, 4, SINT32), (5, 4, SINT32), (6, 4, SINT32), (7, 4, UINT32), (8, 4, UINT32), (9, 4, UINT32), (21, 2, UINT16), (22, 2, UINT16), (25, 1, ENUM)]);
    w.header(3);
    w.u32(end);
    w.u8(9); // okrążenie
    w.u8(1); // stop
    w.u32(ts0);
    w.i32(lat0);
    w.i32(lon0);
    w.i32(lat1);
    w.i32(lon1);
    w.u32(elapsed_ms);
    w.u32(timer_ms);
    w.u32(dist_cm);
    w.u16(st.up.round().clamp(0.0, 65_000.0) as u16);
    w.u16(st.down.round().clamp(0.0, 65_000.0) as u16);
    w.u8(17); // turystyka piesza
    // sesja
    w.define(4, 18, &[(253, 4, UINT32), (0, 1, ENUM), (1, 1, ENUM), (2, 4, UINT32), (3, 4, SINT32), (4, 4, SINT32), (5, 1, ENUM), (6, 1, ENUM), (7, 4, UINT32), (8, 4, UINT32), (9, 4, UINT32), (22, 2, UINT16), (23, 2, UINT16), (25, 2, UINT16), (26, 2, UINT16)]);
    w.header(4);
    w.u32(end);
    w.u8(8); // sesja
    w.u8(1); // stop
    w.u32(ts0);
    w.i32(lat0);
    w.i32(lon0);
    w.u8(17);
    w.u8(0);
    w.u32(elapsed_ms);
    w.u32(timer_ms);
    w.u32(dist_cm);
    w.u16(st.up.round().clamp(0.0, 65_000.0) as u16);
    w.u16(st.down.round().clamp(0.0, 65_000.0) as u16);
    w.u16(0);
    w.u16(1);
    // aktywność
    let local = (end as i64 + crate::sun::warsaw_offset_min(rec.end) as i64 * 60).max(0) as u32;
    w.define(5, 34, &[(253, 4, UINT32), (0, 4, UINT32), (1, 2, UINT16), (2, 1, ENUM), (3, 1, ENUM), (4, 1, ENUM), (5, 4, UINT32)]);
    w.header(5);
    w.u32(end);
    w.u32(timer_ms);
    w.u16(1);
    w.u8(0);
    w.u8(26); // aktywność
    w.u8(1);
    w.u32(local);
    finish(w)
}

/// Ślad z pliku FIT (aktywność z zegarka albo kurs): punkty (dł., szer., wys.) i czas (s uniksowe).
pub struct FitTrack {
    pub points: Vec<(f64, f64, Option<f32>)>,
    pub times: Vec<f64>,
    /// Plik jest aktywnością (nagranym przejściem), a nie kursem do nawigacji.
    pub activity: bool,
}

#[derive(Clone, Default)]
struct Def {
    global: u16,
    big: bool,
    fields: Vec<(u8, u8)>,
    dev: usize,
}

/// Odczyt rekordów (wiadomość 20) z pliku FIT: definicje, zwykłe nagłówki i skrócone nagłówki
/// czasu; pola deweloperskie są pomijane.
pub fn parse(b: &[u8]) -> Result<FitTrack, String> {
    let bad = || crate::i18n::tr("Plik nie jest poprawnym FIT.").to_string();
    let hsize = *b.first().ok_or_else(bad)? as usize;
    if hsize < 12 || b.len() < hsize || b.get(8..12) != Some(&b".FIT"[..]) {
        return Err(bad());
    }
    let size = u32::from_le_bytes([b[4], b[5], b[6], b[7]]) as usize;
    let end = (hsize + size).min(b.len());
    let mut defs: Vec<Option<Def>> = vec![None; 16];
    let mut pos = hsize;
    let mut last_ts: u32 = 0;
    let mut out = FitTrack { points: Vec::new(), times: Vec::new(), activity: false };
    while pos < end {
        let h = b[pos];
        pos += 1;
        if h & 0x80 != 0 {
            // skrócony nagłówek czasu: przesunięcie 5 bitów względem ostatniego znacznika
            let local = ((h >> 5) & 0x3) as usize;
            let off = (h & 0x1F) as u32;
            let mut ts = (last_ts & !0x1F) + off;
            if ts < last_ts {
                ts += 0x20;
            }
            last_ts = ts;
            let def = defs[local].clone().ok_or_else(bad)?;
            pos = read_data(b, pos, end, &def, Some(ts), &mut last_ts, &mut out).ok_or_else(bad)?;
        } else if h & 0x40 != 0 {
            let local = (h & 0x0F) as usize;
            let has_dev = h & 0x20 != 0;
            let d = b.get(pos..pos + 5).ok_or_else(bad)?;
            let big = d[1] == 1;
            let global = if big { u16::from_be_bytes([d[2], d[3]]) } else { u16::from_le_bytes([d[2], d[3]]) };
            let n = d[4] as usize;
            pos += 5;
            let mut fields = Vec::with_capacity(n);
            for _ in 0..n {
                let f = b.get(pos..pos + 3).ok_or_else(bad)?;
                fields.push((f[0], f[1]));
                pos += 3;
            }
            let mut dev = 0;
            if has_dev {
                let nd = *b.get(pos).ok_or_else(bad)? as usize;
                pos += 1;
                for _ in 0..nd {
                    dev += *b.get(pos + 1).ok_or_else(bad)? as usize;
                    pos += 3;
                }
            }
            defs[local] = Some(Def { global, big, fields, dev });
        } else {
            let local = (h & 0x0F) as usize;
            let def = defs[local].clone().ok_or_else(bad)?;
            pos = read_data(b, pos, end, &def, None, &mut last_ts, &mut out).ok_or_else(bad)?;
        }
    }
    if out.points.len() < 2 {
        return Err(crate::i18n::tr("Plik FIT nie zawiera śladu z pozycjami.").into());
    }
    Ok(out)
}

fn read_data(b: &[u8], mut pos: usize, end: usize, def: &Def, ts: Option<u32>, last_ts: &mut u32, out: &mut FitTrack) -> Option<usize> {
    let (mut lat, mut lon, mut alt, mut ealt, mut time) = (None, None, None, None, ts);
    let big = def.big;
    let u = |v: &[u8]| -> u64 {
        let mut x = 0u64;
        for (i, byte) in v.iter().enumerate().take(8) {
            let k = if big { v.len() - 1 - i } else { i };
            x |= (*byte as u64) << (8 * k);
        }
        x
    };
    for &(num, size) in &def.fields {
        let v = b.get(pos..pos + size as usize)?;
        pos += size as usize;
        if pos > end {
            return None;
        }
        match (def.global, num, size) {
            (0, 0, 1) => out.activity = v[0] == 4,
            (_, 253, 4) => {
                let t = u(v) as u32;
                if t != u32::MAX {
                    *last_ts = t;
                    time = Some(t);
                }
            }
            (20, 0, 4) => lat = Some(u(v) as u32 as i32).filter(|x| *x != i32::MAX),
            (20, 1, 4) => lon = Some(u(v) as u32 as i32).filter(|x| *x != i32::MAX),
            (20, 2, 2) => alt = Some(u(v) as u16).filter(|x| *x != u16::MAX),
            (20, 78, 4) => ealt = Some(u(v) as u32).filter(|x| *x != u32::MAX),
            _ => {}
        }
    }
    pos += def.dev;
    if def.global == 20 {
        if let (Some(la), Some(lo)) = (lat, lon) {
            let deg = |s: i32| s as f64 * (180.0 / 2_147_483_648.0);
            let e = ealt.map(|a| a as f64 / 5.0 - 500.0).or(alt.map(|a| a as f64 / 5.0 - 500.0)).map(|e| e as f32);
            out.points.push((deg(lo), deg(la), e));
            out.times.push(time.map(|t| t as f64 + FIT_EPOCH as f64).unwrap_or(0.0));
        }
    }
    Some(pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_known_value() {
        // CRC-16/ARC dla „123456789” to 0xBB3D
        assert_eq!(crc(0, b"123456789"), 0xBB3D);
        // CRC dopisany na końcu zeruje sumę
        let mut v = b"GRAN".to_vec();
        let c = crc(0, &v);
        v.extend_from_slice(&c.to_le_bytes());
        assert_eq!(crc(0, &v), 0);
    }

    #[test]
    fn names_are_cut_on_char_boundary() {
        let f = name_field("Świnica – Kasprowy Wierch");
        assert_eq!(f[NAME_LEN - 1], 0);
        let s = std::str::from_utf8(&f[..f.iter().position(|&b| b == 0).unwrap()]).unwrap();
        assert!("Świnica – Kasprowy Wierch".starts_with(s));
        assert!(s.len() <= NAME_LEN - 1);
    }

    #[test]
    fn activity_roundtrip() {
        use crate::recording::{Point, Recording};
        let points: Vec<Point> = (0..30).map(|i| Point { t: 1_719_820_800.0 + i as f64 * 10.0, lon: 19.98 + i as f64 * 1e-4, lat: 49.23, ele: 1500.0 + i as f32, seg: if i < 15 { 0 } else { 1 }, ..Default::default() }).collect();
        let mut rec = Recording::from_track(1, "Test", "", &points.iter().map(|p| (p.lon, p.lat, p.ele, p.t)).collect::<Vec<_>>()).unwrap();
        rec.points = points.clone();
        let bytes = export_activity(&rec);
        assert_eq!(crc(0, &bytes), 0, "suma kontrolna pliku");
        let back = parse(&bytes).unwrap();
        assert!(back.activity);
        assert_eq!(back.points.len(), 30);
        assert!((back.points[3].0 - points[3].lon).abs() < 1e-6);
        assert_eq!(back.points[3].2.map(|e| e.round()), Some(1503.0));
        assert_eq!(back.times[5], points[5].t);
        assert!(parse(b"nie fit").is_err());
    }

    #[test]
    fn course_is_readable() {
        let pts = vec![(20.0, 49.2, 1000.0), (20.01, 49.21, 1100.0), (20.02, 49.22, 1050.0)];
        let r = crate::graph::route_from_points(&pts, crate::graph::RouteKind::Gpx, "Kurs", 1.0);
        let back = parse(&export(&r, "Kurs", &[], 1_700_000_000)).unwrap();
        assert!(!back.activity);
        assert_eq!(back.points.len(), 3);
    }

    #[test]
    fn semicircle_scale() {
        assert_eq!(semicircles(0.0), 0);
        assert_eq!(semicircles(90.0), 1 << 30);
        assert_eq!(semicircles(-45.0), -(1 << 29));
    }
}
