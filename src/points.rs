//! Punkty społeczności: miejsca na mapie z komentarzami i zdjęciami użytkowników GRANI.
//!
//! Punkty, komentarze i zdjęcia trafiają na redzikowski.com (`/api/map-points`), gdzie przechodzą
//! moderację w panelu strony. Dodawanie wymaga konta ([`crate::account`]); przeglądanie – nie.
//! Program pokazuje zatwierdzone punkty z okolicy widoku oraz własne wpisy (także te czekające na
//! zatwierdzenie). Bez sieci (albo w trybie offline) i bez zalogowania wpisy czekają w kolejce
//! i wysyłają się same po powrocie połączenia i zalogowaniu.
//!
//! Zdjęcia program przed wysłaniem obraca według EXIF, zmniejsza do najwyżej 2048 px i zapisuje
//! jako JPEG o jakości 75 – bez metadanych (położenia, aparatu). Serwer robi to samo jeszcze raz.
//!
//! Plik `%APPDATA%\GRAN\punkty.json`: identyfikator instalacji, własne wpisy, kolejka i ostatnio
//! widziane punkty (pokazywane także bez sieci); zdjęcia czekające w kolejce – `zdjecia\`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::account::Account;

const API: &str = "https://redzikowski.com/api/map-points";
const SITE: &str = "https://redzikowski.com";
/// Bok komórki pobierania punktów (stopnie).
const CELL: f64 = 0.25;
/// Komórka jest odświeżana najwyżej co tyle.
const REFRESH: Duration = Duration::from_secs(600);
/// Najwięcej zapamiętanych punktów z serwera.
const CACHE_MAX: usize = 5000;
/// Zdjęcie: dłuższy bok (px) i jakość JPEG – tak samo jak na serwerze.
pub const PHOTO_MAX: u32 = 2048;
pub const PHOTO_QUALITY: u8 = 75;
/// Miniatura w interfejsie (px) i większy podgląd.
const THUMB: u32 = 480;
const PREVIEW: u32 = 1600;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Info,
    Warning,
    /// Szlak zamknięty (sezonowo, po wypadku, remont).
    Closed,
    /// Żleb lub stok lawiniasty.
    Avalanche,
    /// Łańcuchy, klamry, ekspozycja.
    Exposed,
    Water,
    /// Wejście do parku, kasa biletowa, opłaty.
    Entrance,
    /// Dojazd: przystanek, parking.
    Transport,
    /// Zasięg telefonu, telefon ratunkowy, apteczka.
    Signal,
    /// Schron, szałas, miejsce na przeczekanie burzy (dawne „nocleg i schronienie”).
    Shelter,
    View,
    Photo,
    /// Zakaz (biwakowania, dronów, wejścia).
    Ban,
    /// Także rodzaje usunięte z programu (dawny „parking”) i nieznane.
    #[serde(other)]
    Other,
}

impl Category {
    pub const ALL: [Category; 14] = [
        Category::Warning,
        Category::Closed,
        Category::Avalanche,
        Category::Exposed,
        Category::Info,
        Category::Water,
        Category::Entrance,
        Category::Transport,
        Category::Signal,
        Category::Shelter,
        Category::View,
        Category::Photo,
        Category::Ban,
        Category::Other,
    ];

    pub fn label(self) -> &'static str {
        use crate::i18n::tr;
        match self {
            Category::Info => tr("Informacja"),
            Category::Warning => tr("Utrudnienie"),
            Category::Closed => tr("Szlak zamknięty"),
            Category::Avalanche => tr("Zagrożenie lawinowe"),
            Category::Exposed => tr("Łańcuchy i ekspozycja"),
            Category::Entrance => tr("Wejście i opłaty"),
            Category::Transport => tr("Dojazd i parking"),
            Category::Signal => tr("Zasięg i ratunek"),
            Category::Shelter => tr("Schron"),
            Category::Ban => tr("Zakaz"),
            Category::Water => tr("Woda"),
            Category::View => tr("Punkt widokowy"),
            Category::Photo => tr("Miejsce na zdjęcie"),
            Category::Other => tr("Inne"),
        }
    }

    pub fn icon(self) -> &'static str {
        use crate::ui::theme::icon as i;
        match self {
            Category::Info => i::INFO,
            Category::Warning => i::WARNING,
            Category::Closed => i::BARRICADE,
            Category::Avalanche => i::SNOWFLAKE,
            Category::Exposed => i::LINK_SIMPLE,
            Category::Entrance => i::TICKET,
            Category::Transport => i::BUS,
            Category::Signal => i::FIRST_AID,
            Category::Shelter => i::HOUSE_LINE,
            Category::Ban => i::PROHIBIT,
            Category::Water => i::DROP,
            Category::View => i::BINOCULARS,
            Category::Photo => i::CAMERA,
            Category::Other => i::CHAT_CIRCLE_TEXT,
        }
    }

    pub fn color(self) -> eframe::egui::Color32 {
        use eframe::egui::Color32;
        match self {
            Category::Warning => Color32::from_rgb(247, 160, 60),
            Category::Closed => Color32::from_rgb(236, 84, 72),
            Category::Avalanche => Color32::from_rgb(170, 200, 255),
            Category::Exposed => Color32::from_rgb(255, 130, 90),
            Category::Entrance => Color32::from_rgb(90, 200, 190),
            Category::Transport => Color32::from_rgb(110, 160, 255),
            Category::Signal => Color32::from_rgb(120, 220, 120),
            Category::Shelter => Color32::from_rgb(214, 170, 110),
            Category::Ban => Color32::from_rgb(200, 70, 90),
            Category::Water => Color32::from_rgb(90, 170, 255),
            Category::View => Color32::from_rgb(110, 214, 140),
            Category::Photo => Color32::from_rgb(240, 120, 200),
            Category::Info => Color32::from_rgb(122, 176, 255),
            Category::Other => Color32::from_rgb(190, 196, 206),
        }
    }
}

