//! Szczegółowy teren z lotniczego skanowania laserowego (LiDAR) dla Polski i Czech, pobierany
//! i przeliczany na tym urządzeniu – tylko po zgodzie użytkownika (pytanie przy pierwszym
//! oglądaniu tych krajów, przełącznik w ustawieniach mapy).
//!
//! Źródła (otwarte dane, CC BY 4.0 / ustawa o informacji przestrzennej – wymagane podanie źródła):
//!  - Polska: GUGiK, NMT z siatką 1 m – usługa WCS Geoportalu, wycinek od razu w EPSG:3857,
//!  - Czechy: ČÚZK, DMR 5G – usługa ArcGIS ImageServer (`exportImage`), też w EPSG:3857.
//!
//! Słowacja nie ma usługi z wycinkami (cały kraj tylko jako jeden plik 183 GB) – tam zostaje
//! teren z Mapterhorn (który i tak pochodzi z DMR 5.0, tyle że w ~1,6 m).
//!
//! Jedno zapytanie obejmuje kafel z16 w 512 px (~0,8 m na piksel w Tatrach) i daje cztery kafle
//! terenu poziomu [`LIDAR_Z`] po 256 px. Piksele bez danych (granica kraju, morze) uzupełnia
//! powiększony kafel z16 z Mapterhorn. Blok bez żadnych danych LiDAR zapisuje się jako „brak”,
//! więc w tym miejscu GRAŃ więcej nie pyta. Najwyżej dwa zapytania naraz – to publiczne serwery.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

use crate::bake;
use crate::download::FetchError;
use crate::geo::{lat_to_y, lon_to_x, TileId};
use crate::store::{Layer, Store};

/// Poziom kafli terenu 256 px z LiDAR (zwykły teren kończy się na 16).
pub const LIDAR_Z: u8 = 17;
/// Bok pobieranego bloku (kafel z16 w tej rozdzielczości = 4 kafle [`LIDAR_Z`]).
const BLOCK: usize = 512;
/// Najmniejszy udział pikseli z danymi, żeby blok uznać za pokryty LiDAR-em.
const MIN_VALID: f64 = 0.02;
/// Równoległe zapytania do usług krajowych.
pub const PARALLEL: usize = 2;

/// Prostokąty krajów (zachód, południe, wschód, północ) – zgrubny zasięg; piksele spoza kraju
/// usługa i tak zwraca jako brak danych.
const POLAND: [f64; 4] = [14.07, 49.0, 24.16, 54.84];
const CZECHIA: [f64; 4] = [12.09, 48.55, 18.86, 51.06];

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Włączenie szczegółowego terenu (zgoda użytkownika).
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

fn inside(b: [f64; 4], lon: f64, lat: f64) -> bool {
    lon >= b[0] && lon <= b[2] && lat >= b[1] && lat <= b[3]
}

/// Czy punkt leży w zasięgu usług (Polska, Czechy) – do pytania o zgodę i podpisu źródeł.
pub fn covers(lon: f64, lat: f64) -> bool {
    inside(POLAND, lon, lat) || inside(CZECHIA, lon, lat)
}

fn tile_center(t: TileId) -> (f64, f64) {
    let n = (1u64 << t.z) as f64;
    let lon = (t.x as f64 + 0.5) / n * 360.0 - 180.0;
    let lat = crate::geo::y_to_lat((t.y as f64 + 0.5) / n);
    (lon, lat)
}

/// Najwyższy poziom terenu dla kafla: [`LIDAR_Z`] w zasięgu LiDAR (gdy włączony), inaczej zwykły.
pub fn dem_max_z(t: TileId) -> u8 {
    if enabled() && t.z >= LIDAR_Z {
        let a = if t.z > LIDAR_Z { t.ancestor(t.z - LIDAR_Z).0 } else { t };
        let (lon, lat) = tile_center(a);
        if covers(lon, lat) {
            return LIDAR_Z;
        }
    }
    crate::download::TERRAIN_MAX_Z
}

/// Prostokąt kafla w metrach EPSG:3857 (zachód, południe, wschód, północ).
fn mercator_bbox(t: TileId) -> [f64; 4] {
    const HALF: f64 = 20_037_508.342_789_244;
    let n = (1u64 << t.z) as f64;
    let size = 2.0 * HALF / n;
    let w = -HALF + t.x as f64 * size;
    let nn = HALF - t.y as f64 * size;
    [w, nn - size, w + size, nn]
}

