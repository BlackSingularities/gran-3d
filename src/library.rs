//! Biblioteka: co już leży na dysku (pokrycie w pamięci) i co trzeba dociągnąć (plan jako różnica).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};

use rayon::prelude::*;

use crate::areas::{layer_levels, wanted, Area, Level};
use crate::geo::{BBox, TileId};
use crate::pack::{PackKey, Slot};
use crate::store::Layer;

/// Stan kafli jednej paczki: po bicie na każdy z 256 kafli.
#[derive(Clone, Copy, Default)]
struct Bits {
    data: [u64; 4],
    missing: [u64; 4],
    lidar: [u64; 4],
}

fn bit(words: &[u64; 4], i: usize) -> bool {
    words[i / 64] & (1 << (i % 64)) != 0
}

fn set(words: &mut [u64; 4], i: usize) {
    words[i / 64] |= 1 << (i % 64);
}

fn clear(words: &mut [u64; 4], i: usize) {
    words[i / 64] &= !(1 << (i % 64));
}

/// Pokrycie dysku w pamięci: budowane raz w tle z indeksów paczek, potem aktualizowane przy zapisie.
pub struct Coverage {
    packs: RwLock<HashMap<PackKey, Bits>>,
    ready: AtomicBool,
    /// Kafle, dla których usługi nie mają LiDAR-u. Nie ma ich w paczkach, bo znacznik braku
    /// zablokowałby model globalny w tym samym miejscu – stąd osobny plik.
    no_lidar: RwLock<HashSet<TileId>>,
    no_lidar_file: PathBuf,
    append: Mutex<()>,
}

impl Coverage {
    pub fn new(no_lidar_file: PathBuf) -> Self {
        let no_lidar = fs::read_to_string(&no_lidar_file)
            .map(|s| s.lines().filter_map(parse_tile).collect())
            .unwrap_or_default();
        Self {
            packs: RwLock::default(),
            ready: AtomicBool::new(false),
            no_lidar: RwLock::new(no_lidar),
            no_lidar_file,
            append: Mutex::new(()),
        }
    }

    /// Czy kafel jest w bibliotece; `None`, dopóki indeksy nie są jeszcze wczytane.
    pub fn has(&self, layer: Layer, t: TileId) -> Option<bool> {
        if layer == Layer::Lidar && self.no_lidar.read().unwrap().contains(&t) {
            return Some(true);
        }
        if !self.ready.load(Ordering::Acquire) {
            return None;
        }
        let (key, i) = PackKey::of(layer.id(), t);
        let packs = self.packs.read().unwrap();
        Some(packs.get(&key).is_some_and(|b| match layer {
            Layer::Lidar => bit(&b.lidar, i),
            _ => bit(&b.data, i) || bit(&b.missing, i),
        }))
    }

    pub fn note(&self, layer: Layer, t: TileId, has_data: bool) {
        let (key, i) = PackKey::of(layer.id(), t);
        let mut packs = self.packs.write().unwrap();
        let b = packs.entry(key).or_default();
        if has_data {
            set(&mut b.data, i);
            clear(&mut b.missing, i);
            if layer == Layer::Lidar {
                set(&mut b.lidar, i);
            }
        } else {
            set(&mut b.missing, i);
        }
    }

    pub fn note_no_lidar(&self, t: TileId) {
        if !self.no_lidar.write().unwrap().insert(t) {
            return;
        }
        let _guard = self.append.lock().unwrap();
        let file = fs::OpenOptions::new().create(true).append(true).open(&self.no_lidar_file);
        if let Ok(mut f) = file {
            let _ = writeln!(f, "{} {} {}", t.z, t.x, t.y);
        }
    }

    pub fn remove_pack(&self, key: &PackKey) {
        self.packs.write().unwrap().remove(key);
    }

    /// Dołączenie stanu paczki z dysku. Zapisy z czasu skanowania są już w mapie, więc bity się sumują.
    pub fn merge(&self, key: PackKey, slots: &[(Slot, [u8; 2])]) {
        let mut b = Bits::default();
        for (i, (slot, head)) in slots.iter().enumerate() {
            match slot {
                Slot::Empty => {}
                Slot::Missing => set(&mut b.missing, i),
                Slot::Data { .. } => {
                    set(&mut b.data, i);
                    if crate::bake::is_lidar(head) {
                        set(&mut b.lidar, i);
                    }
                }
            }
        }
        let mut packs = self.packs.write().unwrap();
        let e = packs.entry(key).or_default();
        for w in 0..4 {
            e.data[w] |= b.data[w];
            e.missing[w] |= b.missing[w] & !e.data[w];
            e.lidar[w] |= b.lidar[w];
        }
    }

    pub fn mark_ready(&self) {
        self.ready.store(true, Ordering::Release);
    }
}

fn parse_tile(line: &str) -> Option<TileId> {
    let mut p = line.split_whitespace();
    Some(TileId::new(p.next()?.parse().ok()?, p.next()?.parse().ok()?, p.next()?.parse().ok()?))
}

/// Brakujące kafle jednej warstwy i poziomu.
pub struct PlanLayer {
    pub layer: Layer,
    pub z: u8,
    pub tiles: Vec<TileId>,
}

/// Plan pobrania: różnica między prostokątem a tym, co jest już w bibliotece.
#[derive(Default)]
pub struct Plan {
    pub layers: Vec<PlanLayer>,
    pub tiles_new: u64,
    pub bytes_new: u64,
    pub tiles_have: u64,
    pub bytes_have: u64,
}