/// Stan wpisu z punktu widzenia autora.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Opublikowany (widoczny dla wszystkich).
    #[default]
    Approved,
    /// Wysłany, czeka na zatwierdzenie.
    Pending,
    /// Czeka w kolejce na połączenie (albo zalogowanie).
    Queued,
    /// Odrzucony albo ukryty przez moderację.
    Hidden,
}

impl Status {
    pub fn label(self) -> &'static str {
        use crate::i18n::tr;
        match self {
            Status::Approved => tr("opublikowany"),
            Status::Pending => tr("czeka na zatwierdzenie"),
            Status::Queued => tr("czeka na wysłanie"),
            Status::Hidden => tr("niewidoczny dla innych"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub author: String,
    pub text: String,
    #[serde(default)]
    pub created_at: String,
    /// Odpowiedź autora GRANI (z panelu strony).
    #[serde(default)]
    pub official: bool,
    #[serde(default)]
    pub status: Status,
    /// Własny komentarz (z tej instalacji).
    #[serde(default)]
    pub mine: bool,
}

/// Zdjęcie przy punkcie: z serwera (adresy względne) albo własne, jeszcze na urządzeniu.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Photo {
    #[serde(default)]
    pub id: String,
    /// Pełne zdjęcie (`/api/map-points/photos/…`).
    #[serde(default)]
    pub url: String,
    /// Miniatura.
    #[serde(default)]
    pub thumb: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    /// Własne zdjęcie: przygotowany plik na tym urządzeniu (do wysyłki i podglądu).
    #[serde(default)]
    pub local: String,
    #[serde(default)]
    pub status: Status,
    #[serde(default)]
    pub mine: bool,
}

impl Photo {
    /// Klucz podglądu: plik lokalny albo adres na serwerze.
    pub fn source(&self, large: bool) -> String {
        if !self.local.is_empty() && Path::new(&self.local).exists() {
            return self.local.clone();
        }
        if large || self.thumb.is_empty() {
            self.url.clone()
        } else {
            self.thumb.clone()
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub id: String,
    pub lon: f64,
    pub lat: f64,
    #[serde(default)]
    pub ele: Option<f32>,
    pub category: Category,
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub comments: Vec<Comment>,
    #[serde(default)]
    pub photos: Vec<Photo>,
    #[serde(default)]
    pub status: Status,
    #[serde(default)]
    pub mine: bool,
    /// Oceny innych użytkowników: +1 i −1.
    #[serde(default)]
    pub up: u32,
    #[serde(default)]
    pub down: u32,
}

impl Point {
    /// Bilans ocen.
    pub fn score(&self) -> i64 {
        self.up as i64 - self.down as i64
    }

    /// Data utworzenia do wyświetlenia („2026-11-18”).
    pub fn date(&self) -> &str {
        self.created_at.get(..10).unwrap_or("")
    }

    /// Własne zdjęcia przy tym punkcie (do limitu).
    pub fn my_photos(&self) -> usize {
        self.photos.iter().filter(|p| p.mine).count()
    }
}

/// Wpis czekający na wysłanie.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
enum Outgoing {
    Point { local: String },
    Comment { point: String, local: String },
    /// Zdjęcie: `local` – ścieżka przygotowanego pliku (jednocześnie identyfikator do wysłania).
    Photo { point: String, local: String },
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    #[serde(default)]
    client: String,
    #[serde(default)]
    mine: Vec<Point>,
    #[serde(default)]
    outbox: Vec<Outgoing>,
    #[serde(default)]
    cache: Vec<Point>,
    /// Własne oceny punktów (+1 / −1).
    #[serde(default)]
    votes: HashMap<String, i8>,
}

#[derive(Default)]
struct State {
    saved: Saved,
    server: HashMap<String, Point>,
    cells: HashMap<(i32, i32), Instant>,
    busy: bool,
    sending: bool,
    last_flush: Option<Instant>,
    error: Option<String>,
    version: u64,
    ctx: Option<eframe::egui::Context>,
    /// Zdjęcia przygotowane w tle (albo błąd), do odebrania przez formularz.
    prepared: Vec<Result<PathBuf, String>>,
    preparing: usize,
}

/// Punkty społeczności (tani do klonowania).
#[derive(Clone, Default)]
pub struct Points {
    inner: Arc<Mutex<State>>,
    account: Account,
    textures: Textures,
}

fn file() -> PathBuf {
    crate::settings::config_dir().join("punkty.json")
}

/// Folder przygotowanych zdjęć (czekających na wysyłkę i zatwierdzenie).
fn photo_dir() -> PathBuf {
    crate::settings::config_dir().join("zdjecia")
}

fn now_iso() -> String {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    chrono::DateTime::from_timestamp(s, 0).map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string()).unwrap_or_default()
}

fn random_id() -> String {
    use sha2::Digest;
    let seed = format!("{:?}{:?}{}{:?}", std::time::SystemTime::now(), std::thread::current().id(), std::process::id(), Instant::now());
    let h = sha2::Sha256::digest(seed.as_bytes());
    h.iter().take(12).map(|b| format!("{b:02x}")).collect()
}

fn agent() -> &'static ureq::Agent {
    crate::download::agent()
}

/// Odległość w metrach (wzór haversine).
pub fn distance(lon1: f64, lat1: f64, lon2: f64, lat2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let (dp, dl) = ((lat2 - lat1).to_radians(), (lon2 - lon1).to_radians());
    let h = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * 6_371_000.0 * h.sqrt().min(1.0).asin()
}

/// Zdjęcie gotowe do wysłania: obrót według EXIF, najwyżej 2048 px, JPEG 75 % bez metadanych.
pub fn prepare_photo(src: &Path) -> Result<PathBuf, String> {
    prepare_into(src, &photo_dir())
}

fn prepare_into(src: &Path, dir: &Path) -> Result<PathBuf, String> {
    use image::ImageDecoder;
    let unsupported = || crate::i18n::tr("Nieobsługiwany format zdjęcia – wybierz JPEG, PNG albo WEBP.").to_string();
    let reader = image::ImageReader::open(src).map_err(|e| e.to_string())?.with_guessed_format().map_err(|e| e.to_string())?;
    let mut decoder = reader.into_decoder().map_err(|_| unsupported())?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = image::DynamicImage::from_decoder(decoder).map_err(|_| unsupported())?;
    img.apply_orientation(orientation);
    if img.width().max(img.height()) > PHOTO_MAX {
        img = img.resize(PHOTO_MAX, PHOTO_MAX, image::imageops::FilterType::Lanczos3);
    }
    let rgb = img.to_rgb8();
    let mut out = std::io::Cursor::new(Vec::new());
    rgb.write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, PHOTO_QUALITY)).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let file = dir.join(format!("{}.jpg", random_id()));
    std::fs::write(&file, out.into_inner()).map_err(|e| e.to_string())?;
    Ok(file)
}

