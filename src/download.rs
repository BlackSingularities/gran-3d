//! Pobieranie kafli: zadania obszarów (w tle, z postępem, wznawialne) i pojedyncze kafle na żądanie.

use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use crate::bake;
use crate::geo::TileId;
use crate::store::{Layer, Store};

const USER_AGENT: &str = concat!(
    "GRAN/",
    env!("CARGO_PKG_VERSION"),
    " (+https://redzikowski.com/portfolio/gran-3d)"
);
const WORKERS: usize = 8;

pub fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .user_agent(USER_AGENT)
            .timeout_connect(Duration::from_secs(10))
            .timeout_read(Duration::from_secs(30))
            .max_idle_connections_per_host(WORKERS * 2)
            .build()
    })
}

#[derive(Debug)]
pub enum FetchError {
    /// Źródło nie ma tego kafla (np. morze poza zasięgiem zdjęć) – nie ponawiamy.
    Missing,
    Network(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Missing => write!(f, "brak kafla w źródle"),
            FetchError::Network(e) => write!(f, "{e}"),
        }
    }
}

/// Pobranie kafla z sieci (3 próby z narastającą przerwą).
pub fn fetch(layer: Layer, t: TileId) -> Result<Vec<u8>, FetchError> {
    let url = layer.url(t);
    let mut last = String::new();
    for attempt in 0..3 {
        if attempt > 0 {
            thread::sleep(Duration::from_millis(400 * (1 << attempt)));
        }
        match agent().get(&url).call() {
            Ok(r) => {
                let mut buf = Vec::with_capacity(64 * 1024);
                match r.into_reader().take(16 << 20).read_to_end(&mut buf) {
                    Ok(_) if !buf.is_empty() => return Ok(buf),
                    Ok(_) => last = "pusta odpowiedź".into(),
                    Err(e) => last = e.to_string(),
                }
            }
            Err(ureq::Error::Status(404 | 400, _)) => return Err(FetchError::Missing),
            Err(ureq::Error::Status(code, _)) => last = format!("serwer odpowiedział kodem {code}"),
            Err(ureq::Error::Transport(e)) => last = e.to_string(),
        }
    }
    Err(FetchError::Network(last))
}

/// Źródło kafla: zwykłe (jeden adres) albo składane z usług krajowych.
fn fetch_any(store: &Store, layer: Layer, t: TileId) -> Result<Vec<u8>, FetchError> {
    match layer {
        Layer::Ortho => {
            // tło: zdjęcie satelitarne (przodek do z14), przycięte do kafla
            let bg = (0..=t.z.saturating_sub(1)).find_map(|dz| {
                let (a, off, k) = if t.z - dz > crate::render::tiles::SAT_MAX_Z {
                    return None;
                } else if dz == 0 {
                    (t, [0.0, 0.0], 1.0)
                } else {
                    t.ancestor(dz)
                };
                let blob = get(store, Layer::Sat, a).ok()?;
                let img = image::load_from_memory_with_format(bake::jpeg(&blob)?, image::ImageFormat::Jpeg).ok()?.to_rgba8();
                let s = img.width() as f32;
                let (x, y, w) = ((off[0] * s) as u32, (off[1] * s) as u32, ((k * s) as u32).max(1));
                Some(image::imageops::crop_imm(&img, x, y, w, w).to_image())
            });
            crate::national::fetch_ortho(t, bg)
        }
        Layer::Lidar => {
            let mut h = crate::national::fetch_lidar(t)?;
            // braki (granica, morze) z modelu globalnego
            if h.iter().any(|v| v.is_nan()) {
                let base = get(store, Layer::Dem, t).ok().and_then(|b| bake::decode_dem(&b));
                let mean = {
                    let v: Vec<f32> = h.iter().copied().filter(|v| !v.is_nan()).collect();
                    v.iter().sum::<f32>() / v.len().max(1) as f32
                };
                for (i, v) in h.iter_mut().enumerate() {
                    if v.is_nan() {
                        *v = match &base {
                            Some(d) if d.w == 256 && d.h == 256 => d.elev[i],
                            _ => mean,
                        };
                    }
                }
            }
            Ok(h.iter().flat_map(|v| v.to_le_bytes()).collect())
        }
        _ => fetch(layer, t),
    }
}

