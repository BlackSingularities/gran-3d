//! Drogi, ulice i tory całego świata z kafli wektorowych OpenFreeMap (schemat OpenMapTiles, warstwa
//! `transportation`) – bez Overpass, więc szybko także w dużych miastach. Zapis: `roads\9_x_y.bin.lz4`
//! w folderze danych, jeden plik na sektor.

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::download::agent;
use crate::geo::TileId;
use crate::osm::{SectorId, SECTOR_Z};

/// Poziom kafli z drogami: od niego w kaflach są także ulice osiedlowe.
const ROADS_Z: u8 = 12;

/// Rodzaj drogi – kolejność od najważniejszych; numer zapisywany w pliku.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Class {
    Motorway = 0,
    Trunk = 1,
    Primary = 2,
    Secondary = 3,
    Tertiary = 4,
    /// Ulice i drogi lokalne.
    Street = 5,
    /// Dojazdy, drogi wewnętrzne.
    Service = 6,
    /// Drogi gruntowe i leśne.
    Track = 7,
    Rail = 8,
    Tram = 9,
}

impl Class {
    pub const COUNT: usize = 10;

    fn from_u8(v: u8) -> Option<Class> {
        Some(match v {
            0 => Class::Motorway,
            1 => Class::Trunk,
            2 => Class::Primary,
            3 => Class::Secondary,
            4 => Class::Tertiary,
            5 => Class::Street,
            6 => Class::Service,
            7 => Class::Track,
            8 => Class::Rail,
            9 => Class::Tram,
            _ => return None,
        })
    }

    /// Rodzaj z atrybutów kafla (`class`, `subclass`); ścieżki zostają szlakom, promy i kolejki pomijamy.
    fn from_tile(class: &str, subclass: &str) -> Option<Class> {
        Some(match class {
            "motorway" => Class::Motorway,
            "trunk" => Class::Trunk,
            "primary" => Class::Primary,
            "secondary" => Class::Secondary,
            "tertiary" => Class::Tertiary,
            "minor" | "busway" => Class::Street,
            "service" => Class::Service,
            "track" => Class::Track,
            "rail" => Class::Rail,
            "transit" if matches!(subclass, "tram" | "light_rail") => Class::Tram,
            _ => return None,
        })
    }
}

/// Droga: rodzaj, łamana (dł., szer.) i jej prostokąt (zachód, południe, wschód, północ).
#[derive(Clone, Debug, PartialEq)]
pub struct Road {
    pub class: Class,
    pub line: Vec<(f64, f64)>,
    pub bbox: [f64; 4],
}

impl Road {
    fn new(class: Class, line: Vec<(f64, f64)>) -> Road {
        let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        for &(lon, lat) in &line {
            b = [b[0].min(lon), b[1].min(lat), b[2].max(lon), b[3].max(lat)];
        }
        Road { class, line, bbox: b }
    }
}

pub fn path(root: &Path, id: SectorId) -> PathBuf {
    root.join("roads").join(format!("{}_{}_{}.bin.lz4", SECTOR_Z, id.x, id.y))
}

pub fn exists(root: &Path, id: SectorId) -> bool {
    path(root, id).exists()
}

const MAGIC: &[u8; 4] = b"GRD1";

/// Zapis zwarty: nagłówek, liczba dróg, a dla każdej rodzaj, liczba punktów i punkty w 1e-7 stopnia.
fn encode(roads: &[Road]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + roads.iter().map(|r| 5 + r.line.len() * 8).sum::<usize>());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(roads.len() as u32).to_le_bytes());
    for r in roads {
        out.push(r.class as u8);
        out.extend_from_slice(&(r.line.len() as u32).to_le_bytes());
        for &(lon, lat) in &r.line {
            out.extend_from_slice(&((lon * 1e7).round() as i32).to_le_bytes());
            out.extend_from_slice(&((lat * 1e7).round() as i32).to_le_bytes());
        }
    }
    out
}

fn decode(b: &[u8]) -> Option<Vec<Road>> {
    let u32_at = |i: usize| Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?));
    let i32_at = |i: usize| Some(i32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?));
    if b.get(..4)? != MAGIC {
        return None;
    }
    let n = u32_at(4)? as usize;
    let mut i = 8;
    let mut out = Vec::with_capacity(n.min(1 << 20));
    for _ in 0..n {
        let class = Class::from_u8(*b.get(i)?)?;
        let len = u32_at(i + 1)? as usize;
        i += 5;
        if b.len() < i + len.checked_mul(8)? {
            return None;
        }
        let mut line = Vec::with_capacity(len);
        for k in 0..len {
            line.push((i32_at(i + k * 8)? as f64 / 1e7, i32_at(i + k * 8 + 4)? as f64 / 1e7));
        }
        i += len * 8;
        out.push(Road::new(class, line));
    }
    Some(out)
}

pub fn load(root: &Path, id: SectorId) -> Option<Vec<Road>> {
    let b = std::fs::read(path(root, id)).ok()?;
    decode(&lz4_flex::decompress_size_prepended(&b).ok()?)
}

pub fn save(root: &Path, id: SectorId, roads: &[Road]) -> Result<(), String> {
    let p = path(root, id);
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, lz4_flex::compress_prepend_size(&encode(roads))).map_err(|e| e.to_string())?;
    std::fs::rename(tmp, p).map_err(|e| e.to_string())
}

