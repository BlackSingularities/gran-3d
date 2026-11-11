//! Narzędzia mapy (eksploracja, trasa, pomiar) za wspólnym interfejsem [`MapTool`] oraz soczewki.
//!
//! Stan narzędzi żyje w [`Atlas`] (punkty trasy, punkty pomiaru…), a implementacje [`MapTool`] są
//! bezstanowe. Nowe narzędzie to: wariant [`Tool`], struktura z `impl MapTool`, wpis w
//! [`Tool::behavior`] i [`Tool::ALL`] – pasek narzędzi, skróty, paleta poleceń, nagłówek panelu,
//! kliknięcia i Esc korzystają z interfejsu i nie wymagają dalszych zmian.

use super::*;

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

impl Lens {
    pub const ALL: [Lens; 3] = [Lens::Band, Lens::Rel, Lens::Vis];

    pub fn icon(self) -> &'static str {
        match self {
            Lens::Band => theme::icon::WAVES,
            Lens::Rel => theme::icon::PLUS_MINUS,
            Lens::Vis => theme::icon::EYE,
            Lens::None => theme::icon::CURSOR,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Lens::Band => crate::i18n::tr("Ta sama wysokość"),
            Lens::Rel => crate::i18n::tr("Wysokość względna"),
            Lens::Vis => crate::i18n::tr("Co widać stąd"),
            Lens::None => crate::i18n::tr("Bez soczewki"),
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Lens::Band => "H",
            Lens::Rel => "W",
            Lens::Vis => "O",
            Lens::None => "",
        }
    }
}

/// Zachowanie narzędzia mapy. Metody dostają cały atlas, bo narzędzia korzystają z jego stanu.
pub trait MapTool: Sync {
    /// Ikona na pasku i w nagłówku panelu.
    fn icon(&self) -> &'static str;
    /// Nazwa w nagłówku panelu (przetłumaczona).
    fn title(&self) -> &'static str;
    /// Podpowiedź przycisku na pasku (przetłumaczona, bez skrótu).
    fn hint(&self) -> &'static str;
    /// Klawisz włączający narzędzie.
    fn key(&self) -> &'static str;
    /// Klik w teren (`lon`, `lat`) przy tym narzędziu.
    fn click(&self, a: &mut Atlas, lon: f64, lat: f64);
    /// Wyłączenie narzędzia (Esc, ponowny klik przycisku, klawisz 1): powrót do eksploracji.
    fn close(&self, a: &mut Atlas) {
        a.tool = Tool::Explore;
    }
    /// Backspace / Delete: usuwa ostatni punkt narzędzia.
    fn drop_last(&self, _a: &mut Atlas) {}
    /// Czy klik w etykietę zaznacza punkt (inaczej klik trafia w teren pod etykietą).
    fn picks_labels(&self) -> bool {
        false
    }
    /// Punkt narzędzia pod kursorem `p` do przeciągania (indeks), o ile narzędzie pozwala przeciągać.
    fn grab(&self, _a: &Atlas, _view: &View, _rect: Rect, _p: Pos2) -> Option<usize> {
        None
    }
    /// Przeciąganie chwyconego punktu do (`lon`, `lat`, `e`).
    fn drag(&self, _a: &mut Atlas, _i: usize, _at: (f64, f64, f32)) {}
    /// Koniec przeciągania.
    fn drop_grabbed(&self, _a: &mut Atlas) {}
    /// Treść zakładki „Szczegóły” przy tym narzędziu; `false` – zwykłe szczegóły zaznaczenia.
    fn panel(&self, _a: &mut Atlas, _ui: &mut egui::Ui) -> bool {
        false
    }
}

impl Tool {
    pub const ALL: [Tool; 3] = [Tool::Explore, Tool::Route, Tool::Measure];

    pub fn behavior(self) -> &'static dyn MapTool {
        match self {
            Tool::Explore => &Explore,
            Tool::Route => &RoutePlanner,
            Tool::Measure => &Measure,
        }
    }
}

impl Atlas {
    /// Włącza narzędzie; ponowne włączenie bieżącego je wyłącza (jak przycisk na pasku).
    pub fn toggle_tool(&mut self, t: Tool) {
        if self.tool == t {
            self.tool.behavior().close(self);
        } else {
            self.tool = t;
        }
    }

    /// Wyłącza bieżące narzędzie (powrót do eksploracji).
    pub fn close_tool(&mut self) {
        self.tool.behavior().close(self);
    }

