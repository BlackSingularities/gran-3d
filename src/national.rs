//! Krajowe ortofotomapy (GUGiK – PL, ZBGIS – SK, ČÚZK – CZ). Zasięg krajów z Natural Earth 1:50m.

use std::io::Read;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

use serde::Deserialize;

use crate::download::{agent, FetchError};
use crate::geo::{x_to_lon, y_to_lat, TileId};

#[derive(Deserialize)]
struct Country {
    code: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_has_poland() {
        assert_eq!(country_at(19.94, 50.06), Some("PL"));
        assert_eq!(country_at(14.42, 50.08), Some("CZ"));
        assert_eq!(country_at(17.11, 48.15), Some("SK"));
        assert_eq!(country_at(2.35, 48.85), None);
        let t = crate::geo::tile_at(19.94, 50.06, 14);
        assert_eq!(ortho_sources(t), vec!["pl"]);
    }
}
