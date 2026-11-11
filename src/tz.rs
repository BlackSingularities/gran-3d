//! Czas lokalny miejsca na mapie: strefa czasowa z położenia (granice stref z tzf-rs, bez internetu)
//! i przesunięcie od UTC z czasem letnim (baza IANA z chrono-tz). Na morzu i poza strefami –
//! czas słoneczny zaokrąglony do pełnej godziny.

use std::sync::OnceLock;

use chrono::{Offset, TimeZone};
use chrono_tz::Tz;

use crate::sun::{civil_from_days, days_from_civil, Day};

fn finder() -> &'static tzf_rs::DefaultFinder {
    static F: OnceLock<tzf_rs::DefaultFinder> = OnceLock::new();
    F.get_or_init(tzf_rs::DefaultFinder::new)
}

/// Wczytuje granice stref w tle, żeby pierwsze zapytanie nie wstrzymało interfejsu.
pub fn warm_up() {
    std::thread::spawn(|| {
        finder();
    });
}

/// Strefa czasowa w punkcie (`None` – brak strefy, np. na otwartym morzu).
pub fn zone(lon: f64, lat: f64) -> Option<Tz> {
    let name = finder().get_tz_name(lon, lat);
    if name.is_empty() {
        return None;
    }
    name.parse().ok()
}

/// Przesunięcie czasu lokalnego od UTC w minutach w chwili `unix` (sekundy UTC).
pub fn offset_min(lon: f64, lat: f64, unix: f64) -> i32 {
    let solar = || ((lon / 15.0).round() * 60.0) as i32;
    let Some(tz) = zone(lon, lat) else { return solar() };
    let Some(dt) = chrono::DateTime::from_timestamp(unix.floor() as i64, 0) else { return solar() };
    tz.offset_from_utc_datetime(&dt.naive_utc()).fix().local_minus_utc() / 60
}

/// Sekundy UTC odpowiadające godzinie `hour` (czasu lokalnego) dnia `day` w punkcie.
pub fn local_unix(day: Day, hour: f64, lon: f64, lat: f64) -> f64 {
    let base = days_from_civil(day.y, day.m, day.d) as f64 * 86_400.0;
    // przesunięcie z okolic tej godziny (zmiana czasu w nocy nie przestawia całego dnia)
    let guess = base + hour * 3600.0 - lon / 15.0 * 3600.0;
    let off = offset_min(lon, lat, guess) as f64;
    base + (hour * 60.0 - off) * 60.0
}

/// Lokalny dzień i godzina (ułamkowa) w punkcie w chwili `unix`.
pub fn local_at(unix: f64, lon: f64, lat: f64) -> (Day, f64) {
    let local = unix + offset_min(lon, lat, unix) as f64 * 60.0;
    let days = (local / 86_400.0).floor() as i64;
    (civil_from_days(days), (local - days as f64 * 86_400.0) / 3600.0)
}

pub fn now_unix() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// Teraz w punkcie: lokalny dzień i godzina.
pub fn now_at(lon: f64, lat: f64) -> (Day, f64) {
    local_at(now_unix(), lon, lat)
}

/// Opis strefy, np. „Europe/Warsaw · UTC+1”, „Asia/Kathmandu · UTC+5:45”.
pub fn label(lon: f64, lat: f64, unix: f64) -> String {
    let off = offset_min(lon, lat, unix);
    let sign = if off < 0 { '−' } else { '+' };
    let (h, m) = (off.abs() / 60, off.abs() % 60);
    let utc = if m == 0 { format!("UTC{sign}{h}") } else { format!("UTC{sign}{h}:{m:02}") };
    match zone(lon, lat) {
        Some(z) => format!("{} · {utc}", z.name()),
        None => utc,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unix(y: i32, m: u32, d: u32, h: f64) -> f64 {
        days_from_civil(y, m, d) as f64 * 86_400.0 + h * 3600.0
    }

    #[test]
    fn zones_around_the_world() {
        assert_eq!(zone(19.95, 49.23).map(|z| z.name()), Some("Europe/Warsaw"));
        assert_eq!(zone(86.92, 27.99).map(|z| z.name()), Some("Asia/Kathmandu"));
        assert_eq!(zone(-105.27, 40.01).map(|z| z.name()), Some("America/Denver"));
    }

    #[test]
    fn offsets_follow_daylight_saving() {
        // Zakopane: zima UTC+1, lato UTC+2
        assert_eq!(offset_min(19.95, 49.23, unix(2026, 1, 15, 12.0)), 60);
        assert_eq!(offset_min(19.95, 49.23, unix(2026, 7, 15, 12.0)), 120);
        // Nepal bez czasu letniego, UTC+5:45
        assert_eq!(offset_min(86.92, 27.99, unix(2026, 7, 15, 12.0)), 345);
        // Kolorado latem UTC−6
        assert_eq!(offset_min(-105.27, 40.01, unix(2026, 7, 15, 12.0)), -360);
    }

    #[test]
    fn local_time_round_trip() {
        let d = Day { y: 2026, m: 7, d: 15 };
        let u = local_unix(d, 14.5, -105.27, 40.01);
        let (d2, h) = local_at(u, -105.27, 40.01);
        assert_eq!(d2, d);
        assert!((h - 14.5).abs() < 1e-6);
    }

    #[test]
    fn open_sea_uses_solar_time() {
        // środek Pacyfiku: strefa Etc albo czas słoneczny – w obu przypadkach pełne godziny
        assert_eq!(offset_min(-150.0, 0.0, unix(2026, 7, 15, 12.0)) % 60, 0);
    }
}
