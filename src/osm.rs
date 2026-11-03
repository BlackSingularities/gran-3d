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
const FORMAT: u32 = 2;

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
}

impl PoiKind {
    pub fn label(self) -> &'static str {
        match self {
            PoiKind::Peak => "Szczyt",
            PoiKind::Saddle => "Przełęcz",
            PoiKind::Hut => "Schronisko",
            PoiKind::Shelter => "Wiata / schron",
            PoiKind::Lake => "Staw / jezioro",
            PoiKind::Waterfall => "Wodospad",
            PoiKind::Cave => "Jaskinia",
            PoiKind::Viewpoint => "Punkt widokowy",
            PoiKind::Place => "Miejscowość",
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
}

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
    for attempt in 0..OVERPASS.len() * 2 {
        let url = OVERPASS[(prefer + attempt) % OVERPASS.len()];
        if attempt >= OVERPASS.len() {
            std::thread::sleep(Duration::from_secs(4 * attempt as u64));
        }
        let r = agent()
            .post(url)
            .timeout(Duration::from_secs(300))
            .set("Content-Type", "application/x-www-form-urlencoded")
            .send_string(&format!("data={}", urlencode(query)));
        match r {
            Ok(r) => {
                let mut buf = Vec::new();
                if let Err(e) = r.into_reader().take(400 << 20).read_to_end(&mut buf) {
                    last = e.to_string();
                    continue;
                }
                match serde_json::from_slice(&buf) {
                    Ok(v) => return Ok(v),
                    Err(e) => last = format!("odpowiedź Overpass: {e}"),
                }
            }
            Err(ureq::Error::Status(code, _)) => last = format!("Overpass: kod {code}"),
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

/// Pobranie i przetworzenie sektora (blokujące, ~10–60 s).
pub fn fetch(id: SectorId) -> Result<SectorData, String> {
    let b = id.bbox();
    let bb = format!("{:.6},{:.6},{:.6},{:.6}", b.s, b.w, b.n, b.e);
    let trails_q = format!(
        "[out:json][timeout:240];relation[\"route\"~\"^(hiking|foot)$\"]({bb})->.r;way(r.r)({bb})->.w;.r out body;.w out body;.w >;out skel qt;"
    );
    let poi_q = format!(
        "[out:json][timeout:120];(node[\"natural\"~\"^(peak|saddle|volcano)$\"][\"name\"]({bb});node[\"place\"~\"^(city|town|village|hamlet)$\"][\"name\"]({bb});node[\"mountain_pass\"=\"yes\"][\"name\"]({bb});nwr[\"tourism\"~\"^(alpine_hut|wilderness_hut)$\"]({bb});nwr[\"amenity\"=\"shelter\"][\"name\"]({bb});nwr[\"natural\"=\"water\"][\"name\"]({bb});node[\"waterway\"=\"waterfall\"][\"name\"]({bb});node[\"natural\"=\"cave_entrance\"][\"name\"]({bb});node[\"tourism\"=\"viewpoint\"][\"name\"]({bb}););out center tags;"
    );
    let (tj, pj) = std::thread::scope(|s| {
        let t = s.spawn(|| overpass(&trails_q, 0));
        let p = s.spawn(|| overpass(&poi_q, 1));
        (t.join().unwrap(), p.join().unwrap())
    });
    let tj = tj?;
    let pj = pj?;
    Ok(parse(id, &tj, &pj))
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
            .unwrap_or_else(|| format!("Szlak {}", r["id"]));
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
        if name.is_empty() && kind != PoiKind::Hut {
            continue;
        }
        let ele = get("ele").replace(',', ".").trim_end_matches(" m").parse::<f32>().ok();
        pois.push(Poi {
            kind,
            name: if name.is_empty() {
                "Schronisko".into()
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
