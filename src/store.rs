//! Magazyn danych map na dysku: jeden folder wybrany przez użytkownika.
//!
//! ```text
//! <folder>\GRAN-dane\
//!   areas.json                    obszary zapisane do pracy bez internetu
//!   tiles\dem\z\bx_by.gpk         teren (wypieczony: u16 + LZ4), paczki 16×16 kafli
//!   tiles\sat\z\bx_by.gpk         obraz satelitarny (JPEG), paczki 16×16 kafli
//! ```
//!
//! Paczki obszarów zapisanych przez użytkownika i model globalny do poziomu [`BASE_MAX_Z`] są stałe;
//! resztę (dociągane w miarę oglądania mapy) sprzątacz usuwa od najdawniej używanych, gdy dane
//! przekraczają limit.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

use rayon::prelude::*;

use crate::areas::Area;
use crate::geo::TileId;
use crate::library::Coverage;
use crate::pack::{self, PackKey, Packs, Slot};

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
    pub const ALL: [Layer; 2] = [Layer::Dem, Layer::Sat];
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

    /// Średni rozmiar wypieczonego kafla w górach (do szacowania pobierania), w bajtach.
    pub fn avg_bytes(self, z: u8) -> u64 {
        match self {
            Layer::Dem if z <= 8 => 45_000,
            Layer::Dem => 70_000,
            Layer::Sat if z <= 9 => 18_000,
            Layer::Sat => 26_000,
            Layer::Ortho => 60_000,
            Layer::Lidar => 110_000,
        }
    }
}

