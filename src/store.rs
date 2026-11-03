//! Magazyn danych map na dysku: jeden folder wybrany przez użytkownika.
//!
//! ```text
//! <folder>\GRAN-dane\
//!   tiles\dem\z\bx_by.gpk         teren (wypieczony: u16 + LZ4), paczki 16×16 kafli
//!   tiles\sat\z\bx_by.gpk         obraz satelitarny (JPEG), paczki 16×16 kafli
//!   tiles\ortho\z\bx_by.gpk       ortofotomapa (JPEG)
//!   sectors\, land\                szlaki, nazwy i pokrycie terenu z OSM
//! ```
//!
//! Wszystko zapisuje się samo w miarę oglądania. Po przekroczeniu limitu sprzątacz usuwa paczki
//! od najdawniej używanych; model globalny do poziomu [`BASE_MAX_Z`] zostaje.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Instant, SystemTime};

use crate::geo::TileId;
use crate::pack::{self, PackKey, Packs, Slot};

const NO_LIDAR_FILE: &str = "lidar-brak.txt";

/// Model globalny do tego poziomu nigdy nie jest usuwany (cała Europa to kilkadziesiąt MB).
pub const BASE_MAX_Z: u8 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Layer {
    /// Wysokości terenu (Terrarium, AWS Open Data: SRTM, EU-DEM, GMTED; LiDAR zaimportowany z wersji 1.x).
    Dem,
    /// Obraz satelitarny (Sentinel-2 cloudless 2020 © EOX, CC BY-NC-SA 4.0).
    Sat,
    /// Ortofotomapa z usług krajowych (GUGiK, ZBGIS, ČÚZK) na tle Sentinel-2, JPEG 512 px.
    Ortho,
    /// Teren LiDAR (NMT GUGiK, DMR 5G ČÚZK) – zapisywany w paczkach terenu (`Dem`).
    Lidar,
}

impl Layer {
    /// Warstwy z własnymi paczkami na dysku (LiDAR trafia do paczek terenu).
    pub const STORED: [Layer; 3] = [Layer::Dem, Layer::Sat, Layer::Ortho];

    pub fn id(self) -> u8 {
        match self {
            Layer::Dem | Layer::Lidar => 0,
            Layer::Sat => 1,
            Layer::Ortho => 2,
        }
    }

    pub fn dir(self) -> &'static str {
        match self {
            Layer::Dem | Layer::Lidar => "dem",
            Layer::Sat => "sat",
            Layer::Ortho => "ortho",
        }
    }

    pub fn url(self, t: TileId) -> String {
        match self {
            Layer::Dem => format!(
                "https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{}/{}/{}.png",
                t.z, t.x, t.y
            ),
            Layer::Sat => format!(
                "https://tiles.maps.eox.at/wmts/1.0.0/s2cloudless-2020_3857/default/g/{}/{}/{}.jpg",
                t.z, t.y, t.x
            ),
            // składane z kilku usług (`national`)
            Layer::Ortho | Layer::Lidar => String::new(),
        }
    }

    /// Najwyższy poziom dostępny w źródle.
    pub fn max_z(self) -> u8 {
        15
    }
}

