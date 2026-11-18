//! Aktualizacje przez redzikowski.com: serwer strony udostępnia kopię GitHub Releases w tym samym
//! formacie i pośredniczy w pobieraniu plików. Plik (instalator Windows `GRAN-Setup-<wersja>.exe`
//! albo `GRAN-<wersja>.apk` na Androida) jest sprawdzany sumą SHA-512 z `latest.yml` tego samego
//! wydania. Windows: instalator z podniesieniem uprawnień (UAC) w trybie cichym; Android: okno
//! instalatora systemu.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use sha2::{Digest, Sha512};

use crate::download::agent;

const FEED: &str = "https://redzikowski.com/api/releases/repos/BlackSingularities/gran-3d/releases/latest";

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub exe_name: String,
    pub exe_url: String,
    pub size: u64,
    yml_url: Option<String>,
    pub notes: String,
}

/// Porównanie wersji x.y.z: dodatnie, gdy a > b.
pub fn cmp_version(a: &str, b: &str) -> i64 {
    let p = |s: &str| -> Vec<i64> {
        s.trim_start_matches('v')
            .split(['.', '-'])
            .take(3)
            .map(|n| n.parse().unwrap_or(0))
            .collect()
    };
    let (pa, pb) = (p(a), p(b));
    for i in 0..3 {
        let (x, y) = (pa.get(i).copied().unwrap_or(0), pb.get(i).copied().unwrap_or(0));
        if x != y {
            return x - y;
        }
    }
    0
}

pub fn current() -> &'static str {
    crate::app_version()
}

/// Prefiks tagów osobnych wydań Androida (`android-v1.0.0`).
const ANDROID_TAG: &str = "android-v";

/// Wersja z tagu wydania tej platformy (`v4.0.1` na Windows, `android-v1.0.0` na Androidzie);
/// `None` – wydanie innej platformy.
pub fn tag_version(tag: &str) -> Option<&str> {
    if crate::platform::MOBILE {
        tag.strip_prefix(ANDROID_TAG)
    } else {
        tag.strip_prefix('v').filter(|v| v.starts_with(|c: char| c.is_ascii_digit()))
    }
}

pub fn latest() -> Result<Release, String> {
    let get = |url: &str| -> Result<serde_json::Value, String> {
        let body = agent().get(url).timeout(Duration::from_secs(20)).call().map_err(|e| format!("redzikowski.com: {e}"))?.into_string().map_err(|e| e.to_string())?;
        serde_json::from_str(&body).map_err(|e| e.to_string())
    };
    // Windows: najnowsze wydanie główne; Android: najnowsze z listy z tagiem `android-v…`
    let v = if crate::platform::MOBILE {
        let list = get(&format!("{}?per_page=30", FEED.trim_end_matches("/latest")))?;
        list.as_array()
            .and_then(|l| l.iter().find(|r| !r["prerelease"].as_bool().unwrap_or(false) && r["tag_name"].as_str().is_some_and(|t| tag_version(t).is_some())).cloned())
            .ok_or(crate::i18n::tr("Brak wydania dla tego systemu."))?
    } else {
        get(FEED)?
    };
    let tag = v["tag_name"].as_str().unwrap_or("");
    let version = tag_version(tag).unwrap_or(tag.trim_start_matches('v')).to_string();
    let empty = Vec::new();
    let assets = v["assets"].as_array().unwrap_or(&empty);
    let exe = assets
        .iter()
        .find(|a| is_package(a["name"].as_str().unwrap_or("")))
        .ok_or(crate::i18n::tr("Wydanie nie zawiera instalatora."))?;
    let yml = assets.iter().find(|a| a["name"] == "latest.yml");
    Ok(Release {
        version,
        exe_name: exe["name"].as_str().unwrap_or("").to_string(),
        exe_url: exe["browser_download_url"].as_str().unwrap_or("").to_string(),
        size: exe["size"].as_u64().unwrap_or(0),
        yml_url: yml.and_then(|y| y["browser_download_url"].as_str().map(String::from)),
        notes: v["body"].as_str().unwrap_or("").to_string(),
    })
}

/// Plik aktualizacji dla tego systemu.
pub fn is_package(name: &str) -> bool {
    let n = name.to_lowercase();
    if crate::platform::MOBILE {
        n.starts_with("gran-") && !n.starts_with("gran-setup-") && n.ends_with(".apk")
    } else {
        n.starts_with("gran-setup-") && n.ends_with(".exe")
    }
}