fn gugik_url(b: [f64; 4]) -> String {
    let crs = "http://www.opengis.net/def/crs/EPSG/0/3857";
    format!(
        "https://mapy.geoportal.gov.pl/wss/service/PZGIK/NMT/GRID1/WCS/DigitalTerrainModelFormatTIFF?SERVICE=WCS&VERSION=2.0.1&REQUEST=GetCoverage&COVERAGEID=DTM_PL-KRON86-NH_TIFF&SUBSETTINGCRS={crs}&OUTPUTCRS={crs}&SUBSET=x({:.3},{:.3})&SUBSET=y({:.3},{:.3})&SCALESIZE=x({BLOCK}),y({BLOCK})&FORMAT=image/tiff",
        b[0], b[2], b[1], b[3]
    )
}

fn cuzk_url(b: [f64; 4]) -> String {
    format!(
        "https://ags.cuzk.gov.cz/arcgis2/rest/services/dmr5g/ImageServer/exportImage?bbox={:.3},{:.3},{:.3},{:.3}&bboxSR=3857&imageSR=3857&size={BLOCK},{BLOCK}&format=tiff&pixelType=F32&interpolation=RSP_BilinearInterpolation&f=image",
        b[0], b[1], b[2], b[3]
    )
}

/* ── TIFF z wysokościami (bez kompresji, float32, paski albo kafle) ───────────────── */

/// Wysokości z pliku TIFF usługi: `None` w pikselach bez danych (brakujące kafle pliku, zera,
/// wartości „no data”).
pub fn parse_tiff(b: &[u8]) -> Result<(Vec<f32>, usize, usize), String> {
    let le = match b.get(..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return Err("to nie jest TIFF".into()),
    };
    let u16_at = |o: usize| -> Option<u32> { b.get(o..o + 2).map(|s| if le { u16::from_le_bytes([s[0], s[1]]) } else { u16::from_be_bytes([s[0], s[1]]) } as u32) };
    let u32_at = |o: usize| -> Option<u32> { b.get(o..o + 4).map(|s| if le { u32::from_le_bytes([s[0], s[1], s[2], s[3]]) } else { u32::from_be_bytes([s[0], s[1], s[2], s[3]]) }) };
    if u16_at(2) != Some(42) {
        return Err("nieobsługiwany TIFF (BigTIFF?)".into());
    }
    let ifd = u32_at(4).ok_or("ucięty TIFF")? as usize;
    let count = u16_at(ifd).ok_or("ucięty TIFF")? as usize;
    // znacznik → wartości (SHORT / LONG)
    let mut tags: std::collections::HashMap<u16, Vec<u32>> = Default::default();
    for i in 0..count {
        let e = ifd + 2 + 12 * i;
        let (Some(tag), Some(ty), Some(n)) = (u16_at(e), u16_at(e + 2), u32_at(e + 4)) else { return Err("ucięty TIFF".into()) };
        let size = match ty {
            3 => 2,
            4 => 4,
            _ => continue,
        };
        let n = n as usize;
        let base = if size * n <= 4 { e + 8 } else { u32_at(e + 8).ok_or("ucięty TIFF")? as usize };
        let vals: Option<Vec<u32>> = (0..n).map(|k| if size == 2 { u16_at(base + 2 * k) } else { u32_at(base + 4 * k) }).collect();
        tags.insert(tag as u16, vals.ok_or("ucięty TIFF")?);
    }
    let one = |t: u16| tags.get(&t).and_then(|v| v.first().copied());
    let (w, h) = (one(256).ok_or("brak szerokości")? as usize, one(257).ok_or("brak wysokości")? as usize);
    if one(258) != Some(32) || one(259).unwrap_or(1) != 1 || one(277).unwrap_or(1) != 1 || one(339).unwrap_or(3) != 3 {
        return Err("TIFF nie jest nieskompresowanym float32".into());
    }
    let f32_at = |o: usize| -> Option<f32> { u32_at(o).map(f32::from_bits) };
    let mut out = vec![f32::NAN; w * h];
    if let (Some(offs), Some(counts)) = (tags.get(&324), tags.get(&325)) {
        let (tw, th) = (one(322).ok_or("brak kafla")? as usize, one(323).ok_or("brak kafla")? as usize);
        let across = w.div_ceil(tw);
        for (k, (&o, &c)) in offs.iter().zip(counts).enumerate() {
            // pusty kafel pliku: poza zasięgiem danych
            if o == 0 || (c as usize) < tw * th * 4 {
                continue;
            }
            let (ox, oy) = ((k % across) * tw, (k / across) * th);
            for j in 0..th {
                for i in 0..tw {
                    let (x, y) = (ox + i, oy + j);
                    if x < w && y < h {
                        out[y * w + x] = f32_at(o as usize + 4 * (j * tw + i)).unwrap_or(f32::NAN);
                    }
                }
            }
        }
    } else {
        let offs = tags.get(&273).ok_or("brak pasków")?;
        let rows = one(278).unwrap_or(h as u32) as usize;
        for (k, &o) in offs.iter().enumerate() {
            for r in 0..rows {
                let y = k * rows + r;
                if y >= h {
                    break;
                }
                for x in 0..w {
                    out[y * w + x] = f32_at(o as usize + 4 * (r * w + x)).unwrap_or(f32::NAN);
                }
            }
        }
    }
    for v in &mut out {
        // brak danych: zera (poza krajem), wartości „no data” (−3,4e38, −9999) i nie-liczby
        if !v.is_finite() || *v == 0.0 || *v < -500.0 || *v > 9000.0 {
            *v = f32::NAN;
        }
    }
    Ok((out, w, h))
}

