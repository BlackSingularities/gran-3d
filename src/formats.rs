//! Formaty śladów poza GPX: KML (Google Earth) i GeoJSON – wczytywanie i zapis.
//! Wspólne wejście [`parse_any`] rozpoznaje format po rozszerzeniu albo treści.

use crate::gpx::GpxTrack;
use crate::graph::Route;

/// Rozszerzenia plików, które GRAŃ potrafi wczytać.
pub const EXTENSIONS: [&str; 4] = ["gpx", "kml", "geojson", "json"];

pub fn is_track_file(p: &std::path::Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// Wczytanie śladu z tekstu pliku; `ext` – rozszerzenie (małymi literami), może być puste.
pub fn parse_any(text: &str, ext: &str) -> Result<GpxTrack, String> {
    let head = text.trim_start();
    match ext {
        "gpx" => crate::gpx::parse(text),
        "kml" => parse_kml(text),
        "geojson" | "json" => parse_geojson(text),
        _ if head.starts_with('{') => parse_geojson(text),
        _ if head.contains("<kml") => parse_kml(text),
        _ => crate::gpx::parse(text),
    }
}

fn esc(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '<' | '>' | '&' | '"' | '\'' => format!("&#{};", c as u32),
            c => c.to_string(),
        })
        .collect()
}

pub fn parse_kml(text: &str) -> Result<GpxTrack, String> {
    let doc = roxmltree::Document::parse(text).map_err(|_| crate::i18n::tr("Plik nie jest poprawnym KML.").to_string())?;
    let name = doc
        .descendants()
        .find(|n| n.has_tag_name("name"))
        .and_then(|n| n.text())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| crate::i18n::tr("Ślad KML").to_string());
    let mut points: Vec<(f64, f64, Option<f32>)> = Vec::new();
    // LineString: „dł,szer[,wys] dł,szer[,wys] …”
    for n in doc.descendants().filter(|n| n.has_tag_name("LineString")) {
        if let Some(c) = n.children().find(|c| c.has_tag_name("coordinates")).and_then(|c| c.text()) {
            for t in c.split_whitespace() {
                let v: Vec<&str> = t.split(',').collect();
                if v.len() >= 2 {
                    if let (Ok(lon), Ok(lat)) = (v[0].parse::<f64>(), v[1].parse::<f64>()) {
                        let e = v.get(2).and_then(|s| s.parse::<f32>().ok()).filter(|e| *e != 0.0);
                        points.push((lon, lat, e));
                    }
                }
            }
        }
    }
    // gx:Track: „dł szer wys” w osobnych <gx:coord>
    if points.is_empty() {
        for n in doc.descendants().filter(|n| n.tag_name().name() == "coord") {
            if let Some(t) = n.text() {
                let v: Vec<f64> = t.split_whitespace().filter_map(|s| s.parse().ok()).collect();
                if v.len() >= 2 {
                    points.push((v[0], v[1], v.get(2).map(|e| *e as f32)));
                }
            }
        }
    }
    points.retain(|p| p.0.is_finite() && p.1.is_finite());
    if points.len() < 2 {
        return Err(crate::i18n::tr("KML nie zawiera linii (LineString / gx:Track).").into());
    }
    Ok(GpxTrack { name, points })
}

pub fn parse_geojson(text: &str) -> Result<GpxTrack, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|_| crate::i18n::tr("Plik nie jest poprawnym GeoJSON.").to_string())?;
    let mut name = None;
    let mut points = Vec::new();
    fn coords(c: &serde_json::Value, out: &mut Vec<(f64, f64, Option<f32>)>) {
        if let Some(a) = c.as_array() {
            for p in a {
                let p = p.as_array();
                if let Some(p) = p {
                    if let (Some(lon), Some(lat)) = (p.first().and_then(|x| x.as_f64()), p.get(1).and_then(|x| x.as_f64())) {
                        out.push((lon, lat, p.get(2).and_then(|x| x.as_f64()).map(|e| e as f32)));
                    }
                }
            }
        }
    }
    fn walk(v: &serde_json::Value, name: &mut Option<String>, out: &mut Vec<(f64, f64, Option<f32>)>) {
        if name.is_none() {
            if let Some(n) = v.get("properties").and_then(|p| p.get("name")).and_then(|n| n.as_str()) {
                *name = Some(n.to_string());
            }
        }
        match v.get("type").and_then(|t| t.as_str()) {
            Some("FeatureCollection") => {
                for f in v.get("features").and_then(|f| f.as_array()).into_iter().flatten() {
                    walk(f, name, out);
                }
            }
            Some("Feature") => {
                if let Some(g) = v.get("geometry") {
                    walk(g, name, out);
                }
            }
            Some("GeometryCollection") => {
                for g in v.get("geometries").and_then(|f| f.as_array()).into_iter().flatten() {
                    walk(g, name, out);
                }
            }
            Some("LineString") => {
                if let Some(c) = v.get("coordinates") {
                    coords(c, out);
                }
            }
            Some("MultiLineString") => {
                for l in v.get("coordinates").and_then(|c| c.as_array()).into_iter().flatten() {
                    coords(l, out);
                }
            }
            _ => {}
        }
    }
    walk(&v, &mut name, &mut points);
    points.retain(|p| p.0.is_finite() && p.1.is_finite());
    if points.len() < 2 {
        return Err(crate::i18n::tr("GeoJSON nie zawiera linii (LineString).").into());
    }
    Ok(GpxTrack { name: name.unwrap_or_else(|| crate::i18n::tr("Ślad GeoJSON").to_string()), points })
}

