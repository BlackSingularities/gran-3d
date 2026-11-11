//! Dojazd do początku trasy: najbliższe stacje kolejowe, przystanki i parkingi z OSM oraz odnośniki
//! do planerów podróży (rozkłady jazdy komunikacji publicznej) i do Mapy.com.

use crate::geo::haversine;
use crate::osm::PoiKind;
use crate::world::PoiView;

/// Najbliższe punkty dojazdu: (indeks w migawce, odległość m), po kilka z każdego rodzaju.
pub fn nearby(pois: &[PoiView], lon: f64, lat: f64) -> Vec<(usize, f64)> {
    let mut out = Vec::new();
    for (kind, radius, max) in [(PoiKind::Station, 8_000.0, 2), (PoiKind::BusStop, 2_500.0, 3), (PoiKind::Parking, 2_500.0, 3), (PoiKind::Cableway, 3_000.0, 1)] {
        let mut v: Vec<(usize, f64)> = pois
            .iter()
            .enumerate()
            .filter(|(_, p)| p.poi.kind == kind)
            .map(|(i, p)| (i, haversine(lon, lat, p.poi.lon, p.poi.lat)))
            .filter(|(_, d)| *d <= radius)
            .collect();
        v.sort_by(|a, b| a.1.total_cmp(&b.1));
        // te same nazwy przystanków (dwa kierunki) tylko raz
        let mut names: Vec<&str> = Vec::new();
        for (i, d) in v {
            let n = pois[i].poi.name.as_str();
            if names.contains(&n) {
                continue;
            }
            names.push(n);
            out.push((i, d));
            if names.len() >= max {
                break;
            }
        }
    }
    out
}

/// Planer podróży komunikacją publiczną do punktu (Google Maps, tryb transportu publicznego).
pub fn planner_url(lon: f64, lat: f64) -> String {
    format!("https://www.google.com/maps/dir/?api=1&destination={lat:.6},{lon:.6}&travelmode=transit")
}

/// Dojazd samochodem do punktu.
pub fn drive_url(lon: f64, lat: f64) -> String {
    format!("https://www.google.com/maps/dir/?api=1&destination={lat:.6},{lon:.6}&travelmode=driving")
}

pub fn open_planner(lon: f64, lat: f64) {
    let _ = open::that(planner_url(lon, lat));
}

/// Trasa w Mapy.com (planer pieszy, mapa turystyczna): start, punkty pośrednie (najwyżej 15) i koniec.
pub fn mapy_url(points: &[(f64, f64)]) -> Option<String> {
    let (first, last) = (points.first()?, points.last()?);
    if points.len() < 2 {
        return None;
    }
    let mid: Vec<String> = points[1..points.len() - 1].iter().take(15).map(|(lon, lat)| format!("{lon:.6},{lat:.6}")).collect();
    let mut url = format!(
        "https://mapy.com/fnc/v1/route?mapset=outdoor&start={:.6},{:.6}&end={:.6},{:.6}&routeType=foot_hiking",
        first.0, first.1, last.0, last.1
    );
    if !mid.is_empty() {
        url.push_str("&waypoints=");
        url.push_str(&mid.join(";"));
    }
    Some(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapy_link() {
        assert!(mapy_url(&[(19.9, 49.2)]).is_none());
        let u = mapy_url(&[(19.9, 49.2), (19.95, 49.23), (20.0, 49.25)]).unwrap();
        assert!(u.contains("start=19.900000,49.200000"));
        assert!(u.contains("end=20.000000,49.250000"));
        assert!(u.contains("waypoints=19.950000,49.230000"));
    }

    #[test]
    fn planner_link_has_coordinates() {
        assert!(planner_url(19.95, 49.25).contains("destination=49.250000,19.950000"));
    }
}
