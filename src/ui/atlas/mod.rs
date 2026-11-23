//! Atlas 3D: kamera nad terenem, narzędzia (eksploracja, trasa, pomiar), soczewki, analizy w tle,
//! etykiety, znaczniki i profil. Panele boczne – w `panels.rs`.

use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use eframe::egui_wgpu;
use glam::DVec3;
use serde::{Deserialize, Serialize};

use crate::analysis;
use crate::fmt;
use crate::geo::{bearing, haversine, lat_to_y, lon_to_x, world_m, BBox};
use crate::graph::{route_from_points, Route, RouteKind, Snap};
use crate::heights::{Grid, Heights};
use crate::metrics::{self, TimeModel};
use crate::osm::{PoiKind, SectorId};
use crate::render::camera::{Camera, Frame, View};
use crate::render::{LineInst, Renderer, Scene, TreeInst};
use crate::sun::{self, Day};
use crate::ui::theme;
use crate::world::{Snapshot, World};

mod actions;
mod community;
mod input;
mod jobs;
mod locate;
mod nav;
mod overlays;
mod prefs;
mod route;
mod saved;
mod scene;
mod tools;
mod view;
mod webcam;

pub use actions::*;
pub use locate::{Follow, GpsState};
pub use prefs::*;
pub use tools::*;
pub use webcam::CamState;
use jobs::RoadLook;
use overlays::*;
use scene::*;
use webcam::*;

#[derive(Clone, Debug)]
pub struct Waypoint {
    pub lon: f64,
    pub lat: f64,
    pub e: f32,
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Selection {
    None,
    Poi(usize),
    Point { lon: f64, lat: f64 },
    Trail(u32),
    /// Punkt społeczności (identyfikator z redzikowski.com albo lokalny).
    UserPoint(String),
}

/// Szkic nowego punktu społeczności.
#[derive(Clone, Debug)]
pub struct PointDraft {
    pub lon: f64,
    pub lat: f64,
    pub ele: f32,
    pub category: crate::points::Category,
    pub title: String,
    pub text: String,
    /// Przygotowane zdjęcia (JPEG 2048 px) do wysłania razem z punktem.
    pub photos: Vec<std::path::PathBuf>,
}

/// Okno konta: logowanie, rejestracja, nowe hasło albo usunięcie konta.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AccountMode {
    #[default]
    Login,
    Register,
    Forgot,
    Delete,
}

/// Formularz okna konta (hasła żyją tylko tutaj, do wysłania).
#[derive(Clone, Debug, Default)]
pub struct AccountForm {
    pub mode: AccountMode,
    pub email: String,
    pub password: String,
    pub password2: String,
    pub name: String,
    pub consent: bool,
}

/// Szkic obszaru offline: narożniki i ustawienia pobierania.
#[derive(Clone, Debug)]
pub struct AreaDraft {
    pub a: Option<(f64, f64)>,
    pub b: Option<(f64, f64)>,
    pub detail: crate::offline::Detail,
    pub ortho: bool,
    pub name: String,
}

impl Default for AreaDraft {
    fn default() -> Self {
        Self { a: None, b: None, detail: crate::offline::Detail::Standard, ortho: false, name: String::new() }
    }
}

/// Pytanie do potwierdzenia i działanie po wybraniu pierwszego przycisku.
/// Pytanie i działanie po wyborze (indeks przycisku; `usize::MAX` – zamknięte bez wyboru).
pub type Confirm = (crate::ui::dialog::Ask, Box<dyn FnOnce(&mut Atlas, usize)>);

/// Wynik analizy widoczności z punktu.
pub struct VsInfo {
    pub lon: f64,
    pub lat: f64,
    pub area_km2: f64,
    pub observer: f32,
    /// Widoczne punkty (indeks w migawce, odległość m).
    pub visible: Vec<(usize, f64)>,
    pub eye: f32,
}

/// Wynik czasu dojścia z punktu.
pub struct IsoInfo {
    /// Najdłuższy liczony czas (h).
    pub hours: f32,
    /// Obszar osiągalny w tym czasie (km²).
    pub area_km2: f64,
    /// Osiągalne szczyty, schroniska, przełęcze i stawy (indeks w migawce, czas w s), od najbliższych.
    pub pois: Vec<(usize, f32)>,
}

