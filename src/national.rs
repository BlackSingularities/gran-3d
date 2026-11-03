//! Krajowe źródła danych: ortofotomapy (GUGiK – PL, ZBGIS – SK, ČÚZK – CZ) i modele LiDAR
//! (NMT GUGiK w PUWG-1992, DMR 5G ČÚZK w Web Mercator). Zasięg krajów z Natural Earth 1:50m.

use std::io::Read;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

use serde::Deserialize;

use crate::download::{agent, FetchError};
use crate::geo::{x_to_lon, y_to_lat, TileId};

#[derive(Deserialize)]
struct Country {
    code: String,
    lidar: Option<String>,
    ortho: Option<String>,
    ring: Vec<[f64; 2]>,
}

#[derive(Deserialize)]
struct Coverage {
    countries: Vec<Country>,
}

fn coverage() -> &'static [Country] {
    static C: OnceLock<Vec<Country>> = OnceLock::new();
    C.get_or_init(|| {
        serde_json::from_str::<Coverage>(include_str!("../assets/coverage.json"))
            .map(|c| c.countries)
            .unwrap_or_default()
    })
}

fn in_ring(lon: f64, lat: f64, ring: &[[f64; 2]]) -> bool {
    let mut inside = false;
    let mut j = ring.len().wrapping_sub(1);
    for i in 0..ring.len() {
        let ([xi, yi], [xj, yj]) = (ring[i], ring[j]);
        if (yi > lat) != (yj > lat) && lon < (xj - xi) * (lat - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Czy kafel (z marginesem `pad` ułamka kafla) dotyka kraju spełniającego warunek.
fn tile_touches(t: TileId, pad: f64, pick: impl Fn(&Country) -> bool) -> bool {
    let n = (1u64 << t.z) as f64;
    let cs: Vec<&Country> = coverage().iter().filter(|c| pick(c)).collect();
    if cs.is_empty() {
        return false;
    }
    for j in 0..=4 {
        for i in 0..=4 {
            let fx = (t.x as f64 - pad + (1.0 + 2.0 * pad) * i as f64 / 4.0) / n;
            let fy = (t.y as f64 - pad + (1.0 + 2.0 * pad) * j as f64 / 4.0) / n;
            let (lon, lat) = (x_to_lon(fx), y_to_lat(fy));
            if cs.iter().any(|c| in_ring(lon, lat, &c.ring)) {
                return true;
            }
        }
    }
    false
}

/// Kod kraju w punkcie (PL, CZ, SK) – tylko kraje z danymi krajowymi.
pub fn country_at(lon: f64, lat: f64) -> Option<&'static str> {
    coverage().iter().find(|c| in_ring(lon, lat, &c.ring)).map(|c| c.code.as_str())
}

/// Najniższy poziom z ortofoto (niżej wystarcza Sentinel-2).
pub const ORTHO_MIN_Z: u8 = 13;
pub const ORTHO_MAX_Z: u8 = 17;
/// Rozmiar obrazu ortofoto na kafel (px).
pub const ORTHO_PX: u32 = 512;

/// Krajowe ortofoto obejmujące kafel (w kolejności rysowania).
pub fn ortho_sources(t: TileId) -> Vec<&'static str> {
    if t.z < ORTHO_MIN_Z {
        return Vec::new();
    }
    ["sk", "cz", "pl"]
        .into_iter()
        .filter(|code| tile_touches(t, 0.0, |c| c.ortho.as_deref() == Some(*code)))
        .collect()
}

pub fn has_ortho(t: TileId) -> bool {
    t.z >= ORTHO_MIN_Z && tile_touches(t, 0.0, |c| c.ortho.is_some())
}

/// Kafel może mieć dane LiDAR (Polska, Czechy).
pub fn has_lidar(t: TileId) -> bool {
    tile_touches(t, 0.05, |c| c.lidar.is_some())
}

const WM: f64 = 20_037_508.342_789_244;

/// Granice kafla w metrach EPSG:3857: (x0, y0, x1, y1).
fn merc_bounds(t: TileId) -> (f64, f64, f64, f64) {
    let n = (1u64 << t.z) as f64;
    let x0 = t.x as f64 / n * 2.0 * WM - WM;
    let x1 = (t.x + 1) as f64 / n * 2.0 * WM - WM;
    let y1 = WM - t.y as f64 / n * 2.0 * WM;
    let y0 = WM - (t.y + 1) as f64 / n * 2.0 * WM;
    (x0, y0, x1, y1)
}

/// Usługi krajowe są wolne: najwyżej tyle zapytań naraz, żeby nie zająć wszystkich wątków kafli.
const MAX_NATIONAL: usize = 6;

struct Permit;

impl Permit {
    fn gate() -> &'static (Mutex<usize>, Condvar) {
        static G: OnceLock<(Mutex<usize>, Condvar)> = OnceLock::new();
        G.get_or_init(Default::default)
    }

    fn take() -> Permit {
        let (m, cv) = Self::gate();
        let mut n = m.lock().unwrap();
        while *n >= MAX_NATIONAL {
            n = cv.wait(n).unwrap();
        }
        *n += 1;
        Permit
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        let (m, cv) = Self::gate();
        *m.lock().unwrap() -= 1;
        cv.notify_one();
    }
}

