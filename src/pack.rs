//! Paczki kafli („GRAN pack”): 16×16 kafli jednego poziomu w jednym pliku z indeksem.
//!
//! Windows płaci słono za każdy mały plik (otwarcie, NTFS, antywirus), a model terenu Europy to
//! setki tysięcy kafli. Paczka trzyma do 256 kafli i ma stały nagłówek z indeksem, więc odczyt
//! kafla to jeden odczyt pozycyjny z otwartego już pliku.
//!
//! ```text
//! 0     "GRANPK01"                       magia i wersja
//! 8     u8 warstwa, u8 poziom, 2 B zapas, u32 bx, u32 by
//! 20    12 B zapas
//! 32    256 × { u64 offset, u32 długość, u32 crc32 }    indeks (wiersz po wierszu)
//! 4128  dane kafli dopisywane na końcu
//! ```
//! Długość 0 z offsetem 1 oznacza „źródło nie ma tego kafla” – nie pytamy o niego ponownie.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Once, RwLock};

use crate::geo::TileId;

pub const SIDE: u32 = 16;
const SLOTS: usize = (SIDE * SIDE) as usize;
const MAGIC: &[u8; 8] = b"GRANPK01";
const INDEX_AT: u64 = 32;
const ENTRY: usize = 16;
pub const DATA_AT: u64 = INDEX_AT + (SLOTS * ENTRY) as u64;
const MISSING_OFFSET: u64 = 1;

/// Klucz paczki: warstwa (numer), poziom i blok 16×16.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PackKey {
    pub layer: u8,
    pub z: u8,
    pub bx: u32,
    pub by: u32,
}

impl PackKey {
    pub fn of(layer: u8, t: TileId) -> (PackKey, usize) {
        let key = PackKey {
            layer,
            z: t.z,
            bx: t.x / SIDE,
            by: t.y / SIDE,
        };
        (key, ((t.y % SIDE) * SIDE + (t.x % SIDE)) as usize)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Empty,
    Missing,
    Data { offset: u64, len: u32, crc: u32 },
}

fn read_at(f: &File, buf: &mut [u8], offset: u64) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let mut done = 0;
        while done < buf.len() {
            let n = f.seek_read(&mut buf[done..], offset + done as u64)?;
            if n == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            done += n;
        }
        Ok(())
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        f.read_exact_at(buf, offset)
    }
}

fn write_at(f: &File, buf: &[u8], offset: u64) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let mut done = 0;
        while done < buf.len() {
            done += f.seek_write(&buf[done..], offset + done as u64)?;
        }
        Ok(())
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        f.write_all_at(buf, offset)
    }
}

/// Otwarta paczka: plik, indeks w pamięci i koniec danych. Odczyty są pozycyjne i idą równolegle,
/// zapisy szeregują się na `end`. Indeks wczytuje pierwszy użytkownik po włożeniu paczki do mapy –
/// dzięki temu nigdy nie zastąpi nowszych wpisów starszym odczytem z dysku.
struct Open {
    file: File,
    ready: Once,
    index: RwLock<Vec<Slot>>,
    end: Mutex<u64>,
    used: AtomicU64,
}

impl Open {
    fn new(file: File) -> Self {
        Self {
            file,
            ready: Once::new(),
            index: RwLock::new(vec![Slot::Empty; SLOTS]),
            end: Mutex::new(DATA_AT),
            used: AtomicU64::new(0),
        }
    }

    fn load(&self) {
        self.ready.call_once(|| {
            let mut raw = vec![0u8; SLOTS * ENTRY];
            if read_at(&self.file, &mut raw, INDEX_AT).is_ok() {
                let mut index = self.index.write().unwrap();
                for (slot, e) in index.iter_mut().zip(raw.chunks_exact(ENTRY)) {
                    *slot = parse_entry(e);
                }
            }
            let len = self.file.metadata().map(|m| m.len()).unwrap_or(0);
            *self.end.lock().unwrap() = len.max(DATA_AT);
        });
    }
}

fn parse_entry(e: &[u8]) -> Slot {
    let offset = u64::from_le_bytes(e[0..8].try_into().unwrap());
    let len = u32::from_le_bytes(e[8..12].try_into().unwrap());
    let crc = u32::from_le_bytes(e[12..16].try_into().unwrap());
    match (offset, len) {
        (0, _) => Slot::Empty,
        (MISSING_OFFSET, 0) => Slot::Missing,
        _ => Slot::Data { offset, len, crc },
    }
}

/// Zbiór paczek pod jednym katalogiem z pamięcią otwartych plików.
pub struct Packs {
    root: PathBuf,
    open: Mutex<HashMap<PackKey, Arc<Open>>>,
    max_open: usize,
    clock: AtomicU64,
    /// Tworzenie pliku po jednym wątku: spóźniony nagłówek skasowałby indeks świeżo zapisanych kafli.
    creating: Mutex<()>,
}

