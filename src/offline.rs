//! Tryb offline i obszary pobrane w całości.
//!
//! - **Tryb offline** wyłącza całą sieć: mapa korzysta tylko z danych na dysku, a komentarze
//!   czekają w kolejce na powrót do sieci.
//! - **Obszar offline** to prostokąt mapy pobrany z góry w wybranej szczegółowości: teren, zdjęcia
//!   (opcjonalnie ortofoto krajowe), szlaki i nazwy, drogi oraz lasy i woda. Paczki obszarów są
//!   chronione przed sprzątaniem do limitu (sprzątacz usuwa wtedy tylko resztę).
//!
//! Lista obszarów leży w folderze danych (`offline.json`), więc wędruje razem z danymi.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::geo::{BBox, TileId};
use crate::osm::SectorId;
use crate::pack::PackKey;
use crate::store::{Layer, Store};

static OFFLINE: AtomicBool = AtomicBool::new(false);

/// Tryb offline: żadnych połączeń z siecią.
pub fn is_offline() -> bool {
    OFFLINE.load(Ordering::Relaxed)
}

pub fn set_offline(on: bool) {
    OFFLINE.store(on, Ordering::Relaxed);
    log::info!("tryb offline: {on}");
}

/// Szczegółowość obszaru: najwyższe poziomy zdjęć i terenu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Detail {
    /// Przegląd: zdjęcia do z13 (ok. 19 m/px), teren do z12.
    Basic,
    /// Wędrówka: zdjęcia do z15 (ok. 5 m/px), teren do z14.
    Standard,
    /// Pełna: zdjęcia do z17 (ok. 1 m/px), teren do z16 (LiDAR tam, gdzie jest).
    Full,
}

impl Detail {
    pub const ALL: [Detail; 3] = [Detail::Basic, Detail::Standard, Detail::Full];

    pub fn sat_z(self) -> u8 {
        match self {
            Detail::Basic => 13,
            Detail::Standard => 15,
            Detail::Full => crate::render::tiles::SAT_MAX_Z,
        }
    }

    pub fn dem_z(self) -> u8 {
        match self {
            Detail::Basic => 12,
            Detail::Standard => 14,
            Detail::Full => crate::download::TERRAIN_MAX_Z,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Detail::Basic => crate::i18n::tr("Przegląd"),
            Detail::Standard => crate::i18n::tr("Wędrówka"),
            Detail::Full => crate::i18n::tr("Pełna szczegółowość"),
        }
    }

    pub fn desc(self) -> &'static str {
        match self {
            Detail::Basic => crate::i18n::tr("zdjęcia ok. 19 m/piksel – najmniej miejsca"),
            Detail::Standard => crate::i18n::tr("zdjęcia ok. 5 m/piksel – wygodne planowanie"),
            Detail::Full => crate::i18n::tr("zdjęcia ok. 1 m/piksel, teren LiDAR – najwięcej miejsca"),
        }
    }
}

/// Najniższy poziom kafli pobieranych dla obszaru (niższe są w modelu globalnym albo i tak małe).
const MIN_Z: u8 = 7;
/// Ortofoto krajowe od tego poziomu.
const ORTHO_MIN_Z: u8 = 12;
/// Największa liczba kafli w jednym obszarze (ok. kilkanaście GB).
pub const MAX_TILES: u64 = 400_000;

