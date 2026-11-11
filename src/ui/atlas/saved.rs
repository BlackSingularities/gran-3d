//! Biblioteka: zapisywanie i otwieranie tras, miejsc i widoków.

use super::*;

impl Atlas {
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
}