const MAX_OPEN: usize = 1024;

impl Packs {
    pub fn new(root: PathBuf) -> Self {
        Self::with_limit(root, MAX_OPEN)
    }

    fn with_limit(root: PathBuf, max_open: usize) -> Self {
        Self {
            root,
            open: Mutex::new(HashMap::new()),
            max_open,
            clock: AtomicU64::new(0),
            creating: Mutex::new(()),
        }
    }

    pub fn path(&self, key: &PackKey, layer_dir: &str) -> PathBuf {
        self.root
            .join(layer_dir)
            .join(key.z.to_string())
            .join(format!("{}_{}.gpk", key.bx, key.by))
    }

    fn get(&self, key: PackKey, layer_dir: &str, create: bool) -> io::Result<Option<Arc<Open>>> {
        let cached = self.open.lock().unwrap().get(&key).cloned();
        if let Some(o) = cached {
            o.used.store(self.clock.fetch_add(1, Ordering::Relaxed), Ordering::Relaxed);
            o.load();
            return Ok(Some(o));
        }
        let path = self.path(&key, layer_dir);
        let file = if create {
            if let Some(d) = path.parent() {
                fs::create_dir_all(d)?;
            }
            let _creating = self.creating.lock().unwrap();
            let f = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)?;
            if f.metadata()?.len() < DATA_AT {
                let mut head = vec![0u8; DATA_AT as usize];
                head[..8].copy_from_slice(MAGIC);
                head[8] = key.layer;
                head[9] = key.z;
                head[12..16].copy_from_slice(&key.bx.to_le_bytes());
                head[16..20].copy_from_slice(&key.by.to_le_bytes());
                write_at(&f, &head, 0)?;
            }
            f
        } else {
            match OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .or_else(|_| File::open(&path))
            {
                Ok(f) => f,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e),
            }
        };
        let mut magic = [0u8; 8];
        read_at(&file, &mut magic, 0)?;
        if &magic != MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("uszkodzona paczka {}", path.display()),
            ));
        }
        let o = {
            let mut map = self.open.lock().unwrap();
            if map.len() >= self.max_open && !map.contains_key(&key) {
                evict(&mut map, self.max_open);
            }
            map.entry(key).or_insert_with(|| Arc::new(Open::new(file))).clone()
        };
        o.used.store(self.clock.fetch_add(1, Ordering::Relaxed), Ordering::Relaxed);
        o.load();
        Ok(Some(o))
    }

    /// Stan kafla bez czytania danych.
    pub fn state(&self, layer: u8, layer_dir: &str, t: TileId) -> Slot {
        let (key, i) = PackKey::of(layer, t);
        match self.get(key, layer_dir, false) {
            Ok(Some(o)) => o.index.read().unwrap()[i],
            _ => Slot::Empty,
        }
    }

    /// Dane kafla (sprawdzone sumą CRC); `None` gdy brak albo uszkodzony.
    pub fn read(&self, layer: u8, layer_dir: &str, t: TileId) -> Option<Vec<u8>> {
        let (key, i) = PackKey::of(layer, t);
        let o = self.get(key, layer_dir, false).ok()??;
        let slot = o.index.read().unwrap()[i];
        let Slot::Data { offset, len, crc } = slot else {
            return None;
        };
        let mut buf = vec![0u8; len as usize];
        read_at(&o.file, &mut buf, offset).ok()?;
        (crc32fast::hash(&buf) == crc).then_some(buf)
    }

    /// Zapis kafla: dane na koniec pliku, potem wpis w indeksie (przerwany zapis zostawia stary wpis).
    pub fn write(
        &self,
        layer: u8,
        layer_dir: &str,
        t: TileId,
        data: Option<&[u8]>,
    ) -> io::Result<()> {
        let (key, i) = PackKey::of(layer, t);
        let o = self.get(key, layer_dir, true)?.expect("paczka utworzona");
        let mut end = o.end.lock().unwrap();
        let mut e = [0u8; ENTRY];
        match data {
            Some(d) => {
                write_at(&o.file, d, *end)?;
                e[0..8].copy_from_slice(&end.to_le_bytes());
                e[8..12].copy_from_slice(&(d.len() as u32).to_le_bytes());
                e[12..16].copy_from_slice(&crc32fast::hash(d).to_le_bytes());
                *end += d.len() as u64;
            }
            None => e[0..8].copy_from_slice(&MISSING_OFFSET.to_le_bytes()),
        }
        write_at(&o.file, &e, INDEX_AT + (i * ENTRY) as u64)?;
        o.index.write().unwrap()[i] = parse_entry(&e);
        Ok(())
    }

    /// Zamknięcie paczki (przed usunięciem pliku przez sprzątacza).
    pub fn forget(&self, key: &PackKey) {
        self.open.lock().unwrap().remove(key);
    }

    /// Zamknięcie wszystkich paczek (testy; pliki można potem usunąć).
    #[cfg(test)]
    pub fn forget_all(&self) {
        self.open.lock().unwrap().clear();
    }
}