impl Points {
    pub fn load() -> Self {
        let mut saved: Saved = std::fs::read(file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        if saved.client.is_empty() {
            saved.client = random_id();
        }
        let server = saved.cache.drain(..).map(|p| (p.id.clone(), p)).collect();
        let me = Self { inner: Arc::new(Mutex::new(State { saved, server, ..Default::default() })), account: Account::load(), textures: Textures::default() };
        me.persist();
        me
    }

    /// Konto użytkownika (logowanie, rejestracja).
    pub fn account(&self) -> &Account {
        &self.account
    }

    /// Podglądy zdjęć (tekstury wczytywane w tle).
    pub fn textures(&self) -> &Textures {
        &self.textures
    }

    fn persist(&self) {
        let s = self.inner.lock().unwrap();
        let mut cache: Vec<Point> = s.server.values().cloned().collect();
        cache.truncate(CACHE_MAX);
        let out = serde_json::json!({
            "client": s.saved.client,
            "mine": s.saved.mine,
            "outbox": s.saved.outbox,
            "cache": cache,
            "votes": s.saved.votes,
        });
        drop(s);
        let _ = std::fs::create_dir_all(crate::settings::config_dir());
        let _ = crate::pack::write_file(&file(), out.to_string().as_bytes());
    }

    fn changed(s: &mut State) {
        s.version += 1;
        if let Some(c) = &s.ctx {
            c.request_repaint();
        }
    }

    pub fn version(&self) -> u64 {
        self.inner.lock().unwrap().version
    }

    pub fn busy(&self) -> bool {
        let s = self.inner.lock().unwrap();
        s.busy || s.sending
    }

    pub fn error(&self) -> Option<String> {
        self.inner.lock().unwrap().error.clone()
    }

    pub fn queued(&self) -> usize {
        self.inner.lock().unwrap().saved.outbox.len()
    }

    /// Zdjęcie wybrane w oknie systemu – przygotowanie w tle (wynik: [`Points::take_prepared`]).
    pub fn prepare(&self, src: PathBuf) {
        self.inner.lock().unwrap().preparing += 1;
        let me = self.clone();
        std::thread::spawn(move || {
            let r = prepare_photo(&src);
            if let Err(e) = &r {
                log::warn!("zdjęcie {}: {e}", src.display());
            }
            // kopia z okna systemu (folder tymczasowy) nie jest już potrzebna
            if src.starts_with(std::env::temp_dir()) {
                let _ = std::fs::remove_file(&src);
            }
            let mut s = me.inner.lock().unwrap();
            s.preparing = s.preparing.saturating_sub(1);
            s.prepared.push(r);
            Self::changed(&mut s);
        });
    }

    /// Ile zdjęć jest jeszcze przygotowywanych.
    pub fn preparing(&self) -> usize {
        self.inner.lock().unwrap().preparing
    }

    /// Przygotowane zdjęcia od ostatniego odczytu.
    pub fn take_prepared(&self) -> Vec<Result<PathBuf, String>> {
        std::mem::take(&mut self.inner.lock().unwrap().prepared)
    }

    /// Wszystkie punkty do pokazania: z serwera i własne (własne wersje mają pierwszeństwo).
    pub fn all(&self) -> Vec<Point> {
        let s = self.inner.lock().unwrap();
        let mut out: Vec<Point> = s.saved.mine.iter().filter(|p| p.status != Status::Hidden || p.mine).cloned().collect();
        for p in s.server.values() {
            if let Some(m) = out.iter_mut().find(|m| m.id == p.id) {
                // własny punkt już na serwerze: wpisy z serwera + własne oczekujące
                let own: Vec<Comment> = m.comments.iter().filter(|c| c.mine && c.status != Status::Approved).cloned().collect();
                let own_photos: Vec<Photo> = m.photos.iter().filter(|f| f.mine && f.status != Status::Approved).cloned().collect();
                let mut merged = p.clone();
                merged.mine = m.mine;
                merged.comments.extend(own.into_iter().filter(|c| !p.comments.iter().any(|x| x.id == c.id && !c.id.is_empty())));
                merged.photos.extend(own_photos.into_iter().filter(|f| !p.photos.iter().any(|x| x.id == f.id && !f.id.is_empty())));
                *m = merged;
            } else {
                let mut p = p.clone();
                // własne wpisy pod cudzymi punktami
                if let Some(own) = s.saved.mine.iter().find(|m| m.id == p.id) {
                    p.comments.extend(own.comments.iter().filter(|c| c.mine && c.status != Status::Approved).cloned());
                    p.photos.extend(own.photos.iter().filter(|f| f.mine && f.status != Status::Approved).cloned());
                }
                out.push(p);
            }
        }
        // zdjęcia z serwera dodane z tego urządzenia liczą się do limitu
        let mine_ids: Vec<String> = s.saved.mine.iter().flat_map(|m| m.photos.iter().filter(|f| f.mine && !f.id.is_empty()).map(|f| f.id.clone())).collect();
        for p in &mut out {
            for f in &mut p.photos {
                if mine_ids.contains(&f.id) {
                    f.mine = true;
                }
            }
        }
        out
    }

    pub fn get(&self, id: &str) -> Option<Point> {
        self.all().into_iter().find(|p| p.id == id)
    }

    /// Opublikowane i własne punkty w promieniu `radius` m (od najbliższego) – propozycja
    /// dołączenia nowego wpisu do istniejącego punktu.
    pub fn nearby(&self, lon: f64, lat: f64, radius: f64) -> Vec<(f64, Point)> {
        let mut near: Vec<(f64, Point)> = self
            .all()
            .into_iter()
            .filter(|p| !p.id.starts_with("local-") && p.status != Status::Hidden)
            .map(|p| (distance(lon, lat, p.lon, p.lat), p))
            .filter(|(d, _)| *d <= radius)
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        near
    }

    /// Punkty z okolicy prostokąta (w, s, e, n); pobiera brakujące komórki w tle.
    pub fn want(&self, ctx: &eframe::egui::Context, w: f64, s_: f64, e: f64, n: f64) {
        self.account.fetch_info(ctx);
        let cells: Vec<(i32, i32)> = {
            let mut st = self.inner.lock().unwrap();
            if st.ctx.is_none() {
                st.ctx = Some(ctx.clone());
            }
            if st.busy || crate::offline::is_offline() {
                return;
            }
            let (x0, x1) = ((w / CELL).floor() as i32, (e / CELL).floor() as i32);
            let (y0, y1) = ((s_ / CELL).floor() as i32, (n / CELL).floor() as i32);
            if (x1 - x0) * (y1 - y0) > 64 {
                return; // widok z bardzo daleka – punkty pokazujemy dopiero bliżej
            }
            let cells: Vec<(i32, i32)> = (y0..=y1)
                .flat_map(|y| (x0..=x1).map(move |x| (x, y)))
                .filter(|c| st.cells.get(c).is_none_or(|t| t.elapsed() > REFRESH))
                .collect();
            if cells.is_empty() {
                return;
            }
            for c in &cells {
                st.cells.insert(*c, Instant::now());
            }
            st.busy = true;
            cells
        };
        let me = self.clone();
        std::thread::spawn(move || {
            let (mut w, mut s, mut e, mut n) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for (x, y) in &cells {
                w = w.min(*x as f64 * CELL);
                e = e.max((*x + 1) as f64 * CELL);
                s = s.min(*y as f64 * CELL);
                n = n.max((*y + 1) as f64 * CELL);
            }
            let r = fetch(w, s, e, n);
            let mut st = me.inner.lock().unwrap();
            st.busy = false;
            match r {
                Ok(list) => {
                    st.error = None;
                    // punkty z tego prostokąta, których już nie ma (ukryte, scalone), znikają
                    st.server.retain(|_, p| !(p.lon >= w && p.lon <= e && p.lat >= s && p.lat <= n));
                    let mut done_files = Vec::new();
                    for p in list {
                        // własne wpisy zatwierdzone
                        if let Some(m) = st.saved.mine.iter_mut().find(|m| m.id == p.id) {
                            m.status = Status::Approved;
                            for c in &mut m.comments {
                                if p.comments.iter().any(|x| x.id == c.id) {
                                    c.status = Status::Approved;
                                }
                            }
                            for f in &mut m.photos {
                                if !f.id.is_empty() && p.photos.iter().any(|x| x.id == f.id) {
                                    f.status = Status::Approved;
                                    // opublikowane zdjęcie jest już na serwerze – kopia lokalna zbędna
                                    if !f.local.is_empty() {
                                        done_files.push(std::mem::take(&mut f.local));
                                    }
                                }
                            }
                        }
                        st.server.insert(p.id.clone(), p);
                    }
                    Self::changed(&mut st);
                    drop(st);
                    for f in done_files {
                        let _ = std::fs::remove_file(f);
                    }
                    me.persist();
                }
                Err(e) => {
                    log::warn!("punkty społeczności: {e}");
                    st.error = Some(e);
                    for c in &cells {
                        st.cells.remove(c);
                    }
                    Self::changed(&mut st);
                }
            }
        });
        self.flush();
    }

    fn own_photos(files: &[PathBuf], author: &str) -> Vec<Photo> {
        files
            .iter()
            .map(|f| Photo { local: f.to_string_lossy().into_owned(), author: author.to_string(), created_at: now_iso(), status: Status::Queued, mine: true, ..Default::default() })
            .collect()
    }

    /// Nowy punkt z pierwszym komentarzem i zdjęciami; wysyłany od razu albo po powrocie sieci.
    pub fn add_point(&self, lon: f64, lat: f64, ele: Option<f32>, category: Category, title: &str, text: &str, photos: &[PathBuf]) {
        let local = format!("local-{}", random_id());
        let author = self.account.name();
        {
            let mut s = self.inner.lock().unwrap();
            let comment = Comment { id: String::new(), author: author.clone(), text: text.trim().into(), created_at: now_iso(), official: false, status: Status::Queued, mine: true };
            s.saved.mine.push(Point {
                id: local.clone(),
                lon,
                lat,
                ele,
                category,
                title: title.trim().into(),
                author: author.clone(),
                created_at: now_iso(),
                comments: if text.trim().is_empty() { Vec::new() } else { vec![comment] },
                photos: Self::own_photos(photos, &author),
                status: Status::Queued,
                mine: true,
                up: 0,
                down: 0,
            });
            s.saved.outbox.push(Outgoing::Point { local: local.clone() });
            for f in photos {
                s.saved.outbox.push(Outgoing::Photo { point: local.clone(), local: f.to_string_lossy().into_owned() });
            }
            Self::changed(&mut s);
        }
        self.persist();
        self.flush_now();
    }

    /// Komentarz i/lub zdjęcia pod punktem (własnym albo cudzym).
    pub fn add_comment(&self, point: &Point, text: &str, photos: &[PathBuf]) {
        let local = format!("local-{}", random_id());
        let author = self.account.name();
        {
            let mut s = self.inner.lock().unwrap();
            let comment = (!text.trim().is_empty()).then(|| Comment { id: local.clone(), author: author.clone(), text: text.trim().into(), created_at: now_iso(), official: false, status: Status::Queued, mine: true });
            let new_photos = Self::own_photos(photos, &author);
            match s.saved.mine.iter_mut().find(|m| m.id == point.id) {
                Some(m) => {
                    m.comments.extend(comment.clone());
                    m.photos.extend(new_photos);
                }
                None => {
                    // cudzy punkt: kopia jako nośnik własnych wpisów
                    let mut p = point.clone();
                    p.mine = false;
                    p.comments = comment.clone().into_iter().collect();
                    p.photos = new_photos;
                    s.saved.mine.push(p);
                }
            }
            if comment.is_some() {
                s.saved.outbox.push(Outgoing::Comment { point: point.id.clone(), local });
            }
            for f in photos {
                s.saved.outbox.push(Outgoing::Photo { point: point.id.clone(), local: f.to_string_lossy().into_owned() });
            }
            Self::changed(&mut s);
        }
        self.persist();
        self.flush_now();
    }

    /// Moja ocena punktu: 1, −1 albo 0 (brak).
    pub fn my_vote(&self, id: &str) -> i8 {
        self.inner.lock().unwrap().saved.votes.get(id).copied().unwrap_or(0)
    }

    /// Ocena punktu (+1, −1, 0 – cofnięcie) wysyłana w tle; liczniki odświeżają się po odpowiedzi.
    pub fn vote(&self, id: &str, value: i8) -> Result<(), String> {
        if crate::offline::is_offline() {
            return Err(crate::i18n::tr("Tryb offline – ocenisz punkt po połączeniu z siecią.").into());
        }
        if id.starts_with("local-") {
            return Err(crate::i18n::tr("punkt nie został jeszcze wysłany").into());
        }
        let Some(token) = self.account.token() else {
            return Err(crate::i18n::tr("Zaloguj się, żeby oceniać punkty.").into());
        };
        let prev = {
            let mut s = self.inner.lock().unwrap();
            let prev = s.saved.votes.get(id).copied().unwrap_or(0);
            if value == 0 {
                s.saved.votes.remove(id);
            } else {
                s.saved.votes.insert(id.to_string(), value);
            }
            // od razu w liczniku (poprawione odpowiedzią serwera)
            if let Some(p) = s.server.get_mut(id) {
                let dec = |v: &mut u32| *v = v.saturating_sub(1);
                match prev {
                    1 => dec(&mut p.up),
                    -1 => dec(&mut p.down),
                    _ => {}
                }
                match value {
                    1 => p.up += 1,
                    -1 => p.down += 1,
                    _ => {}
                }
            }
            Self::changed(&mut s);
            prev
        };
        self.persist();
        let me = self.clone();
        let id = id.to_string();
        std::thread::spawn(move || {
            let body = serde_json::json!({ "app": "gran-3d", "value": value });
            let r = agent()
                .post(&format!("{API}/{}/vote", crate::osm::urlencode(&id)))
                .timeout(Duration::from_secs(20))
                .set("Content-Type", "application/json")
                .set("Authorization", &format!("Bearer {token}"))
                .send_string(&body.to_string())
                .map_err(|e| me.err(e))
                .and_then(json);
            let mut s = me.inner.lock().unwrap();
            match r {
                Ok(v) => {
                    if let Some(p) = s.server.get_mut(&id) {
                        p.up = v["up"].as_u64().unwrap_or(p.up as u64) as u32;
                        p.down = v["down"].as_u64().unwrap_or(p.down as u64) as u32;
                    }
                }
                Err(e) => {
                    log::warn!("ocena punktu: {e}");
                    s.error = Some(e);
                    if prev == 0 {
                        s.saved.votes.remove(&id);
                    } else {
                        s.saved.votes.insert(id.clone(), prev);
                    }
                }
            }
            Self::changed(&mut s);
        });
        Ok(())
    }

    /// Zgłoszenie punktu do moderacji (spam, nieprawda, obraźliwe).
    pub fn report(&self, id: &str, reason: &str) -> Result<(), String> {
        if crate::offline::is_offline() {
            return Err(crate::i18n::tr("Tryb offline – zgłoszenie wyślesz po połączeniu z siecią.").into());
        }
        let client = self.inner.lock().unwrap().saved.client.clone();
        let body = serde_json::json!({ "app": "gran-3d", "reason": reason, "client": client });
        let mut req = agent().post(&format!("{API}/{}/report", crate::osm::urlencode(id))).timeout(Duration::from_secs(20)).set("Content-Type", "application/json");
        if let Some(t) = self.account.token() {
            req = req.set("Authorization", &format!("Bearer {t}"));
        }
        req.send_string(&body.to_string()).map(|_| ()).map_err(|e| self.err(e))
    }

    /// Usunięcie własnego wpisu z tej instalacji (oczekujący nie zostanie wysłany).
    pub fn forget(&self, id: &str) {
        let files: Vec<String> = {
            let mut s = self.inner.lock().unwrap();
            let files = s.saved.mine.iter().filter(|p| p.id == id).flat_map(|p| p.photos.iter().map(|f| f.local.clone())).filter(|f| !f.is_empty()).collect();
            s.saved.mine.retain(|p| p.id != id);
            s.saved.outbox.retain(|o| !matches!(o, Outgoing::Point { local } if local == id) && !matches!(o, Outgoing::Photo { point, .. } if point == id));
            Self::changed(&mut s);
            files
        };
        for f in files {
            let _ = std::fs::remove_file(f);
        }
        self.persist();
    }

    /// Wysyłka kolejki od razu (np. po zalogowaniu).
    pub fn flush_now(&self) {
        self.inner.lock().unwrap().last_flush = None;
        self.flush();
    }

    /// Wysyłka kolejki (najwyżej raz na minutę, gdy poprzednia się nie udała). Bez zalogowania
    /// kolejka czeka.
    pub fn flush(&self) {
        let Some(token) = self.account.token() else { return };
        let (items, client) = {
            let mut s = self.inner.lock().unwrap();
            if s.sending || s.saved.outbox.is_empty() || crate::offline::is_offline() || s.last_flush.is_some_and(|t| t.elapsed() < Duration::from_secs(60)) {
                return;
            }
            s.sending = true;
            s.last_flush = Some(Instant::now());
            (s.saved.outbox.clone(), s.saved.client.clone())
        };
        let me = self.clone();
        std::thread::spawn(move || {
            for item in items {
                let r = me.send(&item, &client, &token);
                let mut s = me.inner.lock().unwrap();
                match r {
                    Ok(()) => {
                        s.saved.outbox.retain(|o| !same(o, &item));
                    }
                    Err(e) => {
                        log::warn!("wysyłka wpisu: {e}");
                        s.error = Some(e);
                        Self::changed(&mut s);
                        break;
                    }
                }
                Self::changed(&mut s);
            }
            me.inner.lock().unwrap().sending = false;
            me.persist();
        });
    }

    /// Błąd żądania dla użytkownika; odrzucony token wylogowuje (kolejka czeka na ponowne logowanie).
    fn err(&self, e: ureq::Error) -> String {
        let (status, code, msg) = match e {
            ureq::Error::Status(429, _) => return crate::i18n::tr("Za dużo wpisów naraz – spróbuj za kilka minut.").into(),
            ureq::Error::Status(503, _) if crate::offline::is_offline() => return crate::i18n::tr("tryb offline").into(),
            ureq::Error::Status(code, r) => {
                let v = json(r).unwrap_or_default();
                (code, v["code"].as_str().unwrap_or_default().to_string(), v["error"].as_str().unwrap_or_default().to_string())
            }
            ureq::Error::Transport(t) => return tf!("brak połączenia z redzikowski.com: {}", t),
        };
        if status == 401 || code == "auth" {
            let m = crate::i18n::tr("Sesja wygasła – zaloguj się ponownie, a wpisy wyślą się same.");
            self.account.expire(m);
            return m.to_string();
        }
        if msg.is_empty() {
            tf!("serwer odpowiedział kodem {}", status)
        } else {
            msg
        }
    }

    fn send(&self, item: &Outgoing, client: &str, token: &str) -> Result<(), String> {
        let auth = format!("Bearer {token}");
        let base = |m: &mut serde_json::Map<String, serde_json::Value>| {
            m.insert("app".into(), "gran-3d".into());
            m.insert("appVersion".into(), crate::app_version().into());
            m.insert("platform".into(), crate::platform::NAME.into());
            m.insert("locale".into(), crate::i18n::current().into());
            m.insert("client".into(), client.into());
        };
        match item {
            Outgoing::Point { local } => {
                let p = self.inner.lock().unwrap().saved.mine.iter().find(|p| &p.id == local).cloned();
                let Some(p) = p else { return Ok(()) };
                let mut m = serde_json::Map::new();
                base(&mut m);
                m.insert("lon".into(), p.lon.into());
                m.insert("lat".into(), p.lat.into());
                if let Some(e) = p.ele {
                    m.insert("ele".into(), (e as f64).into());
                }
                m.insert("category".into(), serde_json::to_value(p.category).unwrap_or_default());
                m.insert("title".into(), p.title.clone().into());
                m.insert("text".into(), p.comments.first().map(|c| c.text.clone()).unwrap_or_default().into());
                let v: serde_json::Value = agent()
                    .post(API)
                    .timeout(Duration::from_secs(20))
                    .set("Content-Type", "application/json")
                    .set("Authorization", &auth)
                    .send_string(&serde_json::Value::Object(m).to_string())
                    .map_err(|e| self.err(e))
                    .and_then(json)?;
                let id = v["id"].as_str().unwrap_or_default().to_string();
                let status = if v["status"] == "approved" { Status::Approved } else { Status::Pending };
                let comment_id = v["commentId"].as_str().unwrap_or_default().to_string();
                let author = v["author"].as_str().map(String::from);
                let mut s = self.inner.lock().unwrap();
                // kolejne wpisy w kolejce odnoszą się już do identyfikatora z serwera
                for o in &mut s.saved.outbox {
                    if let Outgoing::Comment { point, .. } | Outgoing::Photo { point, .. } = o {
                        if point == local {
                            *point = id.clone();
                        }
                    }
                }
                if let Some(m) = s.saved.mine.iter_mut().find(|p| &p.id == local) {
                    m.id = id;
                    m.status = status;
                    if let Some(a) = author {
                        m.author = a;
                    }
                    if let Some(c) = m.comments.first_mut() {
                        c.status = status;
                        c.id = comment_id;
                    }
                }
                Ok(())
            }
            Outgoing::Comment { point, local } => {
                if point.starts_with("local-") {
                    return Err(crate::i18n::tr("punkt nie został jeszcze wysłany").into());
                }
                let c = self.inner.lock().unwrap().saved.mine.iter().flat_map(|p| p.comments.iter()).find(|c| &c.id == local).cloned();
                let Some(c) = c else { return Ok(()) };
                let mut m = serde_json::Map::new();
                base(&mut m);
                m.insert("text".into(), c.text.clone().into());
                let v: serde_json::Value = agent()
                    .post(&format!("{API}/{}/comments", crate::osm::urlencode(point)))
                    .timeout(Duration::from_secs(20))
                    .set("Content-Type", "application/json")
                    .set("Authorization", &auth)
                    .send_string(&serde_json::Value::Object(m).to_string())
                    .map_err(|e| self.err(e))
                    .and_then(json)?;
                let status = if v["status"] == "approved" { Status::Approved } else { Status::Pending };
                let id = v["id"].as_str().unwrap_or_default().to_string();
                let mut s = self.inner.lock().unwrap();
                for p in &mut s.saved.mine {
                    for cm in &mut p.comments {
                        if &cm.id == local {
                            cm.status = status;
                            cm.id = id.clone();
                        }
                    }
                }
                Ok(())
            }
            Outgoing::Photo { point, local } => {
                if point.starts_with("local-") {
                    return Err(crate::i18n::tr("punkt nie został jeszcze wysłany").into());
                }
                let Ok(bytes) = std::fs::read(local) else {
                    // plik zniknął (np. usunięty wpis) – pomijamy
                    return Ok(());
                };
                let r = agent()
                    .post(&format!("{API}/{}/photos", crate::osm::urlencode(point)))
                    .timeout(Duration::from_secs(120))
                    .set("Content-Type", "image/jpeg")
                    .set("Authorization", &auth)
                    .set("X-Platform", crate::platform::NAME)
                    .set("X-App-Version", crate::app_version())
                    .send_bytes(&bytes);
                let v = match r {
                    Ok(r) => json(r)?,
                    // limit zdjęć albo plik odrzucony: wpis znika z kolejki, komunikat zostaje
                    Err(ureq::Error::Status(code @ (400 | 404 | 409 | 413 | 415), r)) => {
                        let msg = json(r).ok().and_then(|v| v["error"].as_str().map(String::from)).unwrap_or_else(|| tf!("serwer odpowiedział kodem {}", code));
                        let mut s = self.inner.lock().unwrap();
                        for p in &mut s.saved.mine {
                            p.photos.retain(|f| &f.local != local);
                        }
                        s.error = Some(msg);
                        drop(s);
                        let _ = std::fs::remove_file(local);
                        return Ok(());
                    }
                    Err(e) => return Err(self.err(e)),
                };
                let status = if v["status"] == "approved" { Status::Approved } else { Status::Pending };
                let id = v["id"].as_str().unwrap_or_default().to_string();
                let url = v["url"].as_str().unwrap_or_default().to_string();
                let mut s = self.inner.lock().unwrap();
                for p in &mut s.saved.mine {
                    for f in &mut p.photos {
                        if &f.local == local {
                            f.status = status;
                            f.id = id.clone();
                            f.url = url.clone();
                        }
                    }
                }
                Ok(())
            }
        }
    }
}

fn same(a: &Outgoing, b: &Outgoing) -> bool {
    match (a, b) {
        (Outgoing::Point { local: x }, Outgoing::Point { local: y }) => x == y,
        (Outgoing::Comment { local: x, .. }, Outgoing::Comment { local: y, .. }) => x == y,
        (Outgoing::Photo { local: x, .. }, Outgoing::Photo { local: y, .. }) => x == y,
        _ => false,
    }
}

fn json(r: ureq::Response) -> Result<serde_json::Value, String> {
    let t = r.into_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&t).map_err(|e| e.to_string())
}

