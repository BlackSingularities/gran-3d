//! Obszary zapisane do pracy bez internetu: prostokąt, poziom szczegółów i stan pobrania.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::geo::{BBox, TileId};
use crate::store::{Layer, BASE_MAX_Z};

/// Poziom szczegółów pobieranego obszaru.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Level {
    /// Teren ~76 m, zdjęcia ~38 m – szybki podgląd dużego obszaru.
    Preview,
    /// Teren ~19 m, zdjęcia ~10 m (pełna rozdzielczość Sentinel-2).
    Standard,
    /// Teren ~10 m (najlepszy model globalny), zdjęcia ~5 m.
    Detailed,
    /// Teren LiDAR 3 m (Polska, Czechy) i ortofotomapa ~1 m (PL, CZ, SK).
    Lidar,
}

impl Level {
    pub const ALL: [Level; 4] = [Level::Preview, Level::Standard, Level::Detailed, Level::Lidar];

    pub fn name(self) -> &'static str {
        match self {
            Level::Preview => "Podgląd",
            Level::Standard => "Standard",
            Level::Detailed => "Szczegółowy",
            Level::Lidar => "LiDAR i ortofoto",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Level::Preview => "teren ok. 75 m, zdjęcia ok. 40 m – szybko, mało miejsca",
            Level::Standard => "teren ok. 20 m, zdjęcia satelitarne 10 m – zalecany",
            Level::Detailed => "teren ok. 10 m, zdjęcia powiększone do 5 m",
            Level::Lidar => "LiDAR 3 m (PL, CZ) i ortofotomapa ok. 1 m (PL, CZ, SK) – najwięcej miejsca",
        }
    }

    /// Najwyższy poziom kafli danej warstwy.
    pub fn max_z(self, layer: Layer) -> u8 {
        match (self, layer) {
            (Level::Preview, Layer::Dem) => 10,
            (Level::Preview, Layer::Sat) => 11,
            (Level::Standard, Layer::Dem) => 12,
            (Level::Standard, Layer::Sat) => 13,
            (Level::Detailed, Layer::Dem) => 13,
            (Level::Detailed | Level::Lidar, Layer::Sat) => 14,
            (Level::Lidar, Layer::Dem) => 13,
            (Level::Lidar, Layer::Ortho) => 15,
            (Level::Lidar, Layer::Lidar) => 15,
            (_, Layer::Ortho | Layer::Lidar) => 0,
        }
    }
}

/// Poziomy od których obszar ma własne kafle (niżej wystarcza model globalny).
pub const AREA_MIN_Z: u8 = BASE_MAX_Z + 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Area {
    pub id: String,
    pub name: String,
    pub bbox: BBox,
    pub level: Level,
    /// Liczba pobranych kafli przy ostatnim zakończonym pobieraniu (0 = jeszcze nie pobrany).
    #[serde(default)]
    pub tiles_done: u64,
    #[serde(default)]
    pub complete: bool,
    #[serde(default)]
    pub created: u64,
}

impl Area {
    pub fn new(name: &str, bbox: BBox, level: Level) -> Self {
        let created = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let slug: String = name
            .to_lowercase()
            .chars()
            .map(|c| match c {
                'ą' => 'a',
                'ć' => 'c',
                'ę' => 'e',
                'ł' => 'l',
                'ń' => 'n',
                'ó' => 'o',
                'ś' => 's',
                'ź' | 'ż' => 'z',
                c if c.is_ascii_alphanumeric() => c,
                _ => '-',
            })
            .collect();
        Self {
            id: format!("{}-{created:x}", slug.trim_matches('-')),
            name: name.to_string(),
            bbox,
            level,
            tiles_done: 0,
            complete: false,
            created,
        }
    }