/// Zamknięcie najdawniej używanych paczek. Paczki trzymanej przez inny wątek nie wolno zamknąć:
/// kolejne otwarcie dałoby drugi egzemplarz z własną blokadą zapisu i dwa wątki nadpisałyby sobie dane.
/// Nowe odwołania powstają tylko pod blokadą mapy, więc licznik 1 oznacza, że paczka jest wolna.
fn evict(map: &mut HashMap<PackKey, Arc<Open>>, limit: usize) {
    let mut idle: Vec<(u64, PackKey)> = map
        .iter()
        .filter(|(_, o)| Arc::strong_count(o) == 1)
        .map(|(k, o)| (o.used.load(Ordering::Relaxed), *k))
        .collect();
    idle.sort_unstable_by_key(|e| e.0);
    for (_, k) in idle.into_iter().take((limit / 4).max(1)) {
        map.remove(&k);
    }
}

/// Paczki leżące w katalogu warstwy: (klucz, ścieżka).
pub fn list(root: &Path, layer: u8, layer_dir: &str) -> Vec<(PackKey, PathBuf)> {
    let mut out = Vec::new();
    let Ok(zs) = fs::read_dir(root.join(layer_dir)) else {
        return out;
    };
    for z in zs.flatten() {
        let Some(zv) = z.file_name().to_str().and_then(|s| s.parse::<u8>().ok()) else {
            continue;
        };
        let Ok(files) = fs::read_dir(z.path()) else {
            continue;
        };
        for f in files.flatten() {
            let name = f.file_name();
            let Some(stem) = name.to_str().and_then(|s| s.strip_suffix(".gpk")) else {
                continue;
            };
            let Some((bx, by)) = stem.split_once('_') else {
                continue;
            };
            if let (Ok(bx), Ok(by)) = (bx.parse(), by.parse()) {
                out.push((
                    PackKey {
                        layer,
                        z: zv,
                        bx,
                        by,
                    },
                    f.path(),
                ));
            }
        }
    }
    out
}

/// Zapis całego pliku na raz (używany przez narzędzia i testy).
#[allow(dead_code)]
pub fn write_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut f = File::create(path)?;
    f.write_all(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_missing() {
        let dir = std::env::temp_dir().join(format!("gran-pack-test-{}", std::process::id()));
        let packs = Packs::new(dir.clone());
        let t = TileId::new(12, 2270, 1408);
        assert_eq!(packs.state(0, "dem", t), Slot::Empty);
        packs.write(0, "dem", t, Some(b"teren")).unwrap();
        assert_eq!(packs.read(0, "dem", t).as_deref(), Some(&b"teren"[..]));
        let t2 = TileId::new(12, 2271, 1408);
        packs.write(0, "dem", t2, None).unwrap();
        assert_eq!(packs.state(0, "dem", t2), Slot::Missing);
        // nadpisanie: nowe dane wygrywają
        packs.write(0, "dem", t, Some(b"nowy teren")).unwrap();
        assert_eq!(packs.read(0, "dem", t).as_deref(), Some(&b"nowy teren"[..]));
        packs.forget_all();
        let _ = fs::remove_dir_all(dir);
    }

    /// Wiele wątków, mało uchwytów: paczki są ciągle wymieniane, a żaden kafel nie może zginąć.
    #[test]
    fn concurrent_writes_survive_eviction() {
        let dir = std::env::temp_dir().join(format!("gran-pack-race-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let packs = Packs::with_limit(dir.clone(), 2);
        let tile = |n: u32| TileId::new(12, 2000 + n % 64, 1400 + n / 64);
        let payload = |n: u32| format!("kafel-{n}-").repeat(1 + (n % 7) as usize).into_bytes();
        const TILES: u32 = 64 * 16;
        std::thread::scope(|s| {
            for w in 0..8u32 {
                let (packs, tile, payload) = (&packs, &tile, &payload);
                s.spawn(move || {
                    for n in (0..TILES).filter(|n| n % 8 == w) {
                        packs.write(0, "dem", tile(n), Some(&payload(n))).unwrap();
                        assert!(packs.read(0, "dem", tile(n)).is_some());
                    }
                });
            }
        });
        for n in 0..TILES {
            assert_eq!(packs.read(0, "dem", tile(n)), Some(payload(n)), "kafel {n}");
        }
        // po zamknięciu wszystkiego indeks wraca z dysku
        packs.forget_all();
        for n in 0..TILES {
            assert_eq!(packs.read(0, "dem", tile(n)), Some(payload(n)), "kafel {n} z dysku");
        }
        packs.forget_all();
        let _ = fs::remove_dir_all(dir);
    }
}
