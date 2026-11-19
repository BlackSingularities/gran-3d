//! iOS i iPadOS: wywołania UIKit przez objc2. Okno, Metal i dotyk daje winit/eframe; tu tylko to,
//! czego nie ma w bibliotekach okna: foldery aplikacji, język, linki, wybór pliku (okno „Pliki”),
//! udostępnianie (arkusz systemu – także „Zachowaj w Plikach”) i obszar bezpieczny ekranu.
//!
//! Wszystkie wywołania UIKit idą z wątku interfejsu (winit na iOS działa w wątku głównym).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{class, define_class, msg_send, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSArray, NSString, NSURL};

use super::{Event, Storage};

/// Logger: na iOS dziennik idzie tylko do pliku (konsola Xcode nie jest dostępna u użytkowników).
pub struct TeeLogger(pub env_logger::Logger);

impl log::Log for TeeLogger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        self.0.enabled(m)
    }
    fn log(&self, r: &log::Record) {
        self.0.log(r);
    }
    fn flush(&self) {
        self.0.flush();
    }
}

static DEVICE: OnceLock<String> = OnceLock::new();

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

/// Foldery aplikacji jako zmienne środowiska (te same nazwy co w Windows), język systemu.
/// Dane map leżą w `Library/Application Support` (nie w kopii zapasowej iCloud), dziennik i
/// eksport – w `Documents` (widocznym w aplikacji Pliki).
pub fn init() {
    let support = home().join("Library").join("Application Support");
    let _ = std::fs::create_dir_all(&support);
    std::env::set_var("APPDATA", &support);
    std::env::set_var("LOCALAPPDATA", &support);
    exclude_from_backup(&support);
    // SAFETY: zwykłe zapytania Foundation/UIKit bez efektów ubocznych
    unsafe {
        let langs: *mut AnyObject = msg_send![class!(NSLocale), preferredLanguages];
        if !langs.is_null() {
            let first: *mut AnyObject = msg_send![langs, firstObject];
            if !first.is_null() {
                let s = &*(first as *const NSString);
                std::env::set_var("LANG", s.to_string().replace('-', "_"));
            }
        }
        let dev: *mut AnyObject = msg_send![class!(UIDevice), currentDevice];
        let info = if dev.is_null() {
            "iOS".to_string()
        } else {
            let name: *mut AnyObject = msg_send![dev, systemName];
            let ver: *mut AnyObject = msg_send![dev, systemVersion];
            let model: *mut AnyObject = msg_send![dev, model];
            let s = |p: *mut AnyObject| if p.is_null() { String::new() } else { (*(p as *const NSString)).to_string() };
            format!("{} {} · {}", s(name), s(ver), s(model))
        };
        let _ = DEVICE.set(info);
    }
}

/// Folder bez kopii zapasowej iCloud (gigabajty kafli map nie mają tam sensu).
fn exclude_from_backup(dir: &Path) {
    // SAFETY: NSURL z istniejącej ścieżki, klucz ze stałej Foundation
    unsafe {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&dir.to_string_lossy()));
        let yes: *mut AnyObject = msg_send![class!(NSNumber), numberWithBool: true];
        let key = NSString::from_str("NSURLIsExcludedFromBackupKey");
        let _: bool = msg_send![&*url, setResourceValue: yes, forKey: &*key, error: std::ptr::null_mut::<*mut AnyObject>()];
    }
}

pub fn device() -> String {
    DEVICE.get().cloned().unwrap_or_else(|| "iOS".into())
}

/// Zdarzenia przychodzą od razu z metod delegata (wątek główny).
pub fn poll() {}

fn root_controller() -> *mut AnyObject {
    // SAFETY: odczyt okna i kontrolera w wątku głównym
    unsafe {
        let app: *mut AnyObject = msg_send![class!(UIApplication), sharedApplication];
        if app.is_null() {
            return std::ptr::null_mut();
        }
        let win: *mut AnyObject = msg_send![app, keyWindow];
        let win = if win.is_null() {
            let wins: *mut AnyObject = msg_send![app, windows];
            if wins.is_null() {
                return std::ptr::null_mut();
            }
            msg_send![wins, firstObject]
        } else {
            win
        };
        if win.is_null() {
            return std::ptr::null_mut();
        }
        let mut vc: *mut AnyObject = msg_send![win, rootViewController];
        // kontroler już coś pokazuje – nowe okno nad nim
        loop {
            if vc.is_null() {
                break;
            }
            let shown: *mut AnyObject = msg_send![vc, presentedViewController];
            if shown.is_null() {
                break;
            }
            vc = shown;
        }
        vc
    }
}

