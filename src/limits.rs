//! Limit zużycia zasobów (pamięć RAM, pamięć GPU, wątki pobierania) wybierany w ustawieniach.

use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

static HEIGHT_TILES: AtomicUsize = AtomicUsize::new(700);
static SAT_CACHE: AtomicUsize = AtomicUsize::new(256);
static SECTORS: AtomicUsize = AtomicUsize::new(25);
static GPU_TILES: AtomicUsize = AtomicUsize::new(usize::MAX);
static WORKERS: AtomicUsize = AtomicUsize::new(16);

/// Poziomy: nazwa, opis, przybliżone zużycie pamięci.
pub const LEVELS: [(&str, &str); 4] = [
    ("Oszczędny", "ok. 0,6 GB RAM · mniej kafli w pamięci, wolniejsze doczytywanie"),
    ("Zrównoważony", "ok. 1,2 GB RAM"),
    ("Wysoki", "ok. 2 GB RAM · domyślny"),
    ("Maksymalny", "ok. 4 GB RAM · najwięcej danych w pamięci, najszybsze doczytywanie"),
];

pub const DEFAULT_LEVEL: u8 = 2;

pub fn apply(level: u8) {
    let (h, s, sec, gpu, w) = match level {
        0 => (200, 64, 10, 300, 6),
        1 => (420, 160, 16, 700, 10),
        3 => (1400, 512, 40, usize::MAX, 24),
        _ => (700, 256, 25, usize::MAX, 16),
    };
    HEIGHT_TILES.store(h, Relaxed);
    SAT_CACHE.store(s, Relaxed);
    SECTORS.store(sec, Relaxed);
    GPU_TILES.store(gpu, Relaxed);
    WORKERS.store(w, Relaxed);
}

/// Kafle wysokości w pamięci RAM.
pub fn height_tiles() -> usize {
    HEIGHT_TILES.load(Relaxed)
}
/// Zdjęcia satelitarne trzymane w pamięci.
pub fn sat_cache() -> usize {
    SAT_CACHE.load(Relaxed)
}
/// Wczytane sektory OSM.
pub fn sectors() -> usize {
    SECTORS.load(Relaxed)
}
/// Górna granica kafli w pamięci GPU (teren i zdjęcia osobno).
pub fn gpu_tiles() -> usize {
    GPU_TILES.load(Relaxed)
}
/// Wątki pobierania kafli (zmiana po ponownym uruchomieniu).
pub fn workers() -> usize {
    WORKERS.load(Relaxed)
}