/// Wynik statystyk wysokości względnej.
pub struct RelInfo {
    pub above: f64,
    pub below: f64,
}

struct FlyTo {
    from: Camera,
    to: Camera,
    t0: Instant,
    dur: f32,
}

pub struct Flight {
    pub dist: f32,
    pub paused: bool,
    last: Instant,
    heading: f64,
    pitch: f64,
    /// Środek widocznej części mapy względem środka ekranu (px), wygładzany przy chowaniu pasków.
    shift: Vec2,
}

/// Odległość kamery od punktu trasy w przelocie (m).
const FLIGHT_DIST: f64 = 1300.0;

/// Nachylenia w przelocie: najpierw obrót przy najniższym, wyżej dopiero gdy żaden kierunek nie jest wolny.
const FLIGHT_PITCHES: [f64; 5] = [24.0, 34.0, 45.0, 58.0, 72.0];

/// Czy teren nie zasłania punktu trasy (`lon`, `lat`, `e`) oglądanego z danego kierunku i nachylenia.
fn sight_clear(h: &crate::heights::Heights, exag: f64, (lon, lat, e): (f64, f64, f32), heading: f64, pitch: f64) -> bool {
    let (hr, pr) = (heading.to_radians(), pitch.to_radians());
    let kx = 111_320.0 * lat.to_radians().cos();
    (2..=24).all(|i| {
        let t = i as f64 / 24.0;
        let along = FLIGHT_DIST * t * pr.cos();
        let y = e as f64 * exag + FLIGHT_DIST * t * pr.sin();
        // kamera stoi za punktem, przeciwnie do kierunku patrzenia
        let ground = h.sample_cached(lon - hr.sin() * along / kx, lat - hr.cos() * along / 110_574.0);
        ground.is_none_or(|g| y > g as f64 * exag + 12.0)
    })
}

enum JobResult {
    Routes(u64, Vec<Route>),
    RouteFail(u64, String),
    Viewshed(u64, Box<(VsInfo, Vec<u8>, u32, u32, [f64; 4])>),
    LiveVs(u64, Box<(Vec<u8>, u32, u32, [f64; 4])>),
    /// Mapa cieni (`false`) albo nasłonecznienia (`true`), bok, prostokąt świata.
    Shadow(bool, Box<(Vec<u8>, u32, u32, [f64; 4])>),
    /// Czas dojścia z przypiętego punktu: maska 0..254 / 255, bok, prostokąt świata.
    Iso(u64, Box<(IsoInfo, Vec<u8>, u32, u32, [f64; 4])>),
    Rel(u64, RelInfo),
    Los(u64, Option<f64>, f64),
    Land(Option<Box<(Vec<u8>, u32, [f64; 4], Option<(Vec<f32>, [f64; 4])>)>>),
    /// Drzewa: klucz, instancje (najpierw bliskie), liczba bliskich.
    Trees(TreesKey, Vec<TreeInst>, usize),
    /// Linie dróg i ulic wokół kamery: klucz i odcinki.
    Roads(RoadsKey, Vec<LineInst>),
}

/// Kratka położenia celu kamery, przedział oddalenia, wersja dróg, wersja ramki, styl papierowy, skala pikseli.
type RoadsKey = (i64, i64, i64, u64, u64, RoadLook, u32);

/// Położenie (co 200 m), oko (co 150 m), wersja pokrycia i ramki – po zmianie drzewa liczone od nowa.
type TreesKey = (i64, i64, i64, i64, u64, u64);

/// Mozaika pokrycia terenu trzymana do rozmieszczania drzew: dane RGBA, bok, prostokąt świata.
type LandData = Arc<(Vec<u8>, u32, [f64; 4])>;

/// Prośby atlasu do aplikacji.
#[derive(Default)]
pub struct Requests {
    pub open_settings: bool,
    /// Okno ustawień na pierwszej stronie (paleta poleceń, Ctrl+,).
    pub open_preferences: bool,
    pub check_updates: bool,
}