/// Pokazanie kontrolera (arkusz udostępniania, okno plików); na iPadzie jako dymek z dołu ekranu.
fn present(vc: *mut AnyObject) {
    let root = root_controller();
    if root.is_null() || vc.is_null() {
        log::warn!("iOS: brak kontrolera do pokazania okna");
        return;
    }
    // SAFETY: obiekty UIKit w wątku głównym
    unsafe {
        let pop: *mut AnyObject = msg_send![vc, popoverPresentationController];
        if !pop.is_null() {
            let view: *mut AnyObject = msg_send![root, view];
            let _: () = msg_send![pop, setSourceView: view];
            let bounds: Rect = msg_send![view, bounds];
            let r = Rect { x: bounds.w / 2.0, y: bounds.h - 40.0, w: 1.0, h: 1.0 };
            let _: () = msg_send![pop, setSourceRect: r];
        }
        let _: () = msg_send![root, presentViewController: vc, animated: true, completion: std::ptr::null::<AnyObject>()];
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

// SAFETY: układ zgodny z CGRect (dwie struktury po dwa CGFloat) na 64-bitowym iOS
unsafe impl objc2::encode::Encode for Rect {
    const ENCODING: objc2::encode::Encoding = objc2::encode::Encoding::Struct(
        "CGRect",
        &[
            objc2::encode::Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]),
            objc2::encode::Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]),
        ],
    );
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Insets {
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
}

// SAFETY: układ zgodny z UIEdgeInsets (cztery CGFloat)
unsafe impl objc2::encode::Encode for Insets {
    const ENCODING: objc2::encode::Encoding = objc2::encode::Encoding::Struct("UIEdgeInsets", &[f64::ENCODING, f64::ENCODING, f64::ENCODING, f64::ENCODING]);
}

/// Wycięcie ekranu i pasek wskaźnika na dole: [góra, dół] w pikselach.
pub fn insets() -> [f32; 2] {
    let root = root_controller();
    if root.is_null() {
        return [0.0, 0.0];
    }
    // SAFETY: odczyt właściwości widoku w wątku głównym
    unsafe {
        let view: *mut AnyObject = msg_send![root, view];
        if view.is_null() {
            return [0.0, 0.0];
        }
        let i: Insets = msg_send![view, safeAreaInsets];
        let screen: *mut AnyObject = msg_send![class!(UIScreen), mainScreen];
        let scale: f64 = if screen.is_null() { 2.0 } else { msg_send![screen, scale] };
        [(i.top * scale) as f32, (i.bottom * scale) as f32]
    }
}

pub fn open_url(url: &str) {
    // SAFETY: wywołanie UIApplication w wątku głównym
    unsafe {
        let Some(u) = NSURL::URLWithString(&NSString::from_str(url)) else { return };
        let app: *mut AnyObject = msg_send![class!(UIApplication), sharedApplication];
        let opts: *mut AnyObject = msg_send![class!(NSDictionary), dictionary];
        let _: () = msg_send![app, openURL: &*u, options: opts, completionHandler: std::ptr::null::<AnyObject>()];
    }
}

/// Arkusz udostępniania systemu dla pliku (AirDrop, Poczta, „Zachowaj w Plikach”…).
fn share(path: &Path) {
    // SAFETY: obiekty UIKit tworzone i pokazywane w wątku głównym
    unsafe {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let items = NSArray::from_retained_slice(&[Retained::into_super(url)]);
        let vc: *mut AnyObject = msg_send![class!(UIActivityViewController), alloc];
        let vc: *mut AnyObject = msg_send![vc, initWithActivityItems: &*items, applicationActivities: std::ptr::null::<AnyObject>()];
        present(vc);
    }
}

