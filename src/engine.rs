//! Silnik pobierania obszarów: wspólna kolejka z priorytetami i osobne limity połączeń na źródło.
//! Obszary pobierają się równolegle; pobiera się tylko różnica względem biblioteki.
//! Wątek sieciowy od razu wypieka kafel, więc wypiekanie też idzie na wszystkich rdzeniach.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::areas::{self, Area};
use crate::download::{self, JobState, Outcome, Progress};
use crate::geo::{x_to_lon, y_to_lat, TileId};
use crate::library;
use crate::store::{Layer, Store};

/// Identyfikator grupy modelu globalnego.
pub const BASE: &str = "\u{0}baza";

const PRIO_BASE: u8 = 1;
const PRIO_AREA: u8 = 2;

/// Serwery danych: (nazwa, największa liczba połączeń). Usługi krajowe są wolne i łatwo o blokadę.
pub const SOURCES: [(&str, usize); 5] =
    [("AWS", 32), ("EOX", 16), ("GUGiK", 6), ("ZBGIS", 4), ("ČÚZK", 4)];
const N: usize = SOURCES.len();

fn source_of(layer: Layer, t: TileId) -> usize {
    match layer {
        Layer::Dem => 0,
        Layer::Sat => 1,
        Layer::Ortho | Layer::Lidar => {
            let n = (1u64 << t.z) as f64;
            let (lon, lat) = (x_to_lon((t.x as f64 + 0.5) / n), y_to_lat((t.y as f64 + 0.5) / n));
            match crate::national::country_at(lon, lat) {
                Some("SK") => 3,
                Some("CZ") => 4,
                _ => 2,
            }
        }
    }
}

struct Group {
    id: String,
    label: String,
    prio: u8,
    queues: [VecDeque<(Layer, TileId)>; N],
    planning: bool,
    paused: bool,
    total: usize,
    done: usize,
    failed: usize,
    bytes: u64,
    active: usize,
    last_error: Option<String>,
    started: Instant,
}

impl Group {
    fn queued(&self) -> usize {
        self.queues.iter().map(|q| q.len()).sum()
    }

    fn progress(&self) -> Progress {
        let elapsed = self.started.elapsed().as_secs_f64();
        let left = self.total.saturating_sub(self.done);
        let eta = (self.done > 20 && elapsed > 3.0)
            .then(|| Duration::from_secs_f64(elapsed / self.done as f64 * left as f64));
        let finished = !self.planning && self.active == 0 && self.queued() == 0;
        Progress {
            // planowanie: jeszcze nie wiadomo ile, ale też nie „gotowe”
            done: self.done,
            total: if self.planning { 1 } else { self.total },
            failed: self.failed,
            bytes: self.bytes,
            state: if finished && self.failed > 0 {
                JobState::Failed
            } else if finished {
                JobState::Done
            } else if self.paused {
                JobState::Paused
            } else {
                JobState::Running
            },
            eta,
            last_error: self.last_error.clone(),
        }
    }
}

#[derive(Clone, Default)]
pub struct SourceStats {
    pub active: usize,
    pub queued: usize,
    pub limit: usize,
    pub last_error: Option<String>,
}

/// Stan całego pobierania do wskaźników w interfejsie.
#[derive(Clone, Default)]
pub struct Stats {
    pub bytes_per_s: f64,
    pub tiles_per_s: f64,
    pub done: usize,
    pub total: usize,
    pub paused: bool,
    pub sources: [SourceStats; N],
}

struct Inner {
    groups: Vec<Group>,
    active: [usize; N],
    limit: [usize; N],
    streak: [u32; N],
    errors: [Option<String>; N],
    paused: bool,
    finished: Vec<(String, String, Progress)>,
    bytes: u64,
    tiles: u64,
    sample: (Instant, u64, u64),
    rates: (f64, f64),
}

impl Inner {
    fn pop(&mut self, s: usize) -> Option<(String, Layer, TileId)> {
        if self.paused || self.active[s] >= self.limit[s] {
            return None;
        }
        let g = self.groups.iter_mut().find(|g| !g.paused && !g.queues[s].is_empty())?;
        let (layer, t) = g.queues[s].pop_front()?;
        g.active += 1;
        self.active[s] += 1;
        Some((g.id.clone(), layer, t))
    }

    fn settle(&mut self, id: &str) {
        let Some(i) = self.groups.iter().position(|g| g.id == id) else {
            return;
        };
        let g = &self.groups[i];
        if !g.planning && g.active == 0 && g.queued() == 0 {
            let g = self.groups.remove(i);
            self.finished.push((g.id.clone(), g.label.clone(), g.progress()));
        }
    }
}

type Work = dyn Fn(&Store, Layer, TileId) -> Outcome + Send + Sync;

struct Shared {
    store: Store,
    inner: Mutex<Inner>,
    wake: Condvar,
    stop: AtomicBool,
    work: Box<Work>,
}

pub struct Engine {
    shared: Arc<Shared>,
}

impl Engine {
    pub fn new(store: Store) -> Self {
        Self::with_work(store, Box::new(download::download_tile))
    }