pub struct Atlas {
    pub prefs: Prefs,
    pub cam: Camera,
    pub frame: Frame,
    frame_version: u64,
    pub heights: Heights,
    pub world: World,
    pub renderer: Option<Renderer>,
    pub snap: Arc<Snapshot>,
    pub tool: Tool,
    pub lens: Lens,
    pub lens_pin: Option<(f64, f64, f32)>,
    /// Soczewka sprzed przypięcia z panelu albo menu – wraca po odpięciu.
    lens_back: Option<Lens>,
    pub waypoints: Vec<Waypoint>,
    pub route_mode: RouteMode,
    pub routes: Vec<Route>,
    pub route_sel: usize,
    pub routing: bool,
    pub route_error: Option<String>,
    route_gen: u64,
    route_dirty: bool,
    route_snap_version: u64,
    pub gpx: Option<Route>,
    pub measure: Vec<(f64, f64, f32)>,
    pub los: Option<(Option<f64>, f64)>,
    pub selection: Selection,
    pub hover: Option<(f64, f64, f32)>,
    pub profile_cursor: Option<f32>,
    /// Dystans na trasie wskazany w panelu bocznym (mały profil, lista punktów) – znacznik na mapie i profilu.
    pub panel_cursor: Option<f32>,
    pub vs: Option<VsInfo>,
    pub vs_busy: bool,
    pub rel: Option<RelInfo>,
    pub iso: Option<IsoInfo>,
    live_vs: Option<(f64, f64)>,
    live_vs_busy: bool,
    /// Siatka wysokości do widoczności na żywo: środek i dane.
    live_grid: Arc<std::sync::Mutex<Option<(f64, f64, Arc<Grid>)>>>,
    live_vs_at: Instant,
    shadow_key: Option<(i64, i64, i64, i64)>,
    shadow_busy: bool,
    shadow_rect: Option<[f64; 4]>,
    /// Tekstura cieni niesie nasłonecznienie dnia, nie cień chwili.
    shadow_insol: bool,
    /// Czas dojścia jest liczony w tle.
    pub iso_busy: bool,
    vs_rect: Option<[f64; 4]>,
    live_rect: Option<[f64; 4]>,
    land_key: Option<(SectorId, u64, (i64, i64))>,
    water_rect: Option<[f64; 4]>,
    /// Początek czasu animacji (fale).
    epoch: Instant,
    land_busy: bool,
    land_rect: Option<[f64; 4]>,
    land_data: Option<LandData>,
    land_ver: u64,
    trees: Vec<TreeInst>,
    trees_near: usize,
    trees_key: Option<TreesKey>,
    trees_busy: bool,
    trees_ver: u64,
    /// Spacer: wysokość skoku nad ziemią (m) i prędkość pionowa (m/s).
    walk_up: f64,
    walk_vy: f64,
    /// Pamięć zasłaniania etykiet: położenie oka, czas, wyniki dla punktów.
    occl_cache: std::cell::RefCell<((i64, i64, i64, i64), Instant, std::collections::HashMap<(i64, i64), bool>)>,
    /// Poziom płaszczyzny siatki UTM (dno terenu w okolicy): komórka okolicy, czas odświeżenia, cel, bieżąca wysokość, poprzednia klatka.
    grid_base: std::cell::RefCell<((i64, i64), Instant, Option<f32>, f32, Instant)>,
    gen: u64,
    tx: std::sync::mpsc::Sender<JobResult>,
    rx: Receiver<JobResult>,
    fly: Option<FlyTo>,
    pub flight: Option<Flight>,
    pub panorama: Option<Camera>,
    drag_grab: Option<DVec3>,
    /// Punkt terenu, wokół którego obraca się kamera podczas przeciągania prawym przyciskiem.
    orbit_pivot: Option<(f64, f64, f32)>,
    drag_waypoint: Option<usize>,
    pub ctx_menu: Option<(Pos2, f64, f64, f32)>,
    pub search: Option<crate::ui::search::Search>,
    /// Paleta poleceń (Ctrl+K).
    pub palette: Option<crate::ui::palette::Palette>,
    pub help: bool,
    pub tab: crate::ui::panels::Tab,
    pub toast: Option<(String, Instant)>,
    pub last_view: Option<View>,
    fps: (Instant, u32, f32),
    screenshot: Option<std::path::PathBuf>,
    pub requests: Requests,
    /// Zapisane trasy, miejsca i widoki.
    pub library: crate::library::Library,
    /// Historia punktów trasy do cofania (Ctrl+Z) i ponawiania (Ctrl+Y).
    undo: Vec<RouteState>,
    redo: Vec<RouteState>,
    hist_now: RouteState,
    /// Eksport trasy z biblioteki, gdy tylko zostanie wyznaczona.
    export_pending: bool,
    /// Szkło interfejsu (prostokąty ekranu i siła): pod nim obraz 3D jest rozmyty.
    pub glass: Vec<(Rect, f32)>,
    /// Prostokąty interfejsu nad mapą z ostatniej klatki (paski, panele, przyciski) – etykiety ich omijają.
    ui_rects: Vec<Rect>,
    /// Klikalne obszary narysowanych etykiet (napis z kropką) i indeksy punktów.
    label_hits: Vec<(Rect, usize)>,
    /// Prognozy pogody i komunikaty lawinowe (pobierane w tle).
    pub weather: crate::weather::Weather,
    pub avalanche: crate::avalanche::Avalanche,
    /// Komunikat turystyczny TPN.
    pub notices: crate::notices::Notices,
    /// Miejsce wybrane do zakładki „Warunki” (inaczej zaznaczenie albo środek widoku).
    pub cond_at: Option<crate::ui::conditions::Place>,
    /// Okno ze szczegółami wczytywania danych (klik w stan na dolnym pasku).
    pub load_details: bool,
    /// Obraz kamery internetowej: adres, wynik pobierania, tekstura, czas pobrania.
    webcam: Option<CamSlot>,
    /// Przekrój terenu wzdłuż punktów pomiaru: klucz punktów i „trasa” z wysokościami.
    pub section: Option<(u64, Route)>,
    /// Zaznaczony odcinek profilu (dystanse od startu, m).
    pub profile_sel: Option<(f32, f32)>,
    /// Trasa do porównania: nazwa i ślad.
    pub compare: Option<(String, Route)>,
    last_want: Instant,
    pub status_line: String,
    /// Drogi i ulice: gotowe linie, klucz ostatniego zlecenia, wersja dla renderera.
    road_lines: Vec<LineInst>,
    roads_key: Option<RoadsKey>,
    roads_job: bool,
    roads_ver: u64,
    /// Punkty społeczności, szkic nowego punktu i klikalne znaczniki na mapie.
    pub points: crate::points::Points,
    pub point_draft: Option<PointDraft>,
    /// Okno konta (logowanie, rejestracja) i polityka prywatności.
    pub account_form: Option<AccountForm>,
    pub privacy_open: bool,
    point_hits: Vec<(Rect, String)>,
    /// Obszary offline i szkic nowego obszaru.
    pub offline: crate::offline::Offline,
    pub area_draft: Option<AreaDraft>,
    /// Studio (konta z dostępem, komputer): obszar, ustawienia, przygotowane dane i eksporty.
    pub studio: crate::studio::Studio,
    /// Okno z informacją o Studio dla kont bez dostępu (Windows), jego przykładowa mapa
    /// i prośba o otwarcie formularza kontaktu (obsługuje ją `app.rs`).
    pub studio_intro: bool,
    pub studio_sample: Option<egui::TextureHandle>,
    pub studio_contact: bool,
    /// Pytanie czekające na odpowiedź (usuwanie z biblioteki itp.).
    pub confirm: Option<Confirm>,
    /// Dotyk: trwa gest dwoma palcami (do podniesienia wszystkich palców przesuwanie jednym jest wstrzymane).
    pinch: bool,
    /// Ostatnie położenie palca na mapie (dotknięcie kończy się bez położenia wskaźnika).
    touch_last: Option<Pos2>,
    /// Drążek chodzenia w spacerze na ekranie dotykowym (x – w bok, y – przód/tył, −1…1).
    pub walk_stick: Vec2,
    /// Wąski ekran (telefon): dolny pasek narzędzi i arkusz zamiast panelu bocznego.
    pub compact: bool,
    /// Wysokość arkusza szczegółów na wąskim ekranie; arkusz rozwinięty na cały ekran.
    pub sheet_h: f32,
    pub sheet_full: bool,
    /// Pasek stanu i pasek nawigacji systemu (punkty) – interfejs zaczyna się pod i kończy nad nimi.
    pub safe_top: f32,
    pub safe_bottom: f32,
    /// Przewinięcie dolnego paska stanu na wąskim ekranie (punkty).
    status_scroll: f32,
    /// Położenie GPS na mapie (telefon).
    pub gps: GpsState,
    /// Trwające nagrywanie przejścia.
    pub rec: Option<crate::recording::Recorder>,
}