/// Kafel LiDAR jest już na dysku.
pub fn has_lidar(store: &Store, t: TileId) -> bool {
    store.read(Layer::Dem, t).is_some_and(|b| bake::is_lidar(&b))
}

/// Wypieczony kafel z dysku albo z sieci (pobrany jest od razu wypiekany i zapisywany).
pub fn get(store: &Store, layer: Layer, t: TileId) -> Result<Vec<u8>, FetchError> {
    if let Some(b) = store.read(layer, t) {
        return Ok(b);
    }
    if store.has(layer, t) {
        return Err(FetchError::Missing);
    }
    match fetch_any(store, layer, t) {
        Ok(src) => {
            let baked = bake::bake(layer, &src).map_err(|e| FetchError::Network(e.to_string()))?;
            let _ = store.write(layer, t, Some(&baked));
            Ok(baked)
        }
        Err(FetchError::Missing) => {
            let _ = store.write(layer, t, None);
            Err(FetchError::Missing)
        }
        Err(e) => Err(e),
    }
}

/// Wynik jednego elementu zadania.
pub enum Outcome {
    /// Już był na dysku.
    Skipped,
    /// Zapisany (rozmiar danych ze źródła).
    Saved(u64),
    /// Źródło go nie ma – zapamiętane, nie ponawiamy.
    Missing,
    Failed(String),
}

/// Pobranie, wypieczenie i zapis kafla (element zadania obszaru).
pub fn download_tile(store: &Store, layer: Layer, t: TileId) -> Outcome {
    if layer == Layer::Lidar {
        if has_lidar(store, t) {
            return Outcome::Skipped;
        }
    } else if store.has(layer, t) {
        return Outcome::Skipped;
    }
    match fetch_any(store, layer, t) {
        Ok(src) => match bake::bake(layer, &src) {
            Ok(baked) => match store.write(layer, t, Some(&baked)) {
                Ok(()) => Outcome::Saved(src.len() as u64),
                Err(e) => Outcome::Failed(format!("zapis na dysk: {e}")),
            },
            Err(e) => Outcome::Failed(e.to_string()),
        },
        Err(FetchError::Missing) => {
            // brak LiDAR nie może zablokować modelu globalnego w tej samej paczce
            if layer != Layer::Lidar {
                let _ = store.write(layer, t, None);
            }
            Outcome::Missing
        }
        Err(FetchError::Network(e)) => Outcome::Failed(e),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobState {
    Running,
    Done,
    Cancelled,
    Failed,
}

/// Zadanie pobierania zestawu kafli – stan czytany przez interfejs bez blokowania.
pub struct Job {
    pub label: String,
    pub total: usize,
    done: AtomicUsize,
    skipped: AtomicUsize,
    missing: AtomicUsize,
    failed: AtomicUsize,
    bytes: AtomicU64,
    cancel: AtomicBool,
    state: Mutex<JobState>,
    last_error: Mutex<Option<String>>,
    started: Instant,
}

#[derive(Clone, Debug)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
    pub failed: usize,
    pub bytes: u64,
    pub state: JobState,
    pub eta: Option<Duration>,
    pub last_error: Option<String>,
}

impl Progress {
    /// Zadanie zakończone wcześniej (albo niepotrzebne, bo wszystko jest już na dysku).
    pub fn complete() -> Self {
        Progress {
            done: 1,
            total: 1,
            failed: 0,
            bytes: 0,
            state: JobState::Done,
            eta: None,
            last_error: None,
        }
    }

    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            1.0
        } else {
            self.done as f32 / self.total as f32
        }
    }
}

