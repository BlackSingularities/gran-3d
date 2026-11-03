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
    pub sectors: Vec<SectorId>,
    pub version: u64,
}

impl Snapshot {
    fn empty() -> Self {
        Snapshot {
            graph: TrailGraph::empty(),
            pois: Vec::new(),
            sectors: Vec::new(),
            version: 0,
        }
    }
}

#[derive(Default)]
struct State {
    wanted: Vec<SectorId>,
    /// Sektory, które trzeba mieć na dysku (zapisane obszary) – pobierane w tle po kolei.
    persist: Vec<SectorId>,
    loaded: HashMap<SectorId, Arc<SectorData>>,
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
}

const MAX_LOADED: usize = 25;

impl World {
    pub fn new(heights: Heights, ctx: eframe::egui::Context) -> Self {
        let w = World {
            heights,
            state: Arc::new((Mutex::new(State::default()), Condvar::new())),
            snap: Arc::new(RwLock::new(Arc::new(Snapshot::empty()))),
            version: Arc::default(),
        };
        let me = w.clone();
        thread::Builder::new()
            .name("okolica".into())
            .spawn(move || me.run(ctx))
            .expect("wątek okolicy");
        w
    }

    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.snap.read().unwrap().clone()
    }

    /// Zestaw sektorów, które mają być wczytane (okolica kamery i punktów trasy).
    pub fn want(&self, mut list: Vec<SectorId>) {
        list.sort();
        list.dedup();
        let (m, cv) = &*self.state;
        let mut s = m.lock().unwrap();
        if s.wanted != list {
            s.wanted = list;
            s.changed = true;
            cv.notify_all();
        }
    }

    /// Sektory do zapisania na dysku (pobieranie obszarów bez internetu).
    pub fn persist(&self, list: impl IntoIterator<Item = SectorId>) {
        let (m, cv) = &*self.state;
        let mut s = m.lock().unwrap();
        for id in list {
            if !s.persist.contains(&id) {
                s.persist.push(id);
            }
        }
        cv.notify_all();
    }

    /// Stan dla interfejsu: (sektor w trakcie pobierania, ostatni błąd, zaległe do zapisu).
    pub fn status(&self) -> (Option<SectorId>, Option<String>, usize) {
        let s = self.state.0.lock().unwrap();
        (s.busy, s.error.clone(), s.persist.len())
    }

    /// Po zmianie folderu danych: wszystko od nowa.
    pub fn reset(&self) {
        let (m, cv) = &*self.state;
        let mut s = m.lock().unwrap();
        s.loaded.clear();
        s.failed.clear();
        s.changed = true;
        cv.notify_all();
    }

    fn run(&self, ctx: eframe::egui::Context) {
        loop {
            // następne zadanie: brakujący sektor z okolicy, potem zaległe do zapisu
            let (todo, rebuild) = {
                let (m, cv) = &*self.state;
                let mut s = m.lock().unwrap();
                loop {
                    let root = self.heights.store().root().to_path_buf();
                    let failed_ok = |s: &State, id: &SectorId| {
                        s.failed
                            .get(id)
                            .is_none_or(|t| t.elapsed() > Duration::from_secs(120))
                    };
                    let next = s
                        .wanted
                        .iter()
                        .find(|id| !s.loaded.contains_key(id) && failed_ok(&s, id))
                        .copied();
                    if s.changed {
                        s.changed = false;
                        break (None, true);
                    }
                    if let Some(id) = next {
                        break (Some((id, true)), false);
                    }
                    if let Some(i) = s.persist.iter().position(|id| !osm::exists(&root, *id)) {
                        let id = s.persist[i];
                        if failed_ok(&s, &id) {
                            break (Some((id, false)), false);
                        }
                    }
                    s.persist.retain(|id| !osm::exists(&root, *id));
                    s = cv.wait_timeout(s, Duration::from_secs(30)).unwrap().0;
                }
            };
            if let Some((id, near)) = todo {
                self.state.0.lock().unwrap().busy = Some(id);
                ctx.request_repaint();
                let root = self.heights.store().root().to_path_buf();
                let data = match osm::load(&root, id) {
                    Some(d) => Ok(d),
                    None => osm::fetch(id).inspect(|d| {
                        if let Err(e) = osm::save(&root, id, d) {
                            log::error!("zapis sektora: {e}");
                        }
                    }),
                };
                let mut s = self.state.0.lock().unwrap();
                s.busy = None;
                match data {
                    Ok(d) => {
                        s.error = None;
                        if near {
                            s.loaded.insert(id, Arc::new(d));
                            s.changed = true;
                        } else {
                            s.persist.retain(|p| *p != id);
                        }
                    }
                    Err(e) => {
                        log::warn!("sektor {}/{}: {e}", id.x, id.y);
                        s.error = Some(e);
                        s.failed.insert(id, Instant::now());
                        if !near {
                            s.persist.retain(|p| *p != id);
                        }
                    }
                }
                // nadmiar wczytanych: zostają te z okolicy
                if s.loaded.len() > MAX_LOADED {
                    let keep: HashSet<SectorId> = s.wanted.iter().copied().collect();
                    s.loaded.retain(|k, _| keep.contains(k));
                }
                ctx.request_repaint();
                continue;
            }
            if rebuild {
                let sectors: Vec<(SectorId, Arc<SectorData>)> = {
                    let s = self.state.0.lock().unwrap();
                    s.wanted
                        .iter()
                        .filter_map(|id| s.loaded.get(id).map(|d| (*id, d.clone())))
                        .collect()
                };
                let current: Vec<SectorId> = sectors.iter().map(|s| s.0).collect();
                if current == self.snapshot().sectors {
                    continue;
                }
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
        Snapshot {
            graph,
            pois,
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
