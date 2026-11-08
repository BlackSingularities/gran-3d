//! Dane okolicy z OpenStreetMap (Overpass API) w sektorach – kaflach poziomu 9 (~50×50 km):
//! szlaki (relacje `route=hiking|foot` z drogami) i punkty (szczyty, przełęcze, schroniska, miejscowości…).
//!
//! Odcinek drogi należy do sektora, w którym leży jego pierwszy węzeł, więc sąsiednie sektory
//! dzielą sieć bez nakładek, a po złączeniu (po identyfikatorach węzłów OSM) tworzą ciągły graf.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::download::agent;
use crate::geo::{BBox, TileId};

pub const SECTOR_Z: u8 = 9;
const OVERPASS: [&str; 3] = [
    "https://overpass-api.de/api/interpreter",
    "https://overpass.private.coffee/api/interpreter",
    "https://overpass.kumi.systems/api/interpreter",
];
const FORMAT: u32 = 3;

/// Sektor danych okolicy (kafel z9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SectorId {
    pub x: u32,
    pub y: u32,
}

impl SectorId {
    pub fn at(lon: f64, lat: f64) -> SectorId {
        let t = crate::geo::tile_at(lon, lat, SECTOR_Z);
        SectorId { x: t.x, y: t.y }
    }

    pub fn tile(self) -> TileId {
        TileId::new(SECTOR_Z, self.x, self.y)
    }

    pub fn bbox(self) -> BBox {
        self.tile().bbox()
    }

    pub fn neighbours(self) -> impl Iterator<Item = SectorId> {
        let n = 1i64 << SECTOR_Z;
        (-1i64..=1).flat_map(move |dy| {
            (-1i64..=1).filter_map(move |dx| {
                let y = self.y as i64 + dy;
                ((dx, dy) != (0, 0) && y >= 0 && y < n).then(|| SectorId {
                    x: (self.x as i64 + dx).rem_euclid(n) as u32,
                    y: y as u32,
                })
            })
        })
    }

    pub fn path(self, root: &Path) -> PathBuf {
        root.join("sectors")
            .join(format!("{}_{}_{}.json.lz4", SECTOR_Z, self.x, self.y))
    }
}

/// Relacja szlaku.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RouteInfo {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub r#ref: String,
    pub color: String,
    #[serde(default)]
    pub network: String,
    #[serde(default)]
    pub operator: String,
}

/// Ciąg odcinków jednej drogi (węzły jako indeksy w `nodes` sektora).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WayRun {
    pub way: i64,
    pub nodes: Vec<u32>,
    /// Indeksy relacji w `routes` sektora.
    pub routes: Vec<u32>,
    pub sac: u8,
    pub oneway: i8,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub highway: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PoiKind {
    Peak,
    Saddle,
    Hut,
    Shelter,
    Lake,
    Waterfall,
    Cave,
    Viewpoint,
    Place,
    /// Grań, grzbiet (natural=ridge/arete).
    Ridge,
    /// Hala, polana, nazwane miejsce (place=locality).
    Locality,
    /// Źródło.
    Spring,
    /// Dolina.
    Valley,
    /// Woda pitna (kran, źródełko z ujęciem).
    DrinkingWater,
    Parking,
    BusStop,
    /// Stacja kolejki linowej lub wyciągu.
    Cableway,
    /// Kapliczka, krzyż przydrożny, kaplica.
    Shrine,
    /// Kamera internetowa.
    Webcam,
}