impl Job {
    /// Zadanie w tle na `total` elementach wykonywanych równolegle przez `work` (pobieranie kafli,
    /// import). Elementy już obecne na dysku zwracają `Skipped`, więc przerwane zadanie po ponownym
    /// uruchomieniu dokańcza tylko brakujące.
    pub fn run<F>(label: String, total: usize, threads: usize, work: F) -> Arc<Job>
    where
        F: Fn(usize) -> Outcome + Send + Sync + 'static,
    {
        let job = Arc::new(Job {
            label,
            total,
            done: AtomicUsize::new(0),
            skipped: AtomicUsize::new(0),
            missing: AtomicUsize::new(0),
            failed: AtomicUsize::new(0),
            bytes: AtomicU64::new(0),
            cancel: AtomicBool::new(false),
            state: Mutex::new(JobState::Running),
            last_error: Mutex::new(None),
            started: Instant::now(),
        });
        let work = Arc::new(work);
        let next = Arc::new(AtomicUsize::new(0));
        let j = job.clone();
        thread::Builder::new()
            .name("zadanie".into())
            .spawn(move || {
                let workers: Vec<_> = (0..threads.max(1))
                    .map(|_| {
                        let (j, work, next) = (j.clone(), work.clone(), next.clone());
                        thread::spawn(move || loop {
                            if j.cancel.load(Ordering::Relaxed) {
                                break;
                            }
                            let i = next.fetch_add(1, Ordering::Relaxed);
                            if i >= j.total {
                                break;
                            }
                            match work(i) {
                                Outcome::Skipped => {
                                    j.skipped.fetch_add(1, Ordering::Relaxed);
                                }
                                Outcome::Saved(n) => {
                                    j.bytes.fetch_add(n, Ordering::Relaxed);
                                }
                                Outcome::Missing => {
                                    j.missing.fetch_add(1, Ordering::Relaxed);
                                }
                                Outcome::Failed(e) => j.fail(e),
                            }
                            j.done.fetch_add(1, Ordering::Relaxed);
                        })
                    })
                    .collect();
                for w in workers {
                    let _ = w.join();
                }
                let failed = j.failed.load(Ordering::Relaxed);
                *j.state.lock().unwrap() = if j.cancel.load(Ordering::Relaxed) {
                    JobState::Cancelled
                } else if failed > 0 && failed * 20 > j.total.max(1) {
                    JobState::Failed
                } else {
                    JobState::Done
                };
            })
            .expect("wątek zadania");
        job
    }

    /// Pobieranie zestawu kafli.
    pub fn download(store: Store, label: String, tiles: Vec<(Layer, TileId)>) -> Arc<Job> {
        Job::run(label, tiles.len(), WORKERS, move |i| {
            let (layer, t) = tiles[i];
            download_tile(&store, layer, t)
        })
    }

    /// Import kafli terenu wersji 1.x (PNG Terrarium, także LiDAR) do paczek.
    pub fn import_legacy(store: Store, files: Vec<(TileId, std::path::PathBuf)>) -> Arc<Job> {
        let threads = thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .min(12);
        Job::run(
            "Import danych wersji 1.x".into(),
            files.len(),
            threads,
            move |i| {
                let (t, path) = &files[i];
                let src = match std::fs::read(path) {
                    Ok(s) => s,
                    Err(e) => return Outcome::Failed(format!("{}: {e}", path.display())),
                };
                match bake::bake(Layer::Dem, &src) {
                    Ok(baked) => match store.write(Layer::Dem, *t, Some(&baked)) {
                        Ok(()) => Outcome::Saved(src.len() as u64),
                        Err(e) => Outcome::Failed(e.to_string()),
                    },
                    Err(e) => Outcome::Failed(format!("{}: {e}", path.display())),
                }
            },
        )
    }

    fn fail(&self, e: String) {
        self.failed.fetch_add(1, Ordering::Relaxed);
        *self.last_error.lock().unwrap() = Some(e);
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn progress(&self) -> Progress {
        let done = self.done.load(Ordering::Relaxed);
        let skipped = self.skipped.load(Ordering::Relaxed);
        let elapsed = self.started.elapsed();
        let fetched = done.saturating_sub(skipped);
        let left = self.total.saturating_sub(done);
        let eta = (fetched > 20 && elapsed.as_secs_f64() > 3.0)
            .then(|| Duration::from_secs_f64(elapsed.as_secs_f64() / fetched as f64 * left as f64));
        Progress {
            done,
            total: self.total,
            failed: self.failed.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
            state: *self.state.lock().unwrap(),
            eta,
            last_error: self.last_error.lock().unwrap().clone(),
        }
    }
}

/// „3 min 20 s”, „45 s”, „1 h 05 min”.
pub fn fmt_duration(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{} h {:02} min", s / 3600, (s % 3600) / 60)
    } else if s >= 60 {
        format!("{} min {:02} s", s / 60, s % 60)
    } else {
        format!("{s} s")
    }
}