pub fn export_kml(route: &Route, name: &str, waypoints: &[crate::gpx::Waypoint]) -> String {
    let tr = &route.track;
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<kml xmlns=\"http://www.opengis.net/kml/2.2\">\n<Document>\n");
    out.push_str(&format!("  <name>{}</name>\n", esc(name)));
    out.push_str("  <Style id=\"trasa\"><LineStyle><color>ffff7a3c</color><width>4</width></LineStyle></Style>\n");
    for w in waypoints {
        out.push_str(&format!(
            "  <Placemark><name>{}</name><Point><coordinates>{:.6},{:.6},{:.1}</coordinates></Point></Placemark>\n",
            esc(&w.name),
            w.lon,
            w.lat,
            w.e
        ));
    }
    out.push_str(&format!(
        "  <Placemark><name>{}</name><styleUrl>#trasa</styleUrl><LineString><tessellate>1</tessellate><altitudeMode>clampToGround</altitudeMode><coordinates>\n",
        esc(name)
    ));
    for i in 0..tr.len() {
        out.push_str(&format!("{:.6},{:.6},{:.1}\n", tr.lon[i], tr.lat[i], tr.e[i]));
    }
    out.push_str("  </coordinates></LineString></Placemark>\n</Document>\n</kml>\n");
    out
}

pub fn export_geojson(route: &Route, name: &str, waypoints: &[crate::gpx::Waypoint]) -> String {
    let tr = &route.track;
    let line: Vec<serde_json::Value> = (0..tr.len())
        .map(|i| serde_json::json!([round6(tr.lon[i]), round6(tr.lat[i]), (tr.e[i] * 10.0).round() / 10.0]))
        .collect();
    let mut features = vec![serde_json::json!({
        "type": "Feature",
        "properties": {
            "name": name,
            "distance_m": route.stats.len2.round(),
            "ascent_m": route.stats.up.round(),
            "descent_m": route.stats.down.round(),
            "time_h": (route.stats.time * 100.0).round() / 100.0,
        },
        "geometry": { "type": "LineString", "coordinates": line },
    })];
    for w in waypoints {
        features.push(serde_json::json!({
            "type": "Feature",
            "properties": { "name": w.name },
            "geometry": { "type": "Point", "coordinates": [round6(w.lon), round6(w.lat), (w.e * 10.0).round() / 10.0] },
        }));
    }
    serde_json::to_string_pretty(&serde_json::json!({ "type": "FeatureCollection", "features": features })).unwrap_or_default()
}

fn round6(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{route_from_points, RouteKind};

    fn route() -> Route {
        let pts = vec![(20.0, 49.2, 1000.0), (20.01, 49.21, 1100.0), (20.02, 49.22, 1050.0)];
        route_from_points(&pts, RouteKind::Gpx, "Test", 1.0)
    }

    #[test]
    fn kml_roundtrip() {
        let x = export_kml(&route(), "Trasa & próba", &[]);
        let back = parse_any(&x, "kml").unwrap();
        assert_eq!(back.points.len(), 3);
        assert_eq!(back.name, "Trasa & próba");
        assert_eq!(back.points[1].2, Some(1100.0));
    }

    #[test]
    fn geojson_roundtrip() {
        let w = [crate::gpx::Waypoint { name: "A".into(), lon: 20.0, lat: 49.2, e: 1000.0 }];
        let x = export_geojson(&route(), "Trasa", &w);
        let back = parse_any(&x, "").unwrap();
        assert_eq!(back.points.len(), 3);
        assert_eq!(back.name, "Trasa");
        assert!((back.points[2].0 - 20.02).abs() < 1e-6);
    }

    #[test]
    fn gx_track() {
        let k = r#"<kml xmlns="http://www.opengis.net/kml/2.2" xmlns:gx="http://www.google.com/kml/ext/2.2"><Placemark><gx:Track><gx:coord>20 49 1000</gx:coord><gx:coord>20.1 49.1 1010</gx:coord></gx:Track></Placemark></kml>"#;
        let t = parse_kml(k).unwrap();
        assert_eq!(t.points.len(), 2);
    }
}
