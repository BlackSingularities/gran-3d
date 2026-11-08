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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tool {
    Explore,
    Route,
    Measure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lens {
    None,
    Band,
    Rel,
    Vis,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Style {
    Terrain,
    Hypso,
    Slope,
    Aspect,
    Satellite,
    Map,
}

impl Style {
    pub const ALL: [Style; 6] = [Style::Terrain, Style::Satellite, Style::Map, Style::Hypso, Style::Slope, Style::Aspect];

    pub fn label(self) -> &'static str {
        match self {
            Style::Terrain => crate::i18n::tr("Teren"),
            Style::Hypso => crate::i18n::tr("Hipsometria"),
            Style::Slope => crate::i18n::tr("Nachylenie (lawiny)"),
            Style::Aspect => crate::i18n::tr("Ekspozycja"),
            Style::Satellite => crate::i18n::tr("Satelita"),
            Style::Map => crate::i18n::tr("Mapa"),
        }
    }

    pub fn from_code(c: u32) -> Option<Style> {
        Style::ALL.into_iter().find(|s| s.code() == c)
    }

    pub fn code(self) -> u32 {
        match self {
            Style::Terrain => 0,
            Style::Hypso => 1,
            Style::Slope => 2,
            Style::Aspect => 3,
            Style::Satellite => 4,
            Style::Map => 5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    Min,
    Low,
    Mid,
    High,
    Ultra,
    Custom,
}

impl Quality {
    pub const PRESETS: [Quality; 5] = [Quality::Min, Quality::Low, Quality::Mid, Quality::High, Quality::Ultra];

    pub fn label(self) -> &'static str {
        match self {
            Quality::Min => crate::i18n::tr("Minimalna"),
            Quality::Low => crate::i18n::tr("Niska"),
            Quality::Mid => crate::i18n::tr("Średnia"),
            Quality::High => crate::i18n::tr("Wysoka"),
            Quality::Ultra => crate::i18n::tr("Ultra"),
            Quality::Custom => crate::i18n::tr("Własna"),
        }
    }

    /// (skala renderu, rozmiar kafla px, kafle DEM, kafle zdjęć)
    pub fn values(self) -> (f32, f32, usize, usize) {
        match self {
            Quality::Min => (0.5, 560.0, 200, 260),
            Quality::Low => (0.7, 460.0, 280, 360),
            Quality::Mid => (0.85, 380.0, 380, 480),
            Quality::High | Quality::Custom => (1.0, 300.0, 520, 640),
            Quality::Ultra => (1.0, 220.0, 900, 1100),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteMode {
    Trails,
    Terrain,
}

/// Ustawienia zapamiętywane między uruchomieniami.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub style: Style,
    pub exag: f64,
    pub trails: bool,
    pub labels: bool,
    pub contours: bool,
    pub km_grid: bool,
    pub shadows: bool,
    pub snow: bool,
    /// Ortofotomapa krajowa zamiast zdjęć Esri z bliska (PL, CZ, SK).
    pub national_ortho: bool,
    /// Maska pokrycia terenu z OSM (wody, lasy, skały) w stylu „teren”.
    pub landcover: bool,
    /// Lekka mapa normalnych: drobna rzeźba z bliska.
    pub detail: bool,
    /// Pora dnia i data: `None` – teraz.
    pub custom_time: Option<(i32, u32, u32, f64)>,
    pub quality: Quality,
    pub render_scale: f32,
    pub detail_px: f32,
    pub max_dem: usize,
    pub max_img: usize,
    pub label_density: f32,
    pub din: bool,
    pub pace: f64,
    pub band_tol: f32,
    pub rel_range: f32,
    pub vs_eye: f32,
    pub camera: (f64, f64, f64, f64, f64, f64),
    pub panel_open: bool,
    pub profile_open: bool,
    /// Doczytywanie okolicy poza kadrem: 0 – wyłączone, 1 – lekkie, 2 – mocne.
    pub prefetch: u8,
    /// Kontrolki na mapie chowają się, gdy mysz stoi.
    pub autohide: bool,
    /// Rodzaje nazw na mapie.
    pub names: Names,
    /// Odcinki z łańcuchami i klamrami na szlakach.
    pub chains: bool,
    /// Granice parków narodowych, rezerwatów i państw.
    pub boundaries: bool,
}

/// Które nazwy pokazywać na mapie (główny przełącznik – `Prefs::labels`, klawisz L).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Names {
    pub peaks: bool,
    pub passes: bool,
    pub ridges: bool,
    pub valleys: bool,
    pub huts: bool,
    pub shelters: bool,
    pub lakes: bool,
    pub waterfalls: bool,
    pub springs: bool,
    pub caves: bool,
    pub viewpoints: bool,
    pub localities: bool,
    pub towns: bool,
    pub villages: bool,
    pub water: bool,
    pub parking: bool,
    pub bus: bool,
    pub cableways: bool,
    pub shrines: bool,
    pub webcams: bool,
    /// Wysokość przy nazwach szczytów i przełęczy.
    pub elevation: bool,
}

impl Default for Names {
    fn default() -> Self {
        Names {
            peaks: true,
            passes: true,
            ridges: false,
            valleys: true,
            huts: true,
            shelters: false,
            lakes: true,
            waterfalls: true,
            springs: false,
            caves: false,
            viewpoints: false,
            localities: true,
            towns: true,
            villages: true,
            water: true,
            parking: false,
            bus: false,
            cableways: true,
            shrines: false,
            webcams: true,
            elevation: true,
        }
    }
}

impl Names {
    pub fn shows(&self, p: &crate::osm::Poi) -> bool {
        match p.kind {
            PoiKind::Peak => self.peaks,
            PoiKind::Saddle => self.passes,
            PoiKind::Ridge => self.ridges,
            PoiKind::Valley => self.valleys,
            PoiKind::Hut => self.huts,
            PoiKind::Shelter => self.shelters,
            PoiKind::Lake => self.lakes,
            PoiKind::Waterfall => self.waterfalls,
            PoiKind::Spring => self.springs,
            PoiKind::Cave => self.caves,
            PoiKind::Viewpoint => self.viewpoints,
            PoiKind::Locality => self.localities,
            PoiKind::DrinkingWater => self.water,
            PoiKind::Parking => self.parking,
            PoiKind::BusStop => self.bus,
            PoiKind::Cableway => self.cableways,
            PoiKind::Shrine => self.shrines,
            PoiKind::Webcam => self.webcams,
            PoiKind::Place => {
                if matches!(p.place.as_str(), "city" | "town") {
                    self.towns
                } else {
                    self.villages
                }
            }
        }
    }

    /// Rodzaje nazw: ikona, podpis, przełącznik.
    pub fn kinds(&mut self) -> Vec<(&'static str, &'static str, &mut bool)> {
        use theme::icon as i;
        vec![
            (i::MOUNTAINS, "Szczyty", &mut self.peaks),
            (i::ARROWS_VERTICAL, "Przełęcze", &mut self.passes),
            (i::TREND_UP, "Granie", &mut self.ridges),
            (i::TREND_DOWN, "Doliny", &mut self.valleys),
            (i::HOUSE_LINE, "Schroniska", &mut self.huts),
            (i::TENT, "Wiaty", &mut self.shelters),
            (i::DROP, "Stawy i jeziora", &mut self.lakes),
            (i::WAVES, "Wodospady", &mut self.waterfalls),
            (i::DROP_HALF, "Źródła", &mut self.springs),
            (i::CIRCLE_HALF, "Jaskinie", &mut self.caves),
            (i::BINOCULARS, "Punkty widokowe", &mut self.viewpoints),
            (i::PLANT, "Hale i polany", &mut self.localities),
            (i::BUILDINGS, "Miasta", &mut self.towns),
            (i::HOUSE, "Wsie", &mut self.villages),
            (i::DROP, "Woda pitna", &mut self.water),
            (i::CAR, "Parkingi", &mut self.parking),
            (i::BUS, "Przystanki", &mut self.bus),
            (i::ARROW_UP_RIGHT, "Kolejki linowe", &mut self.cableways),
            (i::CROSS, "Kapliczki", &mut self.shrines),
            (i::CAMERA, "Kamery", &mut self.webcams),
        ]
    }
}

impl Default for Prefs {
    fn default() -> Self {
        let c = Camera::default();
        let (s, d, md, mi) = Quality::High.values();
        Prefs {
            style: Style::Satellite,
            exag: 1.0,
            trails: true,
            labels: true,
            contours: false,
            km_grid: false,
            shadows: true,
            snow: true,
            national_ortho: false,
            landcover: true,
            detail: true,
            custom_time: None,
            quality: Quality::High,
            render_scale: s,
            detail_px: d,
            max_dem: md,
            max_img: mi,
            label_density: 1.0,
            din: false,
            pace: 1.0,
            band_tol: 20.0,
            rel_range: 400.0,
            vs_eye: 1.7,
            camera: (c.lon, c.lat, c.height, c.heading, c.pitch, c.distance),
            panel_open: false,
            profile_open: true,
            prefetch: 1,
            autohide: false,
            names: Names::default(),
            chains: true,
            boundaries: true,
        }
    }
}

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
}

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
    Shadow(u64, Box<(Vec<u8>, u32, u32, [f64; 4])>),
    Rel(u64, RelInfo),
    Los(u64, Option<f64>, f64),
    Land(Option<Box<(Vec<u8>, u32, [f64; 4], Option<(Vec<f32>, [f64; 4])>)>>),
    /// Drzewa: klucz, instancje (najpierw bliskie), liczba bliskich.
    Trees(TreesKey, Vec<TreeInst>, usize),
}

/// Położenie (co 200 m), oko (co 150 m), wersja pokrycia i ramki – po zmianie drzewa liczone od nowa.
type TreesKey = (i64, i64, i64, i64, u64, u64);

/// Mozaika pokrycia terenu trzymana do rozmieszczania drzew: dane RGBA, bok, prostokąt świata.
type LandData = Arc<(Vec<u8>, u32, [f64; 4])>;

/// Prośby atlasu do aplikacji.
#[derive(Default)]
pub struct Requests {
    pub open_settings: bool,
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
    pub vs: Option<VsInfo>,
    pub vs_busy: bool,
    pub rel: Option<RelInfo>,
    live_vs: Option<(f64, f64)>,
    live_vs_busy: bool,
    /// Siatka wysokości do widoczności na żywo: środek i dane.
    live_grid: Arc<std::sync::Mutex<Option<(f64, f64, Arc<Grid>)>>>,
    live_vs_at: Instant,
    shadow_key: Option<(i64, i64, i64, i64)>,
    shadow_busy: bool,
    shadow_rect: Option<[f64; 4]>,
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
    /// Prognozy pogody i komunikaty lawinowe (pobierane w tle).
    pub weather: crate::weather::Weather,
    pub avalanche: crate::avalanche::Avalanche,
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
}

/// Podgląd kamery internetowej w panelu punktu.
pub enum CamState {
    Loading,
    Image(egui::TextureHandle),
    /// Adres prowadzi do strony, nie do obrazu.
    NotImage,
    Failed(String),
}

type CamResult = Arc<std::sync::Mutex<Option<Result<Option<egui::ColorImage>, String>>>>;

struct CamSlot {
    url: String,
    result: CamResult,
    tex: Option<egui::TextureHandle>,
    at: Instant,
}

/// Obraz z adresu kamery: `None`, gdy to nie obraz (strona z odtwarzaczem itp.).
fn fetch_cam(url: &str) -> Result<Option<egui::ColorImage>, String> {
    let r = crate::download::agent().get(url).timeout(Duration::from_secs(20)).call().map_err(|e| e.to_string())?;
    if !r.content_type().starts_with("image/") {
        return Ok(None);
    }
    let mut buf = Vec::new();
    use std::io::Read;
    r.into_reader().take(12 << 20).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let img = image::load_from_memory(&buf).map_err(|e| e.to_string())?;
    let img = if img.width() > 1280 { img.resize(1280, 1280, image::imageops::FilterType::Triangle) } else { img };
    let rgba = img.to_rgba8();
    Ok(Some(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw())))
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
        let (lon, lat, height, heading, pitch, distance) = prefs.camera;
        let cam = Camera { lon, lat, height, heading, pitch, distance, ..Camera::default() };
        metrics::set_model(if prefs.din { TimeModel::Din } else { TimeModel::Pttk }, prefs.pace);
        let (tx, rx) = channel();
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
            weather: Default::default(),
            avalanche: Default::default(),
            cond_at: None,
            profile_sel: None,
            section: None,
            webcam: None,
            load_details: false,

            compare: None,
            last_want: Instant::now() - Duration::from_secs(10),
            status_line: String::new(),
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

    /// Dzień i godzina sceny.
    pub fn when(&self) -> (Day, f64) {
        match self.prefs.custom_time {
            Some((y, m, d, h)) => (Day { y, m, d }, h),
            None => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs_f64())
                    .unwrap_or(0.0);
                let off = sun::warsaw_offset_min(now) as f64 * 60.0;
                let local = now + off;
                (Day::today(), (local.rem_euclid(86_400.0)) / 3600.0)
            }
        }
    }

    pub fn sun(&self) -> (f64, f64) {
        let (day, hour) = self.when();
        sun::position(sun::warsaw_unix(day, hour), self.cam.lat, self.cam.lon)
    }

    // ------------------------------------------------------------------ kamera

    pub fn fly_to(&mut self, lon: f64, lat: f64, distance: Option<f64>) {
        let mut to = self.cam;
        to.lon = lon;
        to.lat = lat;
        to.height = self.heights.sample_cached(lon, lat).map(|h| h as f64).unwrap_or(self.cam.height);
        if let Some(d) = distance {
            to.distance = d;
        }
        self.start_fly(to, 1.2);
    }

    pub fn start_fly(&mut self, to: Camera, dur: f32) {
        self.flight = None;
        self.fly = Some(FlyTo { from: self.cam, to, t0: Instant::now(), dur });
    }

    pub fn fit_bbox(&mut self, b: &BBox) {
        let c = b.center();
        let span = haversine(b.w, c[1], b.e, c[1]).max(haversine(c[0], b.s, c[0], b.n));
        self.fly_to(c[0], c[1], Some((span * 1.25).max(1500.0)));
    }

    /// Kamera płynnie obejmuje całą trasę: widok ukośny, trasa nad profilem i obok panelu szczegółów.
    pub fn fit_route(&mut self, r: &Route) {
        let tr = &r.track;
        if tr.len() == 0 {
            return;
        }
        let mut b = BBox::new(tr.lon[0], tr.lat[0], tr.lon[0], tr.lat[0]);
        for i in 0..tr.len() {
            b.w = b.w.min(tr.lon[i]);
            b.e = b.e.max(tr.lon[i]);
            b.s = b.s.min(tr.lat[i]);
            b.n = b.n.max(tr.lat[i]);
        }
        let c = b.center();
        let span = haversine(b.w, c[1], b.e, c[1]).max(haversine(c[0], b.s, c[0], b.n)).max(400.0);
        let side = crate::ui::panels::right_inset(self) > 0.0;
        let mut to = self.cam;
        to.pitch = self.cam.pitch.clamp(38.0, 60.0);
        to.distance = (span * if side { 1.9 } else { 1.6 }).clamp(1500.0, 400_000.0);
        // środek widoku bliżej patrzącego (trasa wyżej, nad profilem) i w prawo (trasa obok panelu)
        let h = to.heading.to_radians();
        let back = if self.prefs.profile_open { span * 0.18 } else { 0.0 };
        let right = if side { span * 0.28 } else { 0.0 };
        let east = -h.sin() * back + h.cos() * right;
        let north = -h.cos() * back - h.sin() * right;
        let kx = 111_320.0 * c[1].to_radians().cos();
        to.lon = c[0] + east / kx;
        to.lat = c[1] + north / 110_574.0;
        to.height = self.heights.sample_cached(to.lon, to.lat).map(|h| h as f64).unwrap_or(self.cam.height);
        self.start_fly(to, 1.4);
    }

    pub fn enter_panorama(&mut self, lon: f64, lat: f64) {
        if self.panorama.is_none() {
            self.panorama = Some(self.cam);
        }
        let h = self.heights.sample_cached(lon, lat).unwrap_or(0.0) as f64;
        self.fly = None;
        self.flight = None;
        self.cam.lon = lon;
        self.cam.lat = lat;
        self.cam.height = h + 1.7 / self.prefs.exag.max(1.0);
        // oko prawie w punkcie obrotu: przy rozglądaniu nie wędruje w zbocze za plecami
        self.cam.distance = 0.4;
        self.cam.pitch = -2.0;
        self.walk_up = 0.0;
        self.walk_vy = 0.0;
        self.notify(crate::i18n::tr("Spacer: W A S D – chodzenie, Shift – bieg, spacja – skok, prawy przycisk – rozglądanie · Esc – wyjście"));
    }

    pub fn exit_panorama(&mut self) {
        if let Some(c) = self.panorama.take() {
            self.cam = c;
        }
    }

    fn animate(&mut self, ctx: &egui::Context) {
        if let Some(f) = &self.fly {
            let t = (f.t0.elapsed().as_secs_f32() / f.dur).min(1.0);
            let e = (t * t * (3.0 - 2.0 * t)) as f64;
            let (a, b) = (f.from, f.to);
            let mut c = b;
            c.lon = a.lon + (b.lon - a.lon) * e;
            c.lat = a.lat + (b.lat - a.lat) * e;
            c.height = a.height + (b.height - a.height) * e;
            // odległość: logarytmicznie, z lekkim „podskokiem” przy dalekich przelotach
            let hop = haversine(a.lon, a.lat, b.lon, b.lat);
            let lift = (hop * 0.6 - a.distance.max(b.distance)).max(0.0) * (std::f64::consts::PI * e).sin();
            c.distance = (a.distance.ln() + (b.distance.ln() - a.distance.ln()) * e).exp() + lift;
            let dh = ((b.heading - a.heading + 540.0) % 360.0) - 180.0;
            c.heading = (a.heading + dh * e).rem_euclid(360.0);
            c.pitch = a.pitch + (b.pitch - a.pitch) * e;
            self.cam = c;
            if t >= 1.0 {
                self.fly = None;
            }
            ctx.request_repaint();
        }
        self.animate_flight(ctx);
    }

    pub fn toggle_flight(&mut self) {
        if self.flight.is_some() {
            self.flight = None;
            return;
        }
        if self.current_route().is_none() {
            self.notify(crate::i18n::tr("Przelot: najpierw wyznacz trasę albo wczytaj GPX."));
            return;
        }
        self.fly = None;
        self.flight = Some(Flight { dist: 0.0, paused: false, last: Instant::now(), heading: self.cam.heading, pitch: self.cam.pitch });
    }

    fn animate_flight(&mut self, ctx: &egui::Context) {
        let Some(route) = self.current_route().cloned() else {
            self.flight = None;
            return;
        };
        let Some(fl) = &mut self.flight else { return };
        let dt = fl.last.elapsed().as_secs_f32().min(0.1);
        fl.last = Instant::now();
        let tr = &route.track;
        let total = *tr.d2.last().unwrap_or(&0.0);
        // tempo: cała trasa w ~40–90 s
        let speed = (total / 60.0).clamp(30.0, 400.0);
        if !fl.paused {
            fl.dist += speed * dt;
        }
        if fl.dist >= total {
            self.flight = None;
            return;
        }
        let (i, f) = tr.at_dist(fl.dist);
        let (lon, lat, e) = tr.lerp(i, f);
        let (ia, fa) = tr.at_dist((fl.dist + 250.0).min(total));
        let (lon2, lat2, _) = tr.lerp(ia, fa);
        let want = bearing(lon, lat, lon2, lat2);
        let turn = |a: f64, b: f64| ((b - a + 540.0).rem_euclid(360.0)) - 180.0;
        // punkt trasy zawsze widoczny, także za 2 s lotu: kamera obraca się zawczasu
        let here = (lon, lat, e);
        let (ib, fb) = tr.at_dist((fl.dist + speed * 2.0).min(total));
        let soon = tr.lerp(ib, fb);
        let exag = self.prefs.exag;
        let heights = &self.heights;
        let free = |heading: f64, pitch: f64| sight_clear(heights, exag, here, heading, pitch) && sight_clear(heights, exag, soon, heading, pitch);
        let blocked = !sight_clear(heights, exag, here, fl.heading, fl.pitch);
        let current = fl.heading;
        // najmniejsze odejście od kierunku lotu, z lekką preferencją obecnego ustawienia (bez przeskoków)
        let pick = FLIGHT_PITCHES.iter().find_map(|&pitch| {
            (0..24)
                .map(|k| (want + k as f64 * 15.0).rem_euclid(360.0))
                .filter(|&h| free(h, pitch))
                .min_by(|&a, &b| {
                    let cost = |h: f64| turn(want, h).abs() + 0.5 * turn(current, h).abs();
                    cost(a).total_cmp(&cost(b))
                })
                .map(|h| (h, pitch))
        });
        let (want, want_pitch) = pick.unwrap_or((want, 80.0));
        let dh = turn(fl.heading, want);
        let rate = if blocked { 5.0 } else { 2.0 };
        fl.heading = (fl.heading + dh * (dt as f64 * rate).min(1.0)).rem_euclid(360.0);
        let pitch_rate = if want_pitch > fl.pitch { 3.0 } else { 0.7 };
        fl.pitch += (want_pitch - fl.pitch) * (dt as f64 * pitch_rate).min(1.0);
        self.cam.lon = lon;
        self.cam.lat = lat;
        self.cam.height = e as f64;
        self.cam.heading = fl.heading;
        self.cam.pitch = fl.pitch;
        self.cam.distance = FLIGHT_DIST;
        self.profile_cursor = Some(fl.dist);
        ctx.request_repaint();
    }

    /// Obraz kamery (pobierany w tle, odświeżany co 5 minut).
    pub fn webcam_image(&mut self, ctx: &egui::Context, url: &str) -> CamState {
        let stale = self.webcam.as_ref().is_none_or(|c| c.url != url || c.at.elapsed() > Duration::from_secs(300));
        if stale {
            let result: CamResult = Default::default();
            let (slot, u, c) = (result.clone(), url.to_string(), ctx.clone());
            thread::spawn(move || {
                let r = fetch_cam(&u);
                *slot.lock().unwrap() = Some(r);
                c.request_repaint();
            });
            let tex = self.webcam.take().filter(|c| c.url == url).and_then(|c| c.tex);
            self.webcam = Some(CamSlot { url: url.to_string(), result, tex, at: Instant::now() });
        }
        let c = self.webcam.as_mut().unwrap();
        let done = c.result.lock().unwrap().take();
        match done {
            Some(Ok(Some(img))) => c.tex = Some(ctx.load_texture("kamera", img, egui::TextureOptions::LINEAR)),
            Some(Ok(None)) => {
                c.tex = None;
                *c.result.lock().unwrap() = Some(Ok(None));
                return CamState::NotImage;
            }
            Some(Err(e)) => {
                *c.result.lock().unwrap() = Some(Err(e.clone()));
                return match &c.tex {
                    Some(t) => CamState::Image(t.clone()),
                    None => CamState::Failed(e),
                };
            }
            None => {}
        }
        match &c.tex {
            Some(t) => CamState::Image(t.clone()),
            None => CamState::Loading,
        }
    }

    /// Linia do profilu na dole: trasa albo – w narzędziu pomiaru – przekrój terenu.
    pub fn profile_route(&self) -> Option<&Route> {
        self.current_route().or_else(|| if self.tool == Tool::Measure { self.section.as_ref().map(|s| &s.1) } else { None })
    }

    /// Przekrój terenu wzdłuż punktów pomiaru (liczony od nowa po zmianie punktów).
    fn update_section(&mut self) {
        if self.tool != Tool::Measure || self.measure.len() < 2 {
            self.section = None;
            return;
        }
        let key = self.measure.iter().fold(self.measure.len() as u64, |h, p| {
            h.wrapping_mul(31).wrapping_add(p.0.to_bits() ^ p.1.to_bits().rotate_left(17))
        });
        if self.section.as_ref().is_some_and(|s| s.0 == key) {
            return;
        }
        let total: f64 = self.measure.windows(2).map(|w| haversine(w[0].0, w[0].1, w[1].0, w[1].1)).sum();
        let step = (total / 3000.0).max(10.0);
        let mut pts: Vec<(f64, f64, f32)> = Vec::new();
        for w in self.measure.windows(2) {
            let d = haversine(w[0].0, w[0].1, w[1].0, w[1].1);
            let n = (d / step).ceil().max(1.0) as usize;
            for k in 0..n {
                let f = k as f64 / n as f64;
                let (lon, lat) = (w[0].0 + (w[1].0 - w[0].0) * f, w[0].1 + (w[1].1 - w[0].1) * f);
                pts.push((lon, lat, self.heights.sample_cached(lon, lat).unwrap_or(w[0].2)));
            }
        }
        if let Some(l) = self.measure.last() {
            pts.push((l.0, l.1, self.heights.sample_cached(l.0, l.1).unwrap_or(l.2)));
        }
        let r = route_from_points(&pts, RouteKind::Terrain, crate::i18n::tr("Przekrój terenu"), 1.0);
        self.section = Some((key, r));
    }

    /// Bieżąca trasa (wybrany wariant albo ślad GPX).
    pub fn current_route(&self) -> Option<&Route> {
        self.routes.get(self.route_sel).or(self.gpx.as_ref())
    }

    // ------------------------------------------------------------------ trasa

    pub fn add_waypoint(&mut self, lon: f64, lat: f64, at: Option<usize>) {
        let e = self.heights.sample_cached(lon, lat).unwrap_or(0.0);
        let name = self.name_near(lon, lat, 120.0);
        let w = Waypoint { lon, lat, e, name };
        match at {
            Some(i) if i <= self.waypoints.len() => self.waypoints.insert(i, w),
            _ => self.waypoints.push(w),
        }
        self.route_dirty = true;
    }

    /// Wyjście z planowania: trasa, punkty, warianty i profil znikają z mapy.
    pub fn end_planning(&mut self) {
        self.clear_route();
        self.tool = Tool::Explore;
    }

    pub fn clear_route(&mut self) {
        self.profile_sel = None;
        self.waypoints.clear();

        self.routes.clear();
        self.route_error = None;
        self.flight = None;
        self.route_gen += 1;
        self.routing = false;
    }

    pub fn reverse_route(&mut self) {
        self.waypoints.reverse();
        self.route_dirty = true;
    }

    pub fn remove_waypoint(&mut self, i: usize) {
        if i < self.waypoints.len() {
            self.waypoints.remove(i);
            self.route_dirty = true;
        }
    }

    pub fn mark_route_dirty(&mut self) {
        self.route_dirty = true;
    }

    // ------------------------------------------------------------------ historia trasy

    /// Zapamiętuje zmianę punktów trasy (co klatkę; całe przeciąganie punktu to jeden krok).
    fn track_history(&mut self) {
        if self.drag_waypoint.is_some() {
            return;
        }
        let now = (self.waypoints.clone(), self.route_mode);
        if !same_state(&now, &self.hist_now) {
            let prev = std::mem::replace(&mut self.hist_now, now);
            self.undo.push(prev);
            if self.undo.len() > 200 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Cofnięcie ostatniej zmiany punktów trasy (Ctrl+Z).
    pub fn undo_route(&mut self) {
        self.track_history();
        let Some(prev) = self.undo.pop() else {
            return self.notify(crate::i18n::tr("Nie ma czego cofnąć."));
        };
        let cur = std::mem::replace(&mut self.hist_now, prev.clone());
        self.redo.push(cur);
        self.restore_route(prev);
    }

    /// Ponowienie cofniętej zmiany (Ctrl+Y, Ctrl+Shift+Z).
    pub fn redo_route(&mut self) {
        self.track_history();
        let Some(next) = self.redo.pop() else {
            return self.notify(crate::i18n::tr("Nie ma czego ponowić."));
        };
        let cur = std::mem::replace(&mut self.hist_now, next.clone());
        self.undo.push(cur);
        self.restore_route(next);
    }

    fn restore_route(&mut self, st: RouteState) {
        self.waypoints = st.0;
        self.route_mode = st.1;
        self.route_dirty = true;
        self.tool = Tool::Route;
        self.prefs.panel_open = true;
    }

    // ------------------------------------------------------------------ biblioteka

    /// Nazwa bieżącej trasy: „start – cel” albo nazwa śladu.
    pub fn route_name(&self) -> String {
        match (self.waypoints.first(), self.waypoints.last()) {
            (Some(a), Some(b)) if self.waypoints.len() >= 2 && !self.routes.is_empty() => format!(
                "{} – {}",
                a.name.clone().unwrap_or_else(|| crate::i18n::tr("Start").into()),
                b.name.clone().unwrap_or_else(|| crate::i18n::tr("Cel").into())
            ),
            _ => self
                .current_route()
                .and_then(|r| r.labels.first().cloned())
                .unwrap_or_else(|| crate::i18n::tr("Trasa").into()),
        }
    }

    pub fn save_library(&mut self) {
        if let Err(e) = self.library.save() {
            self.notify(tf!("Nie udało się zapisać biblioteki: {}", e));
        }
    }

    /// Zapis bieżącej trasy w bibliotece: zaplanowanej jako punkty, wczytanej jako ślad.
    pub fn save_route_to_library(&mut self) {
        let Some(r) = self.current_route().cloned() else {
            return self.notify(crate::i18n::tr("Brak trasy do zapisania."));
        };
        let name = self.route_name();
        let planned = !self.waypoints.is_empty() && self.routes.get(self.route_sel).is_some();
        let t = &r.track;
        let id = self.library.next_id();
        self.library.routes.push(crate::library::SavedRoute {
            id,
            name: name.clone(),
            group: String::new(),
            created: crate::library::now(),
            waypoints: if planned { self.waypoints.iter().map(|w| (w.lon, w.lat, w.name.clone())).collect() } else { Vec::new() },
            terrain: self.route_mode == RouteMode::Terrain,
            // ślad zawsze: do porównań i podglądu bez wyznaczania od nowa
            track: (0..t.len()).map(|i| (t.lon[i], t.lat[i], t.e[i])).collect(),
            dist: r.stats.len2,
            up: r.stats.up,
            time: r.stats.time,
        });
        self.save_library();
        self.notify(tf!("Zapisano w bibliotece: {}", name));
    }

    /// Otwarcie zapisanej trasy: punkty wracają do planowania, ślad – jak wczytany plik.
    pub fn open_saved_route(&mut self, i: usize) {
        let Some(s) = self.library.routes.get(i).cloned() else { return };
        if s.waypoints.len() >= 2 {
            self.exit_panorama();
            self.clear_route();
            self.gpx = None;
            self.route_mode = if s.terrain { RouteMode::Terrain } else { RouteMode::Trails };
            for (lon, lat, _) in &s.waypoints {
                self.add_waypoint(*lon, *lat, None);
            }
            for (w, (_, _, n)) in self.waypoints.iter_mut().zip(&s.waypoints) {
                if n.is_some() {
                    w.name = n.clone();
                }
            }
            self.tool = Tool::Route;
            let pts: Vec<(f64, f64, f32)> = self.waypoints.iter().map(|w| (w.lon, w.lat, w.e)).collect();
            let r = route_from_points(&pts, RouteKind::Gpx, &s.name, 1.0);
            self.fit_route(&r);
        } else if s.track.len() >= 2 {
            self.exit_panorama();
            if self.tool == Tool::Route {
                self.end_planning();
            }
            let r = route_from_points(&s.track, RouteKind::Gpx, &s.name, 1.0);
            self.fit_route(&r);
            self.gpx = Some(r);
        } else {
            return;
        }
        self.prefs.profile_open = true;
        self.notify(tf!("Otwarto trasę „{}”.", s.name));
    }

    /// Nazwa zaznaczonego punktu (szczyt, schronisko…), jeśli jest.
    pub fn selection_name(&self) -> Option<String> {
        match self.selection {
            Selection::Poi(i) => self.snap.pois.get(i).map(|p| p.poi.name.clone()),
            _ => None,
        }
    }

    /// Dodanie punktu do ulubionych miejsc.
    pub fn save_place(&mut self, lon: f64, lat: f64, name: Option<String>) {
        let e = self.heights.sample_cached(lon, lat).unwrap_or(0.0);
        let name = name.or_else(|| self.name_near(lon, lat, 150.0)).unwrap_or_else(|| fmt::decimal(lon, lat));
        let id = self.library.next_id();
        self.library.places.push(crate::library::Place { id, name: name.clone(), group: String::new(), created: crate::library::now(), lon, lat, e });
        self.save_library();
        self.notify(tf!("Dodano do ulubionych: {}", name));
    }

    /// Zakładka bieżącego widoku (kamera, styl, przewyższenie).
    pub fn save_view(&mut self) {
        let c = self.panorama.unwrap_or(self.cam);
        let name = self
            .name_near(c.lon, c.lat, 4000.0)
            .map(|n| tf!("Widok: {}", n))
            .unwrap_or_else(|| tf!("Widok {}", self.library.views.len() + 1));
        let id = self.library.next_id();
        self.library.views.push(crate::library::SavedView {
            id,
            name: name.clone(),
            group: String::new(),
            created: crate::library::now(),
            camera: (c.lon, c.lat, c.height, c.heading, c.pitch, c.distance),
            style: self.prefs.style.code(),
            exag: self.prefs.exag,
        });
        self.save_library();
        self.notify(tf!("Zapisano widok: {}", name));
    }

    pub fn open_view(&mut self, i: usize) {
        let Some(v) = self.library.views.get(i).cloned() else { return };
        self.exit_panorama();
        if let Some(st) = Style::from_code(v.style) {
            self.prefs.style = st;
        }
        if v.exag >= 1.0 {
            self.prefs.exag = v.exag.clamp(1.0, 3.0);
        }
        let (lon, lat, height, heading, pitch, distance) = v.camera;
        let to = Camera { lon, lat, height, heading, pitch, distance, ..self.cam };
        self.start_fly(to, 1.4);
    }

    /// Eksport, gdy trasa będzie gotowa (zapisane punkty wyznaczają się w tle).
    pub fn export_route_after_routing(&mut self) {
        self.export_pending = true;
    }

    /// Pełny ekran (F11).
    pub fn toggle_fullscreen(&mut self, ctx: &egui::Context) {
        let on = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!on));
    }


    /// Nazwa najbliższego punktu (szczyt, schronisko…) w promieniu `r` m.
    pub fn name_near(&self, lon: f64, lat: f64, r: f64) -> Option<String> {
        self.snap
            .pois
            .iter()
            .filter(|p| p.poi.kind != PoiKind::Place)
            .map(|p| (haversine(lon, lat, p.poi.lon, p.poi.lat), p))
            .filter(|(d, _)| *d < r)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, p)| p.poi.name.clone())
    }

    fn compute_routes(&mut self) {
        self.route_dirty = false;
        self.route_gen += 1;
        let gen = self.route_gen;
        self.route_error = None;
        if self.waypoints.len() < 2 {
            self.routes.clear();
            self.routing = false;
            return;
        }
        self.routing = true;
        let tx = self.tx.clone();
        let wps = self.waypoints.clone();
        match self.route_mode {
            RouteMode::Trails => {
                let snap = self.snap.clone();
                self.route_snap_version = snap.version;
                thread::spawn(move || {
                    let mut snaps: Vec<Snap> = Vec::new();
                    for w in &wps {
                        match snap.graph.snap(w.lon, w.lat, 1500.0) {
                            Some(s) => snaps.push(s),
                            None => {
                                let _ = tx.send(JobResult::RouteFail(gen, crate::i18n::tr("Punkt jest daleko od szlaków (ponad 1,5 km) albo szlaki tej okolicy jeszcze się wczytują. Spróbuj trasy przez teren.").into()));
                                return;
                            }
                        }
                    }
                    let alts = snap.graph.alternatives(&snaps);
                    if alts.is_empty() {
                        let _ = tx.send(JobResult::RouteFail(gen, crate::i18n::tr("Nie znaleziono połączenia szlakami między punktami.").into()));
                    } else {
                        let _ = tx.send(JobResult::Routes(gen, alts));
                    }
                });
            }
            RouteMode::Terrain => {
                let heights = self.heights.clone();
                thread::spawn(move || {
                    let mut pts: Vec<(f64, f64, f32)> = Vec::new();
                    for w in wps.windows(2) {
                        let (a, b) = (&w[0], &w[1]);
                        let d = haversine(a.lon, a.lat, b.lon, b.lat);
                        let pad = (d * 0.35).max(800.0);
                        let kx = 111_320.0 * a.lat.to_radians().cos();
                        let bb = BBox::new(
                            a.lon.min(b.lon) - pad / kx,
                            a.lat.min(b.lat) - pad / 110_574.0,
                            a.lon.max(b.lon) + pad / kx,
                            a.lat.max(b.lat) + pad / 110_574.0,
                        );
                        // poziom siatki: najwyżej ~1400 px na bok
                        let side_m = haversine(bb.w, bb.s, bb.e, bb.s).max(haversine(bb.w, bb.s, bb.w, bb.n));
                        let mut z = 14u8;
                        while z > 9 && side_m / crate::geo::meters_per_pixel(a.lat, z) > 1400.0 {
                            z -= 1;
                        }
                        let g = heights.grid(&bb, z);
                        let pa = g.to_grid(a.lon, a.lat);
                        let pb = g.to_grid(b.lon, b.lat);
                        match analysis::terrain_path(&g, pa, pb, 40.0) {
                            Some(path) => {
                                for (k, (gx, gy)) in path.into_iter().enumerate() {
                                    if k == 0 && !pts.is_empty() {
                                        continue;
                                    }
                                    let (lon, lat) = g.to_lonlat(gx, gy);
                                    pts.push((lon, lat, g.sample(gx, gy)));
                                }
                            }
                            None => {
                                let _ = tx.send(JobResult::RouteFail(gen, crate::i18n::tr("Nie da się przejść tędy z limitem nachylenia 40°.").into()));
                                return;
                            }
                        }
                    }
                    let r = route_from_points(&pts, RouteKind::Terrain, crate::i18n::tr("Przez teren"), 1.35);
                    let _ = tx.send(JobResult::Routes(gen, vec![r]));
                });
            }
        }
    }

    // ------------------------------------------------------------------ analizy w tle

    fn grid_around(heights: &Heights, lon: f64, lat: f64, radius_m: f64, max_px: f64) -> Grid {
        let kx = 111_320.0 * lat.to_radians().cos();
        let bb = BBox::new(lon - radius_m / kx, lat - radius_m / 110_574.0, lon + radius_m / kx, lat + radius_m / 110_574.0);
        let mut z = 13u8;
        while z > 8 && 2.0 * radius_m / crate::geo::meters_per_pixel(lat, z) > max_px {
            z -= 1;
        }
        heights.grid(&bb, z)
    }

    fn world_rect(g: &Grid) -> [f64; 4] {
        let n = (1u64 << g.z) as f64 * 256.0;
        [g.px0 / n, g.py0 / n, (g.px0 + g.w as f64) / n, (g.py0 + g.h as f64) / n]
    }

    pub fn start_viewshed(&mut self, lon: f64, lat: f64) {
        self.gen += 1;
        let gen = self.gen;
        self.vs_busy = true;
        self.vs = None;
        let (heights, tx, snap, eye) = (self.heights.clone(), self.tx.clone(), self.snap.clone(), self.prefs.vs_eye);
        thread::spawn(move || {
            let g = Self::grid_around(&heights, lon, lat, 32_000.0, 2600.0);
            let (gx, gy) = g.to_grid(lon, lat);
            let v = analysis::viewshed(&g, gx, gy, eye, 0.0);
            let mut visible: Vec<(usize, f64)> = snap
                .pois
                .iter()
                .enumerate()
                .filter(|(_, p)| matches!(p.poi.kind, PoiKind::Peak | PoiKind::Hut | PoiKind::Shelter | PoiKind::Saddle))
                .filter_map(|(i, p)| {
                    let (px, py) = g.to_grid(p.poi.lon, p.poi.lat);
                    if !g.inside(px, py) {
                        return None;
                    }
                    let m = v.mask[py.round() as usize * g.w + px.round() as usize];
                    (m > 0).then(|| (i, haversine(lon, lat, p.poi.lon, p.poi.lat)))
                })
                .filter(|(_, d)| *d > 50.0)
                .collect();
            visible.sort_by(|a, b| snap.pois[b.0].ele.total_cmp(&snap.pois[a.0].ele));
            let info = VsInfo { lon, lat, area_km2: v.area_km2, observer: v.observer, visible, eye };
            let rect = Self::world_rect(&g);
            let _ = tx.send(JobResult::Viewshed(gen, Box::new((info, v.mask, g.w as u32, g.h as u32, rect))));
        });
    }

    /// Widoczność na żywo pod kursorem: siatka wysokości budowana raz na okolicę i trzymana
    /// w pamięci (nowa dopiero, gdy kursor odejdzie daleko); każdy ruch to tylko rzut promieni.
    fn start_live_viewshed(&mut self, lon: f64, lat: f64) {
        self.live_vs_busy = true;
        self.live_vs_at = Instant::now();
        let gen = self.gen;
        let (heights, tx, eye, cache) = (self.heights.clone(), self.tx.clone(), self.prefs.vs_eye, self.live_grid.clone());
        thread::spawn(move || {
            const R: f64 = 20_000.0;
            let cached = cache.lock().unwrap().clone().filter(|(clon, clat, _)| haversine(*clon, *clat, lon, lat) < R * 0.35);
            let g = match cached {
                Some((_, _, g)) => g,
                None => {
                    let g = Arc::new(Self::grid_around(&heights, lon, lat, R, 1100.0));
                    *cache.lock().unwrap() = Some((lon, lat, g.clone()));
                    g
                }
            };
            let (gx, gy) = g.to_grid(lon, lat);
            let v = analysis::viewshed(&g, gx, gy, eye, 0.0);
            let rect = Self::world_rect(&g);
            let _ = tx.send(JobResult::LiveVs(gen, Box::new((v.mask, g.w as u32, g.h as u32, rect))));
        });
    }

    fn start_rel_stats(&mut self, lon: f64, lat: f64, e: f32) {
        self.gen += 1;
        let gen = self.gen;
        let (heights, tx) = (self.heights.clone(), self.tx.clone());
        thread::spawn(move || {
            let g = Self::grid_around(&heights, lon, lat, 5_000.0, 900.0);
            let (gx, gy) = g.to_grid(lon, lat);
            let (above, below) = analysis::relative_stats(&g, gx, gy, e, 5_000.0 / g.mpp as f64);
            let _ = tx.send(JobResult::Rel(gen, RelInfo { above, below }));
        });
    }

    fn start_los(&mut self) {
        if self.measure.len() != 2 {
            self.los = None;
            return;
        }
        self.gen += 1;
        let gen = self.gen;
        let (a, b) = (self.measure[0], self.measure[1]);
        let (heights, tx) = (self.heights.clone(), self.tx.clone());
        thread::spawn(move || {
            let d = haversine(a.0, a.1, b.0, b.1);
            let pad = 500.0;
            let kx = 111_320.0 * a.1.to_radians().cos();
            let bb = BBox::new(a.0.min(b.0) - pad / kx, a.1.min(b.1) - pad / 110_574.0, a.0.max(b.0) + pad / kx, a.1.max(b.1) + pad / 110_574.0);
            let mut z = 14u8;
            while z > 9 && (d + 2.0 * pad) / crate::geo::meters_per_pixel(a.1, z) > 3000.0 {
                z -= 1;
            }
            let g = heights.grid(&bb, z);
            let r = analysis::line_of_sight(&g, g.to_grid(a.0, a.1), g.to_grid(b.0, b.1), 1.7);
            let _ = tx.send(JobResult::Los(gen, r, d));
        });
    }

    fn maybe_shadows(&mut self) {
        if !self.prefs.shadows || self.shadow_busy || self.panorama.is_some() && self.shadow_rect.is_some() {
            return;
        }
        let (az, alt) = self.sun();
        let radius = (self.cam.distance * 1.6).clamp(6_000.0, 45_000.0);
        // klucz: położenie (co ~1/4 promienia), promień (skokowo) i Słońce (co ~0,5°)
        let q = radius / 4.0;
        let (x, y) = (lon_to_x(self.cam.lon) * world_m(self.cam.lat), lat_to_y(self.cam.lat) * world_m(self.cam.lat));
        let key = ((x / q) as i64, (y / q) as i64, (radius.log2() * 2.0) as i64, ((az * 2.0) as i64) * 1000 + (alt * 2.0) as i64);
        if self.shadow_key == Some(key) {
            return;
        }
        self.shadow_key = Some(key);
        self.shadow_busy = true;
        let sunv = sun::vector(az, alt);
        let (heights, tx, lon, lat) = (self.heights.clone(), self.tx.clone(), self.cam.lon, self.cam.lat);
        let gen = self.gen;
        thread::spawn(move || {
            let g = Self::grid_around(&heights, lon, lat, radius, 1400.0);
            let m = analysis::shadows(&g, sunv);
            let rect = Self::world_rect(&g);
            let _ = tx.send(JobResult::Shadow(gen, Box::new((m, g.w as u32, g.h as u32, rect))));
        });
    }

    /// Mozaika pokrycia terenu 3×3 sektory wokół kamery (po zmianie sektora albo nowych maskach).
    fn maybe_landcover(&mut self) {
        if !self.prefs.landcover || self.land_busy || self.cam.distance > 120_000.0 {
            return;
        }
        // okno poziomów wody przesuwa się z kamerą co ćwierć sektora
        let ns = (1u64 << crate::osm::SECTOR_Z) as f64;
        let (cx, cy) = (crate::geo::lon_to_x(self.cam.lon), crate::geo::lat_to_y(self.cam.lat));
        let q = ((cx * ns * 4.0).round() as i64, (cy * ns * 4.0).round() as i64);
        let key = (SectorId::at(self.cam.lon, self.cam.lat), self.world.land_version(), q);
        if self.land_key == Some(key) {
            return;
        }
        self.land_key = Some(key);
        self.land_busy = true;
        let (root, tx, heights) = (self.heights.store().root().to_path_buf(), self.tx.clone(), self.heights.clone());
        let (wx, wy) = (q.0 as f64 / 4.0 / ns, q.1 as f64 / 4.0 / ns);
        thread::spawn(move || {
            let m = crate::landcover::mosaic(&root, key.0, 1024).map(|(data, side, rect)| {
                let water = crate::landcover::water_window(&root, wx, wy, &heights, crate::render::NO_WATER);
                Box::new((data, side, rect, water))
            });
            let _ = tx.send(JobResult::Land(m));
        });
    }

    /// Czy rysować drzewa: pokrycie terenu włączone, styl realistyczny, kamera dość blisko.
    fn trees_on(&self) -> bool {
        self.prefs.landcover
            && matches!(self.prefs.style, Style::Terrain | Style::Satellite)
            && (self.cam.distance < 14_000.0 || self.panorama.is_some())
    }

    /// Rozmieszczenie drzew z maski lasu wokół celu kamery, w tle. Bliskie (do ~650 m od oka)
    /// dostają szczegółowe bryły, dalsze proste piramidki; dalej niż kilka km las jest tylko na teksturze.
    fn maybe_trees(&mut self, view: &View) {
        if !self.trees_on() || self.trees_busy {
            return;
        }
        let Some(land) = self.land_data.clone() else { return };
        let (tx_, tz_) = self.frame.to_local(self.cam.lon, self.cam.lat);
        let (ex, ez) = (view.eye.x, view.eye.z);
        let key = (
            (tx_ / 200.0).round() as i64,
            (tz_ / 200.0).round() as i64,
            (ex / 150.0).round() as i64,
            (ez / 150.0).round() as i64,
            self.land_ver,
            self.frame_version,
        );
        if self.trees_key == Some(key) {
            return;
        }
        self.trees_key = Some(key);
        self.trees_busy = true;
        let (tx, heights, frame) = (self.tx.clone(), self.heights.clone(), self.frame);
        // promień: z wysoka szerzej, z bliska węziej (i tak widać tylko najbliższe)
        let radius = (self.cam.distance * 0.9).clamp(1400.0, 3600.0);
        thread::spawn(move || {
            let (data, side, rect) = (&land.0, land.1 as f64, land.2);
            const STEP: f64 = 15.0;
            const MAX: usize = 70_000;
            const NEAR: f64 = 650.0;
            let hash = |a: i64, b: i64, k: u64| {
                let mut h = (a as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (b as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ k.wrapping_mul(0x1656_67B1_9E37_79F9);
                h ^= h >> 31;
                h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
                h ^= h >> 29;
                (h >> 11) as f64 / (1u64 << 53) as f64
            };
            let mut near = Vec::new();
            let mut far = Vec::new();
            let n = (radius / STEP).ceil() as i64;
            let (cx, cz) = ((tx_ / STEP).round() as i64, (tz_ / STEP).round() as i64);
            'outer: for iz in -n..=n {
                for ix in -n..=n {
                    let (gx, gz) = (cx + ix, cz + iz);
                    let jx = (hash(gx, gz, 1) - 0.5) * STEP * 0.9;
                    let jz = (hash(gx, gz, 2) - 0.5) * STEP * 0.9;
                    let (x, z) = (gx as f64 * STEP + jx, gz as f64 * STEP + jz);
                    let d_c = (x - tx_).hypot(z - tz_);
                    if d_c > radius {
                        continue;
                    }
                    let (lon, lat) = frame.to_lonlat(x, z);
                    let (wx, wy) = (lon_to_x(lon), lat_to_y(lat));
                    let u = ((wx - rect[0]) / (rect[2] - rect[0]) * side) as i64;
                    let v = ((wy - rect[1]) / (rect[3] - rect[1]) * side) as i64;
                    if u < 0 || v < 0 || u >= side as i64 || v >= side as i64 {
                        continue;
                    }
                    let i = ((v * side as i64 + u) * 4) as usize;
                    let (water, forest, rock, alpha) = (data[i] as f64 / 255.0, data[i + 1] as f64 / 255.0, data[i + 2] as f64 / 255.0, data[i + 3]);
                    if alpha == 0 || water > 0.5 {
                        continue;
                    }
                    let scrub = rock > 0.3 && rock < 0.55;
                    let roll = hash(gx, gz, 3);
                    let keep = if forest > 0.4 { roll < 0.85 * forest } else if scrub { roll < 0.35 } else { false };
                    if !keep {
                        continue;
                    }
                    let Some(e) = heights.sample_cached(lon, lat) else { continue };
                    let seed = hash(gx, gz, 4) as f32;
                    let size = 0.75 + 0.5 * hash(gx, gz, 5) as f32;
                    // rodzaj: kosodrzewina na zaroślach i wysoko, niżej mieszany las, wyżej świerki
                    let (kind, h, r) = if scrub || e > 1550.0 {
                        (2.0, 2.2 * size, 1.6 * size)
                    } else if e < 900.0 && seed < 0.55 || e < 1200.0 && seed < 0.25 {
                        (1.0, 20.0 * size, 5.0 * size)
                    } else {
                        (0.0, 24.0 * size, 3.6 * size)
                    };
                    // na skraju promienia drzewa maleją – bez ostrej granicy
                    let edge = 1.0 - ((d_c / radius - 0.85) / 0.15).clamp(0.0, 1.0) as f32;
                    if edge <= 0.02 {
                        continue;
                    }
                    let t = TreeInst { pos: [x as f32, e, z as f32], p: [h * edge, r * edge.sqrt(), kind, seed] };
                    if (x - ex).hypot(z - ez) < NEAR {
                        near.push(t);
                    } else {
                        far.push(t);
                    }
                    if near.len() + far.len() >= MAX {
                        break 'outer;
                    }
                }
            }
            let nn = near.len();
            near.extend(far);
            let _ = tx.send(JobResult::Trees(key, near, nn));
        });
    }

    fn receive_jobs(&mut self) {
        while let Ok(r) = self.rx.try_recv() {
            match r {
                JobResult::Routes(g, rs) if g == self.route_gen => {
                    self.routing = false;
                    self.routes = rs;
                    self.route_sel = 0;
                    // nowa trasa: kamera płynnie ją obejmuje (nie podczas przeciągania punktu)
                    if self.drag_waypoint.is_none() {
                        if let Some(r) = self.routes.first().cloned() {
                            self.prefs.profile_open = true;
                            self.fit_route(&r);
                        }
                    }
                }
                JobResult::RouteFail(g, e) if g == self.route_gen => {
                    self.routing = false;
                    self.routes.clear();
                    self.route_error = Some(e);
                }
                JobResult::Viewshed(g, b) if g == self.gen => {
                    let (info, mask, w, h, rect) = *b;
                    self.vs_busy = false;
                    if let Some(r) = &mut self.renderer {
                        r.set_viewshed(Some((&mask, w, h)));
                    }
                    self.vs_rect = Some(rect);
                    self.vs = Some(info);
                }
                JobResult::LiveVs(g, b) => {
                    self.live_vs_busy = false;
                    if g == self.gen && self.lens == Lens::Vis && self.lens_pin.is_none() {
                        let (mask, w, h, rect) = *b;
                        if let Some(r) = &mut self.renderer {
                            r.set_viewshed(Some((&mask, w, h)));
                        }
                        self.live_rect = Some(rect);
                        self.vs_rect = Some(rect);
                    }
                }
                JobResult::Shadow(_, b) => {
                    self.shadow_busy = false;
                    let (mask, w, h, rect) = *b;
                    if let Some(r) = &mut self.renderer {
                        r.set_shadow(Some((&mask, w, h)));
                    }
                    self.shadow_rect = Some(rect);
                }
                JobResult::Rel(g, info) if g == self.gen => self.rel = Some(info),
                JobResult::Los(g, r, d) if g == self.gen => self.los = Some((r, d)),
                JobResult::Land(m) => {
                    self.land_busy = false;
                    match m {
                        Some(b) => {
                            let (data, side, rect, water) = *b;
                            if let Some(r) = &mut self.renderer {
                                let lv = water.as_ref().map(|(lv, _)| lv.as_slice()).unwrap_or(&[]);
                                r.set_landcover(Some((&data, side, lv, crate::landcover::WATER_SIDE)));
                            }
                            self.water_rect = water.map(|(_, r)| r);
                            self.land_rect = Some(rect);
                            self.land_data = Some(Arc::new((data, side, rect)));
                            self.land_ver += 1;
                        }
                        None => {
                            self.land_rect = None;
                            self.water_rect = None;
                            self.land_data = None;
                            self.land_ver += 1;
                        }
                    }
                }
                JobResult::Trees(key, t, near) => {
                    self.trees_busy = false;
                    if self.trees_key == Some(key) {
                        self.trees = t;
                        self.trees_near = near;
                        self.trees_ver += 1;
                    }
                }
                _ => {}
            }
        }
    }

    // ------------------------------------------------------------------ soczewki

    pub fn set_lens(&mut self, l: Lens) {
        self.lens = if self.lens == l { Lens::None } else { l };
        self.unpin_lens();
    }

    pub fn pin_lens(&mut self, lon: f64, lat: f64, mode: Lens) {
        let e = self.heights.sample_cached(lon, lat).unwrap_or(0.0);
        self.lens = mode;
        self.lens_pin = Some((lon, lat, e));
        self.rel = None;
        match mode {
            Lens::Vis => self.start_viewshed(lon, lat),
            Lens::Rel => self.start_rel_stats(lon, lat, e),
            _ => {}
        }
        self.prefs.panel_open = true;
        self.tab = crate::ui::panels::Tab::Info;
    }

    pub fn unpin_lens(&mut self) {
        self.gen += 1;
        self.lens_pin = None;
        self.vs = None;
        self.vs_busy = false;
        self.rel = None;
        self.vs_rect = None;
        self.live_rect = None;
        self.live_vs = None;
        if let Some(r) = &mut self.renderer {
            r.set_viewshed(None);
        }
    }

    // ------------------------------------------------------------------ wejście

    fn ground(&self, view: &View, px: [f32; 2]) -> Option<(f64, f64, f32)> {
        let dir = view.ray(px);
        let frame = view.frame;
        let h = |x: f64, z: f64| {
            let (lon, lat) = frame.to_lonlat(x, z);
            // ten sam poziom terenu co na ekranie: dokładniejszy model z pamięci przesuwałby
            // punkt względem kursora, bo przy ukośnym widoku mała różnica wysokości to duże przesunięcie
            let shown = self.renderer.as_ref().and_then(|r| r.tiles.rendered_dem_z(lon, lat));
            shown
                .and_then(|lvl| self.heights.sample_at(lon, lat, lvl, false))
                .or_else(|| self.heights.sample_cached(lon, lat))
                .map(|v| v as f64)
        };
        let p = view.raycast(dir, &h)?;
        let (lon, lat) = frame.to_lonlat(p.x, p.z);
        Some((lon, lat, p.y as f32))
    }

    fn local(&self, lon: f64, lat: f64, e: f32) -> DVec3 {
        let (x, z) = self.frame.to_local(lon, lat);
        DVec3::new(x, e as f64 * self.prefs.exag, z)
    }

    /// Spacer po terenie: W/S – przód/tył, A/D – w bok, Shift – bieg, spacja – skok (z grawitacją).
    fn walk(&mut self, ctx: &egui::Context) {
        let (dt, fwd, side, run, jump) = ctx.input(|i| {
            let k = |a: egui::Key| i.key_down(a) as i32 as f64;
            (
                i.stable_dt.min(0.05) as f64,
                k(egui::Key::W) - k(egui::Key::S),
                k(egui::Key::D) - k(egui::Key::A),
                i.modifiers.shift,
                i.key_pressed(egui::Key::Space),
            )
        });
        let moving = fwd != 0.0 || side != 0.0;
        if moving {
            let speed = if run { 22.0 } else { 6.0 };
            let len = fwd.hypot(side);
            let (f, s) = (fwd / len * speed * dt, side / len * speed * dt);
            let h = self.cam.heading.to_radians();
            let east = h.sin() * f + h.cos() * s;
            let north = h.cos() * f - h.sin() * s;
            let kx = 111_320.0 * self.cam.lat.to_radians().cos();
            self.cam.lon = (self.cam.lon + east / kx + 540.0).rem_euclid(360.0) - 180.0;
            self.cam.lat = (self.cam.lat + north / 110_574.0).clamp(-84.0, 84.0);
        }
        if jump && self.walk_up <= 0.0 {
            self.walk_vy = 4.2;
        }
        if self.walk_up > 0.0 || self.walk_vy > 0.0 {
            self.walk_vy -= 9.81 * dt;
            self.walk_up = (self.walk_up + self.walk_vy * dt).max(0.0);
            if self.walk_up <= 0.0 {
                self.walk_vy = 0.0;
            }
        }
        // grunt: najwyższy z kilku próbek najdokładniejszego terenu wokół stóp i pod okiem – na ostrych
        // szczytach i graniach zgrubna wysokość bywa niższa od rysowanej i oko trafiało pod powierzchnię
        // (widok „od spodu”, jak do góry nogami)
        let kx = 111_320.0 * self.cam.lat.to_radians().cos();
        let h = self.cam.heading.to_radians();
        let mut g: Option<f32> = None;
        for (de, dn) in [(0.0, 0.0), (4.0, 0.0), (-4.0, 0.0), (0.0, 4.0), (0.0, -4.0), (-h.sin() * 2.0, -h.cos() * 2.0)] {
            let (lon, lat) = (self.cam.lon + de / kx, self.cam.lat + dn / 110_574.0);
            let v = self.heights.sample_at(lon, lat, 15, false).or_else(|| self.heights.sample_cached(lon, lat));
            if let Some(v) = v {
                g = Some(g.map_or(v, |x| x.max(v)));
            }
        }
        if let Some(g) = g {
            let exag = self.prefs.exag.max(1.0);
            self.cam.height = g as f64 + (1.7 + self.walk_up) / exag;
        }
        if moving || self.walk_up > 0.0 {
            ctx.request_repaint();
        }
    }

    fn keyboard(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        if self.panorama.is_some() {
            self.walk(ctx);
        }
        let walking = self.panorama.is_some();
        let (mut pan, mut rot, mut tilt, mut zoom) = (Vec2::ZERO, 0.0f64, 0.0f64, 0.0f64);
        let mut events = Vec::new();
        ctx.input(|i| {
            let dt = i.stable_dt.min(0.05) as f64;
            if i.key_down(egui::Key::ArrowLeft) {
                pan.x -= 1.0;
            }
            if i.key_down(egui::Key::ArrowRight) {
                pan.x += 1.0;
            }
            if i.key_down(egui::Key::ArrowUp) {
                pan.y -= 1.0;
            }
            if i.key_down(egui::Key::ArrowDown) {
                pan.y += 1.0;
            }
            if i.key_down(egui::Key::Q) {
                rot -= 60.0 * dt;
            }
            if i.key_down(egui::Key::E) {
                rot += 60.0 * dt;
            }
            if i.key_down(egui::Key::PageUp) {
                tilt += 40.0 * dt;
            }
            if i.key_down(egui::Key::PageDown) {
                tilt -= 40.0 * dt;
            }
            if i.key_down(egui::Key::Home) {
                zoom -= 1.5 * dt;
            }
            if i.key_down(egui::Key::End) {
                zoom += 1.5 * dt;
            }
            pan *= dt as f32;
            for e in &i.events {
                if let egui::Event::Key { key, pressed: true, modifiers, repeat: false, .. } = e {
                    // w spacerze W A S D i spacja służą do chodzenia
                    if walking && matches!(key, egui::Key::W | egui::Key::A | egui::Key::S | egui::Key::D | egui::Key::Space) {
                        continue;
                    }
                    events.push((*key, *modifiers));
                }
            }
        });
        if pan != Vec2::ZERO || rot != 0.0 || tilt != 0.0 || zoom != 0.0 {
            let h = self.cam.heading.to_radians();
            let k = self.cam.distance * 0.8;
            let (east, south) = (
                pan.x as f64 * h.cos() * k - pan.y as f64 * h.sin() * k,
                pan.x as f64 * h.sin() * k + pan.y as f64 * h.cos() * k,
            );
            self.move_target(east, south);
            self.cam.heading = (self.cam.heading + rot).rem_euclid(360.0);
            self.cam.pitch = (self.cam.pitch + tilt).clamp(self.min_pitch(), 89.0);
            self.cam.distance = (self.cam.distance * zoom.exp()).clamp(self.min_dist(), 900_000.0);
            self.fly = None;
            ctx.request_repaint();
        }
        let question = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Text(t) if t == "?")));
        if question {
            self.help = !self.help;
        }
        for (key, m) in events {
            use egui::Key as K;
            if m.ctrl || m.command {
                match key {
                    K::Z if m.shift => self.redo_route(),
                    K::Z => self.undo_route(),
                    K::Y => self.redo_route(),
                    K::S => self.save_route_to_library(),
                    K::D => self.save_view(),
                    _ => {}
                }
                continue;
            }
            match key {
                K::Escape => {
                    if self.help {
                        self.help = false;
                    } else if self.ctx_menu.is_some() {
                        self.ctx_menu = None;
                    } else if self.panorama.is_some() {
                        self.exit_panorama();
                    } else if self.flight.is_some() {
                        self.flight = None;
                    } else if self.lens_pin.is_some() {
                        self.unpin_lens();
                    } else if self.lens != Lens::None {
                        self.lens = Lens::None;
                    } else if self.selection != Selection::None {
                        self.selection = Selection::None;
                    } else if self.tool == Tool::Route {
                        self.end_planning();
                    } else {
                        self.tool = Tool::Explore;
                    }
                }
                K::Num1 => {
                    if self.tool == Tool::Route {
                        self.end_planning();
                    }
                    self.tool = Tool::Explore;
                }
                K::Num2 => self.tool = Tool::Route,
                K::Num3 => self.tool = Tool::Measure,
                K::H | K::I => self.set_lens(Lens::Band),
                K::W => self.set_lens(Lens::Rel),
                K::O => self.set_lens(Lens::Vis),
                K::Slash => self.search = Some(crate::ui::search::Search::default()),
                K::M => {
                    // styl mapy jest w menu górnego paska; skrót przełącza na następny
                    let all = Style::ALL;
                    let i = all.iter().position(|s| *s == self.prefs.style).unwrap_or(0);
                    self.prefs.style = all[(i + 1) % all.len()];
                }
                K::P => self.prefs.panel_open = !self.prefs.panel_open,
                K::T => self.prefs.trails = !self.prefs.trails,
                K::L => self.prefs.labels = !self.prefs.labels,
                K::C => self.prefs.contours = !self.prefs.contours,
                K::F => self.toggle_flight(),
                K::N => {
                    let mut to = self.cam;
                    to.heading = 0.0;
                    self.start_fly(to, 0.7);
                }
                K::V => {
                    let mut to = self.cam;
                    to.pitch = if self.cam.pitch > 75.0 { 40.0 } else { 89.0 };
                    self.start_fly(to, 0.7);
                }
                K::R => {
                    let d = Camera::default();
                    let mut to = self.cam;
                    to.heading = d.heading;
                    to.pitch = d.pitch;
                    to.distance = d.distance.min(self.cam.distance.max(8000.0));
                    self.start_fly(to, 0.9);
                }
                K::G => self.export_route(),
                K::B => {
                    let lib = self.prefs.panel_open && self.tab == crate::ui::panels::Tab::Library;
                    self.prefs.panel_open = !lib;
                    self.tab = crate::ui::panels::Tab::Library;
                }
                K::F11 => self.toggle_fullscreen(ctx),
                K::K => {
                    let on = self.prefs.panel_open && self.tab == crate::ui::panels::Tab::Conditions;
                    self.prefs.panel_open = !on;
                    self.tab = crate::ui::panels::Tab::Conditions;
                }
                K::S => self.screenshot(ctx),
                K::OpenBracket => self.shift_hour(-0.5),
                K::CloseBracket => self.shift_hour(0.5),
                K::Plus | K::Equals => self.prefs.exag = ((self.prefs.exag + 0.1) * 10.0).round() / 10.0,
                K::Minus => self.prefs.exag = (((self.prefs.exag - 0.1) * 10.0).round() / 10.0).max(1.0),
                K::Backspace | K::Delete => {
                    if self.tool == Tool::Route && !self.waypoints.is_empty() {
                        let n = self.waypoints.len() - 1;
                        self.remove_waypoint(n);
                    } else if self.tool == Tool::Measure && !self.measure.is_empty() {
                        self.measure.pop();
                        self.start_los();
                    }
                }
                _ => {}
            }
        }
        self.prefs.exag = self.prefs.exag.clamp(1.0, 3.0);
    }

    pub fn shift_hour(&mut self, dh: f64) {
        let (day, hour) = self.when();
        let h = (hour + dh).clamp(0.0, 23.99);
        self.prefs.custom_time = Some((day.y, day.m, day.d, h));
    }

    fn min_pitch(&self) -> f64 {
        if self.panorama.is_some() {
            -35.0
        } else {
            4.0
        }
    }

    pub fn min_dist(&self) -> f64 {
        if self.panorama.is_some() {
            0.4
        } else {
            120.0
        }
    }

    /// Przesuwa cel kamery tak, jak obrót całego widoku wokół chwyconego punktu (o `dh` w poziomie
    /// i `dp` w nachyleniu, stopnie): punkt zostaje w tym samym miejscu ekranu.
    fn orbit(&mut self, dh: f64, dp: f64) {
        let Some((lon, lat, e)) = self.orbit_pivot else { return };
        let (tx, tz) = self.frame.to_local(self.cam.lon, self.cam.lat);
        let exag = self.prefs.exag;
        let pivot = self.local(lon, lat, e);
        let v = DVec3::new(tx, self.cam.height * exag, tz) - pivot;
        // nachylenie: obrót w płaszczyźnie kierunku patrzenia i pionu
        let (h, p) = (self.cam.heading.to_radians(), dp.to_radians());
        let (fwd, right) = (DVec3::new(h.sin(), 0.0, -h.cos()), DVec3::new(h.cos(), 0.0, h.sin()));
        let (a, b, c) = (v.dot(fwd), v.dot(right), v.y);
        let v = fwd * (a * p.cos() + c * p.sin()) + right * b + DVec3::Y * (c * p.cos() - a * p.sin());
        // kierunek: obrót wokół pionu
        let q = dh.to_radians();
        let v = DVec3::new(v.x * q.cos() - v.z * q.sin(), v.y, v.x * q.sin() + v.z * q.cos());
        let mut target = pivot + v;
        // cel wraca na teren wzdłuż linii patrzenia: obraz bez zmian, a wygładzanie wysokości nie szarpie
        let (h, p) = ((self.cam.heading + dh).to_radians(), (self.cam.pitch + dp).to_radians());
        let look = DVec3::new(h.sin() * p.cos(), -p.sin(), -h.cos() * p.cos());
        let mut distance = self.cam.distance;
        if p.sin() > 0.1 {
            for _ in 0..3 {
                let (l, t) = self.frame.to_lonlat(target.x, target.z);
                let Some(g) = self.heights.sample_cached(l, t) else { break };
                let step = (g as f64 * exag - target.y) / look.y;
                if distance + step < self.min_dist() {
                    break;
                }
                target += look * step;
                distance += step;
            }
        }
        let (l, t) = self.frame.to_lonlat(target.x, target.z);
        self.cam.lon = (l + 540.0).rem_euclid(360.0) - 180.0;
        self.cam.lat = t.clamp(-84.0, 84.0);
        self.cam.height = target.y / exag;
        self.cam.distance = distance;
    }

    fn move_target(&mut self, east: f64, south: f64) {
        let kx = 111_320.0 * self.cam.lat.to_radians().cos();
        self.cam.lon += east / kx;
        self.cam.lat = (self.cam.lat - south / 110_574.0).clamp(-84.0, 84.0);
        self.cam.lon = (self.cam.lon + 540.0).rem_euclid(360.0) - 180.0;
        self.land_target(1.0);
    }

    /// Cel kamery wraca na teren wzdłuż linii patrzenia: oko zostaje na tej samej wysokości
    /// bezwzględnej, więc przesuwanie mapy nad dolinami i grzbietami nie podbija ani nie obniża kamery.
    fn land_target(&mut self, k: f64) {
        if self.panorama.is_some() || self.flight.is_some() {
            return;
        }
        let exag = self.prefs.exag;
        let (h, p) = (self.cam.heading.to_radians(), self.cam.pitch.to_radians());
        if p.sin() <= 0.1 {
            return;
        }
        let look = DVec3::new(h.sin() * p.cos(), -p.sin(), -h.cos() * p.cos());
        let (tx, tz) = self.frame.to_local(self.cam.lon, self.cam.lat);
        let mut target = DVec3::new(tx, self.cam.height * exag, tz);
        let mut distance = self.cam.distance;
        for _ in 0..3 {
            let (l, t) = self.frame.to_lonlat(target.x, target.z);
            let Some(g) = self.heights.sample_cached(l, t) else { break };
            let step = (g as f64 * exag - target.y) / look.y * k;
            if distance + step < self.min_dist() || distance + step > 900_000.0 {
                break;
            }
            target += look * step;
            distance += step;
        }
        let (l, t) = self.frame.to_lonlat(target.x, target.z);
        self.cam.lon = (l + 540.0).rem_euclid(360.0) - 180.0;
        self.cam.lat = t.clamp(-84.0, 84.0);
        self.cam.height = target.y / exag;
        self.cam.distance = distance;
    }

    /// Eksport bieżącej trasy: GPX, KML albo GeoJSON (według wybranego typu pliku).
    pub fn export_route(&mut self) {
        let Some(r) = self.current_route().cloned() else {
            self.notify(crate::i18n::tr("Brak trasy do zapisania."));
            return;
        };
        let name = self.route_name();
        let wps: Vec<crate::gpx::Waypoint> = self
            .waypoints
            .iter()
            .enumerate()
            .map(|(i, w)| crate::gpx::Waypoint {
                name: w.name.clone().unwrap_or_else(|| tf!("Punkt {}", i + 1)),
                lon: w.lon,
                lat: w.lat,
                e: w.e,
            })
            .collect();
        if let Some(p) = rfd::FileDialog::new()
            .set_title(crate::i18n::tr("Eksportuj trasę"))
            .set_file_name(format!("{}.gpx", file_stem(&name)))
            .add_filter("GPX", &["gpx"])
            .add_filter(crate::i18n::tr("KML (Google Earth)"), &["kml"])
            .add_filter("GeoJSON", &["geojson"])
            .save_file()
        {
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("gpx").to_ascii_lowercase();
            let text = match ext.as_str() {
                "kml" => crate::formats::export_kml(&r, &name, &wps),
                "geojson" | "json" => crate::formats::export_geojson(&r, &name, &wps),
                _ => crate::gpx::export(&r, &name, &wps),
            };
            match std::fs::write(&p, text) {
                Ok(()) => self.notify(tf!("Zapisano {}", p.display())),
                Err(e) => self.notify(tf!("Nie udało się zapisać: {}", e)),
            }
        }
    }

    /// Wczytanie śladu z pliku GPX, KML albo GeoJSON.
    pub fn import_track(&mut self, path: &std::path::Path) {
        let text = match std::fs::read(path) {
            Ok(b) => String::from_utf8_lossy(&b).trim_start_matches('\u{feff}').to_string(),
            Err(e) => return self.notify(tf!("Nie można odczytać pliku: {}", e)),
        };
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        match crate::formats::parse_any(&text, &ext) {
            Ok(t) => {
                let h = &self.heights;
                let pts: Vec<(f64, f64, f32)> = t
                    .points
                    .iter()
                    .map(|&(lon, lat, e)| (lon, lat, e.or_else(|| h.sample_cached(lon, lat)).unwrap_or(0.0)))
                    .collect();
                let r = route_from_points(&pts, RouteKind::Gpx, &t.name, 1.0);
                self.fit_route(&r);
                self.notify(tf!("Wczytano ślad „{}” ({}).", t.name, fmt::dist(r.stats.len2)));
                self.gpx = Some(r);
                self.prefs.profile_open = true;
            }
            Err(e) => self.notify(e),
        }
    }

    fn screenshot(&mut self, ctx: &egui::Context) {
        if let Some(p) = rfd::FileDialog::new()
            .set_title(crate::i18n::tr("Zapisz zrzut ekranu"))
            .set_file_name("gran.png")
            .add_filter("PNG", &["png"])
            .save_file()
        {
            self.screenshot = Some(p);
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
    }

    fn take_screenshot(&mut self, ctx: &egui::Context) {
        if self.screenshot.is_none() {
            return;
        }
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let (Some(img), Some(path)) = (shot, self.screenshot.take()) {
            let [w, h] = img.size;
            let raw: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            let msg = match image::RgbaImage::from_raw(w as u32, h as u32, raw).map(|b| b.save(&path)) {
                Some(Ok(())) => tf!("Zapisano zrzut: {}", path.display()),
                _ => crate::i18n::tr("Nie udało się zapisać zrzutu.").to_string(),
            };
            self.notify(msg);
        }
    }

    // ------------------------------------------------------------------ klatka

    /// Główny widok 3D (w panelu centralnym).
    pub fn view_ui(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.take_screenshot(&ctx);
        if self.renderer.is_none() {
            if let Some(rs) = frame.wgpu_render_state() {
                self.renderer = Some(Renderer::new(rs, self.heights.clone(), ctx.clone()));
            }
        }
        self.receive_jobs();
        let snap = self.world.snapshot();
        if snap.version != self.snap.version {
            self.snap = snap;
            if self.route_mode == RouteMode::Trails && !self.waypoints.is_empty() && self.route_snap_version != self.snap.version {
                self.route_dirty = true;
            }
        }
        if self.route_dirty && !self.routing {
            self.compute_routes();
        } else if self.route_dirty && self.routing {
            // nowe punkty w trakcie liczenia – policz od razu na nowo
            self.compute_routes();
        }
        self.keyboard(&ctx);
        self.track_history();
        self.update_section();

        if self.export_pending && !self.routing && !self.route_dirty {
            self.export_pending = false;
            self.export_route();
        }
        self.animate(&ctx);

        // dane okolicy: sektory wokół celu kamery i punktów trasy
        if self.last_want.elapsed() > Duration::from_millis(400) {
            self.last_want = Instant::now();
            let mut want = Vec::new();
            if self.cam.distance < 90_000.0 {
                let s = SectorId::at(self.cam.lon, self.cam.lat);
                want.push(s);
                if self.cam.distance < 60_000.0 {
                    want.extend(s.neighbours());
                }
            }
            for w in &self.waypoints {
                want.push(SectorId::at(w.lon, w.lat));
            }
            self.world.want(want);
        }

        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let size = [rect.width().max(1.0), rect.height().max(1.0)];

        // ramka świata: przesunięcie, gdy kamera odleci daleko od początku
        let (fx, fz) = self.frame.to_local(self.cam.lon, self.cam.lat);
        if fx.hypot(fz) > 25_000.0 {
            self.frame = Frame::at(self.cam.lon, self.cam.lat);
            self.frame_version += 1;
        }
        // wysokość punktu obrotu podąża za terenem
        if self.panorama.is_none() && self.flight.is_none() {
            if self.fly.is_some() {
                if let Some(h) = self.heights.sample_cached(self.cam.lon, self.cam.lat) {
                    self.cam.height += (h as f64 - self.cam.height) * 0.08;
                }
            } else {
                // cel dochodzi do terenu wzdłuż linii patrzenia: oko stoi w miejscu, obraz nie skacze
                // ani przy przesuwaniu, ani przy obracaniu
                let before = self.cam.height;
                self.land_target(0.35);
                // prawie poziomy widok: łagodne dociąganie wysokości jak dawniej
                if self.cam.height == before {
                    if let Some(h) = self.heights.sample_cached(self.cam.lon, self.cam.lat) {
                        self.cam.height += (h as f64 - self.cam.height) * 0.1;
                    }
                }
            }
        }
        let exag = self.prefs.exag;
        let ground_eye = {
            let v = self.cam.view(self.frame, exag, size, self.cam.height);
            let (lon, lat) = self.frame.to_lonlat(v.eye.x, v.eye.z);
            self.heights.sample_cached(lon, lat).unwrap_or(0.0) as f64
        };
        // kamera nie wchodzi pod teren
        {
            let v = self.cam.view(self.frame, exag, size, ground_eye);
            let min_y = ground_eye * exag + 25.0;
            if v.eye.y < min_y && self.panorama.is_none() {
                let need = (min_y - v.target.y) / self.cam.distance.max(1.0);
                if need > 0.0 {
                    self.cam.pitch = self.cam.pitch.max(need.clamp(-1.0, 1.0).asin().to_degrees());
                }
            }
        }
        let view = self.cam.view(self.frame, exag, size, ground_eye);

        // kursor nad terenem
        let pointer = resp.hover_pos();
        let local_px = |p: Pos2| [p.x - rect.left(), p.y - rect.top()];
        self.hover = pointer.and_then(|p| self.ground(&view, local_px(p)));

        self.handle_mouse(ui, &resp, rect, &view);

        // analizy zależne od kursora i Słońca
        self.maybe_shadows();
        self.maybe_landcover();
        self.maybe_trees(&view);
        if self.lens == Lens::Vis && self.lens_pin.is_none() {
            if let Some((lon, lat, _)) = self.hover {
                let moved = self.live_vs.is_none_or(|(a, b)| haversine(a, b, lon, lat) > 60.0);
                if moved && !self.live_vs_busy && self.live_vs_at.elapsed() > Duration::from_millis(120) {
                    self.live_vs = Some((lon, lat));
                    self.start_live_viewshed(lon, lat);
                }
            }
        }

        let scene = self.scene(&view);
        let lines = self.dyn_lines(&view);
        let ppp = ctx.pixels_per_point();
        let dim = self.current_route().is_some();
        if let Some(r) = &mut self.renderer {
            let key = self.snap.version * 1_000_000
                + self.frame_version * 16
                + self.prefs.trails as u64
                + dim as u64 * 2
                + self.prefs.chains as u64 * 4
                + self.prefs.boundaries as u64 * 8;
            if r.static_version() != key {
                let mut tl = Vec::new();
                // granice pod szlakami, łańcuchy nad nimi
                if self.prefs.boundaries {
                    tl.extend(boundary_lines(&self.snap, &self.frame, ppp));
                }
                if self.prefs.trails {
                    tl.extend(trail_lines(&self.snap, &self.frame, ppp, dim));
                }
                if self.prefs.chains && self.prefs.trails {
                    tl.extend(chain_lines(&self.snap, &self.frame, ppp));
                }
                r.set_static_lines(key, &tl);
            }
            if r.trees_version() != self.trees_ver {
                r.set_trees(self.trees_ver, &self.trees, self.trees_near);
            }
            // paski i panele z tej i poprzedniej klatki (rysowane przed i po widoku)
            let glass: Vec<([f32; 4], f32)> = std::mem::take(&mut self.glass)
                .into_iter()
                .filter_map(|(g, k)| {
                    let i = g.intersect(rect);
                    (i.width() > 0.0 && i.height() > 0.0 && k > 0.01).then(|| {
                        let (w, h) = (rect.width(), rect.height());
                        ([(i.left() - rect.left()) / w, (i.top() - rect.top()) / h, (i.right() - rect.left()) / w, (i.bottom() - rect.top()) / h], k)
                    })
                })
                .collect();
            let (blit, _) = r.render(&view, &scene, &lines, ppp, &glass);
            ui.painter().add(egui_wgpu::Callback::new_paint_callback(rect, blit));
            let [dem, sat, ortho, local] = r.tiles.loading_kinds;
            let mut parts = Vec::new();
            if dem > 0 {
                parts.push(tf!("teren · {}", dem));
            }
            if sat > 0 {
                parts.push(tf!("zdjęcia satelitarne · {}", sat));
            }
            if ortho > 0 {
                parts.push(tf!("ortofoto · {}", ortho));
            }
            if local > 0 {
                parts.push(tf!("składanie ortofoto · {}", local));
            }
            self.status_line = if parts.is_empty() { String::new() } else { tf!("pobieranie: {}", parts.join(", ")) };
        }
        self.last_view = Some(view);
        self.overlays(ui, rect, &view);
        self.fps.1 += 1;
        if self.fps.0.elapsed() > Duration::from_secs(1) {
            self.fps.2 = self.fps.1 as f32 / self.fps.0.elapsed().as_secs_f32();
            self.fps = (Instant::now(), 0, self.fps.2);
        }
        // pełna szybkość tylko przy ruchu (animacja kamery, mysz, klawisze); samo wczytywanie
        // przy nieruchomym widoku wystarczy pokazywać ~30 kl./s – obraz ten sam, karta odpoczywa
        let user = ctx.input(|i| i.pointer.is_moving() || i.pointer.any_down() || !i.keys_down.is_empty() || i.smooth_scroll_delta != Vec2::ZERO);
        if self.fly.is_some() || self.flight.is_some() || user {
            ctx.request_repaint();
        } else if !self.status_line.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(33));
        } else {
            // spoczynek: rzadkie odświeżenie (zegar, stan); klatka 3D i tak nie jest liczona od nowa
            ctx.request_repaint_after(Duration::from_millis(1000));
        }
    }

    pub fn fps(&self) -> f32 {
        self.fps.2
    }

    fn handle_mouse(&mut self, ui: &mut egui::Ui, resp: &egui::Response, rect: Rect, view: &View) {
        let ctx = ui.ctx().clone();
        let to_px = |p: Pos2| [p.x - rect.left(), p.y - rect.top()];
        let pointer = resp.hover_pos().or(resp.interact_pointer_pos());
        // kółko: przybliżanie do kursora
        if let Some(p) = resp.hover_pos() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y + (i.zoom_delta() - 1.0) * 300.0) as f64;
            if scroll.abs() > 0.1 {
                self.fly = None;
                let k = (-scroll * 0.0022).exp();
                let old = self.cam.distance;
                self.cam.distance = (old * k).clamp(self.min_dist(), 900_000.0);
                if self.panorama.is_none() {
                    if let Some((lon, lat, _)) = self.ground(view, to_px(p)) {
                        let f = 1.0 - self.cam.distance / old;
                        self.cam.lon += (lon - self.cam.lon) * f;
                        self.cam.lat += (lat - self.cam.lat) * f;
                    }
                } else {
                    self.cam.fov_y = (self.cam.fov_y * k).clamp(12.0, 75.0);
                    self.cam.distance = old;
                }
            }
        }
        // przeciąganie punktu trasy
        if resp.drag_started_by(egui::PointerButton::Primary) {
            self.drag_waypoint = None;
            self.drag_grab = None;
            if let Some(p) = pointer {
                if self.tool == Tool::Route {
                    for (i, w) in self.waypoints.iter().enumerate() {
                        if let Some(s) = view.project(self.local(w.lon, w.lat, w.e)) {
                            if (Pos2::new(s[0] + rect.left(), s[1] + rect.top()) - p).length() < 12.0 {
                                self.drag_waypoint = Some(i);
                            }
                        }
                    }
                }
                if self.drag_waypoint.is_none() {
                    self.drag_grab = self.ground(view, to_px(p)).map(|(lon, lat, e)| self.local(lon, lat, e));
                }
            }
        }
        if resp.dragged_by(egui::PointerButton::Primary) {
            self.fly = None;
            if let (Some(i), Some((lon, lat, e))) = (self.drag_waypoint, self.hover) {
                if let Some(w) = self.waypoints.get_mut(i) {
                    w.lon = lon;
                    w.lat = lat;
                    w.e = e;
                }
            } else if self.panorama.is_some() {
                // spacer: lewy przycisk to zwykły wskaźnik (rozglądanie – prawym)
            } else if let (Some(g), Some(p)) = (self.drag_grab, pointer) {
                // chwycony punkt terenu zostaje pod kursorem
                let dir = view.ray(to_px(p));
                if dir.y < -0.02 {
                    let t = (g.y - view.eye.y) / dir.y;
                    if t > 0.0 {
                        let hit = view.eye + dir * t;
                        let (de, ds) = (g.x - hit.x, g.z - hit.z);
                        let lim = self.cam.distance * 2.0;
                        self.move_target(de.clamp(-lim, lim), ds.clamp(-lim, lim));
                    }
                } else {
                    let d = resp.drag_delta();
                    let k = self.cam.distance * 0.002;
                    let h = self.cam.heading.to_radians();
                    let (x, y) = (-d.x as f64 * k, -d.y as f64 * k);
                    self.move_target(x * h.cos() - y * h.sin(), x * h.sin() + y * h.cos());
                }
            }
        }
        if resp.drag_stopped_by(egui::PointerButton::Primary) {
            if self.drag_waypoint.take().is_some() {
                if let Some(w) = self.waypoints.last() {
                    let _ = w;
                }
                for w in &mut self.waypoints {
                    w.name = None;
                }
                let names: Vec<Option<String>> = self.waypoints.iter().map(|w| self.name_near(w.lon, w.lat, 120.0)).collect();
                for (w, n) in self.waypoints.iter_mut().zip(names) {
                    w.name = n;
                }
                self.route_dirty = true;
            }
            self.drag_grab = None;
        }
        // obrót i nachylenie
        if resp.drag_started_by(egui::PointerButton::Secondary) || resp.drag_started_by(egui::PointerButton::Middle) {
            self.orbit_pivot = if self.panorama.is_some() { None } else { pointer.and_then(|p| self.ground(view, to_px(p))) };
        }
        if resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) {
            self.fly = None;
            let d = resp.drag_delta();
            let s = if self.panorama.is_some() { -0.15 } else { 0.3 };
            let heading = (self.cam.heading + d.x as f64 * s).rem_euclid(360.0);
            let pitch = (self.cam.pitch + d.y as f64 * 0.22).clamp(self.min_pitch(), 89.0);
            self.orbit(d.x as f64 * s, pitch - self.cam.pitch);
            self.cam.heading = heading;
            self.cam.pitch = pitch;
            self.ctx_menu = None;
        }
        if !ctx.input(|i| i.pointer.secondary_down() || i.pointer.middle_down()) {
            self.orbit_pivot = None;
        }
        if resp.secondary_clicked() {
            if let (Some(p), Some((lon, lat, e))) = (pointer, self.hover) {
                self.ctx_menu = Some((p, lon, lat, e));
            }
        }
        if resp.double_clicked() {
            if let Some((lon, lat, _)) = self.hover {
                if self.panorama.is_none() {
                    self.fly_to(lon, lat, Some((self.cam.distance / 2.5).max(600.0)));
                }
            }
            return;
        }
        if resp.clicked() {
            self.ctx_menu = None;
            let Some((lon, lat, _)) = self.hover else { return };
            // etykieta pod kursorem
            if let Some(p) = pointer {
                if let Some(i) = self.label_hit(p, rect, view) {
                    if self.tool == Tool::Explore && self.lens == Lens::None {
                        self.selection = Selection::Poi(i);
                        self.prefs.panel_open = true;
                        self.tab = crate::ui::panels::Tab::Info;
                        return;
                    }
                }
            }
            if self.lens != Lens::None && self.lens_pin.is_none() {
                self.pin_lens(lon, lat, self.lens);
                return;
            }
            match self.tool {
                Tool::Explore => {
                    // szlak w pobliżu kliknięcia (promień zależny od odległości)
                    let r = (self.cam.distance * 0.006).clamp(15.0, 400.0);
                    if self.prefs.trails {
                        if let Some(s) = self.snap.graph.snap(lon, lat, r) {
                            self.selection = Selection::Trail(s.edge);
                            self.prefs.panel_open = true;
                            self.tab = crate::ui::panels::Tab::Info;
                            return;
                        }
                    }
                    self.selection = Selection::Point { lon, lat };
                    self.prefs.panel_open = true;
                    self.tab = crate::ui::panels::Tab::Info;
                }
                Tool::Route => {
                    self.add_waypoint(lon, lat, None);
                    self.prefs.panel_open = true;
                    self.tab = crate::ui::panels::Tab::Info;
                }
                Tool::Measure => {
                    let e = self.heights.sample_cached(lon, lat).unwrap_or(0.0);
                    self.measure.push((lon, lat, e));
                    self.start_los();
                }
            }
        }
        let _ = ctx;
    }

    fn label_hit(&self, p: Pos2, rect: Rect, view: &View) -> Option<usize> {
        if !self.prefs.labels {
            return None;
        }
        for (i, poi) in self.snap.pois.iter().enumerate().take(600) {
            if let Some(s) = view.project(self.local(poi.poi.lon, poi.poi.lat, poi.dem)) {
                let sp = Pos2::new(s[0] + rect.left(), s[1] + rect.top());
                if (sp - p).length() < 9.0 {
                    return Some(i);
                }
            }
        }
        None
    }

    fn scene(&self, view: &View) -> Scene {
        let (az, alt) = self.sun();
        let sv = sun::vector(az, alt);
        let (day, _) = self.when();
        // barwy: ciepłe nisko nad horyzontem, nocą chłodne i ciemne
        let a = alt as f32;
        let day_k = ((a + 4.0) / 14.0).clamp(0.0, 1.0);
        let warm = 1.0 - ((a - 2.0) / 25.0).clamp(0.0, 1.0);
        let sun_color = [
            (1.0 + 0.25 * warm) * 2.2 * day_k,
            (0.96 - 0.25 * warm) * 2.1 * day_k,
            (0.88 - 0.5 * warm) * 1.95 * day_k,
        ];
        let night = [0.05, 0.07, 0.12];
        let mixc = |d: [f32; 3], n: [f32; 3]| [n[0] + (d[0] - n[0]) * day_k, n[1] + (d[1] - n[1]) * day_k, n[2] + (d[2] - n[2]) * day_k];
        let sky = mixc([0.32, 0.42, 0.58], night);
        let zenith = mixc([0.16, 0.32, 0.62], [0.01, 0.015, 0.035]);
        let horizon_day = [0.62 + 0.25 * warm, 0.70 + 0.05 * warm, 0.80 - 0.25 * warm];
        let horizon = mixc(horizon_day, [0.03, 0.04, 0.07]);
        let lens_mode = match self.lens {
            Lens::None => 0.0,
            Lens::Band => if self.prefs.band_tol > 0.0 { 2.0 } else { 1.0 },
            Lens::Rel => 3.0,
            Lens::Vis => 0.0,
        };
        let lens_src = self.lens_pin.or(self.hover);
        let lens = match (lens_src, lens_mode > 0.0) {
            (Some((lon, lat, e)), true) => {
                let (x, z) = self.frame.to_local(lon, lat);
                [x as f32, z as f32, e, lens_mode]
            }
            _ => [0.0, 0.0, 0.0, 0.0],
        };
        let cursor = if self.lens == Lens::None {
            self.hover.map(|(lon, lat, e)| {
                let (x, z) = self.frame.to_local(lon, lat);
                [x as f32, z as f32, e]
            })
        } else {
            None
        };
        let to_rect = |r: Option<[f64; 4]>| -> [f32; 4] {
            match r {
                Some([x0, y0, x1, y1]) => {
                    let (a, b) = self.frame.world_to_local(x0, y0);
                    let (c, d) = self.frame.world_to_local(x1, y1);
                    [a as f32, b as f32, (1.0 / (c - a)) as f32, (1.0 / (d - b)) as f32]
                }
                None => [0.0; 4],
            }
        };
        // piętra roślinności i śniegu: wyżej bliżej równika (Alpy ~1800 m, Himalaje ~4000 m), niżej na północy
        let lift = (49.5 - self.cam.lat.abs() as f32).clamp(-20.0, 25.0);
        let up = if lift > 0.0 { lift * 120.0 } else { lift * 60.0 };
        let forest = 1300.0 + up;
        let biome = [forest, forest + 180.0 + lift.max(0.0) * 15.0, forest + 350.0 + lift.max(0.0) * 30.0];
        let snow_lift = if lift > 0.0 { lift * 150.0 } else { lift * 60.0 };
        // cięcie poziomic zależne od odległości
        let d = self.cam.distance;
        let contour = if d < 2500.0 { 10.0 } else if d < 9000.0 { 20.0 } else if d < 30_000.0 { 50.0 } else { 100.0 };
        let _ = view;
        Scene {
            style: self.prefs.style.code(),
            contours: self.prefs.contours,
            contour_interval: contour,
            km_grid: self.prefs.km_grid,
            sun_dir: sv,
            sun_power: if alt > -2.0 { 1.0 } else { 0.0 },
            sun_color,
            sky_color: sky,
            zenith,
            horizon,
            // z wysoka mgła słabnie (inaczej przy dużym oddaleniu cały widok jest mleczny)
            fog_density: 1.0 / (190_000.0 * (self.cam.distance / 30_000.0).max(1.0) as f32),
            snowline: sun::snowline(day) as f32 + snow_lift,
            snow: self.prefs.snow,
            autumn: sun::autumn(day),
            lens,
            band_tol: self.prefs.band_tol,
            rel_range: self.prefs.rel_range,
            cursor,
            shadows: self.prefs.shadows && self.shadow_rect.is_some(),
            viewshed: self.lens == Lens::Vis && self.vs_rect.is_some(),
            biome,
            render_scale: self.prefs.render_scale,
            detail_px: self.prefs.detail_px,
            imagery: self.prefs.style == Style::Satellite,
            ortho: self.prefs.national_ortho,
            detail: self.prefs.detail,
            max_dem: self.prefs.max_dem,
            max_img: self.prefs.max_img,
            prefetch: self.prefs.prefetch,
            shadow_rect: to_rect(self.shadow_rect),
            vs_rect: to_rect(self.vs_rect),
            line_fade: (self.cam.distance as f32 * 3.0).clamp(15_000.0, 90_000.0),
            landcover: self.prefs.landcover && self.land_rect.is_some(),
            land_rect: to_rect(self.land_rect),
            water_rect: to_rect(self.water_rect),
            time: self.epoch.elapsed().as_secs_f32() % 3600.0,
            trees: self.trees_on() && !self.trees.is_empty(),
            animate: self.fly.is_some() || self.flight.is_some(),
        }
    }

    /// Linie zmienne: trasy, warianty, ślad GPX, pomiar.
    fn dyn_lines(&self, view: &View) -> Vec<LineInst> {
        let mut out = Vec::new();
        let _ = view;
        let push_track = |out: &mut Vec<LineInst>, r: &Route, color: [f32; 4], width: f32, outline: bool| {
            let tr = &r.track;
            let n = tr.len();
            let step = (n / 6000).max(1);
            let pts: Vec<[f32; 3]> = (0..n)
                .step_by(step)
                .chain(std::iter::once(n - 1))
                .map(|i| {
                    let (x, z) = self.frame.to_local(tr.lon[i], tr.lat[i]);
                    [x as f32, tr.e[i] + 2.0, z as f32]
                })
                .collect();
            for w in pts.windows(2) {
                if outline {
                    out.push(LineInst { p0: w[0], p1: w[1], color: [0.02, 0.02, 0.02, 1.0], wo: [width + 4.0, 0.0] });
                }
            }
            for w in pts.windows(2) {
                out.push(LineInst { p0: w[0], p1: w[1], color, wo: [width, 0.0] });
            }
        };
        for (i, r) in self.routes.iter().enumerate() {
            if i != self.route_sel {
                push_track(&mut out, r, [0.75, 0.72, 0.66, 1.0], 3.0, false);
            }
        }
        if let Some(r) = &self.gpx {
            push_track(&mut out, r, lin([186, 104, 236], 1.0), 4.0, true);
        }
        if let Some((_, r)) = &self.compare {
            push_track(&mut out, r, lin([214, 120, 255], 1.0), 4.0, true);
        }
        if let Some(r) = self.routes.get(self.route_sel) {
            // wybrana trasa: jasna obwódka, ciemny brzeg i barwy nachylenia jak na profilu
            let tr = &r.track;
            let n = tr.len();
            if n >= 2 {
                let step = (n / 6000).max(1);
                let idx: Vec<usize> = (0..n).step_by(step).chain(std::iter::once(n - 1)).collect();
                let pt = |i: usize| {
                    let (x, z) = self.frame.to_local(tr.lon[i], tr.lat[i]);
                    [x as f32, tr.e[i] + 2.5, z as f32]
                };
                let base = if r.kind == RouteKind::Terrain { [255, 140, 60] } else { [255, 186, 60] };
                for (width, color) in [(13.0, lin([255, 255, 255], 1.0)), (9.0, [0.01, 0.01, 0.012, 1.0])] {
                    for w in idx.windows(2) {
                        out.push(LineInst { p0: pt(w[0]), p1: pt(w[1]), color, wo: [width, 0.0] });
                    }
                }
                for w in idx.windows(2) {
                    // nachylenie w oknie ~60 m wokół odcinka
                    let mid = (tr.d2[w[0]] + tr.d2[w[1]]) * 0.5;
                    let (a, b) = (tr.at_dist((mid - 30.0).max(0.0)), tr.at_dist(mid + 30.0));
                    let (ea, eb) = (tr.lerp(a.0, a.1).2, tr.lerp(b.0, b.1).2);
                    let g = (eb - ea) as f64 / 60.0;
                    let k = crate::metrics::grade_class(g);
                    let rgb = if k <= 1 { base } else { crate::metrics::GRADE_RGB[k] };
                    out.push(LineInst { p0: pt(w[0]), p1: pt(w[1]), color: lin(rgb, 1.0), wo: [6.0, 0.0] });
                }
            }
        }
        // pomiar: linia po terenie
        if self.measure.len() >= 2 {
            for w in self.measure.windows(2) {
                let n = (haversine(w[0].0, w[0].1, w[1].0, w[1].1) / 30.0).ceil().clamp(2.0, 400.0) as usize;
                let mut prev: Option<[f32; 3]> = None;
                for k in 0..=n {
                    let f = k as f64 / n as f64;
                    let lon = w[0].0 + (w[1].0 - w[0].0) * f;
                    let lat = w[0].1 + (w[1].1 - w[0].1) * f;
                    let e = self.heights.sample_cached(lon, lat).unwrap_or(0.0);
                    let (x, z) = self.frame.to_local(lon, lat);
                    let p = [x as f32, e + 2.0, z as f32];
                    if let Some(q) = prev {
                        out.push(LineInst { p0: q, p1: p, color: [1.0, 1.0, 1.0, 1.0], wo: [3.0, 0.0] });
                    }
                    prev = Some(p);
                }
            }
            if self.measure.len() == 2 {
                if let Some((vis, _)) = self.los {
                    let (a, b) = (self.measure[0], self.measure[1]);
                    let pa = self.local(a.0, a.1, a.2 + 1.7);
                    let pb = self.local(b.0, b.1, b.2 + 1.7);
                    let c = if vis.is_none() { lin([110, 230, 120], 1.0) } else { lin([255, 90, 54], 1.0) };
                    let ex = self.prefs.exag;
                    out.push(LineInst {
                        p0: [pa.x as f32, (pa.y / ex) as f32, pa.z as f32],
                        p1: [pb.x as f32, (pb.y / ex) as f32, pb.z as f32],
                        color: c,
                        wo: [2.0, 0.0],
                    });
                }
            }
        }
        // zaznaczony odcinek profilu: turkusowe wyróżnienie na trasie
        if let (Some((s0, s1)), Some(r)) = (self.profile_sel, self.current_route()) {
            let tr = &r.track;
            let mut pts: Vec<[f32; 3]> = Vec::new();
            let pt = |(lon, lat, e): (f64, f64, f32)| {
                let (x, z) = self.frame.to_local(lon, lat);
                [x as f32, e + 3.0, z as f32]
            };
            let a = tr.at_dist(s0);
            pts.push(pt(tr.lerp(a.0, a.1)));
            for i in 0..tr.len() {
                if tr.d2[i] > s0 && tr.d2[i] < s1 {
                    pts.push(pt((tr.lon[i], tr.lat[i], tr.e[i])));
                }
            }
            let b = tr.at_dist(s1);
            pts.push(pt(tr.lerp(b.0, b.1)));
            for w in pts.windows(2) {
                out.push(LineInst { p0: w[0], p1: w[1], color: lin([80, 220, 255], 1.0), wo: [9.0, 0.0] });
            }
        }
        out
    }

    /// Etykiety, znaczniki, kompas, podziałka, odczyt kursora i menu kontekstowe.
    fn overlays(&mut self, ui: &mut egui::Ui, rect: Rect, view: &View) {
        let p = ui.painter_at(rect);
        let at = |s: [f32; 2]| Pos2::new(s[0] + rect.left(), s[1] + rect.top());
        // etykiety punktów z rozrzedzaniem i zasłanianiem przez teren
        if self.prefs.labels {
            let mut taken: Vec<Rect> = Vec::new();
            let max = (70.0 * self.prefs.label_density) as usize;
            let max_places = (28.0 * self.prefs.label_density) as usize;
            let mut shown = 0;
            let mut shown_places = 0;
            // miejscowości najpierw (własny przydział), żeby szczyty ich nie wypierały
            let snap = self.snap.clone();
            let names = &self.prefs.names;
            let places = snap.pois.iter().filter(|p| p.poi.kind == PoiKind::Place && names.shows(&p.poi)).take(1500);
            let rest = snap.pois.iter().filter(|p| p.poi.kind != PoiKind::Place && names.shows(&p.poi)).take(2500);
            for poi in places.chain(rest) {
                let is_place = poi.poi.kind == PoiKind::Place;
                if shown >= max || (is_place && shown_places >= max_places) {
                    if is_place {
                        continue;
                    }
                    break;
                }
                let pos = self.local(poi.poi.lon, poi.poi.lat, poi.dem);
                let Some(s) = view.project(pos) else { continue };
                let sp = at(s);
                if !rect.shrink(4.0).contains(sp) {
                    continue;
                }
                let dist = pos.distance(view.eye);
                // drobne punkty tylko z bliska
                let min_prom = (dist / 9000.0) as f32 * if is_place { 0.35 } else { 1.0 };
                if poi.prom < min_prom.min(30.0) * 0.6 {
                    continue;
                }
                if self.occluded(view, pos) {
                    continue;
                }
                let (label, color, size) = match poi.poi.kind {
                    PoiKind::Peak if self.prefs.names.elevation => (format!("{}  {}", poi.poi.name, fmt::int(poi.ele as f64)), theme::PAPER, 12.5),
                    PoiKind::Peak => (poi.poi.name.clone(), theme::PAPER, 12.5),
                    PoiKind::Saddle if self.prefs.names.elevation && poi.ele > 0.0 => (format!("{}  {}", poi.poi.name, fmt::int(poi.ele as f64)), Color32::from_rgb(215, 222, 235), 11.0),
                    PoiKind::Waterfall | PoiKind::Spring => (poi.poi.name.clone(), Color32::from_rgb(150, 205, 240), 11.0),
                    PoiKind::Valley | PoiKind::Ridge => (poi.poi.name.clone(), Color32::from_rgb(205, 214, 190), 11.0),
                    PoiKind::Locality => (poi.poi.name.clone(), Color32::from_rgb(190, 225, 160), 11.0),
                    PoiKind::Place => (poi.poi.name.clone(), Color32::from_rgb(230, 222, 200), if poi.poi.place == "city" || poi.poi.place == "town" { 13.5 } else { 11.5 }),
                    PoiKind::Hut | PoiKind::Shelter => (poi.poi.name.clone(), Color32::from_rgb(255, 214, 150), 11.5),
                    PoiKind::Lake => (poi.poi.name.clone(), Color32::from_rgb(150, 205, 240), 11.0),
                    _ => (poi.poi.name.clone(), theme::PAPER_2, 11.0),
                };
                let font = FontId::proportional(size);
                let galley = p.layout_no_wrap(label, font, color);
                let lr = Rect::from_min_size(sp + Vec2::new(7.0, -galley.size().y - 3.0), galley.size()).expand(2.0);
                // w panoramie pod nazwą jest jeszcze odległość – miejsce zajmuje też ona
                let busy = if self.panorama.is_some() { Rect::from_min_max(lr.min, lr.max + Vec2::new(0.0, 13.0)) } else { lr };
                if taken.iter().any(|t| t.intersects(busy)) {
                    continue;
                }
                taken.push(busy);
                shown += 1;
                shown_places += is_place as usize;
                let dot = match poi.poi.kind {
                    PoiKind::Peak => theme::PAPER,
                    PoiKind::Hut | PoiKind::Shelter => Color32::from_rgb(255, 170, 70),
                    PoiKind::Lake | PoiKind::Waterfall | PoiKind::Spring => Color32::from_rgb(110, 180, 230),
                    PoiKind::Locality => Color32::from_rgb(150, 205, 110),
                    PoiKind::Valley | PoiKind::Ridge => Color32::from_rgb(170, 180, 150),
                    PoiKind::Place => Color32::from_rgb(200, 190, 170),
                    _ => theme::PAPER_2,
                };
                p.line_segment([sp, sp + Vec2::new(5.0, -5.0)], Stroke::new(1.0, Color32::from_black_alpha(160)));
                if poi.poi.kind == PoiKind::Peak {
                    let tri = vec![sp + Vec2::new(0.0, -5.0), sp + Vec2::new(4.5, 3.0), sp + Vec2::new(-4.5, 3.0)];
                    p.add(egui::Shape::convex_polygon(tri, dot, Stroke::new(1.0, Color32::from_black_alpha(200))));
                } else {
                    p.circle(sp, 3.5, dot, Stroke::new(1.0, Color32::from_black_alpha(200)));
                }
                let tp = lr.min + Vec2::splat(2.0);
                // obwódka dla czytelności na jasnym terenie
                for o in [Vec2::new(1.0, 1.0), Vec2::new(-1.0, 1.0), Vec2::new(1.0, -1.0), Vec2::new(-1.0, -1.0)] {
                    p.galley_with_override_text_color(tp + o, galley.clone(), Color32::from_black_alpha(170));
                }
                p.galley(tp, galley, color);
                if self.panorama.is_some() {
                    let d = haversine(self.cam.lon, self.cam.lat, poi.poi.lon, poi.poi.lat);
                    // odległość tuż pod nazwą, zwykłym pismem, z ikoną strzałki
                    let txt = format!("{} {}", theme::icon::ARROWS_HORIZONTAL, fmt::dist(d));
                    let g = p.layout_no_wrap(txt, FontId::proportional(11.0), theme::PAPER_2);
                    let dp = lr.left_bottom() + Vec2::new(2.0, -2.0);
                    for o in [Vec2::new(1.0, 1.0), Vec2::new(-1.0, 1.0), Vec2::new(1.0, -1.0), Vec2::new(-1.0, -1.0)] {
                        p.galley_with_override_text_color(dp + o, g.clone(), Color32::from_black_alpha(170));
                    }
                    p.galley(dp, g, theme::PAPER_2);
                }
            }
        }
        // punkty trasy
        let n = self.waypoints.len();
        for (i, w) in self.waypoints.iter().enumerate() {
            if let Some(s) = view.project(self.local(w.lon, w.lat, w.e)) {
                let sp = at(s);
                let (fill, label) = if i == 0 {
                    (Color32::from_rgb(90, 200, 110), "A".to_string())
                } else if i == n - 1 && n > 1 {
                    (theme::SIGNAL, "B".to_string())
                } else {
                    (theme::BRASS, i.to_string())
                };
                p.circle(sp, 9.0, fill, Stroke::new(2.0, Color32::from_rgb(20, 20, 20)));
                p.text(sp, Align2::CENTER_CENTER, label, FontId::proportional(11.0), Color32::from_rgb(20, 16, 8));
            }
        }
        for (i, m) in self.measure.iter().enumerate() {
            if let Some(s) = view.project(self.local(m.0, m.1, m.2)) {
                let sp = at(s);
                p.circle(sp, 6.0, Color32::WHITE, Stroke::new(2.0, Color32::BLACK));
                p.text(sp + Vec2::new(9.0, -9.0), Align2::LEFT_BOTTOM, (i + 1).to_string(), FontId::proportional(11.0), Color32::WHITE);
            }
        }
        if let Some((lon, lat, e)) = self.lens_pin {
            if let Some(s) = view.project(self.local(lon, lat, e)) {
                p.circle_stroke(at(s), 8.0, Stroke::new(2.5, theme::BRASS));
                p.circle_filled(at(s), 3.0, theme::BRASS);
            }
        }
        match &self.selection {
            Selection::Point { lon, lat } => {
                let e = self.heights.sample_cached(*lon, *lat).unwrap_or(0.0);
                if let Some(s) = view.project(self.local(*lon, *lat, e)) {
                    p.circle_stroke(at(s), 7.0, Stroke::new(2.0, theme::TEAL));
                }
            }
            Selection::Poi(i) => {
                if let Some(poi) = self.snap.pois.get(*i) {
                    if let Some(s) = view.project(self.local(poi.poi.lon, poi.poi.lat, poi.dem)) {
                        p.circle_stroke(at(s), 9.0, Stroke::new(2.0, theme::TEAL));
                    }
                }
            }
            _ => {}
        }
        // kursor profilu na trasie
        if let (Some(d), Some(r)) = (self.profile_cursor, self.current_route()) {
            let (i, f) = r.track.at_dist(d);
            let (lon, lat, e) = r.track.lerp(i, f);
            if let Some(s) = view.project(self.local(lon, lat, e)) {
                p.circle(at(s), 6.0, theme::BRASS, Stroke::new(2.0, Color32::BLACK));
            }
        }

        // punkt obrotu
        if let Some((lon, lat, e)) = self.orbit_pivot {
            if let Some(s) = view.project(self.local(lon, lat, e)) {
                let c = at(s);
                p.circle_filled(c, 3.5, theme::BRASS);
                p.circle_stroke(c, 8.0, Stroke::new(1.5, theme::PAPER));
            }
        }

        // kompas (chowa się razem z kontrolkami)
        let alpha = crate::ui::panels::controls_alpha(self, ui.ctx());
        let mut pc = p.clone();
        pc.multiply_opacity(alpha);
        let r = 30.0;
        // pod górnym paskiem, obok panelu bocznego
        let c = rect.right_top() + Vec2::new(-51.0 - crate::ui::panels::right_inset(self), 51.0 + crate::ui::panels::TOP_BAR_H);
        let compass = Rect::from_center_size(c, Vec2::splat(r * 2.0 + 4.0));
        let resp = ui.interact(compass, ui.id().with("kompas"), if alpha > 0.05 { Sense::click_and_drag() } else { Sense::hover() });
        let hot = resp.hovered() || resp.dragged();
        pc.circle_filled(c + Vec2::new(0.0, 2.0), r + 1.0, Color32::from_black_alpha(90));
        pc.circle(
            c,
            r,
            Color32::from_rgba_unmultiplied(26, 29, 35, 242),
            Stroke::new(1.0, if hot { theme::PAPER_3 } else { theme::INK_4 }),
        );
        // kierunek patrzenia: stały znacznik u góry tarczy
        let top = c - Vec2::new(0.0, r);
        pc.add(egui::Shape::convex_polygon(
            vec![top + Vec2::new(0.0, 6.0), top + Vec2::new(-4.0, -1.0), top + Vec2::new(4.0, -1.0)],
            theme::BRASS,
            Stroke::NONE,
        ));
        // tarcza obraca się z kamerą: podziałka co 15°, główne kierunki co 90°
        let rot = |deg: f32| {
            let a = (deg - self.cam.heading as f32).to_radians();
            Vec2::new(a.sin(), -a.cos())
        };
        for i in 0..24 {
            let deg = i as f32 * 15.0;
            let d = rot(deg);
            let (len, w, col) = match i % 6 {
                0 => (0.0, 0.0, theme::PAPER),
                _ if i % 2 == 0 => (5.0, 1.2, theme::PAPER_3),
                _ => (3.0, 1.0, theme::INK_4),
            };
            if len > 0.0 {
                pc.line_segment([c + d * (r - 2.0), c + d * (r - 2.0 - len)], Stroke::new(w, col));
            }
        }
        for (deg, s) in [(90.0, "E"), (180.0, "S"), (270.0, "W")] {
            pc.text(c + rot(deg) * (r - 9.0), Align2::CENTER_CENTER, s, FontId::proportional(9.5), theme::PAPER_3);
        }
        pc.text(c + rot(0.0) * (r - 9.0), Align2::CENTER_CENTER, "N", FontId::proportional(11.0), theme::SIGNAL);
        // igła: czerwona połowa na północ, jasna na południe, pochylona razem z kamerą
        let n = rot(0.0);
        let side = Vec2::new(-n.y, n.x) * 3.5;
        let tip = 15.0 * (0.45 + 0.55 * (self.cam.pitch as f32).to_radians().sin().abs()).min(1.0);
        pc.add(egui::Shape::convex_polygon(vec![c + n * tip, c + side, c - side], theme::SIGNAL, Stroke::NONE));
        pc.add(egui::Shape::convex_polygon(vec![c - n * tip, c - side, c + side], theme::PAPER_3, Stroke::NONE));
        pc.circle(c, 2.5, Color32::from_rgb(26, 29, 35), Stroke::new(1.0, theme::PAPER_3));
        // odczyt kursu pod tarczą
        let hdg = (self.cam.heading.rem_euclid(360.0).round() as i32) % 360;
        const DIRS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
        let label = tf!("{:03}° {}", hdg, DIRS[((hdg as f32 + 22.5) / 45.0) as usize % 8]);
        let lp = c + Vec2::new(0.0, r + 11.0);
        let galley = pc.layout_no_wrap(label, FontId::monospace(10.5), theme::PAPER);
        let bg = Rect::from_center_size(lp, galley.size() + Vec2::new(10.0, 4.0));
        pc.rect_filled(bg, 4.0, Color32::from_rgba_unmultiplied(26, 29, 35, 230));
        pc.galley(bg.center() - galley.size() / 2.0, galley, theme::PAPER);
        // klik: północ; dwuklik: północ i widok z góry; przeciągnięcie w poziomie: obrót
        if resp.dragged() {
            self.cam.heading = (self.cam.heading - resp.drag_delta().x as f64 * 1.2).rem_euclid(360.0);
        }
        if resp.double_clicked() {
            let mut to = self.cam;
            to.heading = 0.0;
            to.pitch = 89.0;
            self.start_fly(to, 0.7);
        } else if resp.clicked() {
            let mut to = self.cam;
            to.heading = 0.0;
            self.start_fly(to, 0.7);
        }
        resp.on_hover_text(crate::i18n::tr("Klik: północ u góry (N)\nDwuklik: północ i widok z góry\nPrzeciągnij w bok: obrót"));

        // podziałka
        let mpp = view.eye.distance(view.target) * (self.cam.fov_y.to_radians() / 2.0).tan() * 2.0 / rect.height() as f64;
        let target = mpp * 120.0;
        let nice = [1.0, 2.0, 5.0]
            .iter()
            .flat_map(|m| (0..7).map(move |e| m * 10f64.powi(e)))
            .filter(|v| *v <= target)
            .fold(1.0, f64::max);
        let px = (nice / mpp) as f32;
        let base = rect.left_bottom() + Vec2::new(16.0, -16.0 - crate::ui::panels::bottom_inset(self));
        p.line_segment([base, base + Vec2::new(px, 0.0)], Stroke::new(2.0, theme::PAPER));
        p.line_segment([base, base - Vec2::new(0.0, 5.0)], Stroke::new(2.0, theme::PAPER));
        p.line_segment([base + Vec2::new(px, 0.0), base + Vec2::new(px, -5.0)], Stroke::new(2.0, theme::PAPER));
        p.text(base + Vec2::new(px + 6.0, -2.0), Align2::LEFT_CENTER, fmt::dist(nice), FontId::monospace(11.0), theme::PAPER);

        // odczyt kursora
        let mut info = String::new();
        if let Some((lon, lat, e)) = self.hover {
            let g = |dx: f64, dy: f64| self.heights.sample_cached(lon + dx, lat + dy).unwrap_or(e) as f64;
            let kx = 111_320.0 * lat.to_radians().cos();
            let step = 15.0;
            let gx = (g(step / kx, 0.0) - g(-step / kx, 0.0)) / (2.0 * step);
            let gy = (g(0.0, step / 110_574.0) - g(0.0, -step / 110_574.0)) / (2.0 * step);
            let slope = gx.hypot(gy).atan().to_degrees();
            let aspect = (-gx).atan2(-gy).to_degrees().rem_euclid(360.0);
            info = tf!("{}   {}   nachylenie {}°   ekspozycja {}", fmt::coords(lon, lat), fmt::ele(e as f64), slope.round(), fmt::compass(aspect));
            if let Some((rl, rt, re)) = self.lens_pin.filter(|_| self.lens == Lens::Rel) {
                let d = haversine(rl, rt, lon, lat);
                let dh = e - re;
                info.push_str(&format!(
                    "   Δh {}   {}   kąt {}°   azymut {}°",
                    fmt::signed(dh as f64, "m"),
                    fmt::dist(d),
                    fmt::dec((dh as f64 / d.max(1.0)).atan().to_degrees(), 1),
                    bearing(rl, rt, lon, lat).round()
                ));
            }
        }
        let bar = Rect::from_min_max(rect.left_bottom() - Vec2::new(0.0, 24.0), rect.right_bottom());
        self.glass.push((bar, 1.0));
        p.rect_filled(bar, 0.0, theme::GLASS_FILL);
        p.text(bar.left_center() + Vec2::new(12.0, 0.0), Align2::LEFT_CENTER, info, FontId::monospace(11.5), theme::PAPER_2);
        let attr = match self.prefs.style {
            Style::Satellite => crate::i18n::tr("Zdjęcia © Esri, Maxar, Earthstar Geographics · teren © Mapterhorn · © OpenStreetMap"),
            _ => crate::i18n::tr("Teren © Mapterhorn (ÚGKK SR, GUGiK, ČÚZK), AWS Open Data · © OpenStreetMap"),
        };
        p.text(bar.right_center() - Vec2::new(12.0, 0.0), Align2::RIGHT_CENTER, attr, FontId::proportional(10.5), theme::PAPER_3);

        // stan wczytywania i tryby
        let mut chips: Vec<String> = Vec::new();
        let (done, all, work) = self.world.progress();
        // gdy okolica pod kamerą jest gotowa, reszta to tylko dalsze sektory
        let far = self.world.center_ready();
        match (self.world.status(), work) {
            (_, Some(w)) if far => chips.push(tf!("szlaki dalszej okolicy: {}/{} ({})", done, all, w)),
            (_, Some(w)) => chips.push(tf!("nazwy i szlaki: {}/{} sektorów ({})", done, all, w)),
            ((Some(_), _), None) => chips.push(tf!("nazwy i szlaki: {}/{} sektorów", done, all)),
            ((None, Some(e)), None) if done < all => chips.push(tf!("nazwy i szlaki: {}", short(&e))),
            _ => {}
        }
        if !self.status_line.is_empty() {
            chips.push(self.status_line.clone());
        }
        if self.world.land_busy() {
            chips.push(crate::i18n::tr("pobieranie pokrycia terenu (OSM)…").into());
        }
        if self.land_busy {
            chips.push(crate::i18n::tr("wypiekanie pokrycia terenu i poziomów wody…").into());
        }
        if self.shadow_busy {
            chips.push(crate::i18n::tr("liczenie cieni…").into());
        }
        if self.routing {
            chips.push(crate::i18n::tr("wyznaczanie trasy…").into());
        }
        if self.vs_busy {
            chips.push(crate::i18n::tr("liczenie widoczności…").into());
        }
        if self.panorama.is_some() {
            chips.push(crate::i18n::tr("panorama · Esc – powrót").into());
        }
        if self.flight.is_some() {
            chips.push(crate::i18n::tr("przelot · F – stop").into());
        }
        let lens_txt = match self.lens {
            Lens::Band => Some(crate::i18n::tr("soczewka: ta sama wysokość")),
            Lens::Rel => Some(crate::i18n::tr("soczewka: wysokość względna")),
            Lens::Vis => Some(crate::i18n::tr("soczewka: widoczność")),
            Lens::None => None,
        };
        if let Some(t) = lens_txt {
            chips.push(format!("{t}{}", if self.lens_pin.is_some() { " (przypięta · Esc)" } else { " · klik przypina" }));
        }
        // statusy na środku dolnego paska (nad odczytem kursora i atrybucją)
        if !chips.is_empty() {
            // zwykły tekst na pasku, bez tła i obwódki; klik otwiera szczegóły wczytywania
            let g = p.layout_no_wrap(chips.join("  ·  "), FontId::proportional(11.5), theme::PAPER_2);
            let r = Rect::from_center_size(bar.center(), g.size() + Vec2::new(8.0, 4.0));
            let resp = ui.interact(r, ui.id().with("stan-wczytywania"), Sense::click());
            let color = if resp.hovered() { theme::PAPER } else { theme::PAPER_2 };
            p.galley(bar.center() - g.size() / 2.0, g, color);
            if resp.on_hover_text(crate::i18n::tr("Kliknij, żeby zobaczyć szczegóły wczytywania")).clicked() {
                self.load_details = !self.load_details;
            }
        }

        // menu kontekstowe
        if let Some((pos, lon, lat, e)) = self.ctx_menu {
            let mut close = false;
            egui::Area::new(egui::Id::new("menu-3d")).fixed_pos(pos).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
                theme::card(false).show(ui, |ui| {
                    ui.set_width(230.0);
                    ui.label(egui::RichText::new(format!("{}  ·  {}", fmt::decimal(lon, lat), fmt::ele(e as f64))).small().monospace().color(theme::PAPER_3));
                    let mut item = |ui: &mut egui::Ui, t: &str| ui.add(egui::Button::new(t).frame(false)).clicked();
                    if item(ui, crate::i18n::tr("Trasa stąd")) {
                        self.tool = Tool::Route;
                        if self.waypoints.is_empty() { self.add_waypoint(lon, lat, None) } else { self.add_waypoint(lon, lat, Some(0)) }
                        close = true;
                    }
                    if item(ui, crate::i18n::tr("Trasa tutaj")) {
                        self.tool = Tool::Route;
                        self.add_waypoint(lon, lat, None);
                        close = true;
                    }
                    if self.waypoints.len() >= 2 && item(ui, crate::i18n::tr("Przez ten punkt")) {
                        let at = self.waypoints.len() - 1;
                        self.add_waypoint(lon, lat, Some(at));
                        close = true;
                    }
                    ui.separator();
                    if item(ui, crate::i18n::tr("Ta sama wysokość")) {
                        self.pin_lens(lon, lat, Lens::Band);
                        close = true;
                    }
                    if item(ui, crate::i18n::tr("Wysokość względna")) {
                        self.pin_lens(lon, lat, Lens::Rel);
                        close = true;
                    }
                    if item(ui, crate::i18n::tr("Co widać stąd?")) {
                        self.pin_lens(lon, lat, Lens::Vis);
                        close = true;
                    }
                    if item(ui, crate::i18n::tr("Spacer stąd")) {
                        self.enter_panorama(lon, lat);
                        close = true;
                    }
                    if item(ui, crate::i18n::tr("Pomiar stąd")) {
                        self.tool = Tool::Measure;
                        self.measure = vec![(lon, lat, e)];
                        self.los = None;
                        close = true;
                    }
                    ui.separator();
                    if item(ui, crate::i18n::tr("Informacje o punkcie")) {
                        self.tool = Tool::Explore;
                        self.selection = Selection::Point { lon, lat };
                        self.prefs.panel_open = true;
                        close = true;
                    }
                    if item(ui, crate::i18n::tr("Kopiuj współrzędne")) {
                        ui.ctx().copy_text(fmt::decimal(lon, lat));
                        self.notify(crate::i18n::tr("Skopiowano współrzędne."));
                        close = true;
                    }
                });
            });
            if close {
                self.ctx_menu = None;
            }
        }

        // komunikat
        let expired = self.toast.as_ref().is_some_and(|(_, t)| t.elapsed() >= Duration::from_secs(5));
        if expired {
            self.toast = None;
        }
        if let Some((msg, _)) = &self.toast {
            {
                let g = p.layout(msg.clone(), FontId::proportional(13.0), theme::PAPER, 460.0);
                let r = Rect::from_center_size(rect.center_bottom() - Vec2::new(0.0, 70.0), g.size() + Vec2::new(28.0, 18.0));
                p.rect(r, 10.0, theme::INK_2, Stroke::new(1.0, theme::BRASS_DIM), egui::StrokeKind::Inside);
                p.galley(r.min + Vec2::new(14.0, 9.0), g, theme::PAPER);
                ui.ctx().request_repaint_after(Duration::from_millis(300));
            }
        }
    }

    /// Czy teren zasłania punkt (próbkowanie wzdłuż linii z oka, z poziomu ~38 m).
    /// Czy teren zasłania punkt. Wyniki pamiętane, dopóki kamera stoi (i najwyżej 1 s – dane terenu
    /// mogą się doczytać), więc przy nieruchomym widoku etykiety nie liczą linii wzroku co klatkę.
    fn occluded(&self, view: &View, p: DVec3) -> bool {
        let key = (
            (view.eye.x * 2.0).round() as i64,
            (view.eye.y * 2.0).round() as i64,
            (view.eye.z * 2.0).round() as i64,
            (self.prefs.exag * 100.0).round() as i64,
        );
        let pk = ((p.x * 2.0).round() as i64, (p.z * 2.0).round() as i64);
        {
            let mut c = self.occl_cache.borrow_mut();
            if c.0 != key || c.1.elapsed() > Duration::from_secs(1) {
                *c = (key, Instant::now(), std::collections::HashMap::new());
            }
            if let Some(&v) = c.2.get(&pk) {
                return v;
            }
        }
        let v = self.occluded_now(view, p);
        self.occl_cache.borrow_mut().2.insert(pk, v);
        v
    }

    fn occluded_now(&self, view: &View, p: DVec3) -> bool {
        let e = view.eye;
        // krok ~ co 70 m (najwyżej 200 próbek): cienkie granie między okiem a punktem też zasłaniają
        let len = (p.x - e.x).hypot(p.z - e.z);
        let n = ((len / 70.0) as usize).clamp(16, 200);
        for k in 1..n {
            let f = k as f64 / n as f64;
            let q = e + (p - e) * f;
            let (lon, lat) = self.frame.to_lonlat(q.x, q.z);
            let h = self.heights.sample_at(lon, lat, 12, false).or_else(|| self.heights.sample_cached(lon, lat));
            if let Some(h) = h {
                let hd = (q.x - e.x).hypot(q.z - e.z);
                let drop = hd * hd / (2.0 * crate::geo::EARTH_R) * 0.87;
                if h as f64 * self.prefs.exag - drop > q.y + 15.0 {
                    return true;
                }
            }
        }
        false
    }
}