/// Stan planowania do historii: punkty i tryb wyznaczania.
type RouteState = (Vec<Waypoint>, RouteMode);

fn same_state(a: &RouteState, b: &RouteState) -> bool {
    a.1 == b.1 && a.0.len() == b.0.len() && a.0.iter().zip(&b.0).all(|(x, y)| x.lon == y.lon && x.lat == y.lat)
}

/// Bezpieczna nazwa pliku z nazwy trasy.
fn file_stem(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect();
    let s = s.trim().to_string();
    if s.is_empty() { "trasa".into() } else { s }
}

/// Kolory linii (liniowe, do bufora sRGB).
fn lin(c: [u8; 3], a: f32) -> [f32; 4] {
    let f = |v: u8| (v as f32 / 255.0).powf(2.2);
    [f(c[0]), f(c[1]), f(c[2]), a]
}

impl Atlas {
    pub fn new(heights: Heights, world: World, prefs: Option<Prefs>) -> Self {
        let prefs = prefs.unwrap_or_default();
        crate::coords::set_format(prefs.coord_format);
        let (lon, lat, height, heading, pitch, distance) = prefs.camera;
        let cam = Camera { lon, lat, height, heading, pitch, distance, ..Camera::default() };
        metrics::set_model(if prefs.din { TimeModel::Din } else { TimeModel::Pttk }, prefs.pace);
        let (tx, rx) = channel();
        let heights_store = heights.store();
        crate::offline::set_offline(prefs.offline_mode);
        Atlas {
            frame: Frame::at(lon, lat),
            frame_version: 1,
            prefs,
            cam,
            heights,
            snap: world.snapshot(),
            world,
            renderer: None,
            tool: Tool::Explore,
            lens: Lens::None,
            lens_pin: None,
            lens_back: None,
            waypoints: Vec::new(),
            route_mode: RouteMode::Trails,
            routes: Vec::new(),
            route_sel: 0,
            routing: false,
            route_error: None,
            route_gen: 0,
            route_dirty: false,
            route_snap_version: 0,
            gpx: None,
            measure: Vec::new(),
            los: None,
            selection: Selection::None,
            hover: None,
            profile_cursor: None,
            panel_cursor: None,
            vs: None,
            vs_busy: false,
            rel: None,
            live_vs: None,
            live_vs_busy: false,
            live_grid: Default::default(),
            live_vs_at: Instant::now(),
            shadow_key: None,
            shadow_busy: false,
            shadow_rect: None,
            shadow_insol: false,
            iso_busy: false,
            iso: None,
            vs_rect: None,
            live_rect: None,
            land_key: None,
            water_rect: None,
            epoch: Instant::now(),
            land_busy: false,
            land_rect: None,
            land_data: None,
            land_ver: 0,
            trees: Vec::new(),
            trees_near: 0,
            trees_key: None,
            trees_busy: false,
            trees_ver: 0,
            walk_up: 0.0,
            walk_vy: 0.0,
            occl_cache: std::cell::RefCell::new(((0, 0, 0, 0), Instant::now(), Default::default())),
            grid_base: std::cell::RefCell::new(((i64::MAX, 0), Instant::now(), None, 0.0, Instant::now())),
            gen: 0,
            tx,
            rx,
            fly: None,
            flight: None,
            panorama: None,
            drag_grab: None,
            orbit_pivot: None,
            drag_waypoint: None,
            ctx_menu: None,
            palette: None,
            search: None,
            help: false,
            tab: crate::ui::panels::Tab::Info,
            toast: None,
            last_view: None,
            fps: (Instant::now(), 0, 0.0),
            screenshot: None,
            requests: Requests::default(),
            library: crate::library::Library::load(),
            undo: Vec::new(),
            redo: Vec::new(),
            hist_now: (Vec::new(), RouteMode::Trails),
            export_pending: false,
            glass: Vec::new(),
            ui_rects: Vec::new(),
            label_hits: Vec::new(),
            weather: Default::default(),
            avalanche: Default::default(),
            notices: Default::default(),
            cond_at: None,
            profile_sel: None,
            section: None,
            webcam: None,
            load_details: false,

            compare: None,
            last_want: Instant::now() - Duration::from_secs(10),
            status_line: String::new(),
            road_lines: Vec::new(),
            roads_key: None,
            roads_job: false,
            roads_ver: 0,
            points: crate::points::Points::load(),
            point_draft: None,
            account_form: None,
            privacy_open: false,
            point_hits: Vec::new(),
            offline: crate::offline::Offline::new(heights_store),
            area_draft: None,
            studio: crate::studio::Studio::load(),
            studio_intro: false,
            studio_sample: None,
            studio_contact: false,
            confirm: None,
            pinch: false,
            touch_last: None,
            walk_stick: Vec2::ZERO,
            compact: false,
            sheet_h: 0.0,
            sheet_full: false,
            safe_top: 0.0,
            safe_bottom: 0.0,
            status_scroll: 0.0,
            gps: GpsState::default(),
            rec: None,
        }
    }