/// Dziennik programu: udostępnienie pliku.
pub fn show_log(log: &Path) {
    share(log);
}

define_class!(
    // SAFETY: NSObject nie ma wymagań przy dziedziczeniu, klasa nie implementuje Drop
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "GranDocumentPickerDelegate"]
    struct PickerDelegate;

    impl PickerDelegate {
        #[unsafe(method(documentPicker:didPickDocumentsAtURLs:))]
        fn did_pick(&self, _picker: *mut AnyObject, urls: &NSArray<NSURL>) {
            for url in urls.iter() {
                let Some(path) = url.path() else { continue };
                let src = PathBuf::from(path.to_string());
                let dir = std::env::temp_dir().join("import");
                let _ = std::fs::create_dir_all(&dir);
                let dst = dir.join(src.file_name().unwrap_or_default());
                match std::fs::copy(&src, &dst) {
                    Ok(_) => super::push(Event::Opened(dst)),
                    Err(e) => super::push(Event::Message(tf!("Nie udało się wczytać pliku: {}", e))),
                }
            }
        }

        #[unsafe(method(documentPickerWasCancelled:))]
        fn cancelled(&self, _picker: *mut AnyObject) {}
    }
);

impl PickerDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: init NSObject
        unsafe { msg_send![super(this), init] }
    }
}

thread_local! {
    /// Delegat okna plików (UIKit trzyma go słabo – musi żyć tutaj).
    static PICKER: std::cell::RefCell<Option<Retained<PickerDelegate>>> = const { std::cell::RefCell::new(None) };
}

#[link(name = "UIKit", kind = "framework")]
extern "C" {
    /// `NSData *UIImageJPEGRepresentation(UIImage *image, CGFloat quality)`.
    fn UIImageJPEGRepresentation(image: *mut AnyObject, quality: f64) -> *mut AnyObject;
}

define_class!(
    // SAFETY: NSObject nie ma wymagań przy dziedziczeniu, klasa nie implementuje Drop
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "GranImagePickerDelegate"]
    struct ImagePickerDelegate;

    impl ImagePickerDelegate {
        #[unsafe(method(imagePickerController:didFinishPickingMediaWithInfo:))]
        fn did_pick(&self, picker: *mut AnyObject, info: *mut AnyObject) {
            // SAFETY: obiekty UIKit przekazane przez system w wątku głównym
            unsafe {
                let key = NSString::from_str("UIImagePickerControllerOriginalImage");
                let image: *mut AnyObject = msg_send![info, objectForKey: &*key];
                // JPEG przez UIKit: działa też dla zdjęć HEIC; GRAŃ zmniejsza je potem do 2048 px
                let data = if image.is_null() { std::ptr::null_mut() } else { UIImageJPEGRepresentation(image, 0.92) };
                let dir = std::env::temp_dir().join("zdjecia");
                let _ = std::fs::create_dir_all(&dir);
                let file = dir.join(format!("zdjecie-{}.jpg", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)));
                let ok: bool = !data.is_null() && msg_send![data, writeToFile: &*NSString::from_str(&file.to_string_lossy()), atomically: true];
                if ok {
                    super::push(Event::Photo(file));
                } else {
                    super::push(Event::Message(crate::i18n::tr("Nie udało się wczytać zdjęcia.").to_string()));
                }
                let _: () = msg_send![picker, dismissViewControllerAnimated: true, completion: std::ptr::null::<AnyObject>()];
            }
        }

        #[unsafe(method(imagePickerControllerDidCancel:))]
        fn cancelled(&self, picker: *mut AnyObject) {
            // SAFETY: okno systemu zamykane w wątku głównym
            unsafe {
                let _: () = msg_send![picker, dismissViewControllerAnimated: true, completion: std::ptr::null::<AnyObject>()];
            }
        }
    }
);

impl ImagePickerDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: init NSObject
        unsafe { msg_send![super(this), init] }
    }
}

thread_local! {
    /// Delegat okna zdjęć (UIKit trzyma go słabo – musi żyć tutaj).
    static IMAGE_PICKER: std::cell::RefCell<Option<Retained<ImagePickerDelegate>>> = const { std::cell::RefCell::new(None) };
}