    fn open_info(&mut self) {
        self.prefs.panel_open = true;
        self.tab = crate::ui::panels::Tab::Info;
    }
}

/// Eksploracja: klik zaznacza szlak, punkt z nazwą albo miejsce.
pub struct Explore;

impl MapTool for Explore {
    fn icon(&self) -> &'static str {
        theme::icon::CURSOR
    }
    fn title(&self) -> &'static str {
        crate::i18n::tr("Szczegóły")
    }
    fn hint(&self) -> &'static str {
        crate::i18n::tr("Zaznaczanie – zwykły tryb")
    }
    fn key(&self) -> &'static str {
        "1"
    }
    fn picks_labels(&self) -> bool {
        true
    }
    fn click(&self, a: &mut Atlas, lon: f64, lat: f64) {
        // szlak w pobliżu kliknięcia (promień zależny od odległości)
        let r = (a.cam.distance * 0.006).clamp(15.0, 400.0);
        a.selection = match a.prefs.trails.then(|| a.snap.graph.snap(lon, lat, r)).flatten() {
            Some(s) => Selection::Trail(s.edge),
            None => Selection::Point { lon, lat },
        };
        a.open_info();
    }
}

/// Planowanie trasy: klik dodaje punkt, punkty można przeciągać.
pub struct RoutePlanner;

impl MapTool for RoutePlanner {
    fn icon(&self) -> &'static str {
        theme::icon::PATH
    }
    fn title(&self) -> &'static str {
        crate::i18n::tr("Trasa")
    }
    fn hint(&self) -> &'static str {
        crate::i18n::tr("Planowanie trasy")
    }
    fn key(&self) -> &'static str {
        "2"
    }
    fn click(&self, a: &mut Atlas, lon: f64, lat: f64) {
        a.add_waypoint(lon, lat, None);
        a.open_info();
    }
    fn close(&self, a: &mut Atlas) {
        a.end_planning();
    }
    fn drop_last(&self, a: &mut Atlas) {
        if let Some(n) = a.waypoints.len().checked_sub(1) {
            a.remove_waypoint(n);
        }
    }
    fn grab(&self, a: &Atlas, view: &View, rect: Rect, p: Pos2) -> Option<usize> {
        let mut hit = None;
        for (i, w) in a.waypoints.iter().enumerate() {
            if let Some(s) = view.project(a.local(w.lon, w.lat, w.e)) {
                if (Pos2::new(s[0] + rect.left(), s[1] + rect.top()) - p).length() < 12.0 {
                    hit = Some(i);
                }
            }
        }
        hit
    }
    fn drag(&self, a: &mut Atlas, i: usize, (lon, lat, e): (f64, f64, f32)) {
        if let Some(w) = a.waypoints.get_mut(i) {
            w.lon = lon;
            w.lat = lat;
            w.e = e;
        }
    }
    fn drop_grabbed(&self, a: &mut Atlas) {
        let names: Vec<Option<String>> = a.waypoints.iter().map(|w| a.name_near(w.lon, w.lat, 120.0)).collect();
        for (w, n) in a.waypoints.iter_mut().zip(names) {
            w.name = n;
        }
        a.mark_route_dirty();
    }
    fn panel(&self, a: &mut Atlas, ui: &mut egui::Ui) -> bool {
        crate::ui::panels::route_info(a, ui);
        true
    }
}

/// Pomiar po terenie, przekrój i linia wzroku.
pub struct Measure;

impl MapTool for Measure {
    fn icon(&self) -> &'static str {
        theme::icon::RULER
    }
    fn title(&self) -> &'static str {
        crate::i18n::tr("Pomiar")
    }
    fn hint(&self) -> &'static str {
        crate::i18n::tr("Pomiar, przekrój terenu i linia wzroku")
    }
    fn key(&self) -> &'static str {
        "3"
    }
    fn click(&self, a: &mut Atlas, lon: f64, lat: f64) {
        let e = a.heights.sample_cached(lon, lat).unwrap_or(0.0);
        a.measure.push((lon, lat, e));
        a.start_los();
    }
    fn drop_last(&self, a: &mut Atlas) {
        if a.measure.pop().is_some() {
            a.start_los();
        }
    }
    fn panel(&self, a: &mut Atlas, ui: &mut egui::Ui) -> bool {
        crate::ui::panels::measure_info(a, ui);
        true
    }
}