/// Dostęp do danych: tani do klonowania (wspólne paczki i pamięć otwartych plików).
#[derive(Clone)]
pub struct Store {
    root: PathBuf,
    packs: Arc<Packs>,
    touched: Arc<Mutex<HashMap<PackKey, Instant>>>,
    coverage: Arc<Coverage>,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            packs: Arc::new(Packs::new(root.join("tiles"))),
            coverage: Arc::new(Coverage::new(root.join("lidar-brak.txt"))),
            root,
            touched: Arc::default(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Kafel jest na dysku albo wiadomo, że źródło go nie ma. Dla LiDAR-u: kafel terenu jest z LiDAR-u.
    pub fn has(&self, layer: Layer, t: TileId) -> bool {
        if let Some(known) = self.coverage.has(layer, t) {
            return known;
        }
        match layer {
            Layer::Lidar => self.read(Layer::Dem, t).is_some_and(|b| crate::bake::is_lidar(&b)),
            _ => self.packs.state(layer.id(), layer.dir(), t) != Slot::Empty,
        }
    }

    /// Wczytanie pokrycia z indeksów wszystkich paczek (równolegle, po pliku).
    pub fn scan(&self) {
        let tiles = self.root.join("tiles");
        for layer in Layer::STORED {
            pack::list(&tiles, layer.id(), layer.dir()).into_par_iter().for_each(|(key, path)| {
                // tylko tam może leżeć LiDAR, a odczyt początku każdego kafla kosztuje
                let with_head = layer == Layer::Dem && key.z >= 14;
                match pack::scan(&path, with_head) {
                    Ok(slots) => self.coverage.merge(key, &slots),
                    Err(e) => log::warn!("paczka {}: {e}", path.display()),
                }
            });
        }
        self.coverage.mark_ready();
    }

    /// [`Store::scan`] w tle; do końca skanowania `has` pyta dysk.
    pub fn start_scan(&self) {
        let store = self.clone();
        std::thread::spawn(move || store.scan());
    }

    /// Usługi nie mają LiDAR-u dla tego kafla – nie pytamy o niego ponownie.
    pub fn note_no_lidar(&self, t: TileId) {
        self.coverage.note_no_lidar(t);
    }

    #[cfg(test)]
    pub fn release_all(&self) {
        self.packs.forget_all();
    }

    /// Wypieczone dane kafla.
    pub fn read(&self, layer: Layer, t: TileId) -> Option<Vec<u8>> {
        let b = self.packs.read(layer.id(), layer.dir(), t)?;
        self.touch(layer, t);
        Some(b)
    }

    /// Zapis wypieczonego kafla; `None` = źródło nie ma tego kafla.
    pub fn write(&self, layer: Layer, t: TileId, baked: Option<&[u8]>) -> std::io::Result<()> {
        self.packs.write(layer.id(), layer.dir(), t, baked)?;
        self.coverage.note(layer, t, baked.is_some());
        Ok(())
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

    /// Sprzątanie do limitu: usuwa najdawniej używane paczki, których nie chroni żaden zapisany obszar.
    /// Zwraca (liczba usuniętych paczek, zwolnione bajty).
    pub fn collect_garbage(&self, limit: u64, areas: &[Area]) -> (usize, u64) {
        let mut total = self.usage();
        if total <= limit {
            return (0, 0);
        }
        let pinned = pinned_packs(areas);
        let mut free: Vec<(SystemTime, u64, PackKey, PathBuf)> = Vec::new();
        for layer in Layer::STORED {
            for (key, path) in pack::list(&self.root.join("tiles"), layer.id(), layer.dir()) {
                if key.z <= BASE_MAX_Z || pinned.contains(&key) {
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
                self.coverage.remove_pack(&key);
                total = total.saturating_sub(size);
                freed += size;
                n += 1;
            }
        }
        (n, freed)
    }

    /// Usunięcie paczek obszaru, których nie potrzebuje żaden inny zapisany obszar.
    pub fn remove_area_tiles(&self, area: &Area, others: &[Area]) {
        let keep = pinned_packs(others);
        for key in pinned_packs(std::slice::from_ref(area)) {
            if key.z > BASE_MAX_Z && !keep.contains(&key) {
                let dir = Layer::STORED
                    .into_iter()
                    .find(|l| l.id() == key.layer)
                    .unwrap_or(Layer::Dem)
                    .dir();
                self.packs.forget(&key);
                if fs::remove_file(self.packs.path(&key, dir)).is_ok() {
                    self.coverage.remove_pack(&key);
                }
            }
        }
    }
}

/// Paczki chronione przez obszary (każda paczka, w której leży choć jeden kafel obszaru).
fn pinned_packs(areas: &[Area]) -> HashSet<PackKey> {
    areas
        .iter()
        .flat_map(|a| a.tiles())
        .map(|(l, t)| PackKey::of(l.id(), t).0)
        .collect()
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

/// Co leży we wskazanym folderze: dane tej wersji, kafle wersji 1.x do zaimportowania.
#[derive(Clone, Debug, Default)]
pub struct Found {
    pub areas: usize,
    pub packs: bool,
    /// Katalog `.cache\dem` wersji 1.x (kafle terenu, także LiDAR) i liczba plików.
    pub legacy_dem: Option<(PathBuf, usize)>,
}

impl Found {
    /// Opis dla użytkownika: „3 obszary, kafle wersji 1.x do importu: 1520”.
    pub fn describe(&self) -> Option<String> {
        let mut parts = Vec::new();
        if self.areas > 0 {
            parts.push(format!("zapisane obszary: {}", self.areas));
        } else if self.packs {
            parts.push("kafle map tej wersji".to_string());
        }
        if let Some((_, n)) = &self.legacy_dem {
            parts.push(format!(
                "kafle terenu wersji 1.x (także LiDAR): {n} – zostaną zaimportowane"
            ));
        }
        (!parts.is_empty()).then(|| format!("Znaleziono dane: {}.", parts.join(", ")))
    }
}

pub fn detect(dir: &Path) -> Found {
    let areas = crate::areas::load(dir).len();
    let packs = dir.join("tiles").is_dir();
    let legacy = [
        dir.join(".cache").join("dem"),
        dir.parent()
            .map(|p| p.join(".cache").join("dem"))
            .unwrap_or_default(),
    ]
    .into_iter()
    .find(|d| d.is_dir() && !d.join(IMPORTED_MARK).exists())
    .map(|d| {
        let n = legacy_tiles(&d).len();
        (d, n)
    })
    .filter(|(_, n)| *n > 0);
    Found {
        areas,
        packs,
        legacy_dem: legacy,
    }
}

/// Znacznik zakończonego importu w katalogu kafli wersji 1.x.
pub const IMPORTED_MARK: &str = ".gran2-zaimportowano";

/// Kafle Terrarium wersji 1.x: `.cache\dem\z\x\y.png`.
pub fn legacy_tiles(dir: &Path) -> Vec<(TileId, PathBuf)> {
    let num = |p: &Path| {
        p.file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse::<u32>().ok())
    };
    let mut out = Vec::new();
    let Ok(zs) = fs::read_dir(dir) else {
        return out;
    };
    for z in zs.flatten() {
        let Some(zv) = num(&z.path()).filter(|z| *z <= 16) else {
            continue;
        };
        let Ok(xs) = fs::read_dir(z.path()) else {
            continue;
        };
        for x in xs.flatten() {
            let Some(xv) = num(&x.path()) else { continue };
            let Ok(ys) = fs::read_dir(x.path()) else {
                continue;
            };
            for y in ys.flatten() {
                let p = y.path();
                if p.extension().is_some_and(|e| e == "png") {
                    if let Some(yv) = num(&p) {
                        out.push((TileId::new(zv as u8, xv, yv), p));
                    }
                }
            }
        }
    }
    out
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