    /// Nowy szkic punktu społeczności w (`lon`, `lat`).
    pub fn start_point_draft(&mut self, lon: f64, lat: f64) {
        let ele = self.heights.sample_cached(lon, lat).unwrap_or(0.0);
        match self.point_draft.as_mut() {
            // przeniesienie szkicu – wpisany tekst zostaje
            Some(d) => {
                d.lon = lon;
                d.lat = lat;
                d.ele = ele;
            }
            None => {
                let title = self.name_near(lon, lat, 120.0).unwrap_or_default();
                self.point_draft = Some(PointDraft { lon, lat, ele, category: crate::points::Category::Info, title, text: String::new(), photos: Vec::new() });
            }
        }
        self.tool = Tool::Point;
    }

    /// Tryb offline: przełączenie sieci i zapamiętanie wyboru.
    pub fn set_offline(&mut self, on: bool) {
        self.prefs.offline_mode = on;
        crate::offline::set_offline(on);
        self.notify(if on {
            crate::i18n::tr("Tryb offline: mapa korzysta tylko z zapisanych danych.")
        } else {
            crate::i18n::tr("Tryb online: brakujące dane znów będą pobierane.")
        });
        if !on {
            self.points.flush();
        }
    }

    /// Pytanie do potwierdzenia; `run` wykona się po wybraniu pierwszego przycisku.
    pub fn ask(&mut self, ask: crate::ui::dialog::Ask, run: impl FnOnce(&mut Atlas) + 'static) {
        self.confirm = Some((ask, Box::new(move |a: &mut Atlas, i: usize| {
            if i == 0 {
                run(a)
            }
        })));
    }