fn short(s: &str) -> String {
    let mut out: String = s.chars().take(60).collect();
    if s.chars().count() > 60 {
        out.push('…');
    }
    out
}

/// Linie szlaków z grafu: kolory relacji (kilka kolorów – równoległe pasy).
/// Szlaki; przy wyznaczonej trasie przyciemnione i cieńsze, żeby trasa się wyróżniała.
/// Łańcuchy i klamry: pomarańczowe odcinki z ciemnym brzegiem, nad kolorami szlaków.
fn chain_lines(snap: &Snapshot, frame: &Frame, ppp: f32) -> Vec<LineInst> {
    let mut out = Vec::new();
    let w = 4.0 * ppp.max(1.0);
    for line in &snap.chains {
        let pts: Vec<[f32; 3]> = line
            .iter()
            .map(|&(lon, lat, e)| {
                let (x, z) = frame.to_local(lon, lat);
                [x as f32, e + 1.5, z as f32]
            })
            .collect();
        for (width, color) in [(w + 3.0, [0.02, 0.02, 0.02, 1.0]), (w, lin([255, 140, 20], 1.0))] {
            for p in pts.windows(2) {
                out.push(LineInst { p0: p[0], p1: p[1], color, wo: [width, 0.0] });
            }
        }
    }
    out
}

