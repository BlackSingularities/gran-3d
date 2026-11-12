//! Ustawienia atlasu zapamiętywane między uruchomieniami: styl, jakość, warstwy, nazwy.

use super::*;

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
    /// Zapis współrzędnych w interfejsie (stopnie, UTM, MGRS).
    pub coord_format: crate::coords::CoordFormat,
    /// Zamknięte szlaki (komunikaty TPN i TANAP, znaczniki z OSM).
    pub closures: bool,
    /// Trasy narciarskie i wyciągi.
    pub pistes: bool,
    /// Drogi, ulice i tory.
    pub roads: bool,
    /// Wysoki kontrast interfejsu.
    pub high_contrast: bool,
    /// Skala interfejsu (0,75–2).
    pub ui_scale: f32,
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
    /// Komunikacja publiczna: przystanki autobusowe i tramwajowe, stacje, dworce, przystanie.
    pub transit: bool,
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
            transit: true,
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
            PoiKind::BusStop | PoiKind::Station => self.transit,
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
            (i::BUS, "Przystanki i stacje", &mut self.transit),
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
            coord_format: crate::coords::CoordFormat::default(),
            closures: true,
            pistes: false,
            roads: true,
            high_contrast: false,
            ui_scale: 1.0,
        }
    }
}