/// Okno zdjęć systemu (biblioteka zdjęć, bez prośby o dostęp – działa poza procesem GRANI);
/// wybrane zdjęcie przyjdzie jako [`Event::Photo`].
pub fn pick_photo() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let delegate = IMAGE_PICKER.with(|p| p.borrow_mut().get_or_insert_with(|| ImagePickerDelegate::new(mtm)).clone());
    // SAFETY: obiekty UIKit tworzone w wątku głównym
    unsafe {
        let vc: *mut AnyObject = msg_send![class!(UIImagePickerController), alloc];
        let vc: *mut AnyObject = msg_send![vc, init];
        if vc.is_null() {
            return;
        }
        let _: () = msg_send![vc, setDelegate: &*delegate];
        present(vc);
    }
}

/// Okno systemu „Pliki”; wybrany ślad przyjdzie jako [`Event::Opened`].
pub fn pick_track_file() -> Option<PathBuf> {
    let Some(mtm) = MainThreadMarker::new() else { return None };
    let delegate = PICKER.with(|p| p.borrow_mut().get_or_insert_with(|| PickerDelegate::new(mtm)).clone());
    // SAFETY: obiekty UIKit tworzone w wątku głównym
    unsafe {
        let types = NSArray::from_retained_slice(&[NSString::from_str("public.data"), NSString::from_str("public.xml"), NSString::from_str("public.json")]);
        let vc: *mut AnyObject = msg_send![class!(UIDocumentPickerViewController), alloc];
        // tryb „import” (0): system kopiuje plik do skrzynki aplikacji
        let vc: *mut AnyObject = msg_send![vc, initWithDocumentTypes: &*types, inMode: 0isize];
        if vc.is_null() {
            return None;
        }
        let _: () = msg_send![vc, setDelegate: &*delegate];
        present(vc);
    }
    None
}

/// Zapis: plik w folderze tymczasowym, potem arkusz udostępniania („Zachowaj w Plikach”).
pub fn save_file(_title: &str, stem: &str, formats: &[(&str, &str)], bytes: impl FnOnce(&str) -> Vec<u8>) -> Option<Result<String, String>> {
    let ext = formats.first().map(|f| f.0).unwrap_or("bin");
    let dir = std::env::temp_dir().join("eksport");
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join(format!("{stem}.{ext}"));
    if let Err(e) = std::fs::write(&file, bytes(ext)) {
        return Some(Err(e.to_string()));
    }
    share(&file);
    None
}

pub fn screenshot_target() -> Option<PathBuf> {
    let dir = std::env::temp_dir().join("eksport");
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join("gran.png"))
}

pub fn deliver(path: &Path, _name: &str) -> bool {
    share(path);
    true
}

/// Na iOS jedno miejsce na dane: pamięć urządzenia (folder aplikacji).
pub fn storage_choices() -> Vec<Storage> {
    let p = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(home);
    vec![Storage { label: crate::i18n::tr("Pamięć urządzenia").to_string(), path: p }]
}

pub fn pick_folder(_title: &str) -> Option<PathBuf> {
    None
}

/// Aplikacji spoza App Store nie da się zaktualizować z jej wnętrza: otwieramy stronę pobierania.
pub fn install_update(_file: &Path) -> Result<(), String> {
    open_url("https://redzikowski.com/portfolio/gran-3d");
    Ok(())
}

pub const CLOSE_FOR_UPDATE: bool = false;

/// iOS nie ma przycisku „wstecz”.
pub fn move_to_back() {}

pub fn toast(_text: &str) {}

pub fn is_emulator() -> bool {
    false
}

pub fn forced_backend() -> String {
    String::new()
}

/* ── położenie: CoreLocation (CLLocationManager z delegatem) ──────────────────────── */

#[link(name = "CoreLocation", kind = "framework")]
extern "C" {}

pub const LOCATION: bool = true;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Coord {
    lat: f64,
    lon: f64,
}

// SAFETY: układ zgodny z CLLocationCoordinate2D (dwie liczby double)
unsafe impl objc2::encode::Encode for Coord {
    const ENCODING: objc2::encode::Encoding = objc2::encode::Encoding::Struct("CLLocationCoordinate2D", &[f64::ENCODING, f64::ENCODING]);
}