impl Plan {
    /// Część obszaru obecna w bibliotece (0..1).
    pub fn fraction(&self) -> f32 {
        let total = self.tiles_new + self.tiles_have;
        if total == 0 {
            1.0
        } else {
            self.tiles_have as f32 / total as f32
        }
    }

    pub fn tiles(&self) -> impl Iterator<Item = (Layer, TileId)> + '_ {
        self.layers.iter().flat_map(|l| l.tiles.iter().map(move |t| (l.layer, *t)))
    }
}

/// Liczone równolegle (zasięg krajów i pokrycie dla każdego kafla) – wołać poza wątkiem interfejsu.
pub fn plan(bbox: &BBox, level: Level, has: impl Fn(Layer, TileId) -> bool + Sync) -> Plan {
    let parts: Vec<(Layer, u8, Vec<TileId>, u64)> = layer_levels(level)
        .into_par_iter()
        .map(|(layer, z)| {
            let all: Vec<TileId> = bbox.tiles(z).collect();
            let wanted: Vec<TileId> = all.into_par_iter().filter(|t| wanted(layer, *t)).collect();
            let total = wanted.len() as u64;
            let tiles: Vec<TileId> = wanted.into_par_iter().filter(|t| !has(layer, *t)).collect();
            let have = total - tiles.len() as u64;
            (layer, z, tiles, have)
        })
        .collect();
    let mut p = Plan::default();
    for (layer, z, tiles, have) in parts {
        let new = tiles.len() as u64;
        p.tiles_new += new;
        p.bytes_new += new * layer.avg_bytes(z);
        p.tiles_have += have;
        p.bytes_have += have * layer.avg_bytes(z);
        if new > 0 {
            p.layers.push(PlanLayer { layer, z, tiles });
        }
    }
    p
}

/// Zapisany obszar, który w całości zawiera prostokąt na tym samym lub wyższym poziomie.
pub fn covered_by<'a>(areas: &'a [Area], bbox: &BBox, level: Level) -> Option<&'a Area> {
    areas.iter().find(|a| {
        a.level >= level
            && a.bbox.w <= bbox.w
            && a.bbox.e >= bbox.e
            && a.bbox.s <= bbox.s
            && a.bbox.n >= bbox.n
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::areas::PRESETS;
    use crate::store::Store;

    fn in_bbox_level(b: BBox, l: Level) -> impl Fn(Layer, TileId) -> bool + Sync {
        move |layer, t| {
            layer_levels(l).contains(&(layer, t.z)) && {
                let (x0, y0, x1, y1) = b.tile_range(t.z);
                (x0..=x1).contains(&t.x) && (y0..=y1).contains(&t.y)
            }
        }
    }

    #[test]
    fn contained_area_needs_nothing() {
        let big = PRESETS[0].bbox;
        let small = BBox::new(big.w + 0.1, big.s + 0.05, big.e - 0.1, big.n - 0.05);
        let p = plan(&small, Level::Standard, in_bbox_level(big, Level::Detailed));
        assert_eq!(p.tiles_new, 0);
        assert!(p.tiles_have > 0);
        assert_eq!(p.fraction(), 1.0);
    }

    #[test]
    fn lidar_over_detailed_fetches_only_national_layers() {
        let b = PRESETS[0].bbox;
        let p = plan(&b, Level::Lidar, in_bbox_level(b, Level::Detailed));
        assert!(p.tiles_new > 0);
        assert!(p.layers.iter().all(|l| matches!(l.layer, Layer::Ortho | Layer::Lidar)));
    }

    #[test]
    fn covered_by_respects_level() {
        let big = PRESETS[0].bbox;
        let small = BBox::new(big.w + 0.1, big.s + 0.05, big.e - 0.1, big.n - 0.05);
        let areas = [Area::new("Tatry", big, Level::Standard)];
        assert!(covered_by(&areas, &small, Level::Standard).is_some());
        assert!(covered_by(&areas, &small, Level::Detailed).is_none());
        assert!(covered_by(&areas, &BBox::new(10.0, 40.0, 11.0, 41.0), Level::Preview).is_none());
    }

    #[test]
    fn coverage_is_rebuilt_from_disk() {
        let dir = std::env::temp_dir().join(format!("gran-library-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let (a, b, c, d) = (
            TileId::new(14, 9000, 5600),
            TileId::new(14, 9001, 5600),
            TileId::new(14, 9002, 5600),
            TileId::new(14, 9003, 5600),
        );
        let lidar = crate::bake::bake(Layer::Lidar, &vec![0u8; 256 * 256 * 4]).unwrap();
        {
            let s = Store::new(&dir);
            s.write(Layer::Dem, a, Some(&crate::bake::encode_dem(&[1.0; 4], 2, 2))).unwrap();
            s.write(Layer::Dem, b, None).unwrap();
            s.write(Layer::Lidar, c, Some(&lidar)).unwrap();
            s.note_no_lidar(d);
            s.release_all();
        }
        let s = Store::new(&dir);
        s.scan();
        assert!(s.has(Layer::Dem, a) && !s.has(Layer::Lidar, a));
        assert!(s.has(Layer::Dem, b) && !s.has(Layer::Lidar, b));
        assert!(s.has(Layer::Dem, c) && s.has(Layer::Lidar, c));
        assert!(!s.has(Layer::Dem, d) && s.has(Layer::Lidar, d));
        assert!(!s.has(Layer::Sat, a));
        s.release_all();
        let _ = fs::remove_dir_all(dir);
    }
}