fn get_bytes(url: &str, limit: u64) -> Result<Vec<u8>, FetchError> {
    let _permit = Permit::take();
    let mut last = String::new();
    for attempt in 0..3 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(600 * (1 << attempt)));
        }
        match agent().get(url).timeout(Duration::from_secs(90)).call() {
            Ok(r) => {
                let mut buf = Vec::new();
                match r.into_reader().take(limit).read_to_end(&mut buf) {
                    Ok(_) if !buf.is_empty() => return Ok(buf),
                    Ok(_) => last = "pusta odpowiedź".into(),
                    Err(e) => last = e.to_string(),
                }
            }
            Err(ureq::Error::Status(404 | 400, _)) => return Err(FetchError::Missing),
            Err(ureq::Error::Status(c, _)) => last = format!("serwer odpowiedział kodem {c}"),
            Err(ureq::Error::Transport(e)) => last = e.to_string(),
        }
    }
    Err(FetchError::Network(last))
}

/// Ortofoto kafla złożone z usług krajowych (przezroczyste poza krajem) na tle zdjęcia satelitarnego.
/// Zwraca JPEG albo `Missing`, gdy żadna usługa nie ma obrazu.
pub fn fetch_ortho(t: TileId, background: Option<image::RgbaImage>) -> Result<Vec<u8>, FetchError> {
    let sources = ortho_sources(t);
    if sources.is_empty() {
        return Err(FetchError::Missing);
    }
    let (x0, y0, x1, y1) = merc_bounds(t);
    let bb = format!("{x0:.2},{y0:.2},{x1:.2},{y1:.2}");
    let n = ORTHO_PX;
    let mut canvas = match background {
        Some(b) => image::imageops::resize(&b, n, n, image::imageops::FilterType::Triangle),
        None => image::RgbaImage::from_pixel(n, n, image::Rgba([96, 104, 88, 255])),
    };
    let mut any = false;
    let mut net_err = None;
    for src in sources {
        let url = match src {
            "pl" => format!("https://mapy.geoportal.gov.pl/wss/service/PZGIK/ORTO/WMS/HighResolution?SERVICE=WMS&REQUEST=GetMap&VERSION=1.3.0&LAYERS=Raster&STYLES=&CRS=EPSG:3857&BBOX={bb}&WIDTH={n}&HEIGHT={n}&FORMAT=image/png&TRANSPARENT=TRUE"),
            "sk" => format!("https://zbgis.skgeodesy.sk/zbgis/rest/services/Ortofoto/MapServer/export?bbox={bb}&bboxSR=3857&imageSR=3857&size={n},{n}&format=png32&transparent=true&f=image"),
            _ => format!("https://ags.cuzk.gov.cz/arcgis1/rest/services/ORTOFOTO_WM/MapServer/export?bbox={bb}&bboxSR=3857&imageSR=3857&size={n},{n}&format=png32&transparent=true&f=image"),
        };
        match get_bytes(&url, 16 << 20) {
            Ok(b) => {
                let Ok(img) = image::load_from_memory(&b) else { continue };
                let img = img.to_rgba8();
                if img.width() != n || img.height() != n {
                    continue;
                }
                let mut opaque = 0usize;
                for (dst, s) in canvas.pixels_mut().zip(img.pixels()) {
                    let a = s[3] as u32;
                    if a == 0 {
                        continue;
                    }
                    // usługi czasem zwracają białe tło zamiast przezroczystości poza zasięgiem
                    if s[0] > 250 && s[1] > 250 && s[2] > 250 {
                        continue;
                    }
                    opaque += 1;
                    for c in 0..3 {
                        dst[c] = ((s[c] as u32 * a + dst[c] as u32 * (255 - a)) / 255) as u8;
                    }
                }
                any |= opaque > (n * n / 50) as usize;
            }
            Err(FetchError::Network(e)) => net_err = Some(e),
            Err(FetchError::Missing) => {}
        }
    }
    if !any {
        return Err(match net_err {
            Some(e) => FetchError::Network(e),
            None => FetchError::Missing,
        });
    }
    let rgb = image::DynamicImage::ImageRgba8(canvas).to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 86)
        .encode_image(&rgb)
        .map_err(|e| FetchError::Network(e.to_string()))?;
    Ok(out)
}

