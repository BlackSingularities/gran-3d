//! Obiekty z OpenStreetMap dla Studio (ODbL – podpis „© Autorzy OpenStreetMap” w każdym
//! eksporcie): wody, lasy, skały, lodowce, budynki, drogi, ścieżki, szlaki turystyczne z kolorami,
//! koleje i wyciągi, szczyty, przełęcze, schroniska i miejscowości. Jedno zapytanie Overpass
//! (bez kafli wektorowych), współrzędne od razu w lokalnym układzie [`Frame`].

use std::collections::HashMap;

use super::geom::Frame;

pub const OSM_CREDIT: &str = "© Autorzy OpenStreetMap (ODbL, openstreetmap.org/copyright)";

/// Rodzaj linii.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineKind {
    River,
    Stream,
    Path,
    Track,
    Road,
    MajorRoad,
    Rail,
    Aerialway,
    /// Znakowany szlak turystyczny w kolorze RGB.
    Trail([u8; 3]),
}

/// Rodzaj powierzchni.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AreaKind {
    Water,
    Forest,
    Rock,
    Scree,
    Glacier,
    Building,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointKind {
    Peak,
    Saddle,
    Hut,
    /// Miejscowość: 0 – miasto, 1 – miasteczko, 2 – wieś, 3 – przysiółek.
    Place(u8),
}

#[derive(Clone, Debug)]
pub struct Line {
    pub kind: LineKind,
    pub pts: Vec<(f64, f64)>,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct Poly {
    pub kind: AreaKind,
    /// Pierścienie (zewnętrzne i dziury) – wypełnianie regułą parzystości.
    pub rings: Vec<Vec<(f64, f64)>>,
}

#[derive(Clone, Debug)]
pub struct Label {
    pub kind: PointKind,
    pub x: f64,
    pub y: f64,
    pub name: String,
    pub ele: Option<f32>,
}

#[derive(Clone, Debug, Default)]
pub struct Features {
    pub lines: Vec<Line>,
    pub polys: Vec<Poly>,
    pub labels: Vec<Label>,
}

/// Zapytanie Overpass dla prostokąta (w, s, e, n); budynki tylko na mniejszych obszarach.
pub fn query(geo: [f64; 4], buildings: bool) -> String {
    let bb = format!("({:.5},{:.5},{:.5},{:.5})", geo[1], geo[0], geo[3], geo[2]);
    let b = if buildings { format!("way[\"building\"]{bb};") } else { String::new() };
    format!(
        "[out:json][timeout:120][maxsize:536870912];(\
way[\"natural\"~\"^(water|wood|bare_rock|scree|glacier|shingle)$\"]{bb};\
relation[\"natural\"~\"^(water|wood|bare_rock|scree|glacier)$\"]{bb};\
way[\"landuse\"~\"^(forest|reservoir|basin)$\"]{bb};\
relation[\"landuse\"=\"forest\"]{bb};\
way[\"waterway\"~\"^(river|stream|canal|riverbank)$\"]{bb};\
way[\"highway\"~\"^(motorway|trunk|primary|secondary|tertiary|unclassified|residential|living_street|service|track|path|footway|bridleway|steps|cycleway)$\"]{bb};\
way[\"railway\"~\"^(rail|narrow_gauge|funicular)$\"]{bb};\
way[\"aerialway\"]{bb};\
relation[\"route\"=\"hiking\"]{bb};\
node[\"natural\"~\"^(peak|saddle|volcano)$\"]{bb};\
node[\"tourism\"~\"^(alpine_hut|wilderness_hut)$\"]{bb};\
way[\"tourism\"=\"alpine_hut\"]{bb};\
node[\"place\"~\"^(city|town|village|hamlet)$\"]{bb};\
{b});out geom;"
    )
}

/// Kolor szlaku z `osmc:symbol` („red:white:red_bar”) albo `colour`.
pub fn trail_color(tags: &serde_json::Value) -> [u8; 3] {
    let raw = tags["osmc:symbol"].as_str().and_then(|s| s.split(':').next()).or_else(|| tags["colour"].as_str()).unwrap_or("").to_ascii_lowercase();
    match raw.as_str() {
        "red" | "czerwony" => [214, 40, 40],
        "blue" | "niebieski" => [30, 90, 210],
        "green" | "zielony" => [20, 150, 60],
        "yellow" | "żółty" => [235, 190, 0],
        "black" | "czarny" => [30, 30, 30],
        "orange" => [240, 130, 20],
        "purple" | "violet" => [140, 60, 170],
        "brown" => [130, 80, 40],
        "white" => [120, 120, 120],
        s if s.starts_with('#') && s.len() == 7 => {
            let h = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).unwrap_or(0);
            [h(1), h(3), h(5)]
        }
        _ => [200, 50, 120],
    }
}