fn fetch_err(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(503, _) if crate::offline::is_offline() => crate::i18n::tr("tryb offline").into(),
        ureq::Error::Status(code, _) => tf!("serwer odpowiedział kodem {}", code),
        ureq::Error::Transport(t) => tf!("brak połączenia z redzikowski.com: {}", t),
    }
}

/// Zatwierdzone punkty w prostokącie.
fn fetch(w: f64, s: f64, e: f64, n: f64) -> Result<Vec<Point>, String> {
    let url = format!("{API}?app=gran-3d&bbox={w:.4},{s:.4},{e:.4},{n:.4}");
    let v: serde_json::Value = agent().get(&url).timeout(Duration::from_secs(20)).call().map_err(fetch_err).and_then(json)?;
    let list = v["points"].as_array().cloned().unwrap_or_default();
    Ok(list.into_iter().filter_map(|p| serde_json::from_value::<Point>(p).ok()).collect())
}

/* ── podglądy zdjęć ───────────────────────────────────────────────────── */

enum Slot {
    Loading,
    Ready(eframe::egui::TextureHandle),
    Failed,
}

/// Tekstury zdjęć (miniatury i podglądy) wczytywane w tle z pliku albo z redzikowski.com.
#[derive(Clone, Default)]
pub struct Textures {
    inner: Arc<Mutex<HashMap<String, Slot>>>,
}