// ---------------------------------------------------------------- PUWG-1992 (EPSG:2180)

/// Długość i szerokość → (E, N) w metrach PUWG-1992 (szereg Krügera, dokładność < 1 mm).
pub fn to_puwg(lon: f64, lat: f64) -> (f64, f64) {
    let a: f64 = 6_378_137.0;
    let f: f64 = 1.0 / 298.257_222_101;
    let n = f / (2.0 - f);
    let aa = a / (1.0 + n) * (1.0 + n * n / 4.0 + n.powi(4) / 64.0);
    let al = [
        n / 2.0 - 2.0 * n * n / 3.0 + 5.0 * n.powi(3) / 16.0,
        13.0 * n * n / 48.0 - 3.0 * n.powi(3) / 5.0,
        61.0 * n.powi(3) / 240.0,
    ];
    let (k0, e0, n0, lon0) = (0.9993, 500_000.0, -5_300_000.0, 19f64.to_radians());
    let c = 2.0 * n.sqrt() / (1.0 + n);
    let phi = lat.to_radians();
    let dl = lon.to_radians() - lon0;
    let s = phi.sin();
    let t = (s.atanh() - c * (c * s).atanh()).sinh();
    let xi = (t / dl.cos()).atan();
    let eta = (dl.sin() / (1.0 + t * t).sqrt()).atanh();
    let (mut e, mut nn) = (eta, xi);
    for j in 1..=3 {
        let jf = j as f64;
        e += al[j - 1] * (2.0 * jf * xi).cos() * (2.0 * jf * eta).sinh();
        nn += al[j - 1] * (2.0 * jf * xi).sin() * (2.0 * jf * eta).cosh();
    }
    (e0 + k0 * aa * e, n0 + k0 * aa * nn)
}

/// Rastr F32 z pliku TIFF (pierwsze pasmo).
fn read_tiff(bytes: &[u8]) -> Option<(usize, usize, Vec<f32>)> {
    use tiff::decoder::{Decoder, DecodingResult};
    let mut d = Decoder::new(std::io::Cursor::new(bytes)).ok()?;
    let (w, h) = d.dimensions().ok()?;
    let data = match d.read_image().ok()? {
        DecodingResult::F32(v) => v,
        DecodingResult::F64(v) => v.into_iter().map(|x| x as f32).collect(),
        DecodingResult::I16(v) => v.into_iter().map(|x| x as f32).collect(),
        DecodingResult::U16(v) => v.into_iter().map(|x| x as f32).collect(),
        DecodingResult::I32(v) => v.into_iter().map(|x| x as f32).collect(),
        _ => return None,
    };
    let (w, h) = (w as usize, h as usize);
    let bands = data.len() / (w * h).max(1);
    if bands == 0 {
        return None;
    }
    let data = if bands == 1 { data } else { data.iter().step_by(bands).copied().collect() };
    Some((w, h, data))
}