/// Stan położenia współdzielony z delegatem (wywołania w wątku głównym, także w tle).
struct Loc {
    fixes: Vec<crate::gps::Fix>,
    heading: Option<f32>,
    journal: Option<PathBuf>,
    status: i32,
}

static LOC: std::sync::Mutex<Loc> = std::sync::Mutex::new(Loc { fixes: Vec::new(), heading: None, journal: None, status: -1 });

define_class!(
    // SAFETY: NSObject nie ma wymagań przy dziedziczeniu, klasa nie implementuje Drop
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "GranLocationDelegate"]
    struct LocationDelegate;

    impl LocationDelegate {
        #[unsafe(method(locationManager:didUpdateLocations:))]
        fn did_update(&self, _manager: *mut AnyObject, locations: *mut AnyObject) {
            if locations.is_null() {
                return;
            }
            // SAFETY: obiekty CLLocation przekazane przez system w wątku głównym
            unsafe {
                let n: usize = msg_send![locations, count];
                let mut g = LOC.lock().unwrap();
                for i in 0..n {
                    let l: *mut AnyObject = msg_send![locations, objectAtIndex: i];
                    if l.is_null() {
                        continue;
                    }
                    let c: Coord = msg_send![l, coordinate];
                    let acc: f64 = msg_send![l, horizontalAccuracy];
                    // ujemna dokładność – odczyt nieważny
                    if acc < 0.0 {
                        continue;
                    }
                    let alt: f64 = msg_send![l, altitude];
                    let vacc: f64 = msg_send![l, verticalAccuracy];
                    let speed: f64 = msg_send![l, speed];
                    let course: f64 = msg_send![l, course];
                    let date: *mut AnyObject = msg_send![l, timestamp];
                    let t: f64 = if date.is_null() { crate::gps::now() } else { msg_send![date, timeIntervalSince1970] };
                    let f = crate::gps::Fix {
                        t,
                        lat: c.lat,
                        lon: c.lon,
                        alt: (vacc >= 0.0).then_some(alt as f32),
                        acc: acc as f32,
                        vacc: (vacc >= 0.0).then_some(vacc as f32),
                        speed: (speed >= 0.0).then_some(speed as f32),
                        course: (course >= 0.0).then_some(course as f32),
                        // iOS łączy GPS, Wi-Fi i sieć; odczyty do ~65 m traktujemy jak satelitarne
                        // (filtr nagrania i tak sprawdza dokładność)
                        gnss: acc <= 65.0,
                    };
                    if let Some(j) = &g.journal {
                        if f.gnss {
                            crate::gps::journal_append(j, &f);
                        }
                    }
                    if g.fixes.len() > 200_000 {
                        g.fixes.drain(..20_000);
                    }
                    g.fixes.push(f);
                }
            }
        }

        #[unsafe(method(locationManager:didUpdateHeading:))]
        fn did_heading(&self, _manager: *mut AnyObject, heading: *mut AnyObject) {
            if heading.is_null() {
                return;
            }
            // SAFETY: obiekt CLHeading przekazany przez system
            unsafe {
                let t: f64 = msg_send![heading, trueHeading];
                let m: f64 = msg_send![heading, magneticHeading];
                let h = if t >= 0.0 { t } else { m };
                if h >= 0.0 {
                    LOC.lock().unwrap().heading = Some(h as f32);
                }
            }
        }

        #[unsafe(method(locationManagerDidChangeAuthorization:))]
        fn did_auth(&self, manager: *mut AnyObject) {
            // SAFETY: odczyt stanu zgody w wątku głównym
            let status: i32 = unsafe { msg_send![manager, authorizationStatus] };
            let before = std::mem::replace(&mut LOC.lock().unwrap().status, status);
            if before != status && status != 0 {
                super::push(Event::LocationPermission(status == 3 || status == 4));
            }
        }

        #[unsafe(method(locationManager:didFailWithError:))]
        fn did_fail(&self, _manager: *mut AnyObject, _error: *mut AnyObject) {}
    }
);

impl LocationDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: init NSObject
        unsafe { msg_send![super(this), init] }
    }
}

