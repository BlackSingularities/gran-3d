//! Windows: okna dialogowe `rfd`, przeglądarka i Eksplorator przez `open`.

use std::path::{Path, PathBuf};

use super::Storage;

/// Zdarzenia systemu przychodzą na komputerze od razu (okna dialogowe są synchroniczne).
pub fn poll() {}

/// Paski systemu nad oknem programu (góra, dół) w pikselach – na komputerze brak.
pub fn insets() -> [f32; 2] {
    [0.0, 0.0]
}

pub fn device() -> String {
    format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
}

pub fn open_url(url: &str) {
    if let Err(e) = open::that(url) {
        log::warn!("otwieranie {url}: {e}");
    }
}

/// Folder z dziennikiem programu w Eksploratorze.
pub fn show_log(log: &Path) {
    let _ = open::that(log.parent().unwrap_or(Path::new(".")));
}

/// Okno wyboru pliku śladu (GPX, KML, GeoJSON).
pub fn pick_track_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter(crate::i18n::tr("Ślady (GPX, KML, GeoJSON, FIT)"), &crate::formats::EXTENSIONS)
        .add_filter("GPX", &["gpx"])
        .add_filter("KML", &["kml"])
        .add_filter("GeoJSON", &["geojson", "json"])
        .pick_file()
}

/// Okno wyboru zdjęcia do punktu społeczności; wynik przychodzi jako [`super::Event::Photo`].
pub fn pick_photo() {
    if let Some(p) = rfd::FileDialog::new()
        .set_title(crate::i18n::tr("Wybierz zdjęcie"))
        .add_filter(crate::i18n::tr("Zdjęcia (JPEG, PNG, WEBP)"), &["jpg", "jpeg", "png", "webp"])
        .pick_file()
    {
        super::push(super::Event::Photo(p));
    }
}

/// Zapis pliku przez okno „Zapisz jako”: `formats` – (rozszerzenie, opis), pierwszy jest domyślny;
/// `bytes` dostaje rozszerzenie wybrane przez użytkownika. `None` – anulowano.
pub fn save_file(title: &str, stem: &str, formats: &[(&str, &str)], bytes: impl FnOnce(&str) -> Vec<u8>) -> Option<Result<String, String>> {
    let first = formats.first().map(|f| f.0).unwrap_or("bin");
    let mut dlg = rfd::FileDialog::new().set_title(title).set_file_name(format!("{stem}.{first}"));
    for (e, label) in formats {
        dlg = dlg.add_filter(*label, &[*e]);
    }
    let p = dlg.save_file()?;
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or(first).to_ascii_lowercase();
    Some(std::fs::write(&p, bytes(&ext)).map(|_| p.display().to_string()).map_err(|e| e.to_string()))
}

/// Miejsce zapisu zrzutu ekranu (okno „Zapisz jako”).
pub fn screenshot_target() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(crate::i18n::tr("Zapisz zrzut ekranu"))
        .set_file_name("gran.png")
        .add_filter("PNG", &["png"])
        .save_file()
}

/// Zapisany zrzut: na komputerze już leży tam, gdzie wskazał użytkownik.
pub fn deliver(_path: &Path, _name: &str) -> bool {
    false
}

/// Na komputerze folder danych wybiera się w oknie wyboru folderu.
pub fn storage_choices() -> Vec<Storage> {
    Vec::new()
}

pub fn pick_folder(title: &str) -> Option<PathBuf> {
    rfd::FileDialog::new().set_title(title).pick_folder()
}

/// Instalator aktualizacji w trybie cichym z oknem UAC; po instalacji uruchamia nową wersję.
pub fn install_update(file: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let w = |s: &std::ffi::OsStr| s.encode_wide().chain(Some(0)).collect::<Vec<u16>>();
        let verb = w("runas".as_ref());
        let path = w(file.as_os_str());
        let args = w("/S --updated --force-run".as_ref());
        // SAFETY: wszystkie napisy zakończone zerem i żyją do końca wywołania
        let r = unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), path.as_ptr(), args.as_ptr(), std::ptr::null(), SW_SHOWNORMAL) };
        if (r as isize) <= 32 {
            return Err(crate::i18n::tr("Instalacja aktualizacji została przerwana (brak zgody administratora?).").into());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = file;
        Err(crate::i18n::tr("Aktualizacje tylko w systemie Windows.").into())
    }
}

/// Po uruchomieniu instalatora program się zamyka (instalator podmienia plik).
pub const CLOSE_FOR_UPDATE: bool = true;

/// Przycisk „wstecz” nie istnieje na komputerze.
pub fn move_to_back() {}

/// Wyskakujący komunikat systemu – na komputerze wystarcza dymek w programie.
pub fn toast(_text: &str) {}

/* ── położenie: na komputerze brak odbiornika GPS ───────────────────────────────── */

pub const LOCATION: bool = false;

pub fn location_permission() -> crate::gps::Permission {
    crate::gps::Permission::Denied
}

pub fn location_request_permission() {}

pub fn location_start(_journal: Option<&Path>) {}

pub fn location_stop() {}

/// Nowe odczyty i kierunek kompasu (stopnie) od ostatniego wywołania.
pub fn location_poll() -> (Vec<crate::gps::Fix>, Option<f32>) {
    (Vec::new(), None)
}

pub fn location_settings() {}

/// Plik do innej aplikacji: na komputerze – okno „Zapisz jako”.
pub fn share_file(path: &Path, name: &str) {
    let ext = name.rsplit('.').next().unwrap_or("bin").to_string();
    if let Ok(bytes) = std::fs::read(path) {
        let stem = name.rsplit_once('.').map(|x| x.0).unwrap_or(name).to_string();
        let _ = save_file(crate::i18n::tr("Zapisz plik"), &stem, &[(ext.as_str(), ext.as_str())], move |_| bytes);
    }
}
