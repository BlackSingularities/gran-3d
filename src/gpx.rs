//! Eksport trasy do GPX 1.1 i wczytywanie śladów GPX (trkpt / rtept).

use crate::graph::Route;

fn esc(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '<' | '>' | '&' | '"' | '\'' => format!("&#{};", c as u32),
            c => c.to_string(),
        })
        .collect()
}

fn iso(unix: f64) -> String {
    let secs = unix.floor() as i64;
    let d = crate::sun::civil_from_days(secs.div_euclid(86_400));
    let s = secs.rem_euclid(86_400);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        d.y,
        d.m,
        d.d,
        s / 3600,
        (s % 3600) / 60,
        s % 60
    )
}

pub struct Waypoint {
    pub name: String,
    pub lon: f64,
    pub lat: f64,
    pub e: f32,
}

pub fn export(route: &Route, name: &str, waypoints: &[Waypoint]) -> String {
    let tr = &route.track;
    let start = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<gpx version=\"1.1\" creator=\"GRAŃ — atlas szlaków 3D\" xmlns=\"http://www.topografix.com/GPX/1/1\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:schemaLocation=\"http://www.topografix.com/GPX/1/1 http://www.topografix.com/GPX/1/1/gpx.xsd\">\n");
    out.push_str(&format!(
        "  <metadata><name>{}</name><time>{}</time></metadata>\n",
        esc(name),
        iso(start)
    ));
    for w in waypoints {
        out.push_str(&format!(
            "  <wpt lat=\"{:.6}\" lon=\"{:.6}\"><ele>{:.1}</ele><name>{}</name></wpt>\n",
            w.lat,
            w.lon,
            w.e,
            esc(&w.name)
        ));
    }
    out.push_str(&format!(
        "  <trk><name>{}</name><type>hiking</type><trkseg>\n",
        esc(name)
    ));
    for i in 0..tr.len() {
        out.push_str(&format!(
            "    <trkpt lat=\"{:.6}\" lon=\"{:.6}\"><ele>{:.1}</ele><time>{}</time></trkpt>\n",
            tr.lat[i],
            tr.lon[i],
            tr.e[i],
            iso(start + tr.t[i] as f64 * 3600.0)
        ));
    }
    out.push_str("  </trkseg></trk>\n</gpx>\n");
    out
}

pub struct GpxTrack {
    pub name: String,
    /// (dł., szer., wysokość z pliku)
    pub points: Vec<(f64, f64, Option<f32>)>,
}

pub fn parse(text: &str) -> Result<GpxTrack, String> {
    let doc = roxmltree::Document::parse(text).map_err(|_| "Plik nie jest poprawnym GPX.")?;
    let name_of = |parent: &str| {
        doc.descendants()
            .find(|n| n.has_tag_name("name") && n.parent().is_some_and(|p| p.has_tag_name(parent)))
            .and_then(|n| n.text())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let name = name_of("trk")
        .or_else(|| name_of("rte"))
        .or_else(|| name_of("metadata"))
        .unwrap_or_else(|| "Ślad GPX".into());
    let collect = |tag: &str| -> Vec<(f64, f64, Option<f32>)> {
        doc.descendants()
            .filter(|n| n.has_tag_name(tag))
            .filter_map(|n| {
                let lat = n.attribute("lat")?.parse::<f64>().ok()?;
                let lon = n.attribute("lon")?.parse::<f64>().ok()?;
                let e = n
                    .children()
                    .find(|c| c.has_tag_name("ele"))
                    .and_then(|c| c.text())
                    .and_then(|t| t.trim().parse::<f32>().ok());
                (lat.is_finite() && lon.is_finite()).then_some((lon, lat, e))
            })
            .collect()
    };
    let mut points = collect("trkpt");
    if points.is_empty() {
        points = collect("rtept");
    }
    if points.len() < 2 {
        return Err("GPX nie zawiera śladu (trkpt/rtept).".into());
    }
    Ok(GpxTrack { name, points })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{route_from_points, RouteKind};

    #[test]
    fn roundtrip() {
        let pts = vec![(20.0, 49.2, 1000.0), (20.01, 49.21, 1100.0), (20.02, 49.22, 1050.0)];
        let r = route_from_points(&pts, RouteKind::Gpx, "Test", 1.0);
        let x = export(&r, "Trasa <próba>", &[]);
        let back = parse(&x).unwrap();
        assert_eq!(back.points.len(), 3);
        assert_eq!(back.name, "Trasa <próba>");
        assert!((back.points[1].0 - 20.01).abs() < 1e-6);
        assert_eq!(back.points[1].2, Some(1100.0));
    }
}
