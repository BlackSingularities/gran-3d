//! Teren Studio – bez kafli map: bezpośrednio z usług urzędowych i z Copernicus DEM.
//!
//!  - Polska: GUGiK, NMT z siatką 1 m (WCS Geoportalu) – dane otwarte, wymagane podanie źródła,
//!  - Czechy: ČÚZK, DMR 5G (ArcGIS ImageServer) – CC BY 4.0,
//!  - reszta świata i luki: Copernicus DEM GLO-30 (ESA, pliki GeoTIFF 1°×1° z otwartego zasobu
//!    AWS) – licencja Copernicus pozwala na użycie komercyjne z podaniem źródła.
//!
//! Wynik to regularna siatka w lokalnym układzie [`Frame`] (metry), wiersz 0 na północy.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use super::geom::Frame;

/// Najwięcej węzłów siatki na bok.
pub const MAX_SIDE: usize = 4097;

/// Skąd brać teren.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Source {
    /// Urzędowe dane z LiDAR (Polska, Czechy), gdzie indziej i w lukach Copernicus GLO-30.
    #[default]
    Best,
    /// Tylko Copernicus GLO-30 (cały świat, ~30 m).
    Copernicus,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Best => crate::i18n::tr("Najdokładniejszy (LiDAR w Polsce i Czechach, gdzie indziej Copernicus)"),
            Source::Copernicus => crate::i18n::tr("Copernicus GLO-30 (cały świat, ok. 30 m)"),
        }
    }
}

/// Źródła, z których faktycznie pochodzi siatka (do podpisu w eksportach).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Used {
    pub gugik: bool,
    pub cuzk: bool,
    pub copernicus: bool,
}

impl Used {
    /// Podpis źródeł terenu wymagany przez licencje.
    pub fn credits(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.gugik {
            v.push("Teren: Główny Urząd Geodezji i Kartografii (NMT, geoportal.gov.pl)");
        }
        if self.cuzk {
            v.push("Terén: © ČÚZK, DMR 5G (CC BY 4.0)");
        }
        if self.copernicus {
            v.push("Copernicus DEM GLO-30: © DLR e.V. 2010-2014 and © Airbus Defence and Space GmbH 2014-2018 provided under COPERNICUS by the European Union and ESA; all rights reserved");
        }
        v
    }
}

/// Siatka wysokości w metrach: `x = x0 + i·cell`, `y = y1 − j·cell` (wiersz 0 na północy).
#[derive(Clone, Debug)]
pub struct Grid {
    pub nx: usize,
    pub ny: usize,
    pub x0: f64,
    pub y1: f64,
    pub cell: f64,
    pub z: Vec<f32>,
}

impl Grid {
    pub fn new(bounds: [f64; 4], cell: f64) -> Self {
        let nx = (((bounds[2] - bounds[0]) / cell).ceil() as usize + 1).clamp(2, MAX_SIDE);
        let ny = (((bounds[3] - bounds[1]) / cell).ceil() as usize + 1).clamp(2, MAX_SIDE);
        Self { nx, ny, x0: bounds[0], y1: bounds[3], cell, z: vec![f32::NAN; nx * ny] }
    }

    pub fn x(&self, i: usize) -> f64 {
        self.x0 + i as f64 * self.cell
    }

    pub fn y(&self, j: usize) -> f64 {
        self.y1 - j as f64 * self.cell
    }

    pub fn x1(&self) -> f64 {
        self.x(self.nx - 1)
    }

    pub fn y0(&self) -> f64 {
        self.y(self.ny - 1)
    }

    pub fn at(&self, i: usize, j: usize) -> f32 {
        self.z[j * self.nx + i]
    }

