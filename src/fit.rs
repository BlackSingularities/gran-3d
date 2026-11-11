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
    fn semicircle_scale() {
        assert_eq!(semicircles(0.0), 0);
        assert_eq!(semicircles(90.0), 1 << 30);
        assert_eq!(semicircles(-45.0), -(1 << 29));
    }
}
