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
use std::sync::{Arc, Mutex};

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

/// Otwarta paczka: plik i blokada zapisu (odczyty są pozycyjne i idą równolegle).
struct Open {
    file: File,
    write: Mutex<()>,
}

/// Zbiór paczek pod jednym katalogiem z pamięcią otwartych plików.
pub struct Packs {
    root: PathBuf,
    open: Mutex<HashMap<PackKey, Arc<Open>>>,
}

const MAX_OPEN: usize = 192;

impl Packs {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            open: Mutex::new(HashMap::new()),
        }
    }

    pub fn path(&self, key: &PackKey, layer_dir: &str) -> PathBuf {
        self.root
            .join(layer_dir)
            .join(key.z.to_string())
            .join(format!("{}_{}.gpk", key.bx, key.by))
    }

    fn get(&self, key: PackKey, layer_dir: &str, create: bool) -> io::Result<Option<Arc<Open>>> {
        if let Some(o) = self.open.lock().unwrap().get(&key) {
            return Ok(Some(o.clone()));
        }
        let path = self.path(&key, layer_dir);
        let file = if create {
            if let Some(d) = path.parent() {
                fs::create_dir_all(d)?;
            }
            let new = !path.exists();
            let f = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)?;
            if new || f.metadata()?.len() < DATA_AT {
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
        let o = Arc::new(Open {
            file,
            write: Mutex::new(()),
        });
        let mut map = self.open.lock().unwrap();
        if map.len() >= MAX_OPEN {
            map.clear();
        }
        Ok(Some(map.entry(key).or_insert(o).clone()))
    }

    fn slot(o: &Open, i: usize) -> io::Result<Slot> {
        let mut e = [0u8; ENTRY];
        read_at(&o.file, &mut e, INDEX_AT + (i * ENTRY) as u64)?;
        let offset = u64::from_le_bytes(e[0..8].try_into().unwrap());
        let len = u32::from_le_bytes(e[8..12].try_into().unwrap());
        let crc = u32::from_le_bytes(e[12..16].try_into().unwrap());
        Ok(match (offset, len) {
            (0, _) => Slot::Empty,
            (MISSING_OFFSET, 0) => Slot::Missing,
            _ => Slot::Data { offset, len, crc },
        })
    }

    /// Stan kafla bez czytania danych.
    pub fn state(&self, layer: u8, layer_dir: &str, t: TileId) -> Slot {
        let (key, i) = PackKey::of(layer, t);
        match self.get(key, layer_dir, false) {
            Ok(Some(o)) => Self::slot(&o, i).unwrap_or(Slot::Empty),
            _ => Slot::Empty,
        }
    }

    /// Dane kafla (sprawdzone sumą CRC); `None` gdy brak albo uszkodzony.
    pub fn read(&self, layer: u8, layer_dir: &str, t: TileId) -> Option<Vec<u8>> {
        let (key, i) = PackKey::of(layer, t);
        let o = self.get(key, layer_dir, false).ok()??;
        let Slot::Data { offset, len, crc } = Self::slot(&o, i).ok()? else {
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
        let _guard = o.write.lock().unwrap();
        let entry = match data {
            Some(d) => {
                let offset = o.file.metadata()?.len().max(DATA_AT);
                write_at(&o.file, d, offset)?;
                let mut e = [0u8; ENTRY];
                e[0..8].copy_from_slice(&offset.to_le_bytes());
                e[8..12].copy_from_slice(&(d.len() as u32).to_le_bytes());
                e[12..16].copy_from_slice(&crc32fast::hash(d).to_le_bytes());
                e
            }
            None => {
                let mut e = [0u8; ENTRY];
                e[0..8].copy_from_slice(&MISSING_OFFSET.to_le_bytes());
                e
            }
        };
        write_at(&o.file, &entry, INDEX_AT + (i * ENTRY) as u64)
    }

    /// Zamknięcie paczki (przed usunięciem pliku przez sprzątacza).
    pub fn forget(&self, key: &PackKey) {
        self.open.lock().unwrap().remove(key);
    }

    #[cfg(test)]
    pub fn forget_all(&self) {
        self.open.lock().unwrap().clear();
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
}