/* ── pobieranie ─────────────────────────────────────────────────────────────────── */

fn slots() -> &'static (Mutex<(usize, HashSet<TileId>)>, Condvar) {
    static S: OnceLock<(Mutex<(usize, HashSet<TileId>)>, Condvar)> = OnceLock::new();
    S.get_or_init(Default::default)
}

/// Blok pobierany przez ten wątek (inne czekają) i miejsce w limicie równoległych zapytań.
struct Slot(TileId);

impl Slot {
    fn take(block: TileId) -> Slot {
        let (m, cv) = slots();
        let mut g = m.lock().unwrap();
        while g.0 >= PARALLEL || g.1.contains(&block) {
            g = cv.wait(g).unwrap();
        }
        g.0 += 1;
        g.1.insert(block);
        Slot(block)
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let (m, cv) = slots();
        let mut g = m.lock().unwrap();
        g.0 -= 1;
        g.1.remove(&self.0);
        cv.notify_all();
    }
}

fn download(url: &str) -> Result<Vec<u8>, FetchError> {
    use std::io::Read;
    let r = crate::download::agent().get(url).timeout(Duration::from_secs(60)).call();
    match r {
        Ok(r) => {
            let ct = r.content_type().to_string();
            let mut buf = Vec::new();
            r.into_reader().take(16 << 20).read_to_end(&mut buf).map_err(|e| FetchError::Network(e.to_string()))?;
            // błąd usługi przychodzi jako XML albo JSON z kodem 200
            if !ct.contains("tiff") && !buf.starts_with(b"II") && !buf.starts_with(b"MM") {
                return Err(FetchError::Network(tf!("usługa LiDAR: {}", String::from_utf8_lossy(&buf[..buf.len().min(160)]))));
            }
            Ok(buf)
        }
        Err(ureq::Error::Status(404, _)) => Err(FetchError::Missing),
        Err(ureq::Error::Status(code, _)) => Err(FetchError::Network(tf!("usługa LiDAR: kod {}", code))),
        Err(ureq::Error::Transport(e)) => Err(FetchError::Network(e.to_string())),
    }
}

