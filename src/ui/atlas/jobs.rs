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

    pub(super) fn maybe_shadows(&mut self) {
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