    /// Pytanie z kilkoma działaniami: `run` dostaje indeks wybranego przycisku.
    pub fn ask_choice(&mut self, ask: crate::ui::dialog::Ask, run: impl FnOnce(&mut Atlas, usize) + 'static) {
        self.confirm = Some((ask, Box::new(run)));
    }

    /// Okno pytania (wywoływać co klatkę).
    pub fn confirm_ui(&mut self, ctx: &egui::Context) {
        let Some((ask, _)) = &self.confirm else { return };
        if let Some(i) = crate::ui::dialog::show(ctx, "atlas", ask) {
            let (_, run) = self.confirm.take().unwrap();
            run(self, i);
        }
    }

    pub fn save_prefs(&mut self) -> Prefs {
        let c = self.panorama.unwrap_or(self.cam);
        self.prefs.camera = (c.lon, c.lat, c.height, c.heading, c.pitch, c.distance);
        self.prefs.clone()
    }

    /// Link do bieżącego widoku (kamera, styl, punkty trasy): `gran://widok?…`.
    pub fn share_link(&self) -> String {
        let c = self.panorama.unwrap_or(self.cam);
        let mut s = format!(
            "gran://widok?lat={:.5}&lon={:.5}&az={:.0}&nach={:.0}&odl={:.0}&styl={}",
            c.lat,
            c.lon,
            c.heading,
            c.pitch,
            c.distance,
            self.prefs.style.code()
        );
        if self.prefs.exag != 1.0 {
            s.push_str(&format!("&prz={:.1}", self.prefs.exag));
        }
        if !self.waypoints.is_empty() {
            let pts: Vec<String> = self.waypoints.iter().map(|w| format!("{:.5},{:.5}", w.lat, w.lon)).collect();
            s.push_str("&trasa=");
            s.push_str(&pts.join(";"));
            if self.route_mode == RouteMode::Terrain {
                s.push_str("&teren=1");
            }
        }
        s
    }