/// Drogi sektora z kafli wektorowych (blokujące; kafle pobierane równolegle). Błąd któregokolwiek
/// kafla to błąd całości – niepełny sektor nie może trafić na dysk jako gotowy.
pub fn fetch(id: SectorId) -> Result<Vec<Road>, String> {
    let url = crate::osm::vector_url()?;
    let k = 1u32 << (ROADS_Z - SECTOR_Z);
    let tiles: Vec<TileId> = (0..k * k).map(|i| TileId::new(ROADS_Z, id.x * k + i % k, id.y * k + i / k)).collect();
    use rayon::prelude::*;
    let parts: Vec<Result<Vec<Road>, String>> = tiles.par_iter().map(|t| tile_roads(&url, *t)).collect();
    let mut out = Vec::new();
    for p in parts {
        out.append(&mut p?);
    }
    Ok(out)
}

fn tile_roads(url: &str, t: TileId) -> Result<Vec<Road>, String> {
    let u = url.replace("{z}", &t.z.to_string()).replace("{x}", &t.x.to_string()).replace("{y}", &t.y.to_string());
    let r = match agent().get(&u).call() {
        Ok(r) => r,
        // kafel bez danych (np. morze)
        Err(ureq::Error::Status(404, _)) => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
    let mut buf = Vec::new();
    r.into_reader().take(32 << 20).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(roads_from_tile(&buf, t))
}

/// Drogi z jednego kafla: bez tuneli, przycięte do kafla (zapas poza krawędzią należy do sąsiada).
fn roads_from_tile(tile: &[u8], t: TileId) -> Vec<Road> {
    let n = (1u64 << t.z) as f64;
    let mut out = Vec::new();
    for l in crate::mvt::lines(tile, &["transportation"]) {
        if l.text("brunnel") == Some("tunnel") {
            continue;
        }
        let Some(class) = Class::from_tile(l.text("class").unwrap_or(""), l.text("subclass").unwrap_or("")) else { continue };
        for part in &l.parts {
            for run in clip(part) {
                let line: Vec<(f64, f64)> = run
                    .iter()
                    .map(|&(x, y)| (crate::geo::x_to_lon((t.x as f64 + x) / n), crate::geo::y_to_lat((t.y as f64 + y) / n)))
                    .collect();
                out.push(Road::new(class, line));
            }
        }
    }
    out
}

/// Części łamanej leżące w kaflu: zostają odcinki, których choć jeden koniec jest w kaflu (0..1).
fn clip(part: &[(f64, f64)]) -> Vec<Vec<(f64, f64)>> {
    let inside = |p: (f64, f64)| (0.0..=1.0).contains(&p.0) && (0.0..=1.0).contains(&p.1);
    let mut out: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut cur: Vec<(f64, f64)> = Vec::new();
    for w in part.windows(2) {
        if inside(w[0]) || inside(w[1]) {
            if cur.is_empty() {
                cur.push(w[0]);
            }
            cur.push(w[1]);
        } else if cur.len() >= 2 {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.clear();
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_round_trip() {
        let roads = vec![
            Road::new(Class::Primary, vec![(19.4567891, 51.7654321), (19.46, 51.77), (19.47, 51.78)]),
            Road::new(Class::Tram, vec![(-122.4194, 37.7749), (-122.42, 37.78)]),
        ];
        let back = decode(&encode(&roads)).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].class, Class::Primary);
        assert_eq!(back[1].class, Class::Tram);
        for (a, b) in roads.iter().zip(&back) {
            assert_eq!(a.line.len(), b.line.len());
            for (p, q) in a.line.iter().zip(&b.line) {
                assert!((p.0 - q.0).abs() < 1e-6 && (p.1 - q.1).abs() < 1e-6);
            }
        }
        assert!(back[0].bbox[0] <= 19.4568 && back[0].bbox[3] >= 51.78 - 1e-6);
        assert!(decode(b"zle dane").is_none());
        assert!(decode(&encode(&roads)[..20]).is_none(), "ucięty plik");
    }

    #[test]
    fn clips_to_tile_and_maps_classes() {
        // wchodzi z zapasu, biegnie przez kafel, wychodzi daleko poza i wraca
        let part = [(-0.02, 0.5), (0.1, 0.5), (0.9, 0.5), (1.05, 0.5), (1.2, 0.5), (1.3, 0.6), (0.95, 0.6)];
        let runs = clip(&part);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0], vec![(-0.02, 0.5), (0.1, 0.5), (0.9, 0.5), (1.05, 0.5)]);
        assert_eq!(runs[1], vec![(1.3, 0.6), (0.95, 0.6)]);
        assert_eq!(Class::from_tile("minor", ""), Some(Class::Street));
        assert_eq!(Class::from_tile("transit", "tram"), Some(Class::Tram));
        assert_eq!(Class::from_tile("transit", "subway"), None);
        assert_eq!(Class::from_tile("path", ""), None);
        for v in 0..Class::COUNT as u8 {
            assert_eq!(Class::from_u8(v).map(|c| c as u8), Some(v));
        }
        assert!(Class::from_u8(Class::COUNT as u8).is_none());
    }
}