/// Granice: parki narodowe (zielone), rezerwaty (jasnozielone), państwa (jasne).
fn boundary_lines(snap: &Snapshot, frame: &Frame, ppp: f32) -> Vec<LineInst> {
    use crate::osm::BoundaryKind as B;
    let mut out = Vec::new();
    for (kind, _, lines) in &snap.boundaries {
        let (rgb, w) = match kind {
            B::NationalPark => ([70, 190, 100], 2.4),
            B::Reserve => ([150, 215, 120], 1.6),
            B::Country => ([240, 240, 240], 2.8),
        };
        let color = lin(rgb, 0.9);
        for line in lines {
            let pts: Vec<[f32; 3]> = line
                .iter()
                .map(|&(lon, lat, e)| {
                    let (x, z) = frame.to_local(lon, lat);
                    [x as f32, e + 1.0, z as f32]
                })
                .collect();
            // przerywana: co drugi odcinek ~ co 30 m
            for (k, p) in pts.windows(2).enumerate() {
                if k % 2 == 0 || *kind == B::Country {
                    out.push(LineInst { p0: p[0], p1: p[1], color, wo: [w * ppp.max(1.0), 0.0] });
                }
            }
        }
    }
    out
}

fn trail_lines(snap: &Snapshot, frame: &Frame, ppp: f32, dim: bool) -> Vec<LineInst> {
    let mut out = Vec::new();
    let w = if dim { 1.6 } else { 2.6 } * ppp.max(1.0);
    for ed in &snap.graph.edges {
        let n = ed.n();
        // co ~25 m wystarczy do rysowania
        let step = 2usize;
        let idx: Vec<usize> = (0..n).step_by(step).chain(std::iter::once(n - 1)).collect();
        let pts: Vec<[f32; 3]> = idx
            .iter()
            .map(|&i| {
                let (x, z) = frame.to_local(ed.lon[i], ed.lat[i]);
                [x as f32, ed.e[i] + 1.0, z as f32]
            })
            .collect();
        let k = ed.colors.len() as f32;
        for (ci, c) in ed.colors.iter().enumerate() {
            let off = (ci as f32 - (k - 1.0) / 2.0) * w;
            let mut col = lin(c.rgb(), 0.98);
            if *c == crate::metrics::TrailColor::Other {
                col = lin([214, 196, 160], 0.98);
            }
            if dim {
                // wyblakłe: zbliżone do szarości i ciemniejsze
                let l = col[0] * 0.2126 + col[1] * 0.7152 + col[2] * 0.0722;
                for k in 0..3 {
                    col[k] = (col[k] * 0.35 + l * 0.65) * 0.32;
                }
            }
            for p in pts.windows(2) {
                if p[0] == p[1] {
                    continue;
                }
                out.push(LineInst { p0: p[0], p1: p[1], color: col, wo: [w, off] });
            }
        }
    }
    out
}
