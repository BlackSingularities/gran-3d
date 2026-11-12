//! Planowanie trasy: punkty, historia zmian, wyznaczanie wariantów, przekrój pomiaru, GPX.

use super::*;

impl Atlas {
    /// Linia do profilu na dole: trasa albo – w narzędziu pomiaru – przekrój terenu.
    pub fn profile_route(&self) -> Option<&Route> {
        self.current_route().or_else(|| if self.tool == Tool::Measure { self.section.as_ref().map(|s| &s.1) } else { None })
    }

    /// Przekrój terenu wzdłuż punktów pomiaru (liczony od nowa po zmianie punktów).
    pub(super) fn update_section(&mut self) {
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

    /// Zamiana kolejności dwóch punktów trasy (przesuwanie punktu wyżej albo niżej na liście).
    pub fn swap_waypoints(&mut self, i: usize, j: usize) {
        if i != j && i < self.waypoints.len() && j < self.waypoints.len() {
            self.waypoints.swap(i, j);
            self.route_dirty = true;
        }
    }

    /// Pętla: trasa kończy się tam, gdzie się zaczyna.
    pub fn close_loop(&mut self) {
        if let Some(w) = self.waypoints.first().cloned() {
            self.waypoints.push(w);
            self.route_dirty = true;
        }
    }

    /// Tam i z powrotem: po dojściu do celu powrót przez te same punkty do startu.
    pub fn there_and_back(&mut self) {
        let n = self.waypoints.len();
        if n >= 2 {
            let back: Vec<Waypoint> = self.waypoints[..n - 1].iter().rev().cloned().collect();
            self.waypoints.extend(back);
            self.route_dirty = true;
        }
    }

    pub fn mark_route_dirty(&mut self) {
        self.route_dirty = true;
    }

    // ------------------------------------------------------------------ historia trasy

    /// Zapamiętuje zmianę punktów trasy (co klatkę; całe przeciąganie punktu to jeden krok).
    pub(super) fn track_history(&mut self) {
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

    pub(super) fn restore_route(&mut self, st: RouteState) {
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

    /// Eksport, gdy trasa będzie gotowa (zapisane punkty wyznaczają się w tle).
    /// Punkty trasy do eksportu (nazwa z mapy albo „Punkt n”).
    fn export_waypoints(&self) -> Vec<crate::gpx::Waypoint> {
        self.waypoints
            .iter()
            .enumerate()
            .map(|(i, w)| crate::gpx::Waypoint {
                name: w.name.clone().unwrap_or_else(|| tf!("Punkt {}", i + 1)),
                lon: w.lon,
                lat: w.lat,
                e: w.e,
            })
            .collect()
    }

    /// Eksport z domyślnym formatem (`gpx`, `fit`, `kml`, `geojson`); okno zapisu pozwala zmienić format.
    pub fn export_route_as(&mut self, ext: &str) {
        let Some(r) = self.current_route().cloned() else {
            self.notify(crate::i18n::tr("Brak trasy do zapisania."));
            return;
        };
        let name = self.route_name();
        let wps = self.export_waypoints();
        let mut dlg = rfd::FileDialog::new().set_title(crate::i18n::tr("Eksportuj trasę")).set_file_name(format!("{}.{ext}", file_stem(&name)));
        // wybrany format pierwszy na liście
        let mut filters: Vec<(&str, &str)> = vec![
            ("gpx", "GPX"),
            ("fit", crate::i18n::tr("FIT – kurs dla zegarka (Garmin, Suunto, Wahoo, Coros)")),
            ("kml", crate::i18n::tr("KML (OsmAnd, Google Earth)")),
            ("geojson", "GeoJSON"),
        ];
        if let Some(i) = filters.iter().position(|(e, _)| *e == ext) {
            let f = filters.remove(i);
            filters.insert(0, f);
        }
        for (e, label) in filters {
            dlg = dlg.add_filter(label, &[e]);
        }
        let Some(p) = dlg.save_file() else { return };
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or(ext).to_ascii_lowercase();
        let bytes: Vec<u8> = match ext.as_str() {
            "kml" => crate::formats::export_kml(&r, &name, &wps).into_bytes(),
            "geojson" | "json" => crate::formats::export_geojson(&r, &name, &wps).into_bytes(),
            "fit" => {
                let mut pts: Vec<crate::fit::CoursePoint> = wps.iter().map(|w| crate::fit::CoursePoint { name: w.name.clone(), lon: w.lon, lat: w.lat, kind: 0 }).collect();
                // najwyższy punkt trasy jako „szczyt”
                let i = r.stats.max_ele_idx.min(r.track.lon.len().saturating_sub(1));
                if !r.track.lon.is_empty() {
                    pts.push(crate::fit::CoursePoint {
                        name: tf!("Najwyżej {}", fmt::ele(r.stats.max_ele)),
                        lon: r.track.lon[i],
                        lat: r.track.lat[i],
                        kind: 1,
                    });
                }
                let start = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
                crate::fit::export(&r, &name, &pts, start)
            }
            _ => crate::gpx::export(&r, &name, &wps).into_bytes(),
        };
        match std::fs::write(&p, bytes) {
            Ok(()) => self.notify(tf!("Zapisano {}", p.display())),
            Err(e) => self.notify(tf!("Nie udało się zapisać: {}", e)),
        }
    }

    /// Trasa otwarta w Mapy.com (planer pieszy).
    pub fn open_in_mapy(&mut self) {
        let pts: Vec<(f64, f64)> = self.waypoints.iter().map(|w| (w.lon, w.lat)).collect();
        match crate::transit::mapy_url(&pts) {
            Some(u) => {
                let _ = open::that(u);
            }
            None => self.notify(crate::i18n::tr("Trasa potrzebuje co najmniej dwóch punktów.")),
        }
    }

    /// Zamknięcia (z OSM) na bieżącej trasie w wybranym dniu.
    pub fn route_closures(&self) -> Vec<crate::osm::Closure> {
        let Some(r) = self.current_route() else { return Vec::new() };
        let (day, _) = self.when();
        self.snap.closures_on(&r.track.lon, &r.track.lat, (day.m, day.d)).into_iter().cloned().collect()
    }

    pub fn export_route_after_routing(&mut self) {
        self.export_pending = true;
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

    pub(super) fn compute_routes(&mut self) {
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

    /// Eksport bieżącej trasy: GPX, KML albo GeoJSON (według wybranego typu pliku).
    pub fn export_route(&mut self) {
        self.export_route_as("gpx");
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
}
