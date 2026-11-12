//! Analizy liczone w tle: widoczność, linia wzroku, cienie, pokrycie terenu, drzewa, soczewki.

use super::*;

impl Atlas {
    pub(super) fn grid_around(heights: &Heights, lon: f64, lat: f64, radius_m: f64, max_px: f64) -> Grid {
        let kx = 111_320.0 * lat.to_radians().cos();
        let bb = BBox::new(lon - radius_m / kx, lat - radius_m / 110_574.0, lon + radius_m / kx, lat + radius_m / 110_574.0);
        let mut z = 13u8;
        while z > 8 && 2.0 * radius_m / crate::geo::meters_per_pixel(lat, z) > max_px {
            z -= 1;
        }
        heights.grid(&bb, z)
    }

    pub(super) fn world_rect(g: &Grid) -> [f64; 4] {
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
    pub(super) fn start_live_viewshed(&mut self, lon: f64, lat: f64) {
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

    /// Czas dojścia pieszo z punktu: Dijkstra po siatce wysokości z funkcją Toblera; szlaki i drogi
    /// z pełnym tempem, poza nimi wolniej, urwiska nieprzejezdne. Wynik w teksturze widoczności.
    pub fn start_isochrones(&mut self, lon: f64, lat: f64) {
        self.gen += 1;
        let gen = self.gen;
        self.iso_busy = true;
        self.iso = None;
        let (heights, tx, snap, roads) = (self.heights.clone(), self.tx.clone(), self.snap.clone(), self.world.roads());
        let hours = self.prefs.iso_hours.clamp(0.5, 8.0);
        let pace = self.prefs.pace as f32;
        thread::spawn(move || {
            // zasięg: po szlaku najwyżej ~5,5 km/h
            let radius = (hours as f64 * 5_500.0 / pace.max(0.5) as f64).clamp(3_000.0, 48_000.0);
            let g = Self::grid_around(&heights, lon, lat, radius, 1100.0);
            let on = road_mask(&g, &roads);
            let (sx, sy) = g.to_grid(lon, lat);
            let max_s = hours * 3600.0;
            // szlaki: czas jak w planowaniu trasy (PTTK/DIN z tempem) – zgodny z drogowskazami
            let seeds = trail_seeds(&g, &snap.graph, (lon, lat), (sx, sy), pace, max_s as f64);
            let t = analysis::travel_time(&g, sx, sy, &seeds, &on, 40.0, pace, max_s);
            let mask: Vec<u8> = t.iter().map(|&s| if s.is_finite() { ((s / max_s).min(1.0) * 254.0).round() as u8 } else { 255 }).collect();
            let reach = t.iter().filter(|s| s.is_finite()).count();
            let mut pois: Vec<(usize, f32)> = snap
                .pois
                .iter()
                .enumerate()
                .filter(|(_, p)| matches!(p.poi.kind, PoiKind::Peak | PoiKind::Hut | PoiKind::Shelter | PoiKind::Saddle | PoiKind::Lake))
                .filter_map(|(i, p)| {
                    let (px, py) = g.to_grid(p.poi.lon, p.poi.lat);
                    if !g.inside(px, py) {
                        return None;
                    }
                    let s = t[py.round() as usize * g.w + px.round() as usize];
                    (s.is_finite() && haversine(lon, lat, p.poi.lon, p.poi.lat) > 50.0).then_some((i, s))
                })
                .collect();
            pois.sort_by(|a, b| a.1.total_cmp(&b.1));
            let info = IsoInfo { hours, area_km2: reach as f64 * (g.mpp as f64).powi(2) / 1e6, pois };
            let rect = Self::world_rect(&g);
            let _ = tx.send(JobResult::Iso(gen, Box::new((info, mask, g.w as u32, g.h as u32, rect))));
        });
    }

    pub(super) fn start_rel_stats(&mut self, lon: f64, lat: f64, e: f32) {
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

    pub(super) fn start_los(&mut self) {
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

    /// Mapa cieni chwili – albo w stylu nasłonecznienia godziny słońca w ciągu wybranego dnia
    /// (ta sama tekstura; `shadow_insol` mówi, co w niej jest).
    pub(super) fn maybe_shadows(&mut self) {
        let insol = self.prefs.style == Style::Insolation;
        if !(self.prefs.shadows || insol) || self.shadow_busy || self.panorama.is_some() && self.shadow_rect.is_some() && self.shadow_insol == insol {
            return;
        }
        let (az, alt) = self.sun();
        let (day, _) = self.when();
        let radius = (self.cam.distance * 1.6).clamp(6_000.0, 45_000.0);
        // klucz: położenie (co ~1/4 promienia), promień (skokowo) i Słońce (co ~0,5°);
        // nasłonecznienie – dzień zamiast chwili (ujemny, żeby nie pomylić z cieniem)
        let q = radius / 4.0;
        let (x, y) = (lon_to_x(self.cam.lon) * world_m(self.cam.lat), lat_to_y(self.cam.lat) * world_m(self.cam.lat));
        let when = if insol { -(day.y as i64 * 1000 + day.ordinal() as i64) - 1 } else { ((az * 2.0) as i64) * 1000 + (alt * 2.0) as i64 };
        let key = ((x / q) as i64, (y / q) as i64, (radius.log2() * 2.0) as i64, when);
        if self.shadow_key == Some(key) {
            return;
        }
        self.shadow_key = Some(key);
        self.shadow_busy = true;
        let sunv = sun::vector(az, alt);
        let (heights, tx, lon, lat) = (self.heights.clone(), self.tx.clone(), self.cam.lon, self.cam.lat);
        thread::spawn(move || {
            let (m, g) = if insol {
                // co pół godziny przez całą dobę: chwile ze Słońcem nad horyzontem (rzadsza siatka – to ~30 map cieni)
                let suns: Vec<([f32; 3], f32)> = (0..48)
                    .filter_map(|i| {
                        let hour = (i as f64 + 0.5) * 0.5;
                        let (az, alt) = sun::position(crate::tz::local_unix(day, hour, lon, lat), lat, lon);
                        (alt > 0.0).then(|| (sun::vector(az, alt), 0.5))
                    })
                    .collect();
                let g = Self::grid_around(&heights, lon, lat, radius, 900.0);
                (analysis::insolation(&g, &suns), g)
            } else {
                let g = Self::grid_around(&heights, lon, lat, radius, 1400.0);
                (analysis::shadows(&g, sunv), g)
            };
            let rect = Self::world_rect(&g);
            let _ = tx.send(JobResult::Shadow(insol, Box::new((m, g.w as u32, g.h as u32, rect))));
        });
    }

    /// Mozaika pokrycia terenu 3×3 sektory wokół kamery (po zmianie sektora albo nowych maskach).
    pub(super) fn maybe_landcover(&mut self) {
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

    /// Drogi, ulice i tory wokół celu kamery jako linie na terenie – składane w tle, bo w mieście to
    /// setki tysięcy punktów. Z daleka zostają tylko drogi główne, z bliska dochodzą ulice i dojazdy.
    pub(super) fn maybe_roads(&mut self, ppp: f32) {
        self.world.set_roads(self.prefs.roads);
        if !self.prefs.roads {
            if !self.road_lines.is_empty() || self.roads_key.is_some() {
                self.road_lines.clear();
                self.roads_key = None;
                self.roads_ver += 1;
            }
            return;
        }
        if self.roads_job {
            return;
        }
        let d = if self.panorama.is_some() { 600.0 } else { self.cam.distance };
        let radius = (d * 2.4).clamp(3500.0, 60_000.0);
        let (tx, tz) = self.frame.to_local(self.cam.lon, self.cam.lat);
        let cell = radius / 4.0;
        let paper = self.prefs.style.paper();
        let key: RoadsKey = (
            (tx / cell).round() as i64,
            (tz / cell).round() as i64,
            (d.max(1.0).log2() * 2.0).round() as i64,
            self.world.roads_version(),
            self.frame_version,
            paper,
            ppp.to_bits(),
        );
        if self.roads_key == Some(key) {
            return;
        }
        self.roads_key = Some(key);
        self.roads_job = true;
        let (tx_job, heights, frame, sectors) = (self.tx.clone(), self.heights.clone(), self.frame, self.world.roads());
        let (clon, clat) = (self.cam.lon, self.cam.lat);
        thread::spawn(move || {
            let lines = road_lines(&sectors, &heights, &frame, (clon, clat), d, radius, paper, ppp);
            let _ = tx_job.send(JobResult::Roads(key, lines));
        });
    }

    /// Czy rysować drzewa: pokrycie terenu włączone, styl realistyczny, kamera dość blisko.
    pub(super) fn trees_on(&self) -> bool {
        self.prefs.landcover
            && matches!(self.prefs.style, Style::Terrain | Style::Satellite)
            && (self.cam.distance < 14_000.0 || self.panorama.is_some())
    }

    /// Rozmieszczenie drzew z maski lasu wokół celu kamery, w tle. Bliskie (do ~650 m od oka)
    /// dostają szczegółowe bryły, dalsze proste piramidki; dalej niż kilka km las jest tylko na teksturze.
    pub(super) fn maybe_trees(&mut self, view: &View) {
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
                    // zarośla rosną kępami (~60 m): w kępie gęsto, poza nią prawie nic – bez pojedynczych kropek
                    let keep = if forest > 0.4 {
                        roll < 0.85 * forest
                    } else if scrub {
                        roll < if hash(gx.div_euclid(4), gz.div_euclid(4), 9) < 0.55 { 0.9 } else { 0.04 }
                    } else {
                        false
                    };
                    if !keep {
                        continue;
                    }
                    let Some(e) = heights.sample_cached(lon, lat) else { continue };
                    let seed = hash(gx, gz, 4) as f32;
                    let size = 0.75 + 0.5 * hash(gx, gz, 5) as f32;
                    // gatunek: rodzaj lasu z OSM, a w nim dobór według wysokości i szerokości geograficznej;
                    // drzewa jednego gatunku rosną kępami (losowanie w dużej mierze wspólne dla ~120 m)
                    let patch = hash(gx.div_euclid(8), gz.div_euclid(8), 6);
                    let pick = 0.65 * patch + 0.35 * hash(gx, gz, 7);
                    let species = crate::analysis::tree_species(crate::landcover::forest_kind(data[i + 1]), scrub, e as f64, lat, pick, hash(gx, gz, 8));
                    let (h, r) = species.size();
                    let (kind, h, r) = (species as u8 as f32, h * size, r * size);
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

    pub(super) fn receive_jobs(&mut self) {
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
                JobResult::Shadow(insol, b) => {
                    self.shadow_busy = false;
                    if insol != (self.prefs.style == Style::Insolation) {
                        // styl zmienił się w trakcie liczenia: policz od nowa właściwy rodzaj
                        self.shadow_key = None;
                    } else {
                        let (mask, w, h, rect) = *b;
                        if let Some(r) = &mut self.renderer {
                            r.set_shadow(Some((&mask, w, h)));
                        }
                        self.shadow_rect = Some(rect);
                        self.shadow_insol = insol;
                    }
                }
                JobResult::Iso(g, b) if g == self.gen => {
                    let (info, mask, w, h, rect) = *b;
                    self.iso_busy = false;
                    if let Some(r) = &mut self.renderer {
                        r.set_viewshed(Some((&mask, w, h)));
                    }
                    self.vs_rect = Some(rect);
                    self.iso = Some(info);
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
                JobResult::Roads(key, lines) => {
                    self.roads_job = false;
                    if self.roads_key == Some(key) {
                        self.road_lines = lines;
                        self.roads_ver += 1;
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
        // jawny wybór soczewki: odpięcie nie przywraca już wcześniejszego trybu
        self.lens_back = None;
        self.unpin_lens();
        self.lens = if self.lens == l { Lens::None } else { l };
    }

    pub fn pin_lens(&mut self, lon: f64, lat: f64, mode: Lens) {
        let e = self.heights.sample_cached(lon, lat).unwrap_or(0.0);
        // przypięcie z panelu albo menu (bez włączonej tej soczewki): po odpięciu wraca wcześniejszy tryb
        if self.lens != mode && self.lens_back.is_none() {
            self.lens_back = Some(self.lens);
        }
        self.lens = mode;
        self.lens_pin = Some((lon, lat, e));
        self.rel = None;
        match mode {
            Lens::Vis => self.start_viewshed(lon, lat),
            Lens::Rel => self.start_rel_stats(lon, lat, e),
            Lens::Time => self.start_isochrones(lon, lat),
            _ => {}
        }
        self.prefs.panel_open = true;
        self.tab = crate::ui::panels::Tab::Info;
    }

    pub fn unpin_lens(&mut self) {
        self.gen += 1;
        self.lens_pin = None;
        if let Some(l) = self.lens_back.take() {
            self.lens = l;
        }
        self.vs = None;
        self.vs_busy = false;
        self.iso = None;
        self.iso_busy = false;
        self.rel = None;
        self.vs_rect = None;
        self.live_rect = None;
        self.live_vs = None;
        if let Some(r) = &mut self.renderer {
            r.set_viewshed(None);
        }
    }

    // ------------------------------------------------------------------ wejście
}

/// Komórki odcinka `a`–`b` (współrzędne siatki) z wartością liniowo od `ta` do `tb`; `f` dostaje indeks i wartość.
fn raster(g: &Grid, a: (f64, f64), b: (f64, f64), ta: f64, tb: f64, mut f: impl FnMut(usize, f64)) {
    let (w, h) = (g.w as f64, g.h as f64);
    if a.0.max(b.0) < -1.0 || a.1.max(b.1) < -1.0 || a.0.min(b.0) > w || a.1.min(b.1) > h {
        return;
    }
    let n = ((b.0 - a.0).abs().max((b.1 - a.1).abs()) * 2.0).ceil().clamp(1.0, 100_000.0) as usize;
    for i in 0..=n {
        let k = i as f64 / n as f64;
        let (x, y) = ((a.0 + (b.0 - a.0) * k).round(), (a.1 + (b.1 - a.1) * k).round());
        if x >= 0.0 && y >= 0.0 && x < w && y < h {
            f(y as usize * g.w + x as usize, ta + (tb - ta) * k);
        }
    }
}

/// Czasy przejścia krawędzi (s) od jej początku do każdego punktu (`fwd`) i od każdego punktu
/// z powrotem do początku (`back`) – model czasu z planowania trasy (PTTK/DIN, tempo, trudność SAC).
/// Całość jak w planowaniu (podejścia z histerezą), po drodze proporcjonalnie do odcinków.
fn edge_times(ed: &crate::graph::Edge) -> (Vec<f64>, Vec<f64>) {
    let n = ed.n().min(ed.lat.len()).min(ed.e.len());
    let mut fwd = vec![0.0; n];
    let mut back = vec![0.0; n];
    for i in 1..n {
        let d = haversine(ed.lon[i - 1], ed.lat[i - 1], ed.lon[i], ed.lat[i]);
        let dh = (ed.e[i] - ed.e[i - 1]) as f64;
        fwd[i] = fwd[i - 1] + metrics::walk_time(d, dh.max(0.0), (-dh).max(0.0));
        back[i] = back[i - 1] + metrics::walk_time(d, (-dh).max(0.0), dh.max(0.0));
    }
    let sac = metrics::sac_factor(ed.sac);
    let tf = metrics::walk_time(ed.len2, ed.up, ed.down) * sac * 3600.0;
    let tb = metrics::walk_time(ed.len2, ed.down, ed.up) * sac * 3600.0;
    if n > 1 {
        let (sf, sb) = (fwd[n - 1], back[n - 1]);
        for v in &mut fwd {
            *v = if sf > 0.0 { *v / sf * tf } else { 0.0 };
        }
        for v in &mut back {
            *v = if sb > 0.0 { *v / sb * tb } else { 0.0 };
        }
    }
    (fwd, back)
}

/// Wartość w ułamkowym indeksie punktu krawędzi.
fn at_p(v: &[f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let i = (p.floor() as usize).min(v.len() - 1);
    let j = (i + 1).min(v.len() - 1);
    v[i] + (v[j] - v[i]) * (p - i as f64).clamp(0.0, 1.0)
}

/// Czas dojścia po sieci szlaków od punktu (`lon`, `lat`) do każdej komórki szlaku w siatce (s):
/// dojście do najbliższego szlaku w terenie (Tobler), potem Dijkstra po węzłach z czasami jak
/// w planowaniu trasy. Wynik – czasy startowe dla [`analysis::travel_time`].
fn trail_seeds(g: &Grid, graph: &crate::graph::TrailGraph, (lon, lat): (f64, f64), (sx, sy): (f64, f64), pace: f32, max_s: f64) -> Vec<(usize, f32)> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let Some(s) = graph.snap(lon, lat, 400.0) else { return Vec::new() };
    let Some(se) = graph.edges.get(s.edge as usize) else { return Vec::new() };
    let times: Vec<(Vec<f64>, Vec<f64>)> = graph.edges.iter().map(edge_times).collect();
    // dojście do szlaku poza ścieżką
    let e0 = g.sample(sx, sy);
    let t0 = (analysis::tobler(s.dist as f32, s.e - e0) / 0.6 * pace) as f64;
    let (sf, sb) = &times[s.edge as usize];
    let (fp, bp) = (at_p(sf, s.p), at_p(sb, s.p));
    let mut node = vec![f64::INFINITY; graph.nodes.len()];
    let mut heap = BinaryHeap::new();
    let relax = |node: &mut Vec<f64>, heap: &mut BinaryHeap<(Reverse<u64>, u32)>, v: u32, t: f64| {
        if let Some(c) = node.get_mut(v as usize) {
            if t < *c && t <= max_s {
                *c = t;
                heap.push((Reverse(t.to_bits()), v));
            }
        }
    };
    relax(&mut node, &mut heap, se.a, t0 + bp);
    relax(&mut node, &mut heap, se.b, t0 + sf.last().copied().unwrap_or(0.0) - fp);
    while let Some((Reverse(bits), u)) = heap.pop() {
        let t = f64::from_bits(bits);
        if t > node[u as usize] {
            continue;
        }
        for &eid in &graph.nodes[u as usize].edges {
            let ed = &graph.edges[eid as usize];
            let (f, b) = &times[eid as usize];
            if ed.a == u {
                relax(&mut node, &mut heap, ed.b, t + f.last().copied().unwrap_or(0.0));
            }
            if ed.b == u {
                relax(&mut node, &mut heap, ed.a, t + b.last().copied().unwrap_or(0.0));
            }
        }
    }
    // czasy w punktach krawędzi: z węzła początkowego albo końcowego (i na krawędzi startu – od miejsca startu)
    let mut best: std::collections::HashMap<usize, f32> = std::collections::HashMap::new();
    for (eid, ed) in graph.edges.iter().enumerate() {
        let (f, b) = &times[eid];
        let n = f.len();
        if n < 2 {
            continue;
        }
        let ta = node.get(ed.a as usize).copied().unwrap_or(f64::INFINITY);
        let tb = node.get(ed.b as usize).copied().unwrap_or(f64::INFINITY);
        let at = |i: usize| -> f64 {
            let mut t = (ta + f[i]).min(tb + b[n - 1] - b[i]);
            if eid == s.edge as usize {
                let pi = i as f64;
                t = t.min(if pi >= s.p { t0 + f[i] - fp } else { t0 + bp - b[i] });
            }
            t
        };
        if ta.min(tb) > max_s && eid != s.edge as usize {
            continue;
        }
        let pts: Vec<(f64, f64)> = (0..n).map(|i| g.to_grid(ed.lon[i], ed.lat[i])).collect();
        let ts: Vec<f64> = (0..n).map(at).collect();
        for i in 1..n {
            if ts[i - 1].min(ts[i]) > max_s {
                continue;
            }
            raster(g, pts[i - 1], pts[i], ts[i - 1], ts[i], |c, t| {
                let e = best.entry(c).or_insert(f32::INFINITY);
                if (t as f32) < *e {
                    *e = t as f32;
                }
            });
        }
    }
    best.into_iter().collect()
}

/// Komórki siatki na drogach dostępnych pieszo (bez autostrad, dróg ekspresowych i torów) –
/// tam czas dojścia liczy się pełnym tempem (szlaki mają własne czasy z [`trail_seeds`]).
fn road_mask(g: &Grid, roads: &[Arc<Vec<crate::roads::Road>>]) -> Vec<bool> {
    use crate::roads::Class;
    let mut m = vec![false; g.w * g.h];
    let mut seg = |a: (f64, f64), b: (f64, f64)| raster(g, a, b, 0.0, 0.0, |c, _| m[c] = true);
    for sector in roads {
        for r in sector.iter() {
            if matches!(r.class, Class::Motorway | Class::Trunk | Class::Rail | Class::Tram) {
                continue;
            }
            for p in r.line.windows(2) {
                seg(g.to_grid(p[0].0, p[0].1), g.to_grid(p[1].0, p[1].1));
            }
        }
    }
    m
}

/// Wygląd drogi danego rodzaju: barwa, krycie, szerokość (px) i czy ma ciemną obwódkę.
fn road_style(class: crate::roads::Class, paper: bool) -> ([u8; 3], f32, f32, bool) {
    use crate::roads::Class as C;
    match class {
        C::Motorway => ([232, 146, 70], 1.0, 4.0, true),
        C::Trunk => ([240, 170, 90], 1.0, 3.6, true),
        C::Primary => ([250, 214, 120], 1.0, 3.2, true),
        C::Secondary => ([252, 240, 170], 1.0, 2.6, true),
        C::Tertiary => ([255, 255, 255], 1.0, 2.2, true),
        // na papierowej mapie białe ulice potrzebują obwódki; na zdjęciu wystarczy jasna kreska
        C::Street => ([255, 255, 255], if paper { 1.0 } else { 0.85 }, 1.6, paper),
        C::Service => (if paper { [150, 150, 150] } else { [225, 225, 225] }, 0.75, 1.0, false),
        C::Track => (if paper { [150, 115, 70] } else { [200, 165, 115] }, 0.85, 1.1, false),
        C::Rail => ([70, 70, 78], 1.0, 1.8, false),
        C::Tram => ([170, 100, 190], 0.95, 1.3, false),
    }
}

/// Z jakiego oddalenia kamery (m) widać drogi danego rodzaju.
fn road_range(class: crate::roads::Class) -> f64 {
    use crate::roads::Class as C;
    match class {
        C::Motorway | C::Trunk | C::Primary | C::Rail => 400_000.0,
        C::Secondary => 90_000.0,
        C::Tertiary => 45_000.0,
        C::Street | C::Tram => 16_000.0,
        C::Service | C::Track => 7_000.0,
    }
}

/// Linie dróg w promieniu `radius` od (`clon`, `clat`) przy oddaleniu kamery `d`: najpierw obwódki,
/// potem wypełnienia od najmniej do najbardziej ważnych, więc drogi główne leżą na wierzchu.
#[allow(clippy::too_many_arguments)]
fn road_lines(
    sectors: &[Arc<Vec<crate::roads::Road>>],
    heights: &Heights,
    frame: &Frame,
    (clon, clat): (f64, f64),
    d: f64,
    radius: f64,
    paper: bool,
    ppp: f32,
) -> Vec<LineInst> {
    use crate::roads::Class;
    /// Najwięcej odcinków naraz (obwódki liczone osobno).
    const MAX: usize = 260_000;
    let kx = 111_320.0 * clat.to_radians().cos().max(0.05);
    let (dlon, dlat) = (radius / kx, radius / 110_574.0);
    // punkty gęstsze niż ułamek piksela nic nie wnoszą
    let min_step = (d / 600.0).clamp(3.0, 400.0);
    let scale = ppp.max(1.0);
    let mut casings: Vec<LineInst> = Vec::new();
    let mut fills: Vec<Vec<LineInst>> = vec![Vec::new(); Class::COUNT];
    let mut total = 0usize;
    'all: for roads in sectors {
        for r in roads.iter() {
            if d > road_range(r.class) {
                continue;
            }
            if r.bbox[2] < clon - dlon || r.bbox[0] > clon + dlon || r.bbox[3] < clat - dlat || r.bbox[1] > clat + dlat {
                continue;
            }
            let (rgb, alpha, width, casing) = road_style(r.class, paper);
            let color = lin(rgb, alpha);
            let (w, wc) = (width * scale, (width + 2.2) * scale);
            // kolejne punkty na terenie; przerwa w wysokościach albo wyjście poza promień kończy odcinek
            let mut prev: Option<[f32; 3]> = None;
            let mut last = (f64::MAX, f64::MAX);
            let n = r.line.len();
            for (i, &(lon, lat)) in r.line.iter().enumerate() {
                let (ex, ez) = ((lon - clon) * kx, (lat - clat) * 110_574.0);
                if ex.hypot(ez) > radius {
                    prev = None;
                    continue;
                }
                // przerzedzanie: środkowe punkty bliższe niż `min_step` od poprzedniego rysowanego
                if prev.is_some() && i + 1 < n && ((lon - last.0) * kx).hypot((lat - last.1) * 110_574.0) < min_step {
                    continue;
                }
                let Some(e) = heights.sample_cached(lon, lat) else {
                    prev = None;
                    continue;
                };
                let (x, z) = frame.to_local(lon, lat);
                let p = [x as f32, e + 0.7, z as f32];
                if let Some(q) = prev {
                    if casing {
                        casings.push(LineInst { p0: q, p1: p, color: [0.02, 0.02, 0.025, 0.9], wo: [wc, 0.0] });
                    }
                    fills[r.class as usize].push(LineInst { p0: q, p1: p, color, wo: [w, 0.0] });
                    total += 1;
                }
                prev = Some(p);
                last = (lon, lat);
            }
            if total >= MAX {
                break 'all;
            }
        }
    }
    let mut out = casings;
    // dojazdy i drogi gruntowe pod spodem, autostrady wyżej, tory i tramwaje na samym wierzchu
    for c in [Class::Track, Class::Service, Class::Street, Class::Tertiary, Class::Secondary, Class::Primary, Class::Trunk, Class::Motorway, Class::Rail, Class::Tram] {
        out.append(&mut fills[c as usize]);
    }
    out
}