    /// Dwuliniowo w metrach (poza siatką – wartość brzegowa).
    pub fn sample(&self, x: f64, y: f64) -> f32 {
        let gx = ((x - self.x0) / self.cell).clamp(0.0, (self.nx - 1) as f64 - 1e-6);
        let gy = ((self.y1 - y) / self.cell).clamp(0.0, (self.ny - 1) as f64 - 1e-6);
        let (i, j) = (gx as usize, gy as usize);
        let (fx, fy) = ((gx - i as f64) as f32, (gy - j as f64) as f32);
        let (i1, j1) = ((i + 1).min(self.nx - 1), (j + 1).min(self.ny - 1));
        let a = self.at(i, j) * (1.0 - fx) + self.at(i1, j) * fx;
        let b = self.at(i, j1) * (1.0 - fx) + self.at(i1, j1) * fx;
        a * (1.0 - fy) + b * fy
    }

    /// Spadek terenu (dz/dx, dz/dy) w punkcie, różnicami centralnymi na odległość `h` metrów.
    pub fn gradient(&self, x: f64, y: f64, h: f64) -> (f64, f64) {
        let dx = (self.sample(x + h, y) - self.sample(x - h, y)) as f64 / (2.0 * h);
        let dy = (self.sample(x, y + h) - self.sample(x, y - h)) as f64 / (2.0 * h);
        (dx, dy)
    }

