//! Różnice między systemami w jednym miejscu: Windows (okna dialogowe, przeglądarka, Eksplorator)
//! i Android (wywołania `GranActivity` przez JNI). Reszta programu jest wspólna.
//!
//! Na Androidzie wybór i zapis plików są asynchroniczne (okno systemu zwraca wynik później),
//! więc ich wyniki przychodzą jako [`Event`] odbierane co klatkę przez [`drain`].

use std::path::PathBuf;
use std::sync::Mutex;

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub use android::*;

#[cfg(not(target_os = "android"))]
mod desktop;
#[cfg(not(target_os = "android"))]
pub use desktop::*;

/// Telefon albo tablet: dotyk, brak klawiatury fizycznej, przyciski systemu.
pub const MOBILE: bool = cfg!(target_os = "android");

/// Nazwa platformy w zgłoszeniach i komentarzach wysyłanych do redzikowski.com.
pub const NAME: &str = if MOBILE { "android" } else if cfg!(windows) { "windows" } else { "linux" };

/// Zdarzenie od systemu, odbierane w wątku interfejsu.
#[derive(Debug, Clone)]
pub enum Event {
    /// Wybrany plik śladu (kopia w folderze tymczasowym) – do wczytania.
    Opened(PathBuf),
    /// Komunikat dla użytkownika (zapisano plik, odmowa uprawnień…).
    Message(String),
}

static EVENTS: Mutex<Vec<Event>> = Mutex::new(Vec::new());

pub fn push(e: Event) {
    EVENTS.lock().unwrap().push(e);
}

/// Zdarzenia od ostatniej klatki.
pub fn drain() -> Vec<Event> {
    std::mem::take(&mut *EVENTS.lock().unwrap())
}

/// Miejsce na dane do wyboru w ustawieniach (Android: pamięć wewnętrzna, karta SD).
#[derive(Debug, Clone)]
pub struct Storage {
    pub label: String,
    pub path: PathBuf,
}

/// Typ MIME pliku po rozszerzeniu (zapis przez okno systemu).
pub fn mime(ext: &str) -> &'static str {
    match ext {
        "gpx" => "application/gpx+xml",
        "kml" => "application/vnd.google-earth.kml+xml",
        "geojson" | "json" => "application/geo+json",
        "fit" => "application/vnd.ant.fit",
        "png" => "image/png",
        "log" | "txt" => "text/plain",
        _ => "application/octet-stream",
    }
}
