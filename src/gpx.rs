//! Eksport trasy do GPX 1.1 i wczytywanie śladów GPX (trkpt / rtept).

use crate::graph::Route;

pub fn esc(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '<' | '>' | '&' | '"' | '\'' => format!("&#{};", c as u32),
            c => c.to_string(),
        })
        .collect()
}

pub fn iso(unix: f64) -> String {
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

/// Czas ISO 8601 („2024-07-01T08:15:30Z”, z ułamkiem sekund albo strefą „+02:00”) → sekundy uniksowe.
pub fn parse_iso(s: &str) -> Option<f64> {
    let s = s.trim();
    let num = |a: usize, b: usize| s.get(a..b).and_then(|x| x.parse::<i64>().ok());
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    if s.as_bytes().get(10).is_some_and(|c| *c != b'T' && *c != b' ') {
        return None;
    }
    let (h, mi, sec) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    let mut rest = &s[19.min(s.len())..];
    let mut frac = 0.0;
    if let Some(r) = rest.strip_prefix('.') {
        let n = r.bytes().take_while(|c| c.is_ascii_digit()).count();
        frac = format!("0.{}", &r[..n]).parse::<f64>().unwrap_or(0.0);
        rest = &r[n..];
    }
    let offset = match rest.as_bytes().first() {
        Some(b'+') | Some(b'-') => {
            let sign = if rest.starts_with('-') { -1 } else { 1 };
            let oh = rest.get(1..3)?.parse::<i64>().ok()?;
            let om = rest.get(rest.len().saturating_sub(2)..).and_then(|m| m.parse::<i64>().ok()).unwrap_or(0);
            sign * (oh * 3600 + if rest.len() > 3 { om * 60 } else { 0 })
        }
        _ => 0,
    };
    let days = crate::sun::days_from_civil(y as i32, mo as u32, d as u32);
    Some((days * 86_400 + h * 3600 + mi * 60 + sec - offset) as f64 + frac)
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
    /// Czas punktów (sekundy uniksowe), gdy plik go ma dla wszystkich punktów – nagrane przejście.
    pub times: Vec<f64>,
}

pub fn parse(text: &str) -> Result<GpxTrack, String> {
    let doc = roxmltree::Document::parse(text).map_err(|_| crate::i18n::tr("Plik nie jest poprawnym GPX."))?;
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
        .unwrap_or_else(|| crate::i18n::tr("Ślad GPX").into());
    let collect = |tag: &str| -> Vec<(f64, f64, Option<f32>, Option<f64>)> {
        doc.descendants()
            .filter(|n| n.has_tag_name(tag))
            .filter_map(|n| {
                let lat = n.attribute("lat")?.parse::<f64>().ok()?;
                let lon = n.attribute("lon")?.parse::<f64>().ok()?;
                let child = |name: &str| n.children().find(|c| c.has_tag_name(name)).and_then(|c| c.text());
                let e = child("ele").and_then(|t| t.trim().parse::<f32>().ok());
                let t = child("time").and_then(parse_iso);
                (lat.is_finite() && lon.is_finite()).then_some((lon, lat, e, t))
            })
            .collect()
    };
    let mut raw = collect("trkpt");
    if raw.is_empty() {
        raw = collect("rtept");
    }
    if raw.len() < 2 {
        return Err(crate::i18n::tr("GPX nie zawiera śladu (trkpt/rtept).").into());
    }
    let times: Vec<f64> = if raw.iter().all(|p| p.3.is_some()) { raw.iter().filter_map(|p| p.3).collect() } else { Vec::new() };
    let points = raw.into_iter().map(|(lon, lat, e, _)| (lon, lat, e)).collect();
    Ok(GpxTrack { name, points, times })
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
        assert_eq!(back.times.len(), 3, "eksport trasy ma czasy planowane");
    }

    #[test]
    fn iso_times() {
        assert_eq!(parse_iso("1970-01-01T00:00:00Z"), Some(0.0));
        assert_eq!(parse_iso("2024-07-01T10:00:00+02:00"), parse_iso("2024-07-01T08:00:00Z"));
        assert_eq!(parse_iso("2024-07-01T08:00:00.5Z").map(|t| t.fract()), Some(0.5));
        assert_eq!(parse_iso(&iso(1_719_820_800.0)), Some(1_719_820_800.0));
        assert!(parse_iso("wczoraj").is_none());
    }
}
