//! Pobieranie kafli na żądanie: pobrany kafel jest od razu wypiekany i zapisywany na dysku.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use crate::bake;
use crate::geo::TileId;
use crate::store::{Layer, Store};

const USER_AGENT: &str = concat!(
    "GRAN/",
    env!("CARGO_PKG_VERSION"),
    " (+https://redzikowski.com/portfolio/gran-3d)"
);

/// Od tego poziomu teren może pochodzić z LiDAR-u.
const LIDAR_MIN_Z: u8 = 14;
/// LiDAR różniący się od modelu globalnego o tyle metrów to błąd usługi, nie teren.
const LIDAR_MAX_DIFF: f32 = 200.0;

/// Kafel pobierany właśnie przez ten wątek. Inny wątek chcący ten sam kafel czeka i bierze go z dysku.
struct Claim(Layer, TileId);

fn claims() -> &'static (Mutex<HashSet<(Layer, TileId)>>, Condvar) {
    static C: OnceLock<(Mutex<HashSet<(Layer, TileId)>>, Condvar)> = OnceLock::new();
    C.get_or_init(Default::default)
}

impl Claim {
    fn take(layer: Layer, t: TileId) -> Claim {
        let (set, cv) = claims();
        let mut s = set.lock().unwrap();
        while !s.insert((layer, t)) {
            s = cv.wait(s).unwrap();
        }
        Claim(layer, t)
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        let (set, cv) = claims();
        set.lock().unwrap().remove(&(self.0, self.1));
        cv.notify_all();
    }
}

/// Zdekodowane zdjęcia satelitarne: jeden przodek służy wielu kaflom ortofoto.
const SAT_CACHE: usize = 256;

fn decoded_sat(store: &Store, t: TileId) -> Option<Arc<image::RgbaImage>> {
    type Cache = (HashMap<TileId, Arc<image::RgbaImage>>, VecDeque<TileId>);
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(img) = cache.lock().unwrap().0.get(&t) {
        return Some(img.clone());
    }
    let blob = get(store, Layer::Sat, t).ok()?;
    let img = image::load_from_memory_with_format(bake::jpeg(&blob)?, image::ImageFormat::Jpeg).ok()?.to_rgba8();
    let img = Arc::new(img);
    let mut c = cache.lock().unwrap();
    if c.0.insert(t, img.clone()).is_none() {
        c.1.push_back(t);
    }
    if c.1.len() > SAT_CACHE {
        if let Some(old) = c.1.pop_front() {
            c.0.remove(&old);
        }
    }
    Some(img)
}

pub fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .user_agent(USER_AGENT)
            .timeout_connect(Duration::from_secs(10))
            .timeout_read(Duration::from_secs(30))
            .max_idle_connections_per_host(32)
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
                let img = decoded_sat(store, a)?;
                let s = img.width() as f32;
                let (x, y, w) = ((off[0] * s) as u32, (off[1] * s) as u32, ((k * s) as u32).max(1));
                Some(image::imageops::crop_imm(&*img, x, y, w, w).to_image())
            });
            crate::national::fetch_ortho(t, bg)
        }
        Layer::Lidar => {
            let mut h = crate::national::fetch_lidar(t)?;
            // braki (granica, morze) i błędne wartości usług – z modelu globalnego
            let base = get(store, Layer::Dem, t).ok().and_then(|b| bake::decode_dem(&b)).filter(|d| d.w == 256 && d.h == 256);
            let mean = {
                let v: Vec<f32> = h.iter().copied().filter(|v| !v.is_nan()).collect();
                v.iter().sum::<f32>() / v.len().max(1) as f32
            };
            for (i, v) in h.iter_mut().enumerate() {
                match &base {
                    Some(d) if v.is_nan() || (*v - d.elev[i]).abs() > LIDAR_MAX_DIFF => *v = d.elev[i],
                    None if v.is_nan() => *v = mean,
                    _ => {}
                }
            }
            Ok(h.iter().flat_map(|v| v.to_le_bytes()).collect())
        }
        _ => fetch(layer, t),
    }
}

/// Kafel z dysku: dane, wiadomy brak w źródle albo `None`, gdy trzeba pobrać.
fn stored(store: &Store, layer: Layer, t: TileId) -> Option<Result<Vec<u8>, FetchError>> {
    match store.read(layer, t) {
        Some(b) if bake::dem_stale(&b) => None,
        Some(b) => Some(Ok(b)),
        None if store.has(layer, t) => Some(Err(FetchError::Missing)),
        None => None,
    }
}

/// Wypiekanie pobranego kafla. Źródło terenu miewa całe kafle bez danych – uzupełnia je kafel nadrzędny.
fn bake_source(store: &Store, layer: Layer, t: TileId, src: &[u8]) -> Result<Vec<u8>, FetchError> {
    let net = |e: bake::BakeError| FetchError::Network(e.to_string());
    if layer != Layer::Dem {
        return bake::bake(layer, src).map_err(net);
    }
    let (mut elev, w, h) = bake::terrarium(src).map_err(net)?;
    if bake::has_holes(&elev) {
        let parent = (t.z > 0).then(|| t.ancestor(1)).and_then(|(p, off, k)| {
            let d = get(store, Layer::Dem, p).ok().and_then(|b| bake::decode_dem(&b))?;
            Some((d, off, k))
        });
        let view = parent.as_ref().map(|(d, off, k)| (d, *off, *k));
        if !bake::fill_holes(&mut elev, w, h, view) {
            return Err(FetchError::Network("kafel terenu bez danych".into()));
        }
    }
    Ok(bake::encode_dem(&elev, w, h))
}

/// Wypieczony kafel z dysku albo z sieci (pobrany jest od razu wypiekany i zapisywany).
pub fn get(store: &Store, layer: Layer, t: TileId) -> Result<Vec<u8>, FetchError> {
    if let Some(r) = stored(store, layer, t) {
        return r;
    }
    let _claim = Claim::take(layer, t);
    if let Some(r) = stored(store, layer, t) {
        return r;
    }
    match fetch_any(store, layer, t) {
        Ok(src) => {
            let baked = bake_source(store, layer, t, &src)?;
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

/// Kafel terenu do widoku: na bliskich poziomach najpierw LiDAR, gdzie sięgają usługi,
/// a gdy go nie ma albo serwer nie odpowiada – model globalny.
pub fn get_terrain(store: &Store, t: TileId) -> Result<Vec<u8>, FetchError> {
    if t.z >= LIDAR_MIN_Z && crate::national::has_lidar(t) {
        let _claim = Claim::take(Layer::Lidar, t);
        if !store.has(Layer::Lidar, t) {
            match fetch_any(store, Layer::Lidar, t) {
                Ok(src) => {
                    if let Ok(baked) = bake::bake(Layer::Lidar, &src) {
                        let _ = store.write(Layer::Lidar, t, Some(&baked));
                        return Ok(baked);
                    }
                }
                Err(FetchError::Missing) => store.note_no_lidar(t),
                Err(FetchError::Network(e)) => log::warn!("LiDAR {}/{}/{}: {e}", t.z, t.x, t.y),
            }
        }
    }
    get(store, Layer::Dem, t)
}