/// Przybliżony rozmiar kafla na dysku (do szacunku przed pobraniem).
fn tile_bytes(layer: Layer) -> u64 {
    match layer {
        Layer::Dem => 70_000,
        Layer::Sat => 26_000,
        Layer::Ortho => 70_000,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Area {
    pub id: u64,
    pub name: String,
    /// Zachód, południe, wschód, północ.
    pub bbox: [f64; 4],
    pub detail: Detail,
    #[serde(default)]
    pub ortho: bool,
    /// Utworzenie (sekundy od 1970).
    pub created: u64,
    #[serde(default)]
    pub total: u64,
    #[serde(default)]
    pub done: u64,
    #[serde(default)]
    pub failed: u64,
    #[serde(default)]
    pub complete: bool,
}

impl Area {
    pub fn bbox(&self) -> BBox {
        BBox::from_corners([self.bbox[0], self.bbox[1]], [self.bbox[2], self.bbox[3]])
    }

    /// Warstwy i poziomy kafli obszaru.
    pub fn levels(&self) -> Vec<(Layer, u8)> {
        let mut v = Vec::new();
        for z in MIN_Z..=self.detail.dem_z() {
            v.push((Layer::Dem, z));
        }
        for z in MIN_Z..=self.detail.sat_z() {
            v.push((Layer::Sat, z));
        }
        if self.ortho {
            for z in ORTHO_MIN_Z..=self.detail.sat_z().min(Layer::Ortho.max_z()) {
                v.push((Layer::Ortho, z));
            }
        }
        v
    }

    /// Liczba kafli i szacowany rozmiar (bajty).
    pub fn estimate(&self) -> (u64, u64) {
        let b = self.bbox();
        self.levels().iter().fold((0, 0), |(n, s), (l, z)| {
            let c = b.tile_count(*z);
            (n + c, s + c * tile_bytes(*l))
        })
    }

    /// Sektory OSM (szlaki, nazwy, drogi, lasy) pokrywające obszar.
    pub fn sectors(&self) -> Vec<SectorId> {
        let b = self.bbox();
        let z = crate::osm::SECTOR_Z;
        b.tiles(z).map(|t| SectorId { x: t.x, y: t.y }).collect()
    }

    /// Czy paczka należy do obszaru (sprzątacz jej nie rusza).
    pub fn covers(&self, layer: Layer, key: &PackKey) -> bool {
        if !self.levels().contains(&(layer, key.z)) {
            return false;
        }
        let (x0, y0, x1, y1) = self.bbox().tile_range(key.z);
        let side = crate::pack::SIDE;
        let (px0, py0) = (key.bx * side, key.by * side);
        let (px1, py1) = (px0 + side - 1, py0 + side - 1);
        px0 <= x1 && px1 >= x0 && py0 <= y1 && py1 >= y0
    }
}

/// Bieżące pobieranie obszaru.
pub struct Job {
    pub area: u64,
    pub total: AtomicU64,
    pub done: AtomicU64,
    pub failed: AtomicU64,
    pub bytes: AtomicU64,
    pub cancel: AtomicBool,
    /// 0 – szlaki i nazwy, 1 – kafle, 2 – koniec.
    pub phase: AtomicU64,
}

#[derive(Default)]
struct State {
    areas: Vec<Area>,
    job: Option<Arc<Job>>,
    /// Kolejka obszarów do pobrania (po bieżącym).
    queue: Vec<u64>,
}

/// Menedżer obszarów offline (tani do klonowania).
#[derive(Clone)]
pub struct Offline {
    store: Arc<Mutex<Store>>,
    state: Arc<Mutex<State>>,
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn file(root: &Path) -> PathBuf {
    root.join("offline.json")
}

/// Obszary zapisane w folderze danych (także dla sprzątacza, bez menedżera).
pub fn load_areas(root: &Path) -> Vec<Area> {
    std::fs::read(file(root)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

impl Offline {
    pub fn new(store: Store) -> Self {
        let areas = load_areas(store.root());
        Self { store: Arc::new(Mutex::new(store)), state: Arc::new(Mutex::new(State { areas, ..Default::default() })) }
    }

    /// Nowy folder danych: lista obszarów z niego.
    pub fn set_store(&self, store: Store) {
        self.cancel();
        let areas = load_areas(store.root());
        *self.store.lock().unwrap() = store;
        let mut s = self.state.lock().unwrap();
        s.areas = areas;
        s.queue.clear();
    }

    fn save(&self, s: &State) {
        let root = self.store.lock().unwrap().root().to_path_buf();
        let _ = std::fs::create_dir_all(&root);
        if let Err(e) = crate::pack::write_file(&file(&root), &serde_json::to_vec_pretty(&s.areas).unwrap_or_default()) {
            log::error!("zapis obszarów offline: {e}");
        }
    }

    pub fn areas(&self) -> Vec<Area> {
        self.state.lock().unwrap().areas.clone()
    }

    pub fn job(&self) -> Option<Arc<Job>> {
        self.state.lock().unwrap().job.clone()
    }

    pub fn queued(&self, id: u64) -> bool {
        self.state.lock().unwrap().queue.contains(&id)
    }

    /// Dodanie obszaru i rozpoczęcie (albo zakolejkowanie) pobierania.
    pub fn add(&self, mut area: Area) {
        area.id = now() * 1000 + (rand_ish() % 1000);
        area.created = now();
        area.total = area.estimate().0;
        let id = area.id;
        {
            let mut s = self.state.lock().unwrap();
            s.areas.push(area);
            self.save(&s);
        }
        self.download(id);
    }

    pub fn rename(&self, id: u64, name: &str) {
        let mut s = self.state.lock().unwrap();
        if let Some(a) = s.areas.iter_mut().find(|a| a.id == id) {
            a.name = name.trim().to_string();
        }
        self.save(&s);
    }

    /// Usunięcie obszaru z listy: jego dane przestają być chronione i znikną przy sprzątaniu.
    pub fn remove(&self, id: u64) {
        let mut s = self.state.lock().unwrap();
        if s.job.as_ref().is_some_and(|j| j.area == id) {
            if let Some(j) = &s.job {
                j.cancel.store(true, Ordering::Relaxed);
            }
        }
        s.queue.retain(|q| *q != id);
        s.areas.retain(|a| a.id != id);
        self.save(&s);
    }

    pub fn cancel(&self) {
        let mut s = self.state.lock().unwrap();
        s.queue.clear();
        if let Some(j) = &s.job {
            j.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Pobranie (albo uzupełnienie) obszaru; gdy coś już się pobiera – do kolejki.
    pub fn download(&self, id: u64) {
        let mut s = self.state.lock().unwrap();
        if s.job.is_some() {
            if !s.queue.contains(&id) && s.job.as_ref().is_none_or(|j| j.area != id) {
                s.queue.push(id);
            }
            return;
        }
        let Some(area) = s.areas.iter().find(|a| a.id == id).cloned() else { return };
        let job = Arc::new(Job {
            area: id,
            total: AtomicU64::new(area.estimate().0),
            done: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            cancel: AtomicBool::new(false),
            phase: AtomicU64::new(0),
        });
        s.job = Some(job.clone());
        drop(s);
        let me = self.clone();
        let store = self.store.lock().unwrap().clone();
        thread::Builder::new()
            .name("obszar-offline".into())
            .spawn(move || {
                run(&store, &area, &job);
                me.finish(&job);
            })
            .ok();
    }

    fn finish(&self, job: &Job) {
        let next = {
            let mut s = self.state.lock().unwrap();
            let cancelled = job.cancel.load(Ordering::Relaxed);
            if let Some(a) = s.areas.iter_mut().find(|a| a.id == job.area) {
                a.done = job.done.load(Ordering::Relaxed);
                a.failed = job.failed.load(Ordering::Relaxed);
                a.total = job.total.load(Ordering::Relaxed);
                a.complete = !cancelled && a.failed == 0;
            }
            s.job = None;
            self.save(&s);
            if s.queue.is_empty() {
                None
            } else {
                Some(s.queue.remove(0))
            }
        };
        if let Some(id) = next {
            self.download(id);
        }
    }

    /// Paczka chroniona przed sprzątaniem (należy do któregoś obszaru).
    pub fn keeps(&self, layer: Layer, key: &PackKey) -> bool {
        self.state.lock().unwrap().areas.iter().any(|a| a.covers(layer, key))
    }

    /// Czy punkt leży w pobranym obszarze.
    pub fn covered(&self, lon: f64, lat: f64) -> bool {
        self.state.lock().unwrap().areas.iter().any(|a| a.complete && a.bbox().contains(lon, lat))
    }
}

fn rand_ish() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos() as u64).unwrap_or(7)
}

/// Pobieranie obszaru: najpierw dane OSM sektorów, potem kafle (wiele wątków).
fn run(store: &Store, area: &Area, job: &Job) {
    let root = store.root().to_path_buf();
    log::info!("obszar offline „{}”: start ({} kafli)", area.name, job.total.load(Ordering::Relaxed));
    // 1. szlaki, nazwy, drogi i lasy
    let sectors = area.sectors();
    for (i, id) in sectors.iter().enumerate() {
        if job.cancel.load(Ordering::Relaxed) || is_offline() {
            return;
        }
        sector(&root, *id, i);
    }
    job.phase.store(1, Ordering::Relaxed);
    // 2. kafle: od najniższych poziomów (widok z daleka działa najwcześniej)
    let mut tiles: Vec<(Layer, TileId)> = Vec::new();
    let b = area.bbox();
    let mut levels = area.levels();
    levels.sort_by_key(|(l, z)| (*z, l.id()));
    for (l, z) in levels {
        tiles.extend(b.tiles(z).map(|t| (l, t)));
    }
    job.total.store(tiles.len() as u64, Ordering::Relaxed);
    let tiles = Arc::new(tiles);
    let next = Arc::new(AtomicU64::new(0));
    let workers = crate::limits::workers().clamp(2, 16);
    thread::scope(|sc| {
        for _ in 0..workers {
            let (tiles, next) = (tiles.clone(), next.clone());
            sc.spawn(move || loop {
                if job.cancel.load(Ordering::Relaxed) || is_offline() {
                    return;
                }
                let i = next.fetch_add(1, Ordering::Relaxed) as usize;
                let Some(&(layer, t)) = tiles.get(i) else { return };
                if store.has(layer, t) {
                    job.done.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                match crate::download::get(store, layer, t) {
                    Ok(b) => {
                        job.bytes.fetch_add(b.len() as u64, Ordering::Relaxed);
                        job.done.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(crate::download::FetchError::Missing) => {
                        job.done.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(e) => {
                        log::warn!("obszar offline {}/{}/{}: {e}", t.z, t.x, t.y);
                        job.failed.fetch_add(1, Ordering::Relaxed);
                        thread::sleep(Duration::from_millis(200));
                    }
                }
            });
        }
    });
    job.phase.store(2, Ordering::Relaxed);
    log::info!(
        "obszar offline „{}”: {} z {} kafli, błędy: {}",
        area.name,
        job.done.load(Ordering::Relaxed),
        job.total.load(Ordering::Relaxed),
        job.failed.load(Ordering::Relaxed)
    );
}

/// Komplet danych OSM sektora na dysku (to samo, co pobiera się samo w trakcie oglądania).
fn sector(root: &Path, id: SectorId, worker: usize) {
    let mut d = crate::osm::load(root, id);
    if d.is_none() {
        match crate::osm::fetch_pois(id) {
            Ok(p) => {
                let p = crate::osm::partial(p);
                let _ = crate::osm::save(root, id, &p);
                d = Some(p);
            }
            Err(e) => log::warn!("obszar offline: sektor {}/{}: {e}", id.x, id.y),
        }
    }
    if let Some(cur) = d.filter(|d| d.partial) {
        match crate::osm::fetch_trails(id, cur.pois.clone(), worker) {
            Ok(full) => {
                let _ = crate::osm::save(root, id, &full);
            }
            Err(e) => log::warn!("obszar offline: szlaki {}/{}: {e}", id.x, id.y),
        }
    } else if let Some(mut cur) = crate::osm::load(root, id).filter(|d| d.extra_ver < crate::osm::EXTRA_VER) {
        if let Ok(x) = crate::osm::fetch_extra(id, worker) {
            crate::osm::apply_extra(&mut cur, x);
            let _ = crate::osm::save(root, id, &cur);
        }
    }
    if !crate::roads::exists(root, id) {
        if let Ok(r) = crate::roads::fetch(id) {
            let _ = crate::roads::save(root, id, &r);
        }
    }
    if !crate::landcover::exists(root, id) {
        if let Ok(img) = crate::landcover::fetch(id) {
            let _ = crate::landcover::save(root, id, &img);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(detail: Detail) -> Area {
        Area { id: 1, name: "Tatry".into(), bbox: [19.75, 49.15, 20.25, 49.30], detail, ortho: false, created: 0, total: 0, done: 0, failed: 0, complete: false }
    }

    #[test]
    fn estimate_grows_with_detail() {
        let (a, _) = area(Detail::Basic).estimate();
        let (b, _) = area(Detail::Standard).estimate();
        let (c, sz) = area(Detail::Full).estimate();
        assert!(a < b && b < c);
        assert!(sz > 100 << 20);
    }

    #[test]
    fn covers_packs_inside() {
        let a = area(Detail::Standard);
        let t = crate::geo::tile_at(20.0, 49.2, 14);
        let (key, _) = PackKey::of(Layer::Sat.id(), t);
        assert!(a.covers(Layer::Sat, &key));
        let far = crate::geo::tile_at(5.0, 45.0, 14);
        let (key, _) = PackKey::of(Layer::Sat.id(), far);
        assert!(!a.covers(Layer::Sat, &key));
        let deep = crate::geo::tile_at(20.0, 49.2, 17);
        let (key, _) = PackKey::of(Layer::Sat.id(), deep);
        assert!(!a.covers(Layer::Sat, &key), "z17 poza szczegółowością „Wędrówka”");
    }
}