fn geom(f: &Frame, g: &serde_json::Value) -> Vec<(f64, f64)> {
    g.as_array()
        .map(|a| a.iter().filter_map(|p| Some(f.to_m(p["lon"].as_f64()?, p["lat"].as_f64()?))).collect())
        .unwrap_or_default()
}

fn area_kind(t: &serde_json::Value) -> Option<AreaKind> {
    let s = |k: &str| t[k].as_str().unwrap_or("");
    if s("building") != "" {
        return Some(AreaKind::Building);
    }
    match (s("natural"), s("landuse"), s("waterway")) {
        ("water", _, _) | (_, "reservoir" | "basin", _) | (_, _, "riverbank") => Some(AreaKind::Water),
        ("wood", _, _) | (_, "forest", _) => Some(AreaKind::Forest),
        ("bare_rock", _, _) => Some(AreaKind::Rock),
        ("scree" | "shingle", _, _) => Some(AreaKind::Scree),
        ("glacier", _, _) => Some(AreaKind::Glacier),
        _ => None,
    }
}

fn line_kind(t: &serde_json::Value) -> Option<LineKind> {
    let s = |k: &str| t[k].as_str().unwrap_or("");
    if s("aerialway") != "" && s("aerialway") != "pylon" && s("aerialway") != "station" {
        return Some(LineKind::Aerialway);
    }
    match s("waterway") {
        "river" | "canal" => return Some(LineKind::River),
        "stream" => return Some(LineKind::Stream),
        _ => {}
    }
    if !s("railway").is_empty() {
        return Some(LineKind::Rail);
    }
    match s("highway") {
        "motorway" | "trunk" | "primary" | "secondary" => Some(LineKind::MajorRoad),
        "tertiary" | "unclassified" | "residential" | "living_street" | "service" => Some(LineKind::Road),
        "track" => Some(LineKind::Track),
        "path" | "footway" | "bridleway" | "steps" | "cycleway" => Some(LineKind::Path),
        _ => None,
    }
}

/// Składa odcinki (drogi relacji) w pierścienie po wspólnych końcach.
pub fn join_rings(mut parts: Vec<Vec<(f64, f64)>>) -> Vec<Vec<(f64, f64)>> {
    let close = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 0.05 && (a.1 - b.1).abs() < 0.05;
    let mut rings = Vec::new();
    while let Some(mut cur) = parts.pop() {
        loop {
            if cur.len() > 2 && close(cur[0], *cur.last().unwrap()) {
                break;
            }
            let end = *cur.last().unwrap();
            let Some(k) = parts.iter().position(|p| close(p[0], end) || close(*p.last().unwrap(), end)) else { break };
            let mut p = parts.swap_remove(k);
            if !close(p[0], end) {
                p.reverse();
            }
            cur.extend(p.into_iter().skip(1));
        }
        if cur.len() >= 3 {
            rings.push(cur);
        }
    }
    rings
}

