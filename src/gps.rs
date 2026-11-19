//! Położenie z odbiornika GPS (Android, iOS): odczyty systemu, zgoda na lokalizację i dziennik
//! nagrywania.
//!
//! Odczyty przychodzą z kodu systemu (Android: `GranLocation` w Javie, iOS: delegat CoreLocation)
//! jako wiersze tekstu – ten sam format w kolejce do programu i w dzienniku nagrywania na dysku:
//! `czas,szer,dł,wysokość,dokładność,dokł_pionowa,prędkość,kierunek,źródło` (puste pole – brak).
//!
//! Dziennik pisze strona systemowa przy każdym odczycie, także gdy program jest w tle albo ekran
//! jest wyłączony; po zamknięciu programu w trakcie nagrywania przejście odtwarza się z dziennika.

use std::path::PathBuf;

/// Odczyt położenia.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fix {
    /// Czas odczytu (sekundy uniksowe UTC).
    pub t: f64,
    pub lat: f64,
    pub lon: f64,
    /// Wysokość z odbiornika (m) – na Androidzie nad elipsoidą, na iOS nad poziomem morza.
    pub alt: Option<f32>,
    /// Promień dokładności poziomej (m).
    pub acc: f32,
    pub vacc: Option<f32>,
    /// Prędkość (m/s) i kierunek ruchu (stopnie od północy) z odbiornika.
    pub speed: Option<f32>,
    pub course: Option<f32>,
    /// Odczyt z satelitów (a nie z sieci komórkowej / Wi-Fi) – tylko takie trafiają do nagrania.
    pub gnss: bool,
}

fn opt(v: Option<f32>) -> String {
    v.filter(|x| x.is_finite()).map(|x| format!("{x:.2}")).unwrap_or_default()
}

impl Fix {
    /// Wiersz dziennika.
    pub fn to_line(&self) -> String {
        format!(
            "{:.3},{:.7},{:.7},{},{:.1},{},{},{},{}",
            self.t,
            self.lat,
            self.lon,
            opt(self.alt),
            self.acc,
            opt(self.vacc),
            opt(self.speed),
            opt(self.course),
            if self.gnss { "g" } else { "n" }
        )
    }

    /// Odczyt z wiersza dziennika albo kolejki; `None` dla uszkodzonych wierszy.
    pub fn parse(line: &str) -> Option<Fix> {
        let f: Vec<&str> = line.trim().split(',').collect();
        if f.len() < 5 {
            return None;
        }
        let num = |i: usize| f.get(i).and_then(|s| s.trim().parse::<f64>().ok()).filter(|v| v.is_finite());
        let small = |i: usize| num(i).map(|v| v as f32);
        let (t, lat, lon) = (num(0)?, num(1)?, num(2)?);
        if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) || (lat == 0.0 && lon == 0.0) {
            return None;
        }
        Some(Fix {
            t,
            lat,
            lon,
            alt: small(3),
            acc: small(4).unwrap_or(99.0).max(0.0),
            vacc: small(5),
            speed: small(6).filter(|s| *s >= 0.0),
            course: small(7).filter(|c| *c >= 0.0),
            gnss: f.get(8).map_or(true, |s| s.trim() != "n"),
        })
    }
}

/// Zgoda systemu na lokalizację.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    /// Jeszcze nie pytano.
    Unknown,
    Granted,
    /// Odmówiono – zmienić można tylko w ustawieniach systemu.
    Denied,
}

/// Folder nagrań: zapisane przejścia, stan i dziennik bieżącego nagrania.
pub fn dir() -> PathBuf {
    crate::settings::config_dir().join("nagrania")
}

/// Dziennik odczytów bieżącego nagrania (pisany przez stronę systemową).
pub fn journal_path() -> PathBuf {
    dir().join("biezace.csv")
}

/// Dopisanie odczytu do dziennika (iOS – delegat CoreLocation; Android pisze dziennik w Javie).
#[allow(dead_code)]
pub fn journal_append(path: &std::path::Path, f: &Fix) {
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{}", f.to_line());
    }
}

/// Wszystkie odczyty z dziennika (po kolei, bez uszkodzonych wierszy).
pub fn read_journal(path: &std::path::Path) -> Vec<Fix> {
    std::fs::read_to_string(path).map(|t| t.lines().filter_map(Fix::parse).collect()).unwrap_or_default()
}

/// Bieżący czas uniksowy (s).
pub fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_roundtrip() {
        let f = Fix { t: 1_700_000_000.5, lat: 49.2319, lon: 19.9817, alt: Some(1987.4), acc: 4.5, vacc: None, speed: Some(1.2), course: None, gnss: true };
        let back = Fix::parse(&f.to_line()).unwrap();
        assert!((back.lat - f.lat).abs() < 1e-6 && (back.lon - f.lon).abs() < 1e-6);
        assert_eq!(back.alt.map(|a| a.round()), Some(1987.0));
        assert_eq!(back.vacc, None);
        assert!(back.gnss);
        assert!(Fix::parse("bzdura").is_none());
        assert!(Fix::parse("1,0,0,,5").is_none(), "punkt 0,0 to brak odczytu");
        let n = Fix::parse("1,49,19,,30,,,,n").unwrap();
        assert!(!n.gnss);
    }
}