impl PoiKind {
    pub fn label(self) -> &'static str {
        match self {
            PoiKind::Peak => crate::i18n::tr("Szczyt"),
            PoiKind::Saddle => crate::i18n::tr("Przełęcz"),
            PoiKind::Hut => crate::i18n::tr("Schronisko"),
            PoiKind::Shelter => crate::i18n::tr("Wiata / schron"),
            PoiKind::Lake => crate::i18n::tr("Staw / jezioro"),
            PoiKind::Waterfall => crate::i18n::tr("Wodospad"),
            PoiKind::Cave => crate::i18n::tr("Jaskinia"),
            PoiKind::Viewpoint => crate::i18n::tr("Punkt widokowy"),
            PoiKind::Place => crate::i18n::tr("Miejscowość"),
            PoiKind::Ridge => crate::i18n::tr("Grań"),
            PoiKind::Locality => crate::i18n::tr("Hala / polana"),
            PoiKind::Spring => crate::i18n::tr("Źródło"),
            PoiKind::Valley => crate::i18n::tr("Dolina"),
            PoiKind::DrinkingWater => crate::i18n::tr("Woda pitna"),
            PoiKind::Parking => crate::i18n::tr("Parking"),
            PoiKind::BusStop => crate::i18n::tr("Przystanek"),
            PoiKind::Cableway => crate::i18n::tr("Kolejka linowa"),
            PoiKind::Shrine => crate::i18n::tr("Kapliczka"),
            PoiKind::Webcam => crate::i18n::tr("Kamera internetowa"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Poi {
    pub kind: PoiKind,
    pub name: String,
    pub lon: f64,
    pub lat: f64,
    /// Wysokość z OSM (jeśli podana).
    pub ele: Option<f32>,
    #[serde(default)]
    pub wiki: String,
    /// „node/123”, „way/456”.
    pub osm: String,
    /// Miejscowość: city/town/village/hamlet.
    #[serde(default)]
    pub place: String,
    /// Dodatkowe informacje z OSM (klucz → wartość): godziny otwarcia, telefon, strona, noclegi, kamera…
    #[serde(default)]
    pub info: Vec<(String, String)>,
    #[serde(default)]
    pub pop: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SectorData {
    pub format: u32,
    /// Sekundy UTC pobrania.
    pub fetched: u64,
    /// Węzły: (id OSM, dł., szer.).
    pub nodes: Vec<(i64, f64, f64)>,
    pub runs: Vec<WayRun>,
    pub routes: Vec<RouteInfo>,
    pub pois: Vec<Poi>,
    /// Same punkty, bez szlaków (szlaki dociągną się później).
    #[serde(default)]
    pub partial: bool,
    /// Wersja dodatkowych danych (punkty usługowe, łańcuchy, granice); starsze sektory uzupełniają się
    /// jednym lekkim zapytaniem, bez ponownego pobierania szlaków.
    #[serde(default)]
    pub extra_ver: u32,
    /// Odcinki ubezpieczone łańcuchami, klamrami, linami (dł., szer.).
    #[serde(default)]
    pub chains: Vec<Vec<(f64, f64)>>,
    #[serde(default)]
    pub boundaries: Vec<Boundary>,
}

/// Granica obszaru: parku narodowego, rezerwatu albo państwa (linie przycięte do sektora).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Boundary {
    pub kind: BoundaryKind,
    pub name: String,
    pub lines: Vec<Vec<(f64, f64)>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoundaryKind {
    NationalPark,
    Reserve,
    Country,
}

/// Bieżąca wersja dodatkowych danych sektora.
pub const EXTRA_VER: u32 = 1;

pub fn load(root: &Path, id: SectorId) -> Option<SectorData> {
    let b = std::fs::read(id.path(root)).ok()?;
    let raw = lz4_flex::decompress_size_prepended(&b).ok()?;
    let d: SectorData = serde_json::from_slice(&raw).ok()?;
    (d.format == FORMAT).then_some(d)
}

pub fn save(root: &Path, id: SectorId, d: &SectorData) -> std::io::Result<()> {
    let p = id.path(root);
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let raw = serde_json::to_vec(d).unwrap_or_default();
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, lz4_flex::compress_prepend_size(&raw))?;
    std::fs::rename(tmp, p)
}

pub fn exists(root: &Path, id: SectorId) -> bool {
    id.path(root).exists()
}

pub(crate) fn overpass(query: &str, prefer: usize) -> Result<serde_json::Value, String> {
    let mut last = String::new();
    for attempt in 0..OVERPASS.len() {
        let url = OVERPASS[(prefer + attempt) % OVERPASS.len()];
        if attempt > 0 {
            std::thread::sleep(Duration::from_secs(2 * attempt as u64));
        }
        let r = agent()
            .post(url)
            .timeout(Duration::from_secs(200))
            .set("Content-Type", "application/x-www-form-urlencoded")
            .send_string(&format!("data={}", urlencode(query)));
        match r {
            Ok(r) => {
                let mut buf = Vec::new();
                if let Err(e) = r.into_reader().take(400 << 20).read_to_end(&mut buf) {
                    last = e.to_string();
                    continue;
                }
                match serde_json::from_slice::<serde_json::Value>(&buf) {
                    // przekroczony czas lub limit to kod 200 z uwagą i uciętymi danymi – nie wolno ich zapisać
                    Ok(v) if v["remark"].as_str().is_some_and(|r| r.contains("error")) => {
                        last = tf!("Overpass: {}", v["remark"].as_str().unwrap_or(""));
                    }
                    Ok(v) => return Ok(v),
                    Err(e) => last = tf!("odpowiedź Overpass: {}", e),
                }
            }
            Err(ureq::Error::Status(code, _)) => last = tf!("Overpass: kod {}", code),
            Err(e) => last = e.to_string(),
        }
        log::warn!("overpass {url}: {last}");
    }
    Err(last)
}

pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

const COLOR_WORDS: [&str; 9] = [
    "red", "blue", "green", "yellow", "black", "orange", "purple", "white", "brown",
];

fn trail_color(tags: &serde_json::Map<String, serde_json::Value>) -> String {
    let get = |k: &str| tags.get(k).and_then(|v| v.as_str()).unwrap_or("");
    let sym = get("osmc:symbol");
    if !sym.is_empty() {
        let parts: Vec<&str> = sym.split(':').collect();
        if let Some(fg) = parts.get(2).and_then(|p| p.split('_').next()) {
            if COLOR_WORDS.contains(&fg) && fg != "white" {
                return fg.into();
            }
        }
        if COLOR_WORDS.contains(&parts[0]) && parts[0] != "white" {
            return parts[0].into();
        }
    }
    let c = get("colour").to_lowercase();
    let c = if c.is_empty() { get("color").to_lowercase() } else { c };
    if COLOR_WORDS.contains(&c.as_str()) {
        return c;
    }
    match c.as_str() {
        "#ff0000" => "red",
        "#0000ff" => "blue",
        "#008000" | "#00ff00" => "green",
        "#ffff00" => "yellow",
        "#000000" => "black",
        _ => "other",
    }
    .into()
}

fn sac(s: &str) -> u8 {
    match s {
        "hiking" => 1,
        "mountain_hiking" => 2,
        "demanding_mountain_hiking" => 3,
        "alpine_hiking" => 4,
        "demanding_alpine_hiking" => 5,
        "difficult_alpine_hiking" => 6,
        _ => 0,
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Punkty i nazwy sektora: z kafli wektorowych (ułamek sekundy), a gdy niedostępne – z Overpass.
/// Publiczne serwery Overpass bywają przeciążone na minuty, a etykiety nie mogą na nie czekać.
pub fn fetch_pois(id: SectorId) -> Result<Vec<Poi>, String> {
    match vector_pois(id) {
        Ok(p) => Ok(p),
        Err(e) => {
            log::warn!("kafle wektorowe: {e}");
            overpass_pois(id)
        }
    }
}

/// Poziom kafli wektorowych z punktami (wyżej źródło ogranicza liczbę szczytów na kafel).
const VECTOR_Z: u8 = 12;
const VECTOR_LAYERS: [&str; 4] = ["mountain_peak", "place", "poi", "water_name"];

/// Adres kafli: bieżąca wersja danych z opisu zestawu.
fn vector_url() -> Result<String, String> {
    static URL: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
    if let Some(u) = URL.lock().unwrap().clone() {
        return Ok(u);
    }
    let body = agent()
        .get("https://tiles.openfreemap.org/planet")
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let u = v["tiles"][0].as_str().ok_or(crate::i18n::tr("brak adresu kafli wektorowych"))?.to_string();
    *URL.lock().unwrap() = Some(u.clone());
    Ok(u)
}

fn vector_pois(id: SectorId) -> Result<Vec<Poi>, String> {
    let url = vector_url()?;
    let k = 1u32 << (VECTOR_Z - SECTOR_Z);
    let tiles: Vec<TileId> = (0..k * k)
        .map(|i| TileId::new(VECTOR_Z, id.x * k + i % k, id.y * k + i / k))
        .collect();
    use rayon::prelude::*;
    let parts: Vec<Result<Vec<Poi>, String>> = tiles.par_iter().map(|t| vector_tile(&url, *t)).collect();
    let mut out = Vec::new();
    let mut failed = None;
    for p in parts {
        match p {
            Ok(mut v) => out.append(&mut v),
            Err(e) => failed = Some(e),
        }
    }
    match failed {
        Some(e) if out.is_empty() => Err(e),
        _ => {
            out.sort_by(|a, b| (&a.name, a.lon.to_bits()).cmp(&(&b.name, b.lon.to_bits())));
            out.dedup_by(|a, b| a.name == b.name && (a.lon - b.lon).abs() < 1e-4 && (a.lat - b.lat).abs() < 1e-4);
            Ok(out)
        }
    }
}

fn vector_tile(url: &str, t: TileId) -> Result<Vec<Poi>, String> {
    let u = url.replace("{z}", &t.z.to_string()).replace("{x}", &t.x.to_string()).replace("{y}", &t.y.to_string());
    let r = match agent().get(&u).call() {
        Ok(r) => r,
        // kafel bez danych (np. morze)
        Err(ureq::Error::Status(404, _)) => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
    let mut buf = Vec::new();
    r.into_reader().take(16 << 20).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let n = (1u64 << t.z) as f64;
    Ok(crate::mvt::points(&buf, &VECTOR_LAYERS)
        .iter()
        .filter_map(|p| {
            let lon = crate::geo::x_to_lon((t.x as f64 + p.x) / n);
            let lat = crate::geo::y_to_lat((t.y as f64 + p.y) / n);
            vector_poi(p, lon, lat)
        })
        .collect())
}

/// Punkt kafla na punkt programu (schemat OpenMapTiles).
fn vector_poi(p: &crate::mvt::Point, lon: f64, lat: f64) -> Option<Poi> {
    let class = p.text("class").unwrap_or("");
    let sub = p.text("subclass").unwrap_or("");
    let (kind, place) = match p.layer.as_str() {
        "mountain_peak" => match class {
            "peak" | "volcano" => (PoiKind::Peak, ""),
            "saddle" => (PoiKind::Saddle, ""),
            "ridge" | "arete" => (PoiKind::Ridge, ""),
            _ => return None,
        },
        "place" => match class {
            "city" | "town" | "village" | "hamlet" => (PoiKind::Place, class),
            "locality" | "isolated_dwelling" => (PoiKind::Locality, ""),
            _ => return None,
        },
        "water_name" => (PoiKind::Lake, ""),
        "poi" => match sub {
            "alpine_hut" | "wilderness_hut" => (PoiKind::Hut, ""),
            "shelter" => (PoiKind::Shelter, ""),
            "viewpoint" => (PoiKind::Viewpoint, ""),
            "waterfall" => (PoiKind::Waterfall, ""),
            "cave_entrance" => (PoiKind::Cave, ""),
            "spring" => (PoiKind::Spring, ""),
            _ => return None,
        },
        _ => return None,
    };
    let name = p.text("name").filter(|n| !n.is_empty())?;
    Some(Poi {
        kind,
        name: name.to_string(),
        lon,
        lat,
        ele: p.number("ele").map(|e| e as f32),
        wiki: String::new(),
        osm: String::new(),
        place: place.to_string(),
        pop: 0,
        info: Vec::new(),
    })
}

fn overpass_pois(id: SectorId) -> Result<Vec<Poi>, String> {
    let b = id.bbox();
    let bb = format!("{:.6},{:.6},{:.6},{:.6}", b.s, b.w, b.n, b.e);
    let poi_q = format!(
        "[out:json][timeout:90];(node[\"natural\"~\"^(peak|saddle|volcano)$\"][\"name\"]({bb});node[\"place\"~\"^(city|town|village|hamlet)$\"][\"name\"]({bb});node[\"mountain_pass\"=\"yes\"][\"name\"]({bb});nwr[\"tourism\"~\"^(alpine_hut|wilderness_hut)$\"]({bb});nwr[\"amenity\"=\"shelter\"][\"name\"]({bb});nwr[\"natural\"=\"water\"][\"name\"]({bb});node[\"waterway\"=\"waterfall\"][\"name\"]({bb});node[\"natural\"=\"cave_entrance\"][\"name\"]({bb});node[\"tourism\"=\"viewpoint\"][\"name\"]({bb});node[\"natural\"=\"spring\"][\"name\"]({bb});nwr[\"natural\"~\"^(valley|ridge|arete)$\"][\"name\"]({bb});node[\"place\"=\"locality\"][\"name\"]({bb}););out center tags;"
    );
    let pj = overpass(&poi_q, 1)?;
    Ok(parse(id, &serde_json::Value::Null, &pj).pois)
}

/// Szlaki sektora (blokujące, duże zapytanie) połączone z pobranymi wcześniej punktami.
/// `worker` wybiera serwer Overpass, od którego zaczynamy (równoległe pobierania nie biją w jeden).
pub fn fetch_trails(id: SectorId, pois: Vec<Poi>, worker: usize) -> Result<SectorData, String> {
    let b = id.bbox();
    // szlaki i dodatkowe dane (punkty usługowe, łańcuchy, granice) równolegle; niepowodzenie
    // dodatkowych danych nie blokuje szlaków – uzupełnią się później
    let (tj, extra) = std::thread::scope(|sc| {
        let ex = sc.spawn(|| fetch_extra(id, worker + 1));
        let tj = trails_by_quarters(&b, worker);
        (tj, ex.join().unwrap_or_else(|_| Err("panic".into())))
    });
    let tj = tj?;
    let mut d = parse(id, &tj, &serde_json::Value::Null);
    d.pois = pois;
    match extra {
        Ok(x) => apply_extra(&mut d, x),
        Err(e) => log::warn!("dodatkowe dane {}/{}: {e}", id.x, id.y),
    }
    Ok(d)
}

/// Dodatkowe dane sektora: punkty spoza kafli wektorowych (z tagami), łańcuchy i klamry, granice.
pub struct Extra {
    pois: Vec<Poi>,
    chains: Vec<Vec<(f64, f64)>>,
    boundaries: Vec<Boundary>,
}

/// Linie z geometrii Overpass (`geometry`: lista {lat, lon}, przerwy jako null).
fn geometry_lines(g: &serde_json::Value) -> Vec<Vec<(f64, f64)>> {
    let mut out = Vec::new();
    let mut cur: Vec<(f64, f64)> = Vec::new();
    for p in g.as_array().into_iter().flatten() {
        match (p["lon"].as_f64(), p["lat"].as_f64()) {
            (Some(lon), Some(lat)) => cur.push((lon, lat)),
            _ => {
                if cur.len() >= 2 {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
            }
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out
}

pub fn fetch_extra(id: SectorId, worker: usize) -> Result<Extra, String> {
    let b = id.bbox();
    let bb = format!("{:.6},{:.6},{:.6},{:.6}", b.s, b.w, b.n, b.e);
    let points_q = format!(
        "[out:json][timeout:90];(nwr[\"natural\"~\"^(valley|ridge|arete|spring|water)$\"][\"name\"]({bb});node[\"place\"=\"locality\"][\"name\"]({bb});node[\"waterway\"=\"waterfall\"][\"name\"]({bb});nwr[\"tourism\"~\"^(alpine_hut|wilderness_hut)$\"]({bb});nwr[\"amenity\"=\"shelter\"][\"name\"]({bb});node[\"amenity\"=\"drinking_water\"]({bb});nwr[\"amenity\"=\"parking\"][\"access\"!~\"^(private|no)$\"]({bb});node[\"highway\"=\"bus_stop\"][\"name\"]({bb});node[\"aerialway\"=\"station\"]({bb});node[\"historic\"~\"^(wayside_shrine|wayside_cross)$\"]({bb});nwr[\"building\"=\"chapel\"]({bb});nwr[\"contact:webcam\"]({bb});nwr[\"webcam\"]({bb}););out center tags;(way[\"safety_rope\"]({bb});way[\"via_ferrata_scale\"]({bb}););out tags geom;"
    );
    let bounds_q = format!(
        "[out:json][timeout:90];(relation[\"boundary\"=\"national_park\"]({bb});way[\"boundary\"=\"national_park\"]({bb});relation[\"leisure\"=\"nature_reserve\"]({bb});way[\"leisure\"=\"nature_reserve\"]({bb});relation[\"boundary\"=\"protected_area\"][\"protect_class\"~\"^(1|1a|1b|2|4)$\"]({bb});relation[\"boundary\"=\"administrative\"][\"admin_level\"=\"2\"]({bb}););out tags geom({bb});"
    );
    let pj = overpass(&points_q, worker)?;
    let mut chains = Vec::new();
    for el in pj["elements"].as_array().into_iter().flatten() {
        if el["type"] == "way" && el.get("geometry").is_some() {
            chains.extend(geometry_lines(&el["geometry"]));
        }
    }
    let pois = parse(id, &serde_json::Value::Null, &pj).pois;
    // granice: osobne zapytanie; błąd nie przekreśla punktów
    let mut boundaries = Vec::new();
    match overpass(&bounds_q, worker + 1) {
        Ok(bj) => {
            for el in bj["elements"].as_array().into_iter().flatten() {
                let t = &el["tags"];
                let tag = |k: &str| t[k].as_str().unwrap_or("");
                let kind = if tag("admin_level") == "2" && tag("boundary") == "administrative" {
                    BoundaryKind::Country
                } else if tag("boundary") == "national_park" || tag("protect_class") == "2" {
                    BoundaryKind::NationalPark
                } else {
                    BoundaryKind::Reserve
                };
                let name = if tag("name:pl").is_empty() { tag("name") } else { tag("name:pl") }.to_string();
                let mut lines = geometry_lines(&el["geometry"]);
                for m in el["members"].as_array().into_iter().flatten() {
                    lines.extend(geometry_lines(&m["geometry"]));
                }
                if !lines.is_empty() {
                    boundaries.push(Boundary { kind, name, lines });
                }
            }
        }
        Err(e) => log::warn!("granice {}/{}: {e}", id.x, id.y),
    }
    Ok(Extra { pois, chains, boundaries })
}

/// Dołączenie dodatkowych danych do sektora: nowe punkty, informacje do istniejących (np. schronisk
/// z kafli wektorowych), łańcuchy i granice.
pub fn apply_extra(d: &mut SectorData, x: Extra) {
    for p in x.pois {
        let same = d.pois.iter_mut().find(|q| {
            q.kind == p.kind && (q.name == p.name || p.kind == PoiKind::Hut) && (q.lon - p.lon).abs() < 0.004 && (q.lat - p.lat).abs() < 0.004
        });
        match same {
            Some(q) => {
                if q.info.is_empty() {
                    q.info = p.info;
                }
                if q.osm.is_empty() {
                    q.osm = p.osm;
                }
                if q.wiki.is_empty() {
                    q.wiki = p.wiki;
                }
            }
            None => d.pois.push(p),
        }
    }
    d.chains = x.chains;
    d.boundaries = x.boundaries;
    d.extra_ver = EXTRA_VER;
}

/// Sektor z samymi punktami – do czasu pobrania szlaków.
pub fn partial(pois: Vec<Poi>) -> SectorData {
    SectorData { format: FORMAT, fetched: now_secs(), pois, partial: true, ..Default::default() }
}

fn parse(id: SectorId, tj: &serde_json::Value, pj: &serde_json::Value) -> SectorData {
    let b = id.bbox();
    let empty = Vec::new();
    let els = tj["elements"].as_array().unwrap_or(&empty);
    let mut coords: HashMap<i64, (f64, f64)> = HashMap::new();
    let mut ways: Vec<&serde_json::Value> = Vec::new();
    let mut rels: Vec<&serde_json::Value> = Vec::new();
    for el in els {
        match el["type"].as_str() {
            Some("node") => {
                if let (Some(id), Some(lon), Some(lat)) =
                    (el["id"].as_i64(), el["lon"].as_f64(), el["lat"].as_f64())
                {
                    coords.insert(id, (lon, lat));
                }
            }
            Some("way") => ways.push(el),
            Some("relation") => rels.push(el),
            _ => {}
        }
    }
    let empty_map = serde_json::Map::new();
    let mut routes = Vec::new();
    let mut way_routes: HashMap<i64, Vec<u32>> = HashMap::new();
    for r in rels {
        let t = r["tags"].as_object().unwrap_or(&empty_map);
        let get = |k: &str| t.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let idx = routes.len() as u32;
        let name = [get("name:pl"), get("name"), get("ref")]
            .into_iter()
            .find(|s| !s.is_empty())
            .unwrap_or_else(|| tf!("Szlak {}", r["id"]));
        routes.push(RouteInfo {
            id: r["id"].as_i64().unwrap_or(0),
            name,
            r#ref: get("ref"),
            color: trail_color(t),
            network: get("network"),
            operator: get("operator"),
        });
        for m in r["members"].as_array().unwrap_or(&empty) {
            if m["type"] == "way" {
                if let Some(w) = m["ref"].as_i64() {
                    way_routes.entry(w).or_default().push(idx);
                }
            }
        }
    }
    // odcinki, których pierwszy węzeł leży w sektorze (półotwarty prostokąt: bez podwójnych granic)
    let inside = |(lon, lat): (f64, f64)| lon >= b.w && lon < b.e && lat > b.s && lat <= b.n;
    let mut node_idx: HashMap<i64, u32> = HashMap::new();
    let mut nodes = Vec::new();
    let mut idx_of = |id: i64, c: (f64, f64)| -> u32 {
        *node_idx.entry(id).or_insert_with(|| {
            nodes.push((id, c.0, c.1));
            (nodes.len() - 1) as u32
        })
    };
    let mut runs_out: Vec<WayRun> = Vec::new();
    let mut seen_ways = HashSet::new();
    for w in ways {
        let Some(wid) = w["id"].as_i64() else { continue };
        if !seen_ways.insert(wid) {
            continue;
        }
        let t = w["tags"].as_object().unwrap_or(&empty_map);
        let get = |k: &str| t.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let ids: Vec<i64> = w["nodes"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .filter_map(|v| v.as_i64())
            .collect();
        let mut way_runs: Vec<Vec<u32>> = Vec::new();
        let mut cur: Vec<u32> = Vec::new();
        for k in 0..ids.len().saturating_sub(1) {
            let (Some(&a), Some(&c)) = (coords.get(&ids[k]), coords.get(&ids[k + 1])) else {
                if cur.len() > 1 {
                    way_runs.push(std::mem::take(&mut cur));
                }
                cur.clear();
                continue;
            };
            if inside(a) {
                if cur.is_empty() {
                    cur.push(idx_of(ids[k], a));
                }
                cur.push(idx_of(ids[k + 1], c));
            } else if !cur.is_empty() {
                way_runs.push(std::mem::take(&mut cur));
            }
        }
        if cur.len() > 1 {
            way_runs.push(cur);
        }
        if way_runs.is_empty() {
            continue;
        }
        let mut rs = way_routes.get(&wid).cloned().unwrap_or_default();
        rs.sort_unstable();
        rs.dedup();
        let pl = get("name:pl");
        let name = if pl.is_empty() { get("name") } else { pl };
        let oneway = match get("oneway").as_str() {
            "yes" => 1,
            "-1" => -1,
            _ => 0,
        };
        let s = sac(&get("sac_scale"));
        let highway = get("highway");
        for r in way_runs {
            runs_out.push(WayRun {
                way: wid,
                nodes: r,
                routes: rs.clone(),
                sac: s,
                oneway,
                name: name.clone(),
                highway: highway.clone(),
            });
        }
    }

    // punkty
    let mut pois = Vec::new();
    let pels = pj["elements"].as_array().unwrap_or(&empty);
    for el in pels {
        let t = el["tags"].as_object().unwrap_or(&empty_map);
        let get = |k: &str| t.get(k).and_then(|v| v.as_str()).unwrap_or("");
        let lon = el["lon"].as_f64().or_else(|| el["center"]["lon"].as_f64());
        let lat = el["lat"].as_f64().or_else(|| el["center"]["lat"].as_f64());
        let (Some(lon), Some(lat)) = (lon, lat) else { continue };
        if !inside((lon, lat)) {
            continue;
        }
        let nat = get("natural");
        let kind = if nat == "peak" || nat == "volcano" {
            PoiKind::Peak
        } else if nat == "saddle" || get("mountain_pass") == "yes" {
            PoiKind::Saddle
        } else if get("tourism") == "alpine_hut" {
            PoiKind::Hut
        } else if get("tourism") == "wilderness_hut" || get("amenity") == "shelter" {
            PoiKind::Shelter
        } else if nat == "water" {
            PoiKind::Lake
        } else if get("waterway") == "waterfall" {
            PoiKind::Waterfall
        } else if nat == "cave_entrance" {
            PoiKind::Cave
        } else if get("tourism") == "viewpoint" {
            PoiKind::Viewpoint
        } else if nat == "spring" {
            PoiKind::Spring
        } else if nat == "valley" {
            PoiKind::Valley
        } else if nat == "ridge" || nat == "arete" {
            PoiKind::Ridge
        } else if get("place") == "locality" {
            PoiKind::Locality
        } else if get("amenity") == "drinking_water" || get("drinking_water") == "yes" && nat != "spring" {
            PoiKind::DrinkingWater
        } else if get("amenity") == "parking" {
            PoiKind::Parking
        } else if get("highway") == "bus_stop" || get("public_transport") == "platform" {
            PoiKind::BusStop
        } else if get("aerialway") == "station" {
            PoiKind::Cableway
        } else if matches!(get("historic"), "wayside_shrine" | "wayside_cross") || get("building") == "chapel" {
            PoiKind::Shrine
        } else if !get("contact:webcam").is_empty() || !get("webcam").is_empty() {
            PoiKind::Webcam
        } else if !get("place").is_empty() {
            PoiKind::Place
        } else {
            continue;
        };
        let name = if !get("name:pl").is_empty() {
            get("name:pl")
        } else {
            get("name")
        };
        // punkty usługowe często bez nazwy – podpisujemy je rodzajem
        let unnamed_ok = matches!(kind, PoiKind::Hut | PoiKind::DrinkingWater | PoiKind::Parking | PoiKind::Shrine | PoiKind::Cableway | PoiKind::Webcam);
        if name.is_empty() && !unnamed_ok {
            continue;
        }
        let name = if name.is_empty() && kind != PoiKind::Hut { kind.label() } else { name };
        let mut info: Vec<(String, String)> = Vec::new();
        for (k, keys) in [
            ("hours", &["opening_hours"][..]),
            ("phone", &["phone", "contact:phone"][..]),
            ("website", &["website", "contact:website", "url"][..]),
            ("email", &["email", "contact:email"][..]),
            ("beds", &["beds"][..]),
            ("capacity", &["capacity"][..]),
            ("operator", &["operator"][..]),
            ("fee", &["fee"][..]),
            ("webcam", &["contact:webcam", "webcam"][..]),
            ("description", &["description:pl", "description"][..]),
            ("reservation", &["reservation"][..]),
        ] {
            if let Some(v) = keys.iter().map(|x| get(x)).find(|v| !v.is_empty()) {
                info.push((k.to_string(), v.to_string()));
            }
        }

        let ele = get("ele").replace(',', ".").trim_end_matches(" m").parse::<f32>().ok();
        pois.push(Poi {
            kind,
            name: if name.is_empty() {
                crate::i18n::tr("Schronisko").into()
            } else {
                name.into()
            },
            lon,
            lat,
            ele: ele.filter(|e| e.is_finite() && *e > -500.0 && *e < 9000.0),
            wiki: get("wikipedia").into(),
            osm: format!("{}/{}", el["type"].as_str().unwrap_or("node"), el["id"]),
            place: if kind == PoiKind::Place {
                get("place").into()
            } else {
                String::new()
            },
            pop: get("population")
                .chars()
                .filter(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .unwrap_or(0),
            info,
        });
    }
    log::info!(
        "sektor {}/{}: {} tras, {} ciągów dróg, {} punktów",
        id.x,
        id.y,
        routes.len(),
        runs_out.len(),
        pois.len()
    );
    SectorData {
        format: FORMAT,
        fetched: now_secs(),
        nodes,
        runs: runs_out,
        routes,
        pois,
        partial: false,
        ..Default::default()
    }
}

/// Wyszukiwanie miejsc w Nominatim (OSM), najwyżej raz na sekundę.
#[derive(Clone, Debug)]
pub struct Place {
    pub name: String,
    pub label: String,
    pub lon: f64,
    pub lat: f64,
    pub bbox: Option<BBox>,
}

pub fn geocode(q: &str) -> Result<Vec<Place>, String> {
    static LAST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
    {
        let mut l = LAST.lock().unwrap();
        if let Some(t) = *l {
            let el = t.elapsed();
            if el < Duration::from_millis(1100) {
                std::thread::sleep(Duration::from_millis(1100) - el);
            }
        }
        *l = Some(std::time::Instant::now());
    }
    let url = format!(
        "https://nominatim.openstreetmap.org/search?format=jsonv2&limit=8&accept-language=pl&q={}",
        urlencode(q)
    );
    let body = agent()
        .get(&url)
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let empty = Vec::new();
    Ok(v.as_array()
        .unwrap_or(&empty)
        .iter()
        .filter_map(|p| {
            let lon = p["lon"].as_str()?.parse().ok()?;
            let lat = p["lat"].as_str()?.parse().ok()?;
            let label = p["display_name"].as_str().unwrap_or("").to_string();
            let name = p["name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(String::from)
                .unwrap_or_else(|| label.split(',').next().unwrap_or("").to_string());
            let bbox = p["boundingbox"].as_array().and_then(|b| {
                let f = |i: usize| b.get(i)?.as_str()?.parse::<f64>().ok();
                Some(BBox::new(f(2)?, f(0)?, f(3)?, f(1)?))
            });
            Some(Place {
                name,
                label,
                lon,
                lat,
                bbox,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sectors_partition_ways() {
        // droga przechodząca przez granicę sektorów: odcinki dzielą się bez nakładek
        let a = SectorId { x: 284, y: 177 };
        let b = a.bbox();
        let mid = b.e;
        let lat = (b.s + b.n) / 2.0;
        let tj = serde_json::json!({"elements": [
            {"type":"relation","id":1,"tags":{"route":"hiking","osmc:symbol":"red:white:red_bar","name":"Czerwony"},"members":[{"type":"way","ref":10}]},
            {"type":"way","id":10,"nodes":[1,2,3,4],"tags":{"highway":"path"}},
            {"type":"node","id":1,"lon":mid-0.02,"lat":lat},
            {"type":"node","id":2,"lon":mid-0.01,"lat":lat},
            {"type":"node","id":3,"lon":mid+0.01,"lat":lat},
            {"type":"node","id":4,"lon":mid+0.02,"lat":lat},
        ]});
        let pj = serde_json::json!({"elements": []});
        let da = parse(a, &tj, &pj);
        let db = parse(SectorId { x: a.x + 1, y: a.y }, &tj, &pj);
        assert_eq!(da.runs.len(), 1);
        assert_eq!(da.runs[0].nodes.len(), 3); // 1-2-3 (3 już w sąsiednim sektorze)
        assert_eq!(db.runs[0].nodes.len(), 2); // 3-4
        assert_eq!(da.routes[0].color, "red");
    }
}

/// Szlaki sektora w czterech mniejszych zapytaniach (ćwiartki): Overpass liczy je dużo szybciej
/// i rzadziej przekracza limit czasu niż jedno zapytanie o cały sektor. Współrzędne przychodzą od
/// razu w drogach (`out geom`) – bez osobnego, ciężkiego pobierania punktów. Wynik w postaci
/// zwykłej odpowiedzi Overpass (węzły, drogi, relacje), bez powtórzeń.
fn trails_by_quarters(b: &crate::geo::BBox, worker: usize) -> Result<serde_json::Value, String> {
    let (mx, my) = ((b.w + b.e) / 2.0, (b.s + b.n) / 2.0);
    let quarters = [(b.w, b.s, mx, my), (mx, b.s, b.e, my), (b.w, my, mx, b.n), (mx, my, b.e, b.n)];
    let mut seen: std::collections::HashSet<(String, i64)> = std::collections::HashSet::new();
    let mut out: Vec<serde_json::Value> = Vec::new();
    for (k, (w, s, e, n)) in quarters.into_iter().enumerate() {
        let bb = format!("{s:.6},{w:.6},{n:.6},{e:.6}");
        let q = format!(
            "[out:json][timeout:90];relation[\"route\"~\"^(hiking|foot)$\"]({bb})->.r;way(r.r)({bb})->.w;.r out body;.w out body geom;"
        );
        let v = overpass(&q, worker + k)?;
        for el in v["elements"].as_array().into_iter().flatten() {
            let ty = el["type"].as_str().unwrap_or("").to_string();
            let Some(id) = el["id"].as_i64() else { continue };
            if !seen.insert((ty.clone(), id)) {
                continue;
            }
            // węzły drogi z jej geometrii (identyfikatory z listy „nodes”)
            if ty == "way" {
                let ids = el["nodes"].as_array();
                let geo = el["geometry"].as_array();
                if let (Some(ids), Some(geo)) = (ids, geo) {
                    for (nid, g) in ids.iter().zip(geo) {
                        if let (Some(nid), Some(lat), Some(lon)) = (nid.as_i64(), g["lat"].as_f64(), g["lon"].as_f64()) {
                            if seen.insert(("node".into(), nid)) {
                                out.push(serde_json::json!({ "type": "node", "id": nid, "lat": lat, "lon": lon }));
                            }
                        }
                    }
                }
            }
            out.push(el.clone());
        }
    }
    Ok(serde_json::json!({ "elements": out }))
}