    /// Wszystkie kafle obszaru (warstwa, kafel) od [`AREA_MIN_Z`] do poziomu szczegółów.
    pub fn tiles(&self) -> impl Iterator<Item = (Layer, TileId)> + '_ {
        let base = Layer::ALL.into_iter().flat_map(move |layer| {
            (AREA_MIN_Z..=self.level.max_z(layer))
                .flat_map(move |z| self.bbox.tiles(z).map(move |t| (layer, t)))
        });
        // dane krajowe tylko tam, gdzie sięgają usługi
        let national = [(Layer::Ortho, crate::national::ORTHO_MIN_Z), (Layer::Lidar, 14u8)]
            .into_iter()
            .filter(move |_| self.level == Level::Lidar)
            .flat_map(move |(layer, z0)| {
                (z0..=self.level.max_z(layer)).flat_map(move |z| {
                    self.bbox.tiles(z).filter(move |t| match layer {
                        Layer::Ortho => crate::national::has_ortho(*t),
                        _ => crate::national::has_lidar(*t),
                    })
                    .map(move |t| (layer, t))
                })
            });
        base.chain(national)
    }

    pub fn estimate(&self) -> Estimate {
        estimate(&self.bbox, self.level)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Estimate {
    pub tiles: u64,
    pub bytes: u64,
}

impl std::ops::Add for Estimate {
    type Output = Estimate;
    fn add(self, o: Estimate) -> Estimate {
        Estimate {
            tiles: self.tiles + o.tiles,
            bytes: self.bytes + o.bytes,
        }
    }
}

pub fn estimate(bbox: &BBox, level: Level) -> Estimate {
    let mut e = Estimate::default();
    for layer in Layer::ALL {
        for z in AREA_MIN_Z..=level.max_z(layer) {
            let n = bbox.tile_count(z);
            e.tiles += n;
            e.bytes += n * layer.avg_bytes(z);
        }
    }
    if level == Level::Lidar {
        // szacunek bez sprawdzania zasięgu krajów (górna granica)
        for (layer, z0) in [(Layer::Ortho, crate::national::ORTHO_MIN_Z), (Layer::Lidar, 14u8)] {
            for z in z0..=level.max_z(layer) {
                let n = bbox.tile_count(z);
                e.tiles += n;
                e.bytes += n * layer.avg_bytes(z);
            }
        }
    }
    e
}

/// Model globalny (Europa do poziomu [`BASE_MAX_Z`]) – pobierany zawsze przy pierwszym uruchomieniu.
pub const EUROPE: BBox = BBox::new(-11.0, 34.0, 35.0, 71.5);

pub fn base_tiles() -> impl Iterator<Item = (Layer, TileId)> {
    Layer::ALL.into_iter().flat_map(|layer| {
        (2..=BASE_MAX_Z).flat_map(move |z| EUROPE.tiles(z).map(move |t| (layer, t)))
    })
}

/// Gotowe propozycje obszarów (pasma górskie Polski i sąsiadów).
pub struct Preset {
    pub name: &'static str,
    pub country: &'static str,
    pub bbox: BBox,
}

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "Tatry",
        country: "PL · SK",
        bbox: BBox::new(19.70, 49.08, 20.35, 49.35),
    },
    Preset {
        name: "Pieniny",
        country: "PL · SK",
        bbox: BBox::new(20.25, 49.36, 20.60, 49.47),
    },
    Preset {
        name: "Gorce",
        country: "PL",
        bbox: BBox::new(19.95, 49.48, 20.35, 49.62),
    },
    Preset {
        name: "Beskid Sądecki",
        country: "PL",
        bbox: BBox::new(20.50, 49.35, 20.95, 49.55),
    },
    Preset {
        name: "Beskid Żywiecki",
        country: "PL · SK",
        bbox: BBox::new(19.10, 49.45, 19.70, 49.65),
    },
    Preset {
        name: "Beskid Śląski",
        country: "PL",
        bbox: BBox::new(18.75, 49.55, 19.15, 49.75),
    },
    Preset {
        name: "Bieszczady",
        country: "PL · SK · UA",
        bbox: BBox::new(22.30, 49.00, 22.90, 49.30),
    },
    Preset {
        name: "Karkonosze",
        country: "PL · CZ",
        bbox: BBox::new(15.40, 50.65, 16.00, 50.85),
    },
    Preset {
        name: "Góry Stołowe",
        country: "PL · CZ",
        bbox: BBox::new(16.20, 50.40, 16.55, 50.55),
    },
    Preset {
        name: "Masyw Śnieżnika",
        country: "PL · CZ",
        bbox: BBox::new(16.70, 50.12, 16.98, 50.28),
    },
    Preset {
        name: "Mała Fatra",
        country: "SK",
        bbox: BBox::new(18.85, 49.10, 19.20, 49.25),
    },
    Preset {
        name: "Niskie Tatry",
        country: "SK",
        bbox: BBox::new(19.30, 48.82, 19.95, 49.02),
    },
];

pub const MAX_AREA_KM2: f64 = 10_000.0;

pub fn load(dir: &Path) -> Vec<Area> {
    fs::read(dir.join("areas.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save(dir: &Path, areas: &[Area]) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let tmp = dir.join("areas.json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(areas).unwrap_or_default())?;
    fs::rename(tmp, dir.join("areas.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tatry_standard_is_reasonable() {
        let e = estimate(&PRESETS[0].bbox, Level::Standard);
        // kilkaset kafli, kilka–kilkanaście MB
        assert!(e.tiles > 100 && e.tiles < 2_000, "{}", e.tiles);
        assert!(e.bytes > 1 << 20 && e.bytes < 100 << 20, "{}", e.bytes);
    }

    #[test]
    fn slug_is_ascii() {
        assert!(Area::new("Góry Stołowe", PRESETS[8].bbox, Level::Preview)
            .id
            .starts_with("gory-stolowe-"));
    }
}