/// Suma SHA-512 (base64) instalatora z `latest.yml`.
fn expected_sha(rel: &Release) -> Result<String, String> {
    let url = rel.yml_url.as_ref().ok_or(crate::i18n::tr("Wydanie nie zawiera pliku latest.yml z sumą kontrolną."))?;
    let text = agent().get(url).call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())?;
    parse_sha(&text, &rel.exe_name).ok_or_else(|| crate::i18n::tr("Brak sumy SHA-512 w latest.yml.").into())
}

pub fn parse_sha(yml: &str, name: &str) -> Option<String> {
    // blok „- url: <nazwa>” w sekcji files, a w razie czego główne pole sha512
    let block = yml
        .split("- url:")
        .find(|b| b.trim_start().starts_with(name))
        .unwrap_or(yml);
    block
        .lines()
        .find_map(|l| l.trim().strip_prefix("sha512:"))
        .map(|s| s.trim().to_string())
}

/// Pobranie instalatora do katalogu tymczasowego (postęp w bajtach) i sprawdzenie sumy.
pub fn download(rel: &Release, progress: Arc<AtomicU64>) -> Result<PathBuf, String> {
    let sha = expected_sha(rel)?;
    let dir = std::env::temp_dir().join("GRAN-aktualizacja");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join(&rel.exe_name);
    let r = agent()
        .get(&rel.exe_url)
        .timeout(Duration::from_secs(1800))
        .call()
        .map_err(|e| e.to_string())?;
    let mut reader = r.into_reader();
    let mut out = std::fs::File::create(&file).map_err(|e| e.to_string())?;
    let mut hash = Sha512::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        progress.fetch_add(n as u64, Ordering::Relaxed);
    }
    drop(out);
    let got = base64::engine::general_purpose::STANDARD.encode(hash.finalize());
    if got != sha {
        let _ = std::fs::remove_file(&file);
        return Err(crate::i18n::tr("Pobrany instalator ma niezgodną sumę kontrolną – spróbuj ponownie później.").into());
    }
    Ok(file)
}

/// Instalacja pobranego pliku (Windows: UAC i tryb cichy; Android: okno instalatora systemu).
pub fn run_installer(file: &std::path::Path) -> Result<(), String> {
    crate::platform::install_update(file)
}

/// Pozostałość po poprzedniej aktualizacji.
pub fn cleanup() {
    let _ = std::fs::remove_dir_all(std::env::temp_dir().join("GRAN-aktualizacja"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(cmp_version("2.0.0", "1.1.0") > 0);
        assert!(cmp_version("v1.0.3", "1.0.3") == 0);
        assert!(cmp_version("2.0.0", "2.0.1") < 0);
    }

    #[test]
    fn tags_per_platform() {
        if crate::platform::MOBILE {
            assert_eq!(tag_version("android-v1.0.0"), Some("1.0.0"));
            assert_eq!(tag_version("v4.0.1"), None);
        } else {
            assert_eq!(tag_version("v4.0.1"), Some("4.0.1"));
            assert_eq!(tag_version("android-v1.0.0"), None);
        }
    }

    #[test]
    fn package_for_platform() {
        assert!(is_package("GRAN-Setup-4.0.0.exe") != crate::platform::MOBILE);
        assert!(is_package("GRAN-4.0.0.apk") == crate::platform::MOBILE);
        assert!(is_package("GRAN-Android-1.0.0.apk") == crate::platform::MOBILE);
        assert!(!is_package("latest.yml"));
    }

    #[test]
    fn sha_from_two_files() {
        let yml = "version: 4.0.0\nfiles:\n  - url: GRAN-Setup-4.0.0.exe\n    sha512: win\n    size: 10\n  - url: GRAN-4.0.0.apk\n    sha512: apk\n    size: 20\npath: GRAN-Setup-4.0.0.exe\nsha512: win\n";
        assert_eq!(parse_sha(yml, "GRAN-4.0.0.apk").as_deref(), Some("apk"));
        assert_eq!(parse_sha(yml, "GRAN-Setup-4.0.0.exe").as_deref(), Some("win"));
    }

    #[test]
    fn sha_from_yml() {
        let yml = "version: 2.0.0\nfiles:\n  - url: GRAN-Setup-2.0.0.exe\n    sha512: abc+/=\n    size: 10\npath: GRAN-Setup-2.0.0.exe\nsha512: xyz\n";
        assert_eq!(parse_sha(yml, "GRAN-Setup-2.0.0.exe").as_deref(), Some("abc+/="));
    }
}