/// Wysokości bloku z usług krajowych (piksele bez danych: NaN). Dwa kraje na granicy – oba.
fn lidar_block(block: TileId) -> Result<Vec<f32>, FetchError> {
    let b = mercator_bbox(block);
    let (lon, lat) = tile_center(block);
    let mut urls = Vec::new();
    // na granicy (oba prostokąty) – oba źródła, każde wypełnia swój kraj
    if inside(POLAND, lon, lat) {
        urls.push(gugik_url(b));
    }
    if inside(CZECHIA, lon, lat) {
        urls.push(cuzk_url(b));
    }
    let mut out = vec![f32::NAN; BLOCK * BLOCK];
    let mut last = None;
    let mut any = false;
    for u in urls {
        match download(&u).and_then(|bytes| parse_tiff(&bytes).map_err(FetchError::Network)) {
            Ok((v, w, h)) if (w, h) == (BLOCK, BLOCK) => {
                any = true;
                for (o, x) in out.iter_mut().zip(v) {
                    if o.is_nan() && !x.is_nan() {
                        *o = x;
                    }
                }
            }
            Ok((_, w, h)) => last = Some(FetchError::Network(tf!("usługa LiDAR: rozmiar {}×{}", w, h))),
            Err(e) => last = Some(e),
        }
    }
    if any {
        Ok(out)
    } else {
        Err(last.unwrap_or(FetchError::Missing))
    }
}

/// Kafel terenu poziomu [`LIDAR_Z`]: blok z usługi połączony ze zwykłym terenem; zapisuje od razu
/// wszystkie cztery kafle bloku. Zwraca kafel `t`.
pub fn fetch(store: &Store, t: TileId) -> Result<Vec<u8>, FetchError> {
    if t.z != LIDAR_Z || dem_max_z(t) != LIDAR_Z {
        return Err(FetchError::Missing);
    }
    let (block, _, _) = t.ancestor(1);
    let _slot = Slot::take(block);
    // rodzeństwo mogło już pobrać ten blok
    if let Some(b) = store.read(Layer::Dem, t) {
        return Ok(b);
    }
    if store.has(Layer::Dem, t) {
        return Err(FetchError::Missing);
    }
    let lidar = lidar_block(block)?;
    let valid = lidar.iter().filter(|v| !v.is_nan()).count();
    let siblings = [(0u32, 0u32), (1, 0), (0, 1), (1, 1)].map(|(qx, qy)| (qx, qy, TileId::new(LIDAR_Z, block.x * 2 + qx, block.y * 2 + qy)));
    if (valid as f64) < MIN_VALID * (BLOCK * BLOCK) as f64 {
        // tu nie ma LiDAR (np. Słowacja w prostokącie Polski): więcej nie pytamy
        for (_, _, c) in siblings {
            if c != t {
                let _ = store.write(Layer::Dem, c, None);
            }
        }
        return Err(FetchError::Missing);
    }
    // podkład: zwykły kafel z16 (Mapterhorn) powiększony do 512 px – w miejscach bez LiDAR
    let base = crate::download::get(store, Layer::Dem, block).ok().and_then(|b| bake::decode_dem(&b));
    let mut elev = vec![0f32; BLOCK * BLOCK];
    for y in 0..BLOCK {
        for x in 0..BLOCK {
            let v = lidar[y * BLOCK + x];
            elev[y * BLOCK + x] = if !v.is_nan() {
                v
            } else if let Some(d) = &base {
                upsample(d, x, y)
            } else {
                -1000.0 // dziura – uzupełni ją bake::fill_holes
            };
        }
    }
    let mut own = None;
    for (qx, qy, c) in siblings {
        let mut part = bake::quadrant(&elev, qx as usize, qy as usize);
        if bake::has_holes(&part) && !bake::fill_holes(&mut part, 256, 256, None) {
            continue;
        }
        let baked = bake::encode_dem(&part, 256, 256);
        if c == t {
            own = Some(baked);
        } else {
            let _ = store.write(Layer::Dem, c, Some(&baked));
        }
    }
    own.ok_or(FetchError::Missing)
}

/// Wysokość piksela bloku 512 px z kafla 256 px tego samego obszaru (dwuliniowo).
fn upsample(d: &bake::Dem, x: usize, y: usize) -> f32 {
    let s = d.w as f32 / BLOCK as f32;
    let fx = ((x as f32 + 0.5) * s - 0.5).clamp(0.0, (d.w - 1) as f32);
    let fy = ((y as f32 + 0.5) * s - 0.5).clamp(0.0, (d.h - 1) as f32);
    let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(d.w - 1), (y0 + 1).min(d.h - 1));
    let (ax, ay) = (fx - x0 as f32, fy - y0 as f32);
    let at = |x: usize, y: usize| d.elev[y * d.w + x];
    (at(x0, y0) * (1.0 - ax) + at(x1, y0) * ax) * (1.0 - ay) + (at(x0, y1) * (1.0 - ax) + at(x1, y1) * ax) * ay
}

