//! Dane okolicy kamery: sektory OSM (z dysku albo z Overpass), złożony graf szlaków i punkty.
//! Wszystko w wątku roboczym; interfejs czyta gotową migawkę bez blokowania.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use crate::graph::TrailGraph;
use crate::heights::Heights;
use crate::osm::{self, Poi, PoiKind, SectorData, SectorId};

/// Punkt z wysokością i „ważnością” (do rozrzedzania etykiet).
#[derive(Clone, Debug)]
pub struct PoiView {
    pub poi: Poi,
    /// Wysokość do wyświetlania (OSM albo model terenu).
    pub ele: f32,
    /// Wysokość modelu terenu w punkcie.
    pub dem: f32,
    pub prom: f32,
}

pub struct Snapshot {
    pub graph: TrailGraph,
    pub pois: Vec<PoiView>,
    /// Odcinki z łańcuchami i klamrami: (dł., szer., wysokość).
    pub chains: Vec<Vec<(f64, f64, f32)>>,
    /// Granice: rodzaj, nazwa, linie z wysokościami.
    pub boundaries: Vec<(osm::BoundaryKind, String, Vec<Vec<(f64, f64, f32)>>)>,
    pub sectors: Vec<SectorId>,
    pub version: u64,
}

impl Snapshot {
    fn empty() -> Self {
        Snapshot {
            graph: TrailGraph::empty(),
            pois: Vec::new(),
            chains: Vec::new(),
            boundaries: Vec::new(),
            sectors: Vec::new(),
            version: 0,
        }
    }
}

#[derive(Default)]
struct State {
    wanted: Vec<SectorId>,
    loaded: HashMap<SectorId, Arc<SectorData>>,
    /// Wczytane na razie same punkty – szlaki jeszcze do pobrania.
    partial: HashSet<SectorId>,
    trails_failed: HashMap<SectorId, Instant>,
    /// Sektory, których szlaki właśnie się pobierają (kilka naraz), i początek pobierania.
    trails_busy: Vec<(SectorId, Instant)>,
    /// Ostatni błąd pobierania sektora (do okna szczegółów).
    errors: HashMap<SectorId, String>,
    /// Nieudane uzupełnienie dodatkowych danych (ponowienie po 5 minutach).
    extra_failed: HashMap<SectorId, Instant>,
    busy_since: Option<Instant>,
    /// Rośnie przy każdej zmianie danych sektorów.
    data_gen: u64,
    failed: HashMap<SectorId, Instant>,
    busy: Option<SectorId>,
    error: Option<String>,
    changed: bool,
}

#[derive(Clone)]
pub struct World {
    heights: Heights,
    state: Arc<(Mutex<State>, Condvar)>,
    snap: Arc<RwLock<Arc<Snapshot>>>,
    version: Arc<AtomicU64>,
    /// Rośnie po zapisaniu każdej nowej maski pokrycia terenu.
    land_version: Arc<AtomicU64>,
    /// Pobieranie i wypiekanie maski pokrycia terenu w toku.
    land_busy: Arc<std::sync::atomic::AtomicBool>,
}


impl World {
    pub fn new(heights: Heights, ctx: eframe::egui::Context) -> Self {
        let w = World {
            heights,
            state: Arc::new((Mutex::new(State::default()), Condvar::new())),
            snap: Arc::new(RwLock::new(Arc::new(Snapshot::empty()))),
            version: Arc::default(),
            land_version: Arc::default(),
            land_busy: Arc::default(),
        };
        let me = w.clone();
        let c = ctx.clone();
        let c2 = ctx.clone();
        thread::Builder::new()
            .name("okolica".into())
            .spawn(move || me.run(c))
            .expect("wątek okolicy");
        let me = w.clone();
        thread::Builder::new()
            .name("pokrycie".into())
            .spawn(move || me.run_land(ctx))
            .expect("wątek pokrycia terenu");
        // szlaki dwóch sektorów naraz, z różnych serwerów Overpass
        for k in 0..TRAIL_WORKERS {
            let me = w.clone();
            let c = c2.clone();
            thread::Builder::new()
                .name(format!("szlaki-{k}"))
                .spawn(move || me.run_trails(c, k))
                .expect("wątek szlaków");
        }
        w
    }