    pub fn range(&self) -> (f32, f32) {
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for &v in &self.z {
            if v.is_finite() {
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
        if lo > hi {
            (0.0, 1.0)
        } else {
            (lo, hi)
        }
    }

    /// Luki (NaN) wypełnione średnią sąsiadów, a gdy nie ma żadnych danych – zerem.
    pub fn fill_gaps(&mut self) {
        for _ in 0..256 {
            let holes: Vec<usize> = (0..self.z.len()).filter(|&k| !self.z[k].is_finite()).collect();
            if holes.is_empty() {
                return;
            }
            let mut changed = false;
            let snapshot = self.z.clone();
            for k in holes {
                let (i, j) = (k % self.nx, k / self.nx);
                let mut s = 0.0;
                let mut n = 0;
                for (di, dj) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
                    let (a, b) = (i as i64 + di, j as i64 + dj);
                    if a >= 0 && b >= 0 && (a as usize) < self.nx && (b as usize) < self.ny {
                        let v = snapshot[b as usize * self.nx + a as usize];
                        if v.is_finite() {
                            s += v;
                            n += 1;
                        }
                    }
                }
                if n > 0 {
                    self.z[k] = s / n as f32;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        for v in &mut self.z {
            if !v.is_finite() {
                *v = 0.0;
            }
        }
    }

    /// Wygładzanie filtrem 3×3 (`passes` razy) – usuwa szum LiDAR na wydrukach.
    pub fn smooth(&mut self, passes: u8) {
        for _ in 0..passes {
            let src = self.z.clone();
            for j in 0..self.ny {
                for i in 0..self.nx {
                    let mut s = 0.0;
                    let mut w = 0.0;
                    for dj in -1i64..=1 {
                        for di in -1i64..=1 {
                            let (a, b) = (i as i64 + di, j as i64 + dj);
                            if a >= 0 && b >= 0 && (a as usize) < self.nx && (b as usize) < self.ny {
                                let k = if di == 0 && dj == 0 { 4.0 } else if di == 0 || dj == 0 { 2.0 } else { 1.0 };
                                s += src[b as usize * self.nx + a as usize] * k;
                                w += k;
                            }
                        }
                    }
                    self.z[j * self.nx + i] = s / w;
                }
            }
        }
    }

    /// Siatka o innym oczku (do podglądu i siatki modelu): próbkowanie dwuliniowe.
    pub fn resample(&self, cell: f64) -> Grid {
        if cell <= self.cell * 1.0001 {
            return self.clone();
        }
        let mut g = Grid::new([self.x0, self.y0(), self.x1(), self.y1], cell);
        for j in 0..g.ny {
            for i in 0..g.nx {
                let v = self.sample(g.x(i), g.y(j));
                g.z[j * g.nx + i] = v;
            }
        }
        g
    }
}

/// Postęp przygotowania danych (wspólny z interfejsem).
#[derive(Default)]
pub struct Progress {
    pub frac: Mutex<(f32, String)>,
    pub cancel: AtomicBool,
}

impl Progress {
    pub fn set(&self, f: f32, msg: impl Into<String>) {
        *self.frac.lock().unwrap() = (f.clamp(0.0, 1.0), msg.into());
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

fn intersects(b: [f64; 4], geo: [f64; 4]) -> bool {
    geo[0] <= b[2] && geo[2] >= b[0] && geo[1] <= b[3] && geo[3] >= b[1]
}

/// Prostokąt geograficzny (w, s, e, n) obszaru w metrach.
pub fn geo_bounds(f: &Frame, b: [f64; 4]) -> [f64; 4] {
    let mut g = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for (x, y) in [(b[0], b[1]), (b[2], b[1]), (b[2], b[3]), (b[0], b[3]), ((b[0] + b[2]) / 2.0, b[1]), ((b[0] + b[2]) / 2.0, b[3])] {
        let (lon, lat) = f.to_geo(x, y);
        g = [g[0].min(lon), g[1].min(lat), g[2].max(lon), g[3].max(lat)];
    }
    g
}

/// Siatka terenu dla prostokąta `bounds` (metry w układzie `f`) z oczkiem `cell`.
pub fn build(f: &Frame, bounds: [f64; 4], cell: f64, source: Source, cache: &Path, p: &Progress) -> Result<(Grid, Used), String> {
    let mut g = Grid::new(bounds, cell);
    let mut used = Used::default();
    let geo = geo_bounds(f, bounds);
    if source == Source::Best && cell < 28.0 {
        for (country, url, flag) in [
            (crate::lidar::POLAND, crate::lidar::gugik_url as fn([f64; 4]) -> String, 0u8),
            (crate::lidar::CZECHIA, crate::lidar::cuzk_url as fn([f64; 4]) -> String, 1u8),
        ] {
            if !intersects(country, geo) || !g.z.iter().any(|v| v.is_nan()) {
                continue;
            }
            let label = if flag == 0 { "GUGiK" } else { "ČÚZK" };
            match national(f, &mut g, url, label, p) {
                Ok(true) => {
                    if flag == 0 {
                        used.gugik = true;
                    } else {
                        used.cuzk = true;
                    }
                }
                Ok(false) => {}
                Err(e) => log::warn!("studio: {label}: {e}"),
            }
            if p.cancelled() {
                return Err(crate::i18n::tr("Przerwano.").into());
            }
        }
    }
    if g.z.iter().any(|v| v.is_nan()) {
        copernicus(f, &mut g, geo, cache, p)?;
        used.copernicus = true;
    }
    g.fill_gaps();
    Ok((g, used))
}

/// Bloki 512×512 z usługi krajowej w EPSG:3857; `Ok(true)` – coś wpisano do siatki.
fn national(f: &Frame, g: &mut Grid, url: fn([f64; 4]) -> String, label: &str, p: &Progress) -> Result<bool, String> {
    const B: usize = crate::lidar::BLOCK;
    let ms = f.merc_scale();
    // piksel usługi ≈ oczko siatki (nie gęściej niż 1 m w terenie)
    let mut px = g.cell.max(1.0) * ms;
    let (mx0, my0) = f.to_3857(g.x0, g.y0());
    let (mx1, my1) = f.to_3857(g.x1(), g.y1);
    let span = |px: f64| (((mx1 - mx0) / (px * B as f64)).ceil() as usize, ((my1 - my0) / (px * B as f64)).ceil() as usize);
    // najwyżej 144 zapytania – większe obszary rzadszą siatką
    while {
        let (a, b) = span(px);
        a * b > 144
    } {
        px *= 1.25;
    }
    let (bx, by) = span(px);
    let total = bx * by;
    let blocks: Mutex<Vec<(usize, usize, Vec<f32>)>> = Mutex::new(Vec::new());
    let next = std::sync::atomic::AtomicUsize::new(0);
    let done = std::sync::atomic::AtomicUsize::new(0);
    let failed = Mutex::new(None::<String>);
    std::thread::scope(|s| {
        for _ in 0..crate::lidar::PARALLEL {
            s.spawn(|| loop {
                let k = next.fetch_add(1, Ordering::Relaxed);
                if k >= total || p.cancelled() {
                    break;
                }
                let (bi, bj) = (k % bx, k / bx);
                let w = mx0 + bi as f64 * px * B as f64;
                let n = my1 - bj as f64 * px * B as f64;
                let bb = [w, n - px * B as f64, w + px * B as f64, n];
                let r = crate::lidar::download(&url(bb)).map_err(|e| e.to_string()).and_then(|b| crate::lidar::parse_tiff(&b));
                match r {
                    Ok((v, w2, h2)) if w2 == B && h2 == B => blocks.lock().unwrap().push((bi, bj, v)),
                    Ok(_) => {}
                    Err(e) => *failed.lock().unwrap() = Some(e),
                }
                let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                p.set(0.05 + 0.5 * d as f32 / total as f32, tf!("Teren {}: {} z {}", label, d, total));
            });
        }
    });
    let blocks = blocks.into_inner().unwrap();
    if blocks.is_empty() {
        return match failed.into_inner().unwrap() {
            Some(e) => Err(e),
            None => Ok(false),
        };
    }
    let mut map: Vec<Option<&Vec<f32>>> = vec![None; total];
    for (bi, bj, v) in &blocks {
        map[bj * bx + bi] = Some(v);
    }
    let mut any = false;
    for j in 0..g.ny {
        for i in 0..g.nx {
            let k = j * g.nx + i;
            if g.z[k].is_finite() {
                continue;
            }
            let (mx, my) = f.to_3857(g.x(i), g.y(j));
            // piksel w całej mozaice bloków (środki pikseli w połówkach)
            let gx = (mx - mx0) / px - 0.5;
            let gy = (my1 - my) / px - 0.5;
            if gx < 0.0 || gy < 0.0 {
                continue;
            }
            let v = mosaic(&map, bx, by, gx, gy);
            if v.is_finite() {
                g.z[k] = v;
                any = true;
            }
        }
    }
    Ok(any)
}

fn mosaic(map: &[Option<&Vec<f32>>], bx: usize, by: usize, gx: f64, gy: f64) -> f32 {
    const B: usize = crate::lidar::BLOCK;
    let at = |x: usize, y: usize| -> f32 {
        let (bi, bj) = (x / B, y / B);
        if bi >= bx || bj >= by {
            return f32::NAN;
        }
        match map[bj * bx + bi] {
            Some(v) => v[(y % B) * B + x % B],
            None => f32::NAN,
        }
    };
    let (x, y) = (gx as usize, gy as usize);
    let (fx, fy) = ((gx - x as f64) as f32, (gy - y as f64) as f32);
    let (a, b, c, d) = (at(x, y), at(x + 1, y), at(x, y + 1), at(x + 1, y + 1));
    if a.is_finite() && b.is_finite() && c.is_finite() && d.is_finite() {
        (a * (1.0 - fx) + b * fx) * (1.0 - fy) + (c * (1.0 - fx) + d * fx) * fy
    } else {
        a
    }
}

/* ── Copernicus GLO-30 ─────────────────────────────────────────────────────────── */

/// Nazwa pliku 1°×1° z lewym dolnym narożnikiem (lon, lat).
pub fn copernicus_name(lon: i32, lat: i32) -> String {
    format!(
        "Copernicus_DSM_COG_10_{}{:02}_00_{}{:03}_00_DEM",
        if lat >= 0 { 'N' } else { 'S' },
        lat.unsigned_abs(),
        if lon >= 0 { 'E' } else { 'W' },
        lon.unsigned_abs()
    )
}

/// Plik Copernicusa z pamięci podręcznej albo z sieci; `None` – w tym miejscu nie ma lądu.
fn copernicus_file(lon: i32, lat: i32, cache: &Path) -> Result<Option<PathBuf>, String> {
    let name = copernicus_name(lon, lat);
    let path = cache.join(format!("{name}.tif"));
    let none = cache.join(format!("{name}.none"));
    if path.exists() {
        return Ok(Some(path));
    }
    if none.exists() {
        return Ok(None);
    }
    let _ = std::fs::create_dir_all(cache);
    let url = format!("https://copernicus-dem-30m.s3.amazonaws.com/{name}/{name}.tif");
    match crate::download::agent().get(&url).timeout(std::time::Duration::from_secs(300)).call() {
        Ok(r) => {
            use std::io::Read;
            let mut buf = Vec::new();
            r.into_reader().take(400 << 20).read_to_end(&mut buf).map_err(|e| e.to_string())?;
            let tmp = cache.join(format!("{name}.part"));
            std::fs::write(&tmp, &buf).map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
            Ok(Some(path))
        }
        // brak pliku: morze (Copernicus nie ma plików nad oceanem)
        Err(ureq::Error::Status(403 | 404, _)) => {
            let _ = std::fs::write(&none, b"");
            Ok(None)
        }
        Err(e) => Err(tf!("Copernicus DEM: {}", e)),
    }
}

/// Raster z georeferencją: środek piksela (0, 0) w (`lon0`, `lat0`), krok w stopniach.
struct Raster {
    w: usize,
    h: usize,
    lon0: f64,
    lat0: f64,
    dx: f64,
    dy: f64,
    v: Vec<f32>,
}

impl Raster {
    fn sample(&self, lon: f64, lat: f64) -> f32 {
        let gx = (lon - self.lon0) / self.dx;
        let gy = (self.lat0 - lat) / self.dy;
        if gx < -0.5 || gy < -0.5 || gx > self.w as f64 - 0.5 || gy > self.h as f64 - 0.5 {
            return f32::NAN;
        }
        let gx = gx.clamp(0.0, self.w as f64 - 1.000_001);
        let gy = gy.clamp(0.0, self.h as f64 - 1.000_001);
        let (x, y) = (gx as usize, gy as usize);
        let (fx, fy) = ((gx - x as f64) as f32, (gy - y as f64) as f32);
        let at = |x: usize, y: usize| self.v[y.min(self.h - 1) * self.w + x.min(self.w - 1)];
        (at(x, y) * (1.0 - fx) + at(x + 1, y) * fx) * (1.0 - fy) + (at(x, y + 1) * (1.0 - fx) + at(x + 1, y + 1) * fx) * fy
    }
}

/// Odczyt GeoTIFF (float32 albo int16, kompresja Deflate/LZW, kafle albo paski) z georeferencją.
fn read_geotiff(path: &Path) -> Result<Raster, String> {
    use tiff::decoder::{Decoder, DecodingResult};
    use tiff::tags::Tag;
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut d = Decoder::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let (w, h) = d.dimensions().map_err(|e| e.to_string())?;
    let (w, h) = (w as usize, h as usize);
    let scale = d.get_tag_f64_vec(Tag::ModelPixelScaleTag).map_err(|e| e.to_string())?;
    let tie = d.get_tag_f64_vec(Tag::ModelTiepointTag).map_err(|e| e.to_string())?;
    if scale.len() < 2 || tie.len() < 6 {
        return Err("GeoTIFF bez georeferencji".into());
    }
    // GTRasterTypeGeoKey (1025): 2 – wartości w punktach (narożnik = środek piksela)
    let point = d
        .get_tag_u32_vec(Tag::GeoKeyDirectoryTag)
        .ok()
        .map(|k| k.chunks(4).skip(1).any(|q| q.len() == 4 && q[0] == 1025 && q[3] == 2))
        .unwrap_or(false);
    let nodata: Option<f32> = d.get_tag_ascii_string(Tag::GdalNodata).ok().and_then(|s| s.trim().trim_end_matches('\0').parse().ok());
    let v: Vec<f32> = match d.read_image().map_err(|e| e.to_string())? {
        DecodingResult::F32(v) => v,
        DecodingResult::I16(v) => v.into_iter().map(|x| x as f32).collect(),
        DecodingResult::F64(v) => v.into_iter().map(|x| x as f32).collect(),
        DecodingResult::I32(v) => v.into_iter().map(|x| x as f32).collect(),
        DecodingResult::U16(v) => v.into_iter().map(|x| x as f32).collect(),
        _ => return Err("nieobsługiwany typ pikseli GeoTIFF".into()),
    };
    let v = v
        .into_iter()
        .map(|x| if !x.is_finite() || Some(x) == nodata || x < -1000.0 || x > 9000.0 { f32::NAN } else { x })
        .collect::<Vec<_>>();
    if v.len() < w * h {
        return Err("ucięty GeoTIFF".into());
    }
    let (dx, dy) = (scale[0], scale[1]);
    // narożnik z punktu wiążącego (piksel tie[0], tie[1])
    let (lon_c, lat_c) = (tie[3] - tie[0] * dx, tie[4] + tie[1] * dy);
    let (lon0, lat0) = if point { (lon_c, lat_c) } else { (lon_c + dx / 2.0, lat_c - dy / 2.0) };
    Ok(Raster { w, h, lon0, lat0, dx, dy, v })
}

fn copernicus(f: &Frame, g: &mut Grid, geo: [f64; 4], cache: &Path, p: &Progress) -> Result<(), String> {
    let cache = cache.join("copernicus");
    let (lo0, la0, lo1, la1) = (geo[0].floor() as i32, geo[1].floor() as i32, geo[2].floor() as i32, geo[3].floor() as i32);
    let tiles: Vec<(i32, i32)> = (la0..=la1).flat_map(|la| (lo0..=lo1).map(move |lo| (lo, la))).collect();
    if tiles.len() > 16 {
        return Err(crate::i18n::tr("Obszar jest zbyt duży dla Copernicus DEM (najwyżej 16 plików 1°×1°).").into());
    }
    // węzły bez danych, z położeniem geograficznym
    let holes: Vec<(usize, f64, f64)> = (0..g.z.len())
        .filter(|&k| g.z[k].is_nan())
        .map(|k| {
            let (lon, lat) = f.to_geo(g.x(k % g.nx), g.y(k / g.nx));
            (k, lon, lat)
        })
        .collect();
    for (n, (lon, lat)) in tiles.iter().enumerate() {
        if p.cancelled() {
            return Err(crate::i18n::tr("Przerwano.").into());
        }
        p.set(0.55 + 0.35 * n as f32 / tiles.len() as f32, tf!("Copernicus DEM: plik {} z {}", n + 1, tiles.len()));
        let Some(path) = copernicus_file(*lon, *lat, &cache)? else {
            // morze
            for &(k, lo, la) in &holes {
                if lo.floor() as i32 == *lon && la.floor() as i32 == *lat && g.z[k].is_nan() {
                    g.z[k] = 0.0;
                }
            }
            continue;
        };
        let r = match read_geotiff(&path) {
            Ok(r) => r,
            Err(e) => {
                // uszkodzony plik – następnym razem pobierzemy od nowa
                let _ = std::fs::remove_file(&path);
                return Err(tf!("Copernicus DEM: {}", e));
            }
        };
        for &(k, lo, la) in &holes {
            if g.z[k].is_nan() {
                let v = r.sample(lo, la);
                if v.is_finite() {
                    g.z[k] = v;
                }
            }
        }
    }
    Ok(())
}

/// Zatrzymanie z innego wątku.
pub type Shared = Arc<Progress>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(copernicus_name(19, 49), "Copernicus_DSM_COG_10_N49_00_E019_00_DEM");
        assert_eq!(copernicus_name(-1, -1), "Copernicus_DSM_COG_10_S01_00_W001_00_DEM");
    }

    #[test]
    fn grid_sampling_and_gaps() {
        let mut g = Grid::new([0.0, 0.0, 30.0, 20.0], 10.0);
        assert_eq!((g.nx, g.ny), (4, 3));
        for j in 0..g.ny {
            for i in 0..g.nx {
                g.z[j * g.nx + i] = g.x(i) as f32;
            }
        }
        g.z[5] = f32::NAN;
        g.fill_gaps();
        assert!(g.z.iter().all(|v| v.is_finite()));
        assert!((g.sample(15.0, 10.0) - 15.0).abs() < 1.0);
        let r = g.resample(15.0);
        assert!(r.nx < g.nx);
    }
}