/// Dostęp do danych: tani do klonowania (wspólne paczki i pamięć otwartych plików).
#[derive(Clone)]
pub struct Store {
    root: PathBuf,
    packs: Arc<Packs>,
    touched: Arc<Mutex<HashMap<PackKey, Instant>>>,
    no_lidar: Arc<RwLock<HashSet<TileId>>>,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let no_lidar = fs::read_to_string(root.join(NO_LIDAR_FILE))
            .map(|s| s.lines().filter_map(parse_tile).collect())
            .unwrap_or_default();
        Self {
            no_lidar: Arc::new(RwLock::new(no_lidar)),
            packs: Arc::new(Packs::new(root.join("tiles"))),
            root,
            touched: Arc::default(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Kafel jest na dysku albo wiadomo, że źródło go nie ma. Dla LiDAR-u: kafel terenu jest z LiDAR-u
    /// albo usługi nie mają dla niego danych.
    pub fn has(&self, layer: Layer, t: TileId) -> bool {
        match layer {
            Layer::Lidar => {
                self.no_lidar.read().unwrap().contains(&t)
                    || self.read(Layer::Dem, t).is_some_and(|b| crate::bake::is_lidar(&b))
            }
            _ => self.packs.state(layer.id(), layer.dir(), t) != Slot::Empty,
        }
    }

    /// Usługi nie mają LiDAR-u dla tego kafla – nie pytamy o niego ponownie. Nie ma tego w paczkach,
    /// bo znacznik braku zablokowałby w tym miejscu model globalny.
    pub fn note_no_lidar(&self, t: TileId) {
        if self.no_lidar.write().unwrap().insert(t) {
            let file = fs::OpenOptions::new().create(true).append(true).open(self.root.join(NO_LIDAR_FILE));
            if let Ok(mut f) = file {
                let _ = writeln!(f, "{} {} {}", t.z, t.x, t.y);
            }
        }
    }

    /// Wypieczone dane kafla.
    pub fn read(&self, layer: Layer, t: TileId) -> Option<Vec<u8>> {
        let b = self.packs.read(layer.id(), layer.dir(), t)?;
        self.touch(layer, t);
        Some(b)
    }

    /// Zapis wypieczonego kafla; `None` = źródło nie ma tego kafla.
    pub fn write(&self, layer: Layer, t: TileId, baked: Option<&[u8]>) -> std::io::Result<()> {
        self.packs.write(layer.id(), layer.dir(), t, baked)
    }

    /// Oznaczenie użycia paczki (czas modyfikacji = ostatnie użycie), najwyżej raz na minutę.
    fn touch(&self, layer: Layer, t: TileId) {
        let (key, _) = PackKey::of(layer.id(), t);
        let mut map = self.touched.lock().unwrap();
        if map.get(&key).is_some_and(|at| at.elapsed().as_secs() < 60) {
            return;
        }
        map.insert(key, Instant::now());
        if let Ok(f) = fs::File::options()
            .write(true)
            .open(self.packs.path(&key, layer.dir()))
        {
            let _ = f.set_modified(SystemTime::now());
        }
    }

    pub fn usage(&self) -> u64 {
        dir_size(&self.root)
    }

    /// Sprzątanie do limitu: usuwa najdawniej używane paczki (model globalny zostaje).
    /// Zwraca (liczba usuniętych paczek, zwolnione bajty).
    pub fn collect_garbage(&self, limit: u64) -> (usize, u64) {
        let mut total = self.usage();
        if total <= limit {
            return (0, 0);
        }
        let mut free: Vec<(SystemTime, u64, PackKey, PathBuf)> = Vec::new();
        for layer in Layer::STORED {
            for (key, path) in pack::list(&self.root.join("tiles"), layer.id(), layer.dir()) {
                if key.z <= BASE_MAX_Z {
                    continue;
                }
                if let Ok(m) = fs::metadata(&path) {
                    free.push((
                        m.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                        m.len(),
                        key,
                        path,
                    ));
                }
            }
        }
        free.sort_by_key(|f| f.0);
        let (mut n, mut freed) = (0, 0);
        for (_, size, key, path) in free {
            if total <= limit * 9 / 10 {
                break;
            }
            self.packs.forget(&key);
            if fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(size);
                freed += size;
                n += 1;
            }
        }
        (n, freed)
    }
}

fn parse_tile(line: &str) -> Option<TileId> {
    let mut p = line.split_whitespace();
    Some(TileId::new(p.next()?.parse().ok()?, p.next()?.parse().ok()?, p.next()?.parse().ok()?))
}

pub fn dir_size(dir: &Path) -> u64 {
    let Ok(rd) = fs::read_dir(dir) else { return 0 };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            _ => e.metadata().map(|m| m.len()).unwrap_or(0),
        })
        .sum()
}

/// Wolne miejsce na dysku, na którym leży `dir` (pierwszy istniejący katalog nadrzędny).
pub fn free_space(dir: &Path) -> Option<u64> {
    let mut d = dir;
    while !d.exists() {
        d = d.parent()?;
    }
    free_space_impl(d)
}

#[cfg(windows)]
fn free_space_impl(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut avail = 0u64;
    // SAFETY: ścieżka zakończona zerem, wskaźniki na lokalne zmienne lub null
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut avail,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(avail)
}

#[cfg(not(windows))]
fn free_space_impl(_dir: &Path) -> Option<u64> {
    None
}

/// „12,4 GB”, „380 MB”.
pub fn fmt_bytes(b: u64) -> String {
    const G: f64 = 1024.0 * 1024.0 * 1024.0;
    let f = b as f64;
    if f >= 1024.0 * G {
        format!("{:.1} TB", f / 1024.0 / G).replace('.', ",")
    } else if f >= 10.0 * G {
        format!("{:.0} GB", f / G)
    } else if f >= G {
        format!("{:.1} GB", f / G).replace('.', ",")
    } else {
        format!("{:.0} MB", f / 1024.0 / 1024.0)
    }
}