    pub fn land_busy(&self) -> bool {
        self.land_busy.load(Ordering::Relaxed)
    }

    pub fn land_version(&self) -> u64 {
        self.land_version.load(Ordering::Relaxed)
    }

    /// Maski pokrycia terenu dla wczytanych sektorów okolicy (po kolei, z przerwą po błędzie).
    fn run_land(&self, ctx: eframe::egui::Context) {
        let mut failed: HashMap<SectorId, Instant> = HashMap::new();
        let mut idle = false;
        loop {
            // bez pracy – rzadkie sprawdzanie; po zapisanym sektorze od razu następny
            thread::sleep(if idle { Duration::from_secs(2) } else { Duration::from_millis(200) });
            idle = true;
            let root = self.heights.store().root().to_path_buf();
            let next = {
                let s = self.state.0.lock().unwrap();
                // sektor pod kamerą (pierwszy na liście) od razu, niezależnie od szlaków – woda,
                // lasy i skały nie czekają minutami na pobranie szlaków; dalsze po wczytaniu szlaków
                s.wanted
                    .iter()
                    .enumerate()
                    .filter(|(k, id)| *k == 0 || (s.loaded.contains_key(id) && !s.partial.contains(id)))
                    .map(|(_, id)| id)
                    .find(|id| {
                        !crate::landcover::exists(&root, **id)
                            && failed.get(id).is_none_or(|t| t.elapsed() > Duration::from_secs(90))
                    })
                    .copied()
            };
            let Some(id) = next else { continue };
            idle = false;
            self.land_busy.store(true, Ordering::Relaxed);
            ctx.request_repaint();
            let r = crate::landcover::fetch(id).and_then(|img| crate::landcover::save(&root, id, &img));
            self.land_busy.store(false, Ordering::Relaxed);
            match r {
                Ok(()) => {
                    log::info!("pokrycie terenu {}/{} zapisane", id.x, id.y);
                    self.land_version.fetch_add(1, Ordering::Relaxed);
                    ctx.request_repaint();
                }
                Err(e) => {
                    log::warn!("pokrycie terenu {}/{}: {e}", id.x, id.y);
                    failed.insert(id, Instant::now());
                }
            }
        }
    }

    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.snap.read().unwrap().clone()
    }

    /// Zestaw sektorów, które mają być wczytane (okolica kamery i punktów trasy).
    /// Kolejność listy to kolejność wczytywania: najpierw to, na co patrzy kamera.
    pub fn want(&self, list: Vec<SectorId>) {
        let mut seen = HashSet::new();
        let list: Vec<SectorId> = list.into_iter().filter(|id| seen.insert(*id)).collect();
        let (m, cv) = &*self.state;
        let mut s = m.lock().unwrap();
        if s.wanted != list {
            s.wanted = list;
            s.changed = true;
            cv.notify_all();
        }
    }

    /// Stan dla interfejsu: (sektor w trakcie pobierania, ostatni błąd).
    pub fn status(&self) -> (Option<SectorId>, Option<String>) {
        let s = self.state.0.lock().unwrap();
        (s.busy.or(s.trails_busy.first().map(|b| b.0)), s.error.clone())
    }

    /// Czy sektor pod kamerą ma już nazwy i szlaki.
    pub fn center_ready(&self) -> bool {
        let s = self.state.0.lock().unwrap();
        s.wanted.first().is_some_and(|id| s.loaded.contains_key(id) && !s.partial.contains(id))
    }

    /// Krótki stan wczytywania do paska: (gotowe sektory, wszystkie, opis bieżącej pracy).
    pub fn progress(&self) -> (usize, usize, Option<String>) {
        let s = self.state.0.lock().unwrap();
        let done = s.wanted.iter().filter(|id| s.loaded.contains_key(id) && !s.partial.contains(id)).count();
        let work = if let Some(t) = s.busy_since.filter(|_| s.busy.is_some()) {
            Some(tf!("nazwy {} s", t.elapsed().as_secs()))
        } else {
            s.trails_busy.iter().map(|(_, t)| t.elapsed().as_secs()).max().map(|t| tf!("szlaki {} s", t))
        };
        (done, s.wanted.len(), work)
    }

    /// Stan każdego sektora okolicy (do okna szczegółów): nazwa sektora, opis, czy gotowy.
    pub fn details(&self) -> Vec<(String, String, bool)> {
        let s = self.state.0.lock().unwrap();
        s.wanted
            .iter()
            .enumerate()
            .map(|(k, id)| {
                let name = if k == 0 { tf!("{}/{} (pod kamerą)", id.x, id.y) } else { format!("{}/{}", id.x, id.y) };
                let err = s.errors.get(id).map(|e| short_err(e)).unwrap_or_default();
                let (txt, ok) = if s.busy == Some(*id) {
                    (tf!("pobieranie nazw… {} s", s.busy_since.map(|t| t.elapsed().as_secs()).unwrap_or(0)), false)
                } else if let Some((_, t)) = s.trails_busy.iter().find(|b| b.0 == *id) {
                    if s.partial.contains(id) {
                        (tf!("nazwy gotowe · pobieranie szlaków z Overpass… {} s", t.elapsed().as_secs()), false)
                    } else {
                        (tf!("gotowe · uzupełnianie schronisk, punktów i granic… {} s", t.elapsed().as_secs()), true)
                    }
                } else if s.partial.contains(id) {
                    match s.trails_failed.get(id) {
                        Some(t) => (tf!("nazwy gotowe · szlaki: błąd ({}), ponowienie za {} s", err, 90u64.saturating_sub(t.elapsed().as_secs())), false),
                        None => (crate::i18n::tr("nazwy gotowe · szlaki w kolejce").to_string(), false),
                    }
                } else if s.loaded.contains_key(id) {
                    (crate::i18n::tr("gotowe (zapisane na dysku)").to_string(), true)
                } else if let Some(t) = s.failed.get(id) {
                    (tf!("błąd ({}), ponowienie za {} s", err, 120u64.saturating_sub(t.elapsed().as_secs())), false)
                } else {
                    (crate::i18n::tr("w kolejce").to_string(), false)
                };
                (name, txt, ok)
            })
            .collect()
    }

    /// Po zmianie folderu danych: wszystko od nowa.
    pub fn reset(&self) {
        let (m, cv) = &*self.state;
        let mut s = m.lock().unwrap();
        s.loaded.clear();
        s.partial.clear();
        s.failed.clear();
        s.trails_failed.clear();
        s.data_gen += 1;
        s.changed = true;
        cv.notify_all();
    }

    fn run(&self, ctx: eframe::egui::Context) {
        let mut built_gen = 0;
        loop {
            // następne zadanie: brakujący sektor z okolicy (szlaki ma osobny wątek)
            let (todo, rebuild) = {
                let (m, cv) = &*self.state;
                let mut s = m.lock().unwrap();
                loop {
                    if s.changed {
                        s.changed = false;
                        break (None, true);
                    }
                    let next = s
                        .wanted
                        .iter()
                        .find(|id| {
                            !s.loaded.contains_key(id)
                                && s.failed.get(id).is_none_or(|t| t.elapsed() > Duration::from_secs(120))
                        })
                        .copied();
                    if let Some(id) = next {
                        break (Some(id), false);
                    }
                    s = cv.wait_timeout(s, Duration::from_secs(30)).unwrap().0;
                }
            };
            if let Some(id) = todo {
                self.load_sector(id, &ctx);
                let mut s = self.state.0.lock().unwrap();
                s.busy = None;
                // nadmiar wczytanych: zostają te z okolicy
                if s.loaded.len() > crate::limits::sectors() {
                    let keep: HashSet<SectorId> = s.wanted.iter().copied().collect();
                    s.loaded.retain(|k, _| keep.contains(k));
                    s.partial.retain(|k| keep.contains(k));
                }
                drop(s);
                ctx.request_repaint();
                continue;
            }
            if rebuild {
                let (mut sectors, gen): (Vec<(SectorId, Arc<SectorData>)>, u64) = {
                    let s = self.state.0.lock().unwrap();
                    let list = s
                        .wanted
                        .iter()
                        .filter_map(|id| s.loaded.get(id).map(|d| (*id, d.clone())))
                        .collect();
                    (list, s.data_gen)
                };
                // ten sam zestaw w innej kolejności to te same dane
                sectors.sort_by_key(|s| s.0);
                let current: Vec<SectorId> = sectors.iter().map(|s| s.0).collect();
                if current == self.snapshot().sectors && gen == built_gen {
                    continue;
                }
                built_gen = gen;
                let t0 = Instant::now();
                let snap = self.build(&sectors);
                log::info!(
                    "okolica: {} sektorów, {} km szlaków, {} punktów ({:.0} ms)",
                    snap.sectors.len(),
                    snap.graph.total_km as u64,
                    snap.pois.len(),
                    t0.elapsed().as_secs_f64() * 1000.0
                );
                *self.snap.write().unwrap() = Arc::new(snap);
                ctx.request_repaint();
            }
        }
    }

    /// Wczytanie sektora: z dysku, a gdy brak – same punkty, od razu widoczne.
    /// Zapisane na dysku sektory wczytują się bez komunikatu o pobieraniu; pobrane punkty zapisują
    /// się od razu (także bez szlaków), więc po ponownym uruchomieniu nic nie pobiera się od nowa.
    fn load_sector(&self, id: SectorId, ctx: &eframe::egui::Context) {
        let root = self.heights.store().root().to_path_buf();
        let result = match osm::load(&root, id) {
            Some(d) => {
                let partial = d.partial;
                Ok((d, partial))
            }
            None => {
                {
                    let mut s = self.state.0.lock().unwrap();
                    s.busy = Some(id);
                    s.busy_since = Some(Instant::now());
                }
                ctx.request_repaint();
                osm::fetch_pois(id).map(|p| {
                    let d = osm::partial(p);
                    if let Err(e) = osm::save(&root, id, &d) {
                        log::error!("zapis sektora: {e}");
                    }
                    (d, true)
                })
            }
        };
        let (m, cv) = &*self.state;
        let mut s = m.lock().unwrap();
        match result {
            Ok((d, is_partial)) => {
                s.error = None;
                s.errors.remove(&id);
                s.failed.remove(&id);
                s.loaded.insert(id, Arc::new(d));
                if is_partial {
                    s.partial.insert(id);
                }
                s.data_gen += 1;
                s.changed = true;
                cv.notify_all();
            }
            Err(e) => {
                log::warn!("sektor {}/{}: {e}", id.x, id.y);
                s.errors.insert(id, e.clone());
                s.error = Some(e);
                s.failed.insert(id, Instant::now());
            }
        }
    }

    /// Szlaki sektorów z okolicy, po kolei: Overpass bywa wolny i nie może blokować reszty.
    fn run_trails(&self, ctx: eframe::egui::Context, worker: usize) {
        loop {
            let next = {
                let mut s = self.state.0.lock().unwrap();
                let id = s
                    .wanted
                    .iter()
                    .find(|id| {
                        s.partial.contains(id)
                            && !s.trails_busy.iter().any(|b| b.0 == **id)
                            && s.trails_failed.get(id).is_none_or(|t| t.elapsed() > Duration::from_secs(90))
                    })
                    .copied();
                if let Some(id) = id {
                    s.trails_busy.push((id, Instant::now()));
                }
                id.and_then(|id| s.loaded.get(&id).map(|d| (id, d.pois.clone())))
            };
            let Some((id, pois)) = next else {
                // bez szlaków do pobrania: uzupełnienie dodatkowych danych w zapisanych sektorach
                if !self.upgrade_extra(&ctx, worker) {
                    thread::sleep(Duration::from_secs(1));
                }
                continue;
            };
            ctx.request_repaint();
            let root = self.heights.store().root().to_path_buf();
            let result = osm::fetch_trails(id, pois, worker);
            let (m, cv) = &*self.state;
            let mut s = m.lock().unwrap();
            s.trails_busy.retain(|b| b.0 != id);
            match result {
                Ok(d) => {
                    if let Err(e) = osm::save(&root, id, &d) {
                        log::error!("zapis sektora: {e}");
                    }
                    s.error = None;
                    s.errors.remove(&id);
                    s.trails_failed.remove(&id);
                    s.loaded.insert(id, Arc::new(d));
                    s.partial.remove(&id);
                    s.data_gen += 1;
                    s.changed = true;
                    cv.notify_all();
                }
                Err(e) => {
                    log::warn!("szlaki {}/{}: {e}", id.x, id.y);
                    s.errors.insert(id, e.clone());
                    s.error = Some(e);

                    s.trails_failed.insert(id, Instant::now());
                }
            }
            drop(s);
            ctx.request_repaint();
        }
    }

    /// Sektor zapisany przed nowszą wersją dodatkowych danych (punkty usługowe, łańcuchy, granice):
    /// jedno lekkie zapytanie zamiast ponownego pobierania szlaków. `false`, gdy nie było pracy.
    fn upgrade_extra(&self, ctx: &eframe::egui::Context, worker: usize) -> bool {
        let next = {
            let mut s = self.state.0.lock().unwrap();
            let id = s
                .wanted
                .iter()
                .find(|id| {
                    !s.partial.contains(id)
                        && s.loaded.get(id).is_some_and(|d| d.extra_ver < osm::EXTRA_VER)
                        && !s.trails_busy.iter().any(|b| b.0 == **id)
                        && s.extra_failed.get(id).is_none_or(|t| t.elapsed() > Duration::from_secs(300))
                })
                .copied();
            if let Some(id) = id {
                s.trails_busy.push((id, Instant::now()));
            }
            id.and_then(|id| s.loaded.get(&id).map(|d| (id, (**d).clone())))
        };
        let Some((id, mut d)) = next else { return false };
        ctx.request_repaint();
        let r = osm::fetch_extra(id, worker);
        let root = self.heights.store().root().to_path_buf();
        let (m, cv) = &*self.state;
        let mut s = m.lock().unwrap();
        s.trails_busy.retain(|b| b.0 != id);
        match r {
            Ok(x) => {
                osm::apply_extra(&mut d, x);
                if let Err(e) = osm::save(&root, id, &d) {
                    log::error!("zapis sektora: {e}");
                }
                s.loaded.insert(id, Arc::new(d));
                s.data_gen += 1;
                s.changed = true;
                cv.notify_all();
            }
            Err(e) => {
                log::warn!("dodatkowe dane {}/{}: {e}", id.x, id.y);
                s.extra_failed.insert(id, Instant::now());
            }
        }
        drop(s);
        ctx.request_repaint();
        true
    }

    fn build(&self, sectors: &[(SectorId, Arc<SectorData>)]) -> Snapshot {
        let h = self.heights.clone();
        let height = move |lon: f64, lat: f64| h.sample(lon, lat).unwrap_or(0.0);
        let refs: Vec<&SectorData> = sectors.iter().map(|s| &*s.1).collect();
        let graph = TrailGraph::build(&refs, &height);
        use rayon::prelude::*;
        let raw: Vec<&Poi> = refs.iter().flat_map(|s| s.pois.iter()).collect();
        let mut pois: Vec<PoiView> = raw
            .par_iter()
            .map(|p| {
                let dem = height(p.lon, p.lat);
                PoiView {
                    poi: (*p).clone(),
                    ele: p.ele.unwrap_or(dem),
                    dem,
                    prom: 0.0,
                }
            })
            .collect();
        prominence(&mut pois, &height);
        pois.sort_by(|a, b| b.prom.total_cmp(&a.prom));
        let with_e = |l: &Vec<(f64, f64)>| -> Vec<(f64, f64, f32)> { l.iter().map(|&(lon, lat)| (lon, lat, height(lon, lat))).collect() };
        let chains: Vec<Vec<(f64, f64, f32)>> = refs.iter().flat_map(|s| s.chains.iter()).map(with_e).collect();
        // granice: ta sama linia z kilku sektorów tylko raz
        let mut seen = std::collections::HashSet::new();
        let mut boundaries = Vec::new();
        for b in refs.iter().flat_map(|s| s.boundaries.iter()) {
            let lines: Vec<Vec<(f64, f64, f32)>> = b
                .lines
                .iter()
                .filter(|l| seen.insert((l[0].0.to_bits(), l[0].1.to_bits(), l.len())))
                .map(|l| {
                    // co ~30 m (granice z OSM mają rzadkie punkty – linia ma leżeć na terenie)
                    let mut out = Vec::new();
                    for w in l.windows(2) {
                        let d = crate::geo::haversine(w[0].0, w[0].1, w[1].0, w[1].1);
                        let n = (d / 30.0).ceil().clamp(1.0, 400.0) as usize;
                        for k in 0..n {
                            let f = k as f64 / n as f64;
                            let (lon, lat) = (w[0].0 + (w[1].0 - w[0].0) * f, w[0].1 + (w[1].1 - w[0].1) * f);
                            out.push((lon, lat, height(lon, lat)));
                        }
                    }
                    if let Some(&(lon, lat)) = l.last() {
                        out.push((lon, lat, height(lon, lat)));
                    }
                    out
                })
                .collect();
            if !lines.is_empty() {
                boundaries.push((b.kind, b.name.clone(), lines));
            }
        }
        Snapshot {
            graph,
            pois,
            chains,
            boundaries,
            sectors: sectors.iter().map(|s| s.0).collect(),
            version: self.version.fetch_add(1, Ordering::Relaxed) + 1,
        }
    }
}