/// Wysokości LiDAR kafla (256×256, środki pikseli) albo `Missing` poza zasięgiem.
pub fn fetch_lidar(t: TileId) -> Result<Vec<f32>, FetchError> {
    let n = (1u64 << t.z) as f64;
    let (clon, clat) = (x_to_lon((t.x as f64 + 0.5) / n), y_to_lat((t.y as f64 + 0.5) / n));
    let country = country_at(clon, clat);
    let try_cz = || -> Result<Vec<f32>, FetchError> {
        let (x0, y0, x1, y1) = merc_bounds(t);
        let url = format!(
            "https://ags.cuzk.gov.cz/arcgis/rest/services/3D/dmr5g_wm/ImageServer/exportImage?bbox={x0:.3},{y0:.3},{x1:.3},{y1:.3}&bboxSR=3857&imageSR=3857&size=256,256&format=tiff&pixelType=F32&interpolation=RSP_BilinearInterpolation&noData=-9999&f=image"
        );
        let b = get_bytes(&url, 32 << 20)?;
        let (w, h, d) = read_tiff(&b).ok_or(FetchError::Network("odpowiedź ČÚZK nie jest rastrem TIFF".into()))?;
        if w != 256 || h != 256 {
            return Err(FetchError::Network("nieoczekiwany rozmiar rastra ČÚZK".into()));
        }
        Ok(d)
    };
    let try_pl = || -> Result<Vec<f32>, FetchError> {
        // zasięg kafla w PUWG-1992 z marginesem; rozdzielczość ~ piksel kafla
        let px_m = crate::geo::meters_per_pixel(clat, t.z).max(1.0);
        let res = if px_m < 4.0 { 1.0 } else { (px_m / 2.0).floor().max(1.0) };
        let (mut e0, mut e1, mut n0, mut n1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for j in 0..=4 {
            for i in 0..=4 {
                let lon = x_to_lon((t.x as f64 + i as f64 / 4.0) / n);
                let lat = y_to_lat((t.y as f64 + j as f64 / 4.0) / n);
                let (e, nn) = to_puwg(lon, lat);
                e0 = e0.min(e);
                e1 = e1.max(e);
                n0 = n0.min(nn);
                n1 = n1.max(nn);
            }
        }
        let pad = res * 3.0;
        e0 = ((e0 - pad) / res).floor() * res;
        n0 = ((n0 - pad) / res).floor() * res;
        e1 = ((e1 + pad) / res).ceil() * res;
        n1 = ((n1 + pad) / res).ceil() * res;
        let (gw, gh) = (((e1 - e0) / res).round() as usize, ((n1 - n0) / res).round() as usize);
        let url = format!(
            "https://mapy.geoportal.gov.pl/wss/service/PZGIK/NMT/GRID1/WCS/DigitalTerrainModelFormatTIFF?SERVICE=WCS&VERSION=1.0.0&REQUEST=GetCoverage&COVERAGE=DTM_PL-KRON86-NH_TIFF&CRS=EPSG:2180&FORMAT=image/tiff&INTERPOLATION=bilinear&BBOX={e0},{n0},{e1},{n1}&WIDTH={gw}&HEIGHT={gh}"
        );
        let b = get_bytes(&url, 64 << 20)?;
        let (w, h, g) = read_tiff(&b).ok_or(FetchError::Network("odpowiedź GUGiK nie jest rastrem TIFF".into()))?;
        let (sx, sy) = ((e1 - e0) / w as f64, (n1 - n0) / h as f64);
        let mut out = vec![f32::NAN; 256 * 256];
        let mut covered = 0;
        for j in 0..256 {
            let lat = y_to_lat((t.y as f64 + (j as f64 + 0.5) / 256.0) / n);
            for i in 0..256 {
                let lon = x_to_lon((t.x as f64 + (i as f64 + 0.5) / 256.0) / n);
                let (e, nn) = to_puwg(lon, lat);
                let gx = (e - e0) / sx - 0.5;
                let gy = (n1 - nn) / sy - 0.5;
                let (xi, yi) = (gx.floor() as isize, gy.floor() as isize);
                if xi < 0 || yi < 0 || xi as usize >= w - 1 || yi as usize >= h - 1 {
                    continue;
                }
                let (fx, fy) = ((gx - xi as f64) as f32, (gy - yi as f64) as f32);
                let k = yi as usize * w + xi as usize;
                let (a, b2, c, d) = (g[k], g[k + 1], g[k + w], g[k + w + 1]);
                // 0 i mniej = brak danych (poza Polską)
                if a <= 1.0 || b2 <= 1.0 || c <= 1.0 || d <= 1.0 {
                    continue;
                }
                out[j * 256 + i] = (a * (1.0 - fx) + b2 * fx) * (1.0 - fy) + (c * (1.0 - fx) + d * fx) * fy;
                covered += 1;
            }
        }
        if covered == 0 {
            return Err(FetchError::Missing);
        }
        Ok(out)
    };
    let mut r = match country {
        Some("CZ") => try_cz(),
        Some("PL") => try_pl(),
        _ => {
            // przy granicy: najpierw PL, potem CZ
            try_pl().or_else(|_| try_cz())
        }
    }?;
    // komórki bez danych (poza krajem usługi potrafią zwrócić 0 m) – NaN; wypełnia je model globalny
    for v in &mut r {
        if !v.is_finite() || *v <= 1.0 || *v > 9000.0 {
            *v = f32::NAN;
        }
    }
    // pojedyncze piksele na brzegu zasięgu to nie są dane tego kafla
    if r.iter().filter(|v| !v.is_nan()).count() * 50 < r.len() {
        return Err(FetchError::Missing);
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn puwg_known_point() {
        // Kasprowy Wierch ~ E 567 950, N 145 560 (EPSG:2180)
        let (e, n) = to_puwg(19.9817, 49.2319);
        assert!((e - 571_440.0).abs() < 6000.0, "{e}");
        assert!((n - 146_300.0).abs() < 6000.0, "{n}");
        // początek układu: południk 19° → E = 500 000
        let (e0, _) = to_puwg(19.0, 52.0);
        assert!((e0 - 500_000.0).abs() < 0.01);
    }

    #[test]
    fn coverage_has_poland() {
        assert_eq!(country_at(19.94, 50.06), Some("PL"));
        assert_eq!(country_at(14.42, 50.08), Some("CZ"));
        assert_eq!(country_at(17.11, 48.15), Some("SK"));
        assert_eq!(country_at(2.35, 48.85), None);
        let t = crate::geo::tile_at(19.94, 50.06, 14);
        assert_eq!(ortho_sources(t), vec!["pl"]);
        assert!(has_lidar(t));
    }
}
