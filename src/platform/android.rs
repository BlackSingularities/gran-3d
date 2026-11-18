//! Android: wywołania metod `GranActivity` (Java, `android/app/src/main/java/.../GranActivity.java`)
//! przez JNI. Wyniki okien systemu (wybór pliku, zapis) Java odkłada w kolejce, a program odbiera
//! je w [`poll`] – bez rejestrowania metod natywnych.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use jni::objects::{JObject, JString, JValue};
use jni::{JNIEnv, JavaVM};

use super::{Event, Storage};

/// Obiekt `GranActivity` (globalna referencja trzymana przez NativeActivity przez cały czas życia
/// procesu). Kontekst z `ndk_context` to `Application`, nie aktywność – stąd osobny wskaźnik.
static ACTIVITY: OnceLock<usize> = OnceLock::new();

/// Wywołanie na aktywności; błędy Javy trafiają do dziennika i dają `None`.
fn with_activity<R>(what: &str, f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<R>) -> Option<R> {
    let ctx = ndk_context::android_context();
    let ptr = *ACTIVITY.get()?;
    // SAFETY: maszyna wirtualna z kontekstu ndk i aktywność z android-activity żyją do końca procesu
    let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) }.ok()?;
    let activity = unsafe { JObject::from_raw(ptr as jni::sys::jobject) };
    let mut env = vm.attach_current_thread().ok()?;
    let r = f(&mut env, &activity);
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    match r {
        Ok(v) => Some(v),
        Err(e) => {
            log::warn!("JNI {what}: {e}");
            None
        }
    }
}

fn string(env: &mut JNIEnv, o: JObject) -> jni::errors::Result<String> {
    if o.is_null() {
        return Ok(String::new());
    }
    let s = JString::from(o);
    let r: String = env.get_string(&s)?.into();
    Ok(r)
}

/// Metoda `()Ljava/lang/String;`.
fn call_str(name: &str) -> String {
    with_activity(name, |env, a| {
        let o = env.call_method(a, name, "()Ljava/lang/String;", &[])?.l()?;
        string(env, o)
    })
    .unwrap_or_default()
}

/// Metoda z argumentami tekstowymi, zwracająca tekst (pusty – sukces albo brak wyniku).
fn call_args(name: &str, args: &[&str]) -> String {
    let sig = format!("({})Ljava/lang/String;", "Ljava/lang/String;".repeat(args.len()));
    with_activity(name, |env, a| {
        let objs: Vec<JObject> = args.iter().map(|s| env.new_string(s).map(JObject::from)).collect::<Result<_, _>>()?;
        let vals: Vec<JValue> = objs.iter().map(JValue::Object).collect();
        let o = env.call_method(a, name, &sig, &vals)?.l()?;
        string(env, o)
    })
    .unwrap_or_default()
}

/// Logger piszący jednocześnie do pliku dziennika i do logcat.
pub struct TeeLogger(pub env_logger::Logger, pub android_logger::AndroidLogger);

impl log::Log for TeeLogger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        self.0.enabled(m)
    }
    fn log(&self, r: &log::Record) {
        self.0.log(r);
        if self.1.enabled(r.metadata()) {
            self.1.log(r);
        }
    }
    fn flush(&self) {
        self.0.flush();
    }
}

static DEVICE: OnceLock<String> = OnceLock::new();
/// Pasek stanu i pasek nawigacji (piksele), odświeżane w [`poll`].
static INSETS: std::sync::Mutex<[f32; 2]> = std::sync::Mutex::new([0.0, 0.0]);

/// Paski systemu nad obrazem programu: [góra, dół] w pikselach. Okno NativeActivity zajmuje cały
/// ekran, więc interfejs musi sam ominąć pasek stanu i pasek nawigacji.
pub fn insets() -> [f32; 2] {
    *INSETS.lock().unwrap()
}

fn refresh_insets() {
    let t = call_str("insets");
    let v: Vec<f32> = t.split_whitespace().filter_map(|x| x.parse().ok()).collect();
    if v.len() >= 2 {
        *INSETS.lock().unwrap() = [v[0], v[1]];
    }
}

/// Foldery aplikacji jako zmienne środowiska (te same nazwy co w Windows), język systemu.
pub fn init(app: &android_activity::AndroidApp) {
    let _ = ACTIVITY.set(app.activity_as_ptr() as usize);
    let internal = app.internal_data_path().unwrap_or_else(|| PathBuf::from(call_str("filesDir")));
    let external = app.external_data_path().filter(|p| std::fs::create_dir_all(p).is_ok());
    let cache = PathBuf::from(call_str("cacheDir"));
    // na samym początku, zanim powstaną inne wątki programu
    std::env::set_var("APPDATA", &internal);
    std::env::set_var("LOCALAPPDATA", external.as_ref().unwrap_or(&internal));
    if cache.is_dir() {
        std::env::set_var("TMPDIR", &cache);
    }
    let lang = call_str("language");
    if !lang.is_empty() {
        std::env::set_var("LANG", lang.replace('-', "_"));
    }
    let _ = DEVICE.set(call_str("deviceInfo"));
    refresh_insets();
}