/// Przybliżona „ważność” punktu: izolacja szczytu i wybitność lokalna, typ miejsca.
fn prominence(pois: &mut [PoiView], height: &(dyn Fn(f64, f64) -> f32 + Sync)) {
    use rayon::prelude::*;
    let peaks: Vec<(f64, f64, f32)> = pois
        .iter()
        .filter(|p| p.poi.kind == PoiKind::Peak)
        .map(|p| (p.poi.lon, p.poi.lat, p.ele))
        .collect();
    pois.par_iter_mut().for_each(|p| {
        p.prom = match p.poi.kind {
            PoiKind::Peak => {
                let kx = 111_320.0 * p.poi.lat.to_radians().cos();
                let mut iso = 30_000.0f64;
                for &(lon, lat, e) in &peaks {
                    if e > p.ele {
                        let d = ((lon - p.poi.lon) * kx).hypot((lat - p.poi.lat) * 110_574.0);
                        iso = iso.min(d);
                    }
                }
                let mut low = p.ele;
                for a in 0..16 {
                    let ang = a as f64 / 16.0 * std::f64::consts::TAU;
                    let (dx, dy) = (ang.cos() * 600.0, ang.sin() * 600.0);
                    low = low.min(height(p.poi.lon + dx / kx, p.poi.lat + dy / 110_574.0));
                }
                (iso / 1000.0) as f32 + (p.ele - low) / 120.0
            }
            PoiKind::Hut => 6.0,
            PoiKind::Lake => 3.0,
            PoiKind::Saddle => 1.2,
            PoiKind::Valley | PoiKind::Ridge => 2.0,
            PoiKind::Locality => 1.5,
            PoiKind::Place => {
                let base = match p.poi.place.as_str() {
                    "city" => 16.0,
                    "town" => 9.0,
                    "village" => 2.6,
                    "hamlet" => 0.7,
                    _ => 1.0,
                };
                base + (p.poi.pop.max(1) as f32).log10()
                    * if p.poi.place == "village" { 0.35 } else { 0.2 }
            }
            _ => 0.8,
        };
    });
}

/// Tyle sektorów pobiera szlaki naraz (różne serwery Overpass).
const TRAIL_WORKERS: usize = 2;

fn short_err(e: &str) -> String {
    let mut s: String = e.chars().take(60).collect();
    if e.chars().count() > 60 {
        s.push('…');
    }
    s
}