thread_local! {
    /// Menedżer położenia i jego delegat (CoreLocation trzyma delegata słabo).
    static MANAGER: std::cell::RefCell<Option<(Retained<AnyObject>, Retained<LocationDelegate>)>> = const { std::cell::RefCell::new(None) };
}

/// Menedżer położenia (tworzony przy pierwszym użyciu, w wątku głównym).
fn manager() -> Option<*mut AnyObject> {
    let mtm = MainThreadMarker::new()?;
    MANAGER.with(|m| {
        let mut m = m.borrow_mut();
        if m.is_none() {
            let delegate = LocationDelegate::new(mtm);
            // SAFETY: CLLocationManager tworzony i ustawiany w wątku głównym
            unsafe {
                let man: *mut AnyObject = msg_send![class!(CLLocationManager), alloc];
                let man: *mut AnyObject = msg_send![man, init];
                let man = Retained::from_raw(man)?;
                let _: () = msg_send![&*man, setDelegate: &*delegate];
                // najlepsza dokładność, każdy odczyt, tryb „aktywność fizyczna”
                let _: () = msg_send![&*man, setDesiredAccuracy: -1.0f64];
                let _: () = msg_send![&*man, setDistanceFilter: -1.0f64];
                let _: () = msg_send![&*man, setActivityType: 3isize];
                let _: () = msg_send![&*man, setPausesLocationUpdatesAutomatically: false];
                let _: () = msg_send![&*man, setHeadingFilter: 2.0f64];
                let status: i32 = msg_send![&*man, authorizationStatus];
                LOC.lock().unwrap().status = status;
                *m = Some((man, delegate));
            }
        }
        m.as_ref().map(|(man, _)| Retained::as_ptr(man) as *mut AnyObject)
    })
}

pub fn location_permission() -> crate::gps::Permission {
    if manager().is_none() {
        return crate::gps::Permission::Denied;
    }
    match LOC.lock().unwrap().status {
        3 | 4 => crate::gps::Permission::Granted,
        1 | 2 => crate::gps::Permission::Denied,
        _ => crate::gps::Permission::Unknown,
    }
}

/// Okno systemu z prośbą o lokalizację „podczas używania” (nagrywanie w tle działa z nią dzięki
/// trybowi tła `location` i niebieskiemu wskaźnikowi na pasku stanu).
pub fn location_request_permission() {
    if let Some(m) = manager() {
        // SAFETY: wywołanie CoreLocation w wątku głównym
        unsafe {
            let _: () = msg_send![m, requestWhenInUseAuthorization];
        }
    }
}

pub fn location_start(journal: Option<&Path>) {
    let Some(m) = manager() else { return };
    let record = journal.is_some();
    LOC.lock().unwrap().journal = journal.map(Path::to_path_buf);
    // SAFETY: wywołania CoreLocation w wątku głównym
    unsafe {
        let _: () = msg_send![m, setAllowsBackgroundLocationUpdates: record];
        let _: () = msg_send![m, setShowsBackgroundLocationIndicator: record];
        let _: () = msg_send![m, startUpdatingLocation];
        let _: () = msg_send![m, startUpdatingHeading];
    }
}

pub fn location_stop() {
    LOC.lock().unwrap().journal = None;
    if let Some(m) = manager() {
        // SAFETY: wywołania CoreLocation w wątku głównym
        unsafe {
            let _: () = msg_send![m, stopUpdatingLocation];
            let _: () = msg_send![m, stopUpdatingHeading];
            let _: () = msg_send![m, setAllowsBackgroundLocationUpdates: false];
        }
    }
}

/// Nowe odczyty i kierunek kompasu (stopnie) od ostatniego wywołania.
pub fn location_poll() -> (Vec<crate::gps::Fix>, Option<f32>) {
    let mut g = LOC.lock().unwrap();
    (std::mem::take(&mut g.fixes), g.heading.take())
}

/// Ustawienia GRANI w aplikacji Ustawienia (zgoda na lokalizację).
pub fn location_settings() {
    open_url("app-settings:");
}

/// Plik do innej aplikacji przez arkusz udostępniania.
pub fn share_file(path: &Path, _name: &str) {
    share(path);
}