/// Obiekty z odpowiedzi Overpass (`out geom`).
pub fn parse(f: &Frame, v: &serde_json::Value) -> Features {
    let mut out = Features::default();
    let empty = Vec::new();
    // drogi będące częścią szlaku – rysujemy je jako szlak, nie jako zwykłą ścieżkę
    let mut trail_ways: HashMap<i64, [u8; 3]> = HashMap::new();
    for e in v["elements"].as_array().unwrap_or(&empty) {
        if e["type"] == "relation" && e["tags"]["route"] == "hiking" {
            let c = trail_color(&e["tags"]);
            for m in e["members"].as_array().unwrap_or(&empty) {
                if m["type"] == "way" {
                    if let Some(id) = m["ref"].as_i64() {
                        trail_ways.insert(id, c);
                    }
                    let pts = geom(f, &m["geometry"]);
                    if pts.len() >= 2 {
                        out.lines.push(Line { kind: LineKind::Trail(c), pts, name: e["tags"]["name"].as_str().unwrap_or("").to_string() });
                    }
                }
            }
        }
    }
    for e in v["elements"].as_array().unwrap_or(&empty) {
        let t = &e["tags"];
        let name = crate::osm::local_name(&|k| t[k].as_str().unwrap_or("")).to_string();
        match e["type"].as_str() {
            Some("node") => {
                let (Some(lon), Some(lat)) = (e["lon"].as_f64(), e["lat"].as_f64()) else { continue };
                let (x, y) = f.to_m(lon, lat);
                let ele = t["ele"].as_str().and_then(|s| s.trim_end_matches(" m").replace(',', ".").parse::<f32>().ok());
                let kind = match (t["natural"].as_str(), t["tourism"].as_str(), t["place"].as_str()) {
                    (Some("peak" | "volcano"), _, _) => PointKind::Peak,
                    (Some("saddle"), _, _) => PointKind::Saddle,
                    (_, Some(_), _) => PointKind::Hut,
                    (_, _, Some("city")) => PointKind::Place(0),
                    (_, _, Some("town")) => PointKind::Place(1),
                    (_, _, Some("village")) => PointKind::Place(2),
                    (_, _, Some(_)) => PointKind::Place(3),
                    _ => continue,
                };
                if name.is_empty() && !matches!(kind, PointKind::Peak) {
                    continue;
                }
                out.labels.push(Label { kind, x, y, name, ele });
            }
            Some("way") => {
                let pts = geom(f, &e["geometry"]);
                if pts.len() < 2 {
                    continue;
                }
                if t["tourism"] == "alpine_hut" {
                    let n = pts.len() as f64;
                    let (x, y) = (pts.iter().map(|p| p.0).sum::<f64>() / n, pts.iter().map(|p| p.1).sum::<f64>() / n);
                    out.labels.push(Label { kind: PointKind::Hut, x, y, name, ele: None });
                    continue;
                }
                let closed = pts.len() > 3 && pts[0] == *pts.last().unwrap();
                if let (true, Some(k)) = (closed, area_kind(t)) {
                    out.polys.push(Poly { kind: k, rings: vec![pts] });
                    continue;
                }
                if let Some(k) = line_kind(t) {
                    // ścieżki na szlakach już są (w kolorze szlaku)
                    if matches!(k, LineKind::Path | LineKind::Track) && e["id"].as_i64().is_some_and(|id| trail_ways.contains_key(&id)) {
                        continue;
                    }
                    out.lines.push(Line { kind: k, pts, name });
                }
            }
            Some("relation") => {
                let Some(k) = area_kind(t) else { continue };
                let parts: Vec<Vec<(f64, f64)>> = e["members"]
                    .as_array()
                    .unwrap_or(&empty)
                    .iter()
                    .filter(|m| m["type"] == "way")
                    .map(|m| geom(f, &m["geometry"]))
                    .filter(|p| p.len() >= 2)
                    .collect();
                let rings = join_rings(parts);
                if !rings.is_empty() {
                    out.polys.push(Poly { kind: k, rings });
                }
            }
            _ => {}
        }
    }
    // najważniejsze na wierzchu: lasy, skały, lodowce, woda, budynki
    out.polys.sort_by_key(|p| match p.kind {
        AreaKind::Forest => 0,
        AreaKind::Scree => 1,
        AreaKind::Rock => 2,
        AreaKind::Glacier => 3,
        AreaKind::Water => 4,
        AreaKind::Building => 5,
    });
    out.lines.sort_by_key(|l| match l.kind {
        LineKind::Stream => 0,
        LineKind::River => 1,
        LineKind::Track => 2,
        LineKind::Path => 3,
        LineKind::Road => 4,
        LineKind::MajorRoad => 5,
        LineKind::Rail => 6,
        LineKind::Trail(_) => 7,
        LineKind::Aerialway => 8,
    });
    out
}

/// Pobranie obiektów dla prostokąta geograficznego.
pub fn fetch(f: &Frame, geo: [f64; 4], buildings: bool) -> Result<Features, String> {
    let v = crate::osm::overpass_public(&query(geo, buildings), 0)?;
    Ok(parse(f, &v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_and_rings() {
        let t = serde_json::json!({ "osmc:symbol": "red:white:red_bar" });
        assert_eq!(trail_color(&t), [214, 40, 40]);
        let rings = join_rings(vec![vec![(0.0, 0.0), (1.0, 0.0)], vec![(1.0, 1.0), (1.0, 0.0)], vec![(1.0, 1.0), (0.0, 0.0)]]);
        assert_eq!(rings.len(), 1);
        assert!(rings[0].len() >= 4);
    }

    #[test]
    fn parses_elements() {
        let f = Frame::new(20.0, 49.2);
        let v = serde_json::json!({ "elements": [
            { "type": "node", "lon": 20.0, "lat": 49.2, "tags": { "natural": "peak", "name": "Szczyt", "ele": "2499" } },
            { "type": "way", "id": 5, "tags": { "highway": "path" }, "geometry": [ { "lon": 20.0, "lat": 49.2 }, { "lon": 20.01, "lat": 49.21 } ] },
            { "type": "relation", "tags": { "route": "hiking", "osmc:symbol": "blue:white:blue_bar" }, "members": [
                { "type": "way", "ref": 5, "geometry": [ { "lon": 20.0, "lat": 49.2 }, { "lon": 20.01, "lat": 49.21 } ] } ] }
        ]});
        let r = parse(&f, &v);
        assert_eq!(r.labels.len(), 1);
        assert_eq!(r.labels[0].ele, Some(2499.0));
        assert_eq!(r.lines.len(), 1);
        assert!(matches!(r.lines[0].kind, LineKind::Trail(_)));
    }
}