/// Emulator Androida (grafika programowa – tylko OpenGL ES działa pewnie).
pub fn is_emulator() -> bool {
    call_str("isEmulator") == "1"
}

pub fn device() -> String {
    DEVICE.get().cloned().unwrap_or_else(|| "Android".into())
}

/// Zdarzenia odłożone przez Javę (wybrane pliki, komunikaty), najwyżej kilka razy na sekundę.
pub fn poll() {
    static LAST: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
    {
        let mut last = LAST.lock().unwrap();
        if last.is_some_and(|t| t.elapsed() < Duration::from_millis(250)) {
            return;
        }
        *last = Some(Instant::now());
    }
    refresh_insets();
    let text = call_str("pollEvents");
    for line in text.lines() {
        match line.split_once('\t') {
            Some(("open", p)) => super::push(Event::Opened(PathBuf::from(p))),
            Some(("msg", m)) => super::push(Event::Message(crate::i18n::tr(m).to_string())),
            Some(("link", l)) => super::push(Event::Link(l.to_string())),
            Some(("capture", _)) => super::push(Event::Capture),
            Some(("saved", name)) => super::push(Event::Message(tf!("Zapisano {}", name))),
            Some(("denied", _)) => super::push(Event::Message(crate::i18n::tr("Zezwól GRANI na instalowanie aplikacji (okno ustawień), a potem ponów aktualizację.").to_string())),
            _ => {}
        }
    }
}

pub fn open_url(url: &str) {
    call_args("openUrl", &[url]);
}

/// Dziennik programu: udostępnienie pliku (e-mail, komunikator, Dysk).
pub fn show_log(log: &Path) {
    call_args("shareFile", &[&log.to_string_lossy(), "text/plain"]);
}

/// Okno systemu „Otwórz”; wybrany plik przyjdzie jako [`Event::Opened`].
pub fn pick_track_file() -> Option<PathBuf> {
    call_args("pickTrack", &[]);
    None
}

/// Zapis przez okno systemu „Zapisz jako” w pierwszym formacie z listy. Wynik przyjdzie jako komunikat.
pub fn save_file(_title: &str, stem: &str, formats: &[(&str, &str)], bytes: impl FnOnce(&str) -> Vec<u8>) -> Option<Result<String, String>> {
    let ext = formats.first().map(|f| f.0).unwrap_or("bin");
    let dir = std::env::temp_dir().join("eksport");
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join(format!("{stem}.{ext}"));
    if let Err(e) = std::fs::write(&file, bytes(ext)) {
        return Some(Err(e.to_string()));
    }
    deliver(&file, &format!("{stem}.{ext}"));
    None
}

/// Zrzut ekranu zapisuje się najpierw w folderze tymczasowym, potem trafia do okna „Zapisz jako”.
pub fn screenshot_target() -> Option<PathBuf> {
    let dir = std::env::temp_dir().join("eksport");
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join("gran.png"))
}

/// Gotowy plik do okna systemu „Zapisz jako” (Java kopiuje go pod wskazany adres).
pub fn deliver(path: &Path, name: &str) -> bool {
    let ext = name.rsplit('.').next().unwrap_or("");
    call_args("createDocument", &[&path.to_string_lossy(), name, super::mime(ext)]);
    true
}

/// Pamięć wewnętrzna i karty SD (foldery aplikacji – bez dodatkowych uprawnień).
pub fn storage_choices() -> Vec<Storage> {
    call_str("storageDirs")
        .lines()
        .filter_map(|l| {
            let (kind, path) = l.split_once('\t')?;
            let label = match kind {
                "internal" => crate::i18n::tr("Pamięć wewnętrzna (tylko dla GRANI)"),
                "primary" => crate::i18n::tr("Pamięć urządzenia"),
                _ => crate::i18n::tr("Karta pamięci"),
            };
            Some(Storage { label: label.to_string(), path: PathBuf::from(path) })
        })
        .collect()
}

pub fn pick_folder(_title: &str) -> Option<PathBuf> {
    None
}

/// Instalacja pobranego APK oknem instalatora systemu.
pub fn install_update(file: &Path) -> Result<(), String> {
    match call_args("installApk", &[&file.to_string_lossy()]).as_str() {
        "" => Ok(()),
        "denied" => Err(crate::i18n::tr("Zezwól GRANI na instalowanie aplikacji (okno ustawień), a potem ponów aktualizację.").into()),
        e => Err(e.to_string()),
    }
}

/// Na Androidzie instalator systemu sam zamyka i podmienia aplikację.
pub const CLOSE_FOR_UPDATE: bool = false;

/// Przycisk „wstecz”, gdy nie ma już czego zamykać: aplikacja w tło (jak w innych programach).
pub fn move_to_back() {
    call_args("moveToBack", &[]);
}

/// Krótki komunikat systemu.
pub fn toast(text: &str) {
    call_args("toast", &[text]);
}
