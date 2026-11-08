//! Ustawienia programu: folder danych, limit rozmiaru, aktualizacje.
//! Plik `%APPDATA%\GRAN\gran.json`; przy pierwszym starcie folder i limit są podpowiadane
//! z ustawień poprzedniej wersji (`settings.json` wersji 1.x).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DATA_SUBDIR: &str = "GRAN-dane";
pub const LIMIT_STEPS_GB: [u32; 16] = [
    1, 2, 3, 5, 8, 10, 15, 20, 30, 50, 75, 100, 150, 200, 300, 500,
];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub data_dir: PathBuf,
    pub limit_gb: u32,
    #[serde(default = "yes")]
    pub auto_update: bool,
    /// Kreator pierwszego uruchomienia zakończony.
    #[serde(default)]
    pub setup_done: bool,
    /// Synchronizacja klatek z monitorem (zmiana działa po ponownym uruchomieniu).
    #[serde(default = "yes")]
    pub vsync: bool,
    /// Limit zużycia zasobów: 0 oszczędny … 3 maksymalny.
    #[serde(default = "res_level")]
    pub resources: u8,
}

fn res_level() -> u8 {
    crate::limits::DEFAULT_LEVEL
}

fn yes() -> bool {
    true
}

pub fn config_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("GRAN")
}

pub fn default_data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(config_dir)
        .join("GRAN")
        .join(DATA_SUBDIR)
}

/// Folder danych w wybranym miejscu: zawsze własny podfolder, żeby nie mieszać z plikami użytkownika.
pub fn data_dir_in(picked: &Path) -> PathBuf {
    // wskazany folder z danymi (także wersji 1.x albo przemianowany) przyjmujemy bez zmian
    let has_data = ["areas.json", "tiles", ".cache"]
        .iter()
        .any(|n| picked.join(n).exists());
    if has_data || picked.file_name().is_some_and(|n| n == DATA_SUBDIR) {
        picked.to_path_buf()
    } else {
        picked.join(DATA_SUBDIR)
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            data_dir: default_data_dir(),
            limit_gb: 10,
            auto_update: true,
            setup_done: false,
            vsync: true,
            resources: res_level(),
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let file = config_dir().join("gran.json");
        if let Some(s) = fs::read(&file)
            .ok()
            .and_then(|b| serde_json::from_slice::<Settings>(&b).ok())
        {
            return s;
        }
        // podpowiedź z wersji 1.x (Electron)
        let mut s = Settings::default();
        if let Some(v) = fs::read(config_dir().join("settings.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        {
            if let Some(d) = v.get("dataDir").and_then(|d| d.as_str()) {
                s.data_dir = PathBuf::from(d);
            }
            if let Some(l) = v.get("limitGB").and_then(|l| l.as_u64()) {
                s.limit_gb = (l as u32).clamp(1, 500);
            }
        }
        s
    }

    pub fn save(&self) -> std::io::Result<()> {
        let dir = config_dir();
        fs::create_dir_all(&dir)?;
        fs::write(
            dir.join("gran.json"),
            serde_json::to_vec_pretty(self).unwrap_or_default(),
        )?;
        // ścieżka danych dla deinstalatora (UTF-16 LE z BOM)
        let mut bytes = vec![0xff, 0xfe];
        for u in self.data_dir.to_string_lossy().encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        fs::write(dir.join("datadir.txt"), bytes)
    }

    pub fn limit_bytes(&self) -> u64 {
        self.limit_gb as u64 * 1024 * 1024 * 1024
    }
}