    fn with_work(store: Store, work: Box<Work>) -> Self {
        let shared = Arc::new(Shared {
            store,
            inner: Mutex::new(Inner {
                groups: Vec::new(),
                active: [0; N],
                limit: SOURCES.map(|s| s.1),
                streak: [0; N],
                errors: Default::default(),
                paused: false,
                finished: Vec::new(),
                bytes: 0,
                tiles: 0,
                sample: (Instant::now(), 0, 0),
                rates: (0.0, 0.0),
            }),
            wake: Condvar::new(),
            stop: AtomicBool::new(false),
            work,
        });
        for (s, (name, count)) in SOURCES.iter().enumerate() {
            for _ in 0..*count {
                let shared = shared.clone();
                thread::Builder::new()
                    .name(format!("pobieranie {name}"))
                    .spawn(move || worker(shared, s))
                    .expect("wątek pobierania");
            }
        }
        Self { shared }
    }

    fn register(&self, id: &str, label: &str, prio: u8) -> bool {
        let mut inner = self.shared.inner.lock().unwrap();
        if inner.groups.iter().any(|g| g.id == id) {
            return false;
        }
        let g = Group {
            id: id.to_string(),
            label: label.to_string(),
            prio,
            queues: Default::default(),
            planning: true,
            paused: false,
            total: 0,
            done: 0,
            failed: 0,
            bytes: 0,
            active: 0,
            last_error: None,
            started: Instant::now(),
        };
        // stabilnie: w obrębie priorytetu kolejność dodania
        let at = inner.groups.iter().position(|o| o.prio > prio).unwrap_or(inner.groups.len());
        inner.groups.insert(at, g);
        true
    }

    fn fill(shared: &Shared, id: &str, tiles: impl Iterator<Item = (Layer, TileId)>) {
        let mut inner = shared.inner.lock().unwrap();
        if let Some(g) = inner.groups.iter_mut().find(|g| g.id == id) {
            for (layer, t) in tiles {
                g.queues[source_of(layer, t)].push_back((layer, t));
                g.total += 1;
            }
            g.planning = false;
        }
        inner.settle(id);
        drop(inner);
        shared.wake.notify_all();
    }

    /// Model globalny Europy (brakujące kafle).
    pub fn add_base(&self) {
        if !self.register(BASE, "Model globalny Europy", PRIO_BASE) {
            return;
        }
        let shared = self.shared.clone();
        thread::spawn(move || {
            let store = shared.store.clone();
            let missing: Vec<_> = areas::base_tiles().filter(|(l, t)| !store.has(*l, *t)).collect();
            Self::fill(&shared, BASE, missing.into_iter());
        });
    }

    /// Obszar: plan liczony w tle jako różnica z biblioteką, więc nic nie leci drugi raz.
    pub fn add_area(&self, a: &Area) {
        if !self.register(&a.id, &a.name, PRIO_AREA) {
            return;
        }
        let shared = self.shared.clone();
        let (id, bbox, level) = (a.id.clone(), a.bbox, a.level);
        thread::spawn(move || {
            let store = shared.store.clone();
            let plan = library::plan(&bbox, level, |l, t| store.has(l, t));
            Self::fill(&shared, &id, plan.tiles());
        });
    }

    pub fn progress(&self, id: &str) -> Option<Progress> {
        let inner = self.shared.inner.lock().unwrap();
        inner.groups.iter().find(|g| g.id == id).map(|g| g.progress())
    }

    pub fn busy(&self) -> bool {
        !self.shared.inner.lock().unwrap().groups.is_empty()
    }

    /// Grupy zakończone od ostatniego wywołania: (id, nazwa, wynik).
    pub fn take_finished(&self) -> Vec<(String, String, Progress)> {
        std::mem::take(&mut self.shared.inner.lock().unwrap().finished)
    }

    pub fn cancel(&self, id: &str) {
        let mut inner = self.shared.inner.lock().unwrap();
        inner.groups.retain(|g| g.id != id);
    }

    pub fn set_paused(&self, id: &str, paused: bool) {
        let mut inner = self.shared.inner.lock().unwrap();
        if let Some(g) = inner.groups.iter_mut().find(|g| g.id == id) {
            g.paused = paused;
        }
        drop(inner);
        self.shared.wake.notify_all();
    }

    pub fn set_all_paused(&self, paused: bool) {
        self.shared.inner.lock().unwrap().paused = paused;
        self.shared.wake.notify_all();
    }

    pub fn stats(&self) -> Stats {
        let mut guard = self.shared.inner.lock().unwrap();
        let inner = &mut *guard;
        let dt = inner.sample.0.elapsed().as_secs_f64();
        if dt >= 1.0 {
            let bps = (inner.bytes - inner.sample.1) as f64 / dt;
            let tps = (inner.tiles - inner.sample.2) as f64 / dt;
            inner.rates = (inner.rates.0 * 0.6 + bps * 0.4, inner.rates.1 * 0.6 + tps * 0.4);
            inner.sample = (Instant::now(), inner.bytes, inner.tiles);
        }
        let mut st = Stats {
            bytes_per_s: inner.rates.0,
            tiles_per_s: inner.rates.1,
            paused: inner.paused,
            ..Default::default()
        };
        for g in &inner.groups {
            st.done += g.done;
            st.total += g.total;
        }
        for s in 0..N {
            st.sources[s] = SourceStats {
                active: inner.active[s],
                queued: inner.groups.iter().map(|g| g.queues[s].len()).sum(),
                limit: inner.limit[s],
                last_error: inner.errors[s].clone(),
            };
        }
        st
    }

