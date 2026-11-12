//! Wszystkie polecenia programu w jednym rejestrze: nazwa, ikona, grupa, skrót klawiszowy, stan.
//! Z rejestru korzystają skróty klawiszowe ([`Action::for_key`]), paleta poleceń (Ctrl+K) i okno
//! skrótów, więc nowe polecenie dopisane tutaj od razu działa we wszystkich trzech miejscach.

use super::*;

/// Skrót: klawisz z modyfikatorami.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keys {
    pub key: egui::Key,
    pub ctrl: bool,
    pub shift: bool,
}

const fn k(key: egui::Key) -> Option<Keys> {
    Some(Keys { key, ctrl: false, shift: false })
}

const fn ctrl(key: egui::Key) -> Option<Keys> {
    Some(Keys { key, ctrl: true, shift: false })
}

impl Keys {
    /// Zapis skrótu do wyświetlenia, np. „Ctrl+Shift+Z”.
    pub fn text(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Ctrl+");
        }
        if self.shift {
            s.push_str("Shift+");
        }
        s.push_str(self.key.symbol_or_name());
        s
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Tools,
    View,
    Layers,
    Panels,
    Route,
    Interface,
}

impl Group {
    pub fn label(self) -> &'static str {
        match self {
            Group::Tools => crate::i18n::tr("Narzędzia"),
            Group::View => crate::i18n::tr("Widok"),
            Group::Layers => crate::i18n::tr("Warstwy"),
            Group::Panels => crate::i18n::tr("Panele i okna"),
            Group::Route => crate::i18n::tr("Trasa"),
            Group::Interface => crate::i18n::tr("Interfejs"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Trails,
    Labels,
    Contours,
    Grid,
    Shadows,
    Boundaries,
    Chains,
    Ortho,
    Landcover,
    Snow,
    Closures,
    Pistes,
    Roads,
}

impl Layer {
    pub const ALL: [Layer; 13] = [
        Layer::Trails,
        Layer::Labels,
        Layer::Contours,
        Layer::Grid,
        Layer::Shadows,
        Layer::Boundaries,
        Layer::Chains,
        Layer::Ortho,
        Layer::Landcover,
        Layer::Snow,
        Layer::Closures,
        Layer::Pistes,
        Layer::Roads,
    ];

    pub fn flag(self, p: &mut Prefs) -> &mut bool {
        match self {
            Layer::Trails => &mut p.trails,
            Layer::Labels => &mut p.labels,
            Layer::Contours => &mut p.contours,
            Layer::Grid => &mut p.km_grid,
            Layer::Shadows => &mut p.shadows,
            Layer::Boundaries => &mut p.boundaries,
            Layer::Chains => &mut p.chains,
            Layer::Ortho => &mut p.national_ortho,
            Layer::Landcover => &mut p.landcover,
            Layer::Snow => &mut p.snow,
            Layer::Closures => &mut p.closures,
            Layer::Pistes => &mut p.pistes,
            Layer::Roads => &mut p.roads,
        }
    }

    pub fn on(self, p: &Prefs) -> bool {
        match self {
            Layer::Trails => p.trails,
            Layer::Labels => p.labels,
            Layer::Contours => p.contours,
            Layer::Grid => p.km_grid,
            Layer::Shadows => p.shadows,
            Layer::Boundaries => p.boundaries,
            Layer::Chains => p.chains,
            Layer::Ortho => p.national_ortho,
            Layer::Landcover => p.landcover,
            Layer::Snow => p.snow,
            Layer::Closures => p.closures,
            Layer::Pistes => p.pistes,
            Layer::Roads => p.roads,
        }
    }

    fn info(self) -> (&'static str, &'static str, Option<Keys>) {
        use egui::Key as K;
        use theme::icon as i;
        match self {
            Layer::Trails => (i::PATH, "Szlaki", k(K::T)),
            Layer::Labels => (i::TEXT_AA, "Nazwy na mapie", k(K::L)),
            Layer::Contours => (i::WAVE_SINE, "Poziomice", k(K::C)),
            Layer::Grid => (i::GRID_FOUR, "Siatka UTM", k(K::U)),
            Layer::Shadows => (i::CLOUD_SUN, "Cienie", None),
            Layer::Boundaries => (i::FLAG, "Granice parków", None),
            Layer::Chains => (i::LINK, "Łańcuchy i klamry", None),
            Layer::Ortho => (i::IMAGE, "Ortofoto krajowe", None),
            Layer::Landcover => (i::TREE_EVERGREEN, "Lasy i woda", None),
            Layer::Snow => (i::SNOWFLAKE, "Śnieg", None),
            Layer::Closures => (i::PROHIBIT, "Zamknięte szlaki", k(K::Z)),
            Layer::Pistes => (i::PERSON_SIMPLE_SKI, "Trasy narciarskie", k(K::J)),
            Layer::Roads => (i::ROAD_HORIZON, "Drogi i ulice", None),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Tool(Tool),
    Lens(Lens),
    Walk,
    Flight,
    North,
    TopView,
    ResetView,
    Fullscreen,
    NextStyle,
    Style(Style),
    ExagUp,
    ExagDown,
    HourBack,
    HourForward,
    Now,
    Layer(Layer),
    Panel,
    Details,
    Library,
    Conditions,
    Search,
    Palette,
    Help,
    Settings,
    LoadDetails,
    ExportGpx,
    ExportFit,
    ExportKml,
    ImportTrack,
    Undo,
    Redo,
    SaveRoute,
    ReverseRoute,
    ClearRoute,
    DropLast,
    FitRoute,
    TransitToStart,
    OpenMapy,
    SaveView,
    Screenshot,
    HighContrast,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    CheckUpdates,
}

impl Action {
    /// Wszystkie polecenia w kolejności palety.
    pub fn all() -> Vec<Action> {
        let mut v: Vec<Action> = Tool::ALL.iter().map(|t| Action::Tool(*t)).collect();
        v.extend(Lens::ALL.iter().map(|l| Action::Lens(*l)));
        v.extend([Action::Walk, Action::Flight, Action::North, Action::TopView, Action::ResetView, Action::Fullscreen, Action::NextStyle]);
        v.extend(Style::ALL.iter().map(|s| Action::Style(*s)));
        v.extend([Action::ExagUp, Action::ExagDown, Action::HourBack, Action::HourForward, Action::Now]);
        v.extend(Layer::ALL.iter().map(|l| Action::Layer(*l)));
        v.extend([
            Action::Panel,
            Action::Details,
            Action::Library,
            Action::Conditions,
            Action::Search,
            Action::Palette,
            Action::Help,
            Action::Settings,
            Action::LoadDetails,
            Action::ExportGpx,
            Action::ExportFit,
            Action::ExportKml,
            Action::ImportTrack,
            Action::Undo,
            Action::Redo,
            Action::SaveRoute,
            Action::ReverseRoute,
            Action::ClearRoute,
            Action::DropLast,
            Action::FitRoute,
            Action::TransitToStart,
            Action::OpenMapy,
            Action::SaveView,
            Action::Screenshot,
            Action::HighContrast,
            Action::ZoomIn,
            Action::ZoomOut,
            Action::ZoomReset,
            Action::CheckUpdates,
        ]);
        v
    }

    /// Ikona, nazwa (przetłumaczona) i grupa.
    pub fn info(self) -> (&'static str, &'static str, Group) {
        use crate::i18n::tr;
        use theme::icon as i;
        match self {
            Action::Tool(t) => (t.behavior().icon(), t.behavior().hint(), Group::Tools),
            Action::Lens(l) => (l.icon(), l.label(), Group::Tools),
            Action::Walk => (i::PERSON_SIMPLE_WALK, tr("Spacer ze środka widoku"), Group::Tools),
            Action::Flight => (i::AIRPLANE_TILT, tr("Przelot nad trasą"), Group::Tools),
            Action::North => (i::COMPASS, tr("Północ u góry"), Group::View),
            Action::TopView => (i::ARROW_FAT_DOWN, tr("Widok z góry / ukośny"), Group::View),
            Action::ResetView => (i::ARROW_COUNTER_CLOCKWISE, tr("Domyślne nachylenie widoku"), Group::View),
            Action::Fullscreen => (i::CORNERS_OUT, tr("Pełny ekran"), Group::View),
            Action::NextStyle => (i::PAINT_BRUSH, tr("Następny styl mapy"), Group::View),
            Action::Style(s) => (i::PAINT_BRUSH, s.label(), Group::View),
            Action::ExagUp => (i::ARROW_UP, tr("Większe przewyższenie"), Group::View),
            Action::ExagDown => (i::ARROW_DOWN, tr("Mniejsze przewyższenie"), Group::View),
            Action::HourBack => (i::CLOCK_COUNTER_CLOCKWISE, tr("Pół godziny wcześniej"), Group::View),
            Action::HourForward => (i::CLOCK_CLOCKWISE, tr("Pół godziny później"), Group::View),
            Action::Now => (i::CLOCK, tr("Bieżąca pora dnia"), Group::View),
            Action::Layer(l) => {
                let (g, t, _) = l.info();
                (g, tr(t), Group::Layers)
            }
            Action::Panel => (i::SIDEBAR_SIMPLE, tr("Panel boczny"), Group::Panels),
            Action::Details => (i::INFO, tr("Szczegóły okolicy"), Group::Panels),
            Action::Library => (i::BOOKMARKS, tr("Biblioteka"), Group::Panels),
            Action::Conditions => (i::CLOUD_SUN, tr("Warunki: pogoda i lawiny"), Group::Panels),
            Action::Search => (i::MAGNIFYING_GLASS, tr("Szukaj na mapie"), Group::Panels),
            Action::Palette => (i::COMMAND, tr("Paleta poleceń"), Group::Panels),
            Action::Help => (i::QUESTION, tr("Skróty i sterowanie"), Group::Panels),
            Action::Settings => (i::GEAR, tr("Ustawienia"), Group::Panels),
            Action::LoadDetails => (i::CLOUD_ARROW_DOWN, tr("Szczegóły wczytywania danych"), Group::Panels),
            Action::ExportGpx => (i::EXPORT, tr("Eksport trasy do GPX"), Group::Route),
            Action::ExportFit => (i::WATCH, tr("Eksport kursu do zegarka (FIT)"), Group::Route),
            Action::ExportKml => (i::MAP_TRIFOLD, tr("Eksport do KML (OsmAnd, Google Earth)"), Group::Route),
            Action::ImportTrack => (i::FOLDER_OPEN, tr("Wczytaj ślad (GPX, KML, FIT)"), Group::Route),
            Action::Undo => (i::ARROW_U_UP_LEFT, tr("Cofnij zmianę trasy"), Group::Route),
            Action::Redo => (i::ARROW_U_UP_RIGHT, tr("Ponów zmianę trasy"), Group::Route),
            Action::SaveRoute => (i::FLOPPY_DISK, tr("Zapisz trasę w bibliotece"), Group::Route),
            Action::ReverseRoute => (i::ARROWS_LEFT_RIGHT, tr("Odwróć kierunek trasy"), Group::Route),
            Action::ClearRoute => (i::TRASH, tr("Zakończ planowanie i usuń trasę"), Group::Route),
            Action::DropLast => (i::BACKSPACE, tr("Usuń ostatni punkt"), Group::Route),
            Action::FitRoute => (i::FRAME_CORNERS, tr("Pokaż całą trasę"), Group::Route),
            Action::TransitToStart => (i::BUS, tr("Dojazd do początku trasy"), Group::Route),
            Action::OpenMapy => (i::ARROW_SQUARE_OUT, tr("Otwórz trasę w Mapy.com"), Group::Route),
            Action::SaveView => (i::BOOKMARK_SIMPLE, tr("Zapisz widok w bibliotece"), Group::Interface),
            Action::Screenshot => (i::CAMERA, tr("Zrzut ekranu"), Group::Interface),
            Action::HighContrast => (i::CIRCLE_HALF_TILT, tr("Wysoki kontrast"), Group::Interface),
            Action::ZoomIn => (i::MAGNIFYING_GLASS_PLUS, tr("Powiększ interfejs"), Group::Interface),
            Action::ZoomOut => (i::MAGNIFYING_GLASS_MINUS, tr("Pomniejsz interfejs"), Group::Interface),
            Action::ZoomReset => (i::TEXT_AA, tr("Domyślny rozmiar interfejsu"), Group::Interface),
            Action::CheckUpdates => (i::ARROW_CIRCLE_UP, tr("Sprawdź aktualizacje"), Group::Interface),
        }
    }

    /// Skrót klawiszowy polecenia.
    pub fn keys(self) -> Option<Keys> {
        use egui::Key as K;
        match self {
            Action::Tool(Tool::Explore) => k(K::Num1),
            Action::Tool(Tool::Route) => k(K::Num2),
            Action::Tool(Tool::Measure) => k(K::Num3),
            Action::Lens(Lens::Band) => k(K::H),
            Action::Lens(Lens::Rel) => k(K::W),
            Action::Lens(Lens::Vis) => k(K::O),
            Action::Lens(Lens::Time) => k(K::I),
            Action::Lens(Lens::None) => None,
            Action::Flight => k(K::F),
            Action::North => k(K::N),
            Action::TopView => k(K::V),
            Action::ResetView => k(K::R),
            Action::Fullscreen => k(K::F11),
            Action::NextStyle => k(K::M),
            Action::ExagUp => k(K::Equals),
            Action::ExagDown => k(K::Minus),
            Action::HourBack => k(K::OpenBracket),
            Action::HourForward => k(K::CloseBracket),
            Action::Layer(l) => l.info().2,
            Action::Panel => k(K::P),
            Action::Library => k(K::B),
            Action::Conditions => k(K::K),
            Action::Search => k(K::Slash),
            Action::Palette => ctrl(K::K),
            Action::Help => k(K::F1),
            Action::Settings => ctrl(K::Comma),
            Action::ExportGpx => k(K::G),
            Action::ImportTrack => ctrl(K::O),
            Action::Undo => ctrl(K::Z),
            Action::Redo => ctrl(K::Y),
            Action::SaveRoute => ctrl(K::S),
            Action::DropLast => k(K::Backspace),
            Action::FitRoute => ctrl(K::E),
            Action::SaveView => ctrl(K::D),
            Action::Screenshot => k(K::S),
            Action::HighContrast => ctrl(K::H),
            Action::ZoomIn => ctrl(K::Equals),
            Action::ZoomOut => ctrl(K::Minus),
            Action::ZoomReset => ctrl(K::Num0),
            _ => None,
        }
    }

    /// Polecenie przypisane do naciśniętego klawisza.
    pub fn for_key(key: egui::Key, m: egui::Modifiers) -> Option<Action> {
        use egui::Key as K;
        let c = m.ctrl || m.command;
        // zapasowe skróty, których nie pokazujemy osobno
        match (key, c, m.shift) {
            (K::Z, true, true) => return Some(Action::Redo),
            (K::Plus, false, _) => return Some(Action::ExagUp),
            (K::Plus, true, _) => return Some(Action::ZoomIn),
            (K::Delete, false, _) => return Some(Action::DropLast),
            (K::P, true, true) => return Some(Action::Palette),
            _ => {}
        }
        Action::all().into_iter().find(|a| a.keys().is_some_and(|b| b.key == key && b.ctrl == c && (b.shift == m.shift || !b.ctrl && m.shift)))
    }

    /// Stan przełącznika (`None` – zwykłe polecenie).
    pub fn checked(self, a: &Atlas) -> Option<bool> {
        let p = &a.prefs;
        Some(match self {
            Action::Tool(t) => a.tool == t,
            Action::Lens(l) => a.lens == l,
            Action::Walk => a.panorama.is_some(),
            Action::Flight => a.flight.is_some(),
            Action::Style(s) => p.style == s,
            Action::Layer(l) => l.on(p),
            Action::Panel => p.panel_open,
            Action::Fullscreen => return None,
            Action::HighContrast => p.high_contrast,
            _ => return None,
        })
    }

    /// Czy polecenie ma teraz sens (pozostałe są wyszarzone w palecie).
    pub fn enabled(self, a: &Atlas) -> bool {
        let route = a.current_route().is_some();
        match self {
            Action::Flight | Action::ExportGpx | Action::ExportFit | Action::ExportKml | Action::FitRoute | Action::SaveRoute => route,
            Action::TransitToStart => !a.waypoints.is_empty(),
            Action::OpenMapy => a.waypoints.len() >= 2,
            Action::Undo => a.can_undo(),
            Action::Redo => a.can_redo(),
            Action::ReverseRoute | Action::ClearRoute => !a.waypoints.is_empty(),
            Action::DropLast => (a.tool == Tool::Route && !a.waypoints.is_empty()) || (a.tool == Tool::Measure && !a.measure.is_empty()),
            Action::ZoomIn => a.prefs.ui_scale < UI_SCALE_MAX - 0.01,
            Action::ZoomOut => a.prefs.ui_scale > UI_SCALE_MIN + 0.01,
            _ => true,
        }
    }
}

pub const UI_SCALE_MIN: f32 = 0.75;
pub const UI_SCALE_MAX: f32 = 2.0;

impl Atlas {
    /// Wykonuje polecenie z rejestru.
    pub fn run(&mut self, act: Action, ctx: &egui::Context) {
        use crate::ui::panels::Tab;
        match act {
            Action::Tool(Tool::Explore) => {
                self.close_tool();
                if self.lens_pin.is_some() {
                    self.unpin_lens();
                }
                self.lens = Lens::None;
                if self.panorama.is_some() {
                    self.exit_panorama();
                }
            }
            Action::Tool(t) => {
                self.toggle_tool(t);
                self.prefs.panel_open = true;
                self.tab = Tab::Info;
            }
            Action::Lens(l) => self.set_lens(l),
            Action::Walk => {
                if self.panorama.is_some() {
                    self.exit_panorama();
                } else {
                    let (lon, lat) = (self.cam.lon, self.cam.lat);
                    self.enter_panorama(lon, lat);
                }
            }
            Action::Flight => self.toggle_flight(),
            Action::North => {
                let mut to = self.cam;
                to.heading = 0.0;
                self.start_fly(to, 0.7);
            }
            Action::TopView => {
                let mut to = self.cam;
                to.pitch = if self.cam.pitch > 75.0 { 40.0 } else { 89.0 };
                self.start_fly(to, 0.7);
            }
            Action::ResetView => {
                let d = Camera::default();
                let mut to = self.cam;
                to.heading = d.heading;
                to.pitch = d.pitch;
                to.distance = d.distance.min(self.cam.distance.max(8000.0));
                self.start_fly(to, 0.9);
            }
            Action::Fullscreen => self.toggle_fullscreen(ctx),
            Action::NextStyle => {
                let all = Style::ALL;
                let i = all.iter().position(|s| *s == self.prefs.style).unwrap_or(0);
                self.prefs.style = all[(i + 1) % all.len()];
            }
            Action::Style(s) => self.prefs.style = s,
            Action::ExagUp => self.prefs.exag = (((self.prefs.exag + 0.1) * 10.0).round() / 10.0).min(3.0),
            Action::ExagDown => self.prefs.exag = (((self.prefs.exag - 0.1) * 10.0).round() / 10.0).max(1.0),
            Action::HourBack => self.shift_hour(-0.5),
            Action::HourForward => self.shift_hour(0.5),
            Action::Now => self.prefs.custom_time = None,
            Action::Layer(l) => {
                let f = l.flag(&mut self.prefs);
                *f = !*f;
            }
            Action::Panel => self.prefs.panel_open = !self.prefs.panel_open,
            Action::Details => self.toggle_tab(Tab::Info),
            Action::Library => self.toggle_tab(Tab::Library),
            Action::Conditions => self.toggle_tab(Tab::Conditions),
            Action::Search => self.search = Some(crate::ui::search::Search::default()),
            Action::Palette => self.palette = Some(Default::default()),
            Action::Help => self.help = !self.help,
            Action::Settings => self.requests.open_preferences = true,
            Action::LoadDetails => self.load_details = !self.load_details,
            Action::ExportGpx => self.export_route(),
            Action::ExportFit => self.export_route_as("fit"),
            Action::ExportKml => self.export_route_as("kml"),
            Action::OpenMapy => self.open_in_mapy(),
            Action::ImportTrack => {
                if let Some(p) = crate::ui::panels::pick_track_file() {
                    self.import_track(&p);
                }
            }
            Action::Undo => self.undo_route(),
            Action::Redo => self.redo_route(),
            Action::SaveRoute => self.save_route_to_library(),
            Action::ReverseRoute => self.reverse_route(),
            Action::ClearRoute => self.end_planning(),
            Action::DropLast => self.tool.behavior().drop_last(self),
            Action::FitRoute => {
                if let Some(r) = self.current_route().cloned() {
                    self.fit_route(&r);
                }
            }
            Action::TransitToStart => {
                if let Some(w) = self.waypoints.first() {
                    crate::transit::open_planner(w.lon, w.lat);
                }
            }
            Action::SaveView => self.save_view(),
            Action::Screenshot => self.screenshot(ctx),
            Action::HighContrast => self.prefs.high_contrast = !self.prefs.high_contrast,
            Action::ZoomIn => self.prefs.ui_scale = ((self.prefs.ui_scale + 0.1) * 10.0).round() / 10.0,
            Action::ZoomOut => self.prefs.ui_scale = ((self.prefs.ui_scale - 0.1) * 10.0).round() / 10.0,
            Action::ZoomReset => self.prefs.ui_scale = 1.0,
            Action::CheckUpdates => self.requests.check_updates = true,
        }
        self.prefs.ui_scale = self.prefs.ui_scale.clamp(UI_SCALE_MIN, UI_SCALE_MAX);
    }

    fn toggle_tab(&mut self, t: crate::ui::panels::Tab) {
        let on = self.prefs.panel_open && self.tab == t;
        self.prefs.panel_open = !on;
        self.tab = t;
    }
}