impl Textures {
    /// Tekstura zdjęcia: `source` – ścieżka pliku albo adres `/api/…`; `large` – podgląd zamiast
    /// miniatury. `None` – jeszcze się wczytuje albo nie udało się (`failed`).
    pub fn get(&self, ctx: &eframe::egui::Context, source: &str, large: bool) -> Option<eframe::egui::TextureHandle> {
        if source.is_empty() {
            return None;
        }
        let key = format!("{}{source}", if large { "L:" } else { "S:" });
        {
            let mut map = self.inner.lock().unwrap();
            match map.get(&key) {
                Some(Slot::Ready(t)) => return Some(t.clone()),
                Some(_) => return None,
                None => {
                    map.insert(key.clone(), Slot::Loading);
                }
            }
        }
        let me = self.clone();
        let ctx = ctx.clone();
        let source = source.to_string();
        std::thread::spawn(move || {
            let bytes = if source.starts_with('/') && !Path::new(&source).exists() {
                if crate::offline::is_offline() {
                    Err("offline".to_string())
                } else {
                    agent().get(&format!("{SITE}{source}")).timeout(Duration::from_secs(60)).call().map_err(|e| e.to_string()).and_then(|r| {
                        let mut buf = Vec::new();
                        std::io::Read::read_to_end(&mut r.into_reader(), &mut buf).map(|_| buf).map_err(|e| e.to_string())
                    })
                }
            } else {
                std::fs::read(&source).map_err(|e| e.to_string())
            };
            let slot = bytes.and_then(|b| image::load_from_memory(&b).map_err(|e| e.to_string())).map(|img| {
                let side = if large { PREVIEW } else { THUMB };
                let img = if img.width().max(img.height()) > side { img.thumbnail(side, side) } else { img };
                let rgba = img.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let color = eframe::egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                ctx.load_texture(format!("zdjecie-{key}"), color, eframe::egui::TextureOptions::LINEAR)
            });
            let slot = match slot {
                Ok(t) => Slot::Ready(t),
                Err(e) => {
                    log::info!("podgląd zdjęcia {source}: {e}");
                    Slot::Failed
                }
            };
            me.inner.lock().unwrap().insert(key, slot);
            ctx.request_repaint();
        });
        None
    }