    /// Zatrzymanie wątków (kafle w toku dokończą zapis).
    pub fn shutdown(&self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.inner.lock().unwrap().groups.clear();
        self.shared.wake.notify_all();
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn worker(shared: Arc<Shared>, s: usize) {
    loop {
        let job = {
            let mut inner = shared.inner.lock().unwrap();
            loop {
                if shared.stop.load(Ordering::Relaxed) {
                    return;
                }
                if let Some(job) = inner.pop(s) {
                    break job;
                }
                inner = shared.wake.wait_timeout(inner, Duration::from_millis(500)).unwrap().0;
            }
        };
        let (id, layer, t) = job;
        let outcome = (shared.work)(&shared.store, layer, t);
        let mut inner = shared.inner.lock().unwrap();
        inner.active[s] -= 1;
        // serwer nie nadąża: mniej połączeń; po serii sukcesów powoli z powrotem
        let (mut bytes, mut error) = (0, None);
        match outcome {
            Outcome::Saved(n) => bytes = n,
            Outcome::Failed(e) => error = Some(e),
            Outcome::Skipped | Outcome::Missing => {}
        }
        if let Some(e) = &error {
            inner.limit[s] = (inner.limit[s] / 2).max(1);
            inner.streak[s] = 0;
            inner.errors[s] = Some(e.clone());
        } else {
            inner.streak[s] += 1;
            if inner.streak[s] >= 20 && inner.limit[s] < SOURCES[s].1 {
                inner.limit[s] += 1;
                inner.streak[s] = 0;
                inner.errors[s] = None;
            }
        }
        inner.bytes += bytes;
        inner.tiles += 1;
        if let Some(g) = inner.groups.iter_mut().find(|g| g.id == id) {
            g.active -= 1;
            g.done += 1;
            g.bytes += bytes;
            if let Some(e) = error {
                g.failed += 1;
                g.last_error = Some(e);
            }
        }
        inner.settle(&id);
        drop(inner);
        shared.wake.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn engine(calls: Arc<AtomicUsize>, name: &str) -> Engine {
        let dir = std::env::temp_dir().join(format!("gran-engine-{name}-{}", std::process::id()));
        Engine::with_work(
            Store::new(dir),
            Box::new(move |_, _, t| {
                calls.fetch_add(1, Ordering::SeqCst);
                if t.x == 13 {
                    Outcome::Failed("błąd".into())
                } else {
                    Outcome::Saved(10)
                }
            }),
        )
    }

    fn wait_finished(e: &Engine) -> Vec<(String, String, Progress)> {
        for _ in 0..500 {
            let f = e.take_finished();
            if !f.is_empty() {
                return f;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("grupa się nie zakończyła");
    }

    fn tiles(n: u32) -> impl Iterator<Item = (Layer, TileId)> {
        (0..n).map(|x| (Layer::Dem, TileId::new(10, x, 5)))
    }

    #[test]
    fn group_finishes_and_reports_failures() {
        let calls = Arc::new(AtomicUsize::new(0));
        let e = engine(calls.clone(), "wynik");
        assert!(e.register("a", "A", PRIO_AREA));
        assert!(!e.register("a", "A", PRIO_AREA));
        Engine::fill(&e.shared, "a", tiles(100));
        let f = wait_finished(&e);
        assert_eq!(f[0].0, "a");
        assert_eq!(f[0].2.state, JobState::Failed);
        assert_eq!(f[0].2.failed, 1);
        assert_eq!(f[0].2.done, 100);
        assert_eq!(calls.load(Ordering::SeqCst), 100);
        assert!(!e.busy());
    }

    #[test]
    fn paused_group_waits() {
        let calls = Arc::new(AtomicUsize::new(0));
        let e = engine(calls.clone(), "pauza");
        e.register("a", "A", PRIO_AREA);
        e.set_paused("a", true);
        Engine::fill(&e.shared, "a", tiles(10));
        thread::sleep(Duration::from_millis(100));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(e.progress("a").unwrap().state, JobState::Paused);
        e.set_paused("a", false);
        assert_eq!(wait_finished(&e)[0].2.state, JobState::Done);
    }

    #[test]
    fn planning_is_not_done_and_empty_plan_finishes() {
        let e = engine(Arc::new(AtomicUsize::new(0)), "plan");
        e.register("a", "A", PRIO_AREA);
        assert!(e.progress("a").unwrap().fraction() < 1.0);
        Engine::fill(&e.shared, "a", tiles(0));
        assert_eq!(wait_finished(&e)[0].2.state, JobState::Done);
    }
}