    /// Otwarcie linku `gran://widok?…`; `false`, gdy to nie jest link GRANIA.
    pub fn apply_link(&mut self, link: &str) -> bool {
        let link = link.trim();
        let Some(q) = link.strip_prefix("gran://").and_then(|r| r.split_once('?')).map(|(_, q)| q) else {
            return false;
        };
        let mut cam = self.cam;
        let mut wps: Vec<(f64, f64)> = Vec::new();
        let mut terrain = false;
        let mut ok = false;
        for kv in q.split('&') {
            let Some((k, v)) = kv.split_once('=') else { continue };
            let v = v.replace("%3B", ";").replace("%2C", ",").replace("%3b", ";").replace("%2c", ",");
            let num = v.parse::<f64>().ok().filter(|x| x.is_finite());
            match (k, num) {
                ("lat", Some(x)) => {
                    cam.lat = x.clamp(-84.0, 84.0);
                    ok = true;
                }
                ("lon", Some(x)) => cam.lon = x,
                ("az", Some(x)) => cam.heading = x.rem_euclid(360.0),
                ("nach", Some(x)) => cam.pitch = x.clamp(4.0, 89.0),
                ("odl", Some(x)) => cam.distance = x.clamp(120.0, 900_000.0),
                ("styl", Some(x)) => {
                    if let Some(s) = Style::from_code(x as u32) {
                        self.prefs.style = s;
                    }
                }
                ("prz", Some(x)) => self.prefs.exag = x.clamp(1.0, 3.0),
                ("teren", _) => terrain = v == "1",
                // otwarty panel albo narzędzie (zrzuty do dokumentacji, linki z instrukcji)
                ("panel", _) => match v.as_str() {
                    "warunki" => {
                        self.prefs.panel_open = true;
                        self.tab = crate::ui::panels::Tab::Conditions;
                    }
                    "biblioteka" => {
                        self.prefs.panel_open = true;
                        self.tab = crate::ui::panels::Tab::Library;
                    }
                    "punkty" => {
                        self.tool = Tool::Point;
                        self.open_info();
                    }
                    "offline" => {
                        self.tool = Tool::Area;
                        self.open_info();
                    }
                    "szczegoly" => self.open_info(),
                    _ => {}
                },
                ("trasa", _) => {
                    wps = v
                        .split(';')
                        .filter_map(|p| {
                            let (a, b) = p.split_once(',')?;
                            Some((b.parse().ok()?, a.parse().ok()?))
                        })
                        .collect();
                }
                _ => {}
            }
        }
        if !ok {
            return false;
        }
        self.exit_panorama();
        cam.height = self.heights.sample_cached(cam.lon, cam.lat).map(|h| h as f64).unwrap_or(cam.height);
        self.start_fly(cam, 1.4);
        if !wps.is_empty() {
            self.clear_route();
            self.route_mode = if terrain { RouteMode::Terrain } else { RouteMode::Trails };
            for (lon, lat) in wps {
                self.add_waypoint(lon, lat, None);
            }
            self.tool = Tool::Route;
            self.prefs.panel_open = true;
        }
        true
    }

    pub fn notify(&mut self, s: impl Into<String>) {
        self.toast = Some((s.into(), Instant::now()));
    }

    /// Dzień i godzina sceny w czasie lokalnym oglądanego miejsca (środka widoku).
    pub fn when(&self) -> (Day, f64) {
        match self.prefs.custom_time {
            Some((y, m, d, h)) => (Day { y, m, d }, h),
            None => crate::tz::now_at(self.cam.lon, self.cam.lat),
        }
    }

    /// Chwila sceny w sekundach UTC.
    pub fn when_unix(&self) -> f64 {
        let (day, hour) = self.when();
        crate::tz::local_unix(day, hour, self.cam.lon, self.cam.lat)
    }

    pub fn sun(&self) -> (f64, f64) {
        sun::position(self.when_unix(), self.cam.lat, self.cam.lon)
    }

    // ------------------------------------------------------------------ kamera
}