    /// Wczytanie się nie udało (brak sieci, uszkodzony plik).
    pub fn failed(&self, source: &str, large: bool) -> bool {
        let key = format!("{}{source}", if large { "L:" } else { "S:" });
        matches!(self.inner.lock().unwrap().get(&key), Some(Slot::Failed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_server_point() {
        let old: Point = serde_json::from_value(serde_json::json!({ "id": "x", "lon": 1.0, "lat": 2.0, "category": "parking", "title": "Stary" })).unwrap();
        assert_eq!(old.category, Category::Other, "usunięte rodzaje jako „Inne”");
        for c in Category::ALL {
            let v = serde_json::to_value(c).unwrap();
            assert_eq!(serde_json::from_value::<Category>(v).unwrap(), c);
        }
        let shelter: Category = serde_json::from_value(serde_json::json!("shelter")).unwrap();
        assert_eq!(shelter, Category::Shelter, "dawne „nocleg i schronienie” to schron");
        assert!(old.photos.is_empty());
        let v = serde_json::json!({
            "id": "abc", "lon": 20.0, "lat": 49.2, "ele": 1987.5, "category": "water", "title": "Źródło", "up": 5, "down": 2,
            "author": "Ania", "createdAt": "2026-11-18T10:00:00Z",
            "comments": [{ "id": "c1", "author": "Ania", "text": "Płynie", "createdAt": "2026-11-18T10:00:00Z" },
                         { "id": "c2", "author": "GRAŃ", "text": "Dzięki!", "official": true }],
            "photos": [{ "id": "f1", "url": "/api/map-points/photos/f1.jpg", "thumb": "/api/map-points/photos/f1.jpg?w=480", "author": "Ania", "width": 2048, "height": 1536 }]
        });
        let p: Point = serde_json::from_value(v).unwrap();
        assert_eq!(p.category, Category::Water);
        assert_eq!(p.comments.len(), 2);
        assert!(p.comments[1].official);
        assert_eq!(p.status, Status::Approved);
        assert_eq!(p.date(), "2026-11-18");
        assert_eq!(p.score(), 3);
        assert_eq!(p.photos.len(), 1);
        assert_eq!(p.photos[0].source(false), "/api/map-points/photos/f1.jpg?w=480");
        assert_eq!(p.photos[0].source(true), "/api/map-points/photos/f1.jpg");
    }

    #[test]
    fn outbox_roundtrip() {
        let o = vec![
            Outgoing::Point { local: "local-1".into() },
            Outgoing::Comment { point: "x".into(), local: "local-2".into() },
            Outgoing::Photo { point: "x".into(), local: "C:/zdjecia/a.jpg".into() },
        ];
        let s = serde_json::to_string(&o).unwrap();
        let back: Vec<Outgoing> = serde_json::from_str(&s).unwrap();
        assert!(same(&back[0], &o[0]) && same(&back[1], &o[1]) && same(&back[2], &o[2]));
    }

    #[test]
    fn distance_in_meters() {
        let d = distance(20.0, 49.2, 20.0005, 49.2003);
        assert!((d - 49.0).abs() < 2.0, "{d}");
        assert!(distance(20.0, 49.0, 20.0, 49.0) < 1e-6);
    }

    #[test]
    fn photo_is_scaled_and_reencoded() {
        let dir = std::env::temp_dir().join(format!("gran-zdjecie-{}", random_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("duze.png");
        image::RgbImage::from_pixel(3000, 1000, image::Rgb([40, 120, 60])).save(&src).unwrap();
        let out = prepare_into(&src, &dir.join("zdjecia")).unwrap();
        let img = image::open(&out).unwrap();
        assert_eq!(img.width(), 2048);
        assert!((682..=683).contains(&img.height()), "{}", img.height());
        // małe zdjęcie zostaje w swoim rozmiarze
        let small = dir.join("male.png");
        image::RgbImage::from_pixel(800, 600, image::Rgb([200, 200, 200])).save(&small).unwrap();
        let out = prepare_into(&small, &dir.join("zdjecia")).unwrap();
        assert_eq!(image::image_dimensions(&out).unwrap(), (800, 600));
        assert_eq!(image::ImageFormat::from_path(&out).unwrap(), image::ImageFormat::Jpeg);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