/// Do podpisu źródeł: środek widoku w zasięgu włączonego LiDAR.
pub fn active_at(lon: f64, lat: f64) -> bool {
    enabled() && covers(lon, lat)
}

#[allow(dead_code)]
fn tile_at(lon: f64, lat: f64, z: u8) -> TileId {
    let n = (1u64 << z) as f64;
    TileId::new(z, (lon_to_x(lon) * n) as u32, (lat_to_y(lat) * n) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TIFF float32 bez kompresji: paski po `rows` wierszy.
    fn tiff_strips(w: usize, h: usize, rows: usize, vals: &[f32]) -> Vec<u8> {
        let strips = h.div_ceil(rows);
        let data_off = 8usize;
        let mut b = b"II*\0".to_vec();
        let ifd = data_off + vals.len() * 4 + strips * 8;
        b.extend((ifd as u32).to_le_bytes());
        for v in vals {
            b.extend(v.to_le_bytes());
        }
        let offs_at = b.len();
        for k in 0..strips {
            b.extend(((data_off + k * rows * w * 4) as u32).to_le_bytes());
        }
        let counts_at = b.len();
        for k in 0..strips {
            let r = rows.min(h - k * rows);
            b.extend(((r * w * 4) as u32).to_le_bytes());
        }
        let entries: Vec<(u16, u16, u32, u32)> = vec![
            (256, 3, 1, w as u32),
            (257, 3, 1, h as u32),
            (258, 3, 1, 32),
            (259, 3, 1, 1),
            (273, 4, strips as u32, offs_at as u32),
            (277, 3, 1, 1),
            (278, 3, 1, rows as u32),
            (279, 4, strips as u32, counts_at as u32),
            (339, 3, 1, 3),
        ];
        b.extend((entries.len() as u16).to_le_bytes());
        for (tag, ty, n, v) in entries {
            b.extend(tag.to_le_bytes());
            b.extend(ty.to_le_bytes());
            b.extend(n.to_le_bytes());
            if ty == 3 && n == 1 {
                b.extend((v as u16).to_le_bytes());
                b.extend([0, 0]);
            } else if n == 1 {
                b.extend(v.to_le_bytes());
            } else {
                b.extend(v.to_le_bytes());
            }
        }
        b.extend(0u32.to_le_bytes());
        b
    }

    #[test]
    fn reads_float_strips_and_marks_nodata() {
        let mut vals: Vec<f32> = (0..12).map(|i| 1000.0 + i as f32).collect();
        vals[3] = 0.0;
        vals[7] = -3.4e38;
        let b = tiff_strips(4, 3, 2, &vals);
        let (v, w, h) = parse_tiff(&b).unwrap();
        assert_eq!((w, h), (4, 3));
        assert_eq!(v[0], 1000.0);
        assert_eq!(v[11], 1011.0);
        assert!(v[3].is_nan() && v[7].is_nan(), "brak danych");
        assert!(parse_tiff(b"nie tiff").is_err());
    }

    #[test]
    fn coverage_and_levels() {
        set_enabled(true);
        // Kasprowy Wierch (Polska), Śnieżka (granica PL/CZ), Matterhorn (poza zasięgiem)
        assert!(covers(19.98, 49.23));
        assert!(covers(15.74, 50.74));
        assert!(!covers(7.66, 45.98));
        assert_eq!(dem_max_z(tile_at(19.98, 49.23, 17)), LIDAR_Z);
        assert_eq!(dem_max_z(tile_at(19.98, 49.23, 18)), LIDAR_Z);
        assert_eq!(dem_max_z(tile_at(7.66, 45.98, 17)), crate::download::TERRAIN_MAX_Z);
        assert_eq!(dem_max_z(tile_at(19.98, 49.23, 15)), crate::download::TERRAIN_MAX_Z);
        set_enabled(false);
        assert_eq!(dem_max_z(tile_at(19.98, 49.23, 17)), crate::download::TERRAIN_MAX_Z);
    }

    #[test]
    fn mercator_box_of_tile() {
        let b = mercator_bbox(TileId::new(1, 1, 0));
        assert!((b[0] - 0.0).abs() < 1e-6 && (b[3] - 20_037_508.342_789_244).abs() < 1e-3);
        assert!(b[2] > b[0] && b[3] > b[1]);
    }
}
