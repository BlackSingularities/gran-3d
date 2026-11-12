//! Klatka widoku 3D: dane okolicy, kamera nad terenem, render i zrzut ekranu.

use super::*;

impl Atlas {
    pub(super) fn screenshot(&mut self, ctx: &egui::Context) {
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

    pub(super) fn take_screenshot(&mut self, ctx: &egui::Context) {
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
                    // najwyżej 89°: przy 90° widok traci „górę” i mapa przewraca się do góry nogami
                    self.cam.pitch = self.cam.pitch.max(need.clamp(-1.0, 1.0).asin().to_degrees()).min(89.0);
                }
                // gdy samo nachylenie nie wystarcza (oko nadal pod terenem), cały widok idzie w górę:
                // przez „podłogę” nie da się przejść
                let v = self.cam.view(self.frame, exag, size, ground_eye);
                if v.eye.y < min_y {
                    self.cam.height += (min_y - v.eye.y) / exag.max(0.01);
                }
            }
            // nachylenie zawsze w dozwolonym zakresie, także po obrocie wokół chwyconego punktu i przelotach
            if self.panorama.is_none() && self.cam.pitch.is_finite() {
                self.cam.pitch = self.cam.pitch.clamp(0.5, 89.0);
            }
            if !self.cam.distance.is_finite() || self.cam.distance < self.min_dist() {
                self.cam.distance = self.min_dist();
            }
        }
        let view = self.cam.view(self.frame, exag, size, ground_eye);

        // kursor nad terenem
        let pointer = resp.hover_pos();
        let local_px = |p: Pos2| [p.x - rect.left(), p.y - rect.top()];
        self.hover = pointer.and_then(|p| self.ground(&view, local_px(p)));

        // nad etykietą – kursor wskazujący (kliknięcie zaznacza punkt)
        if pointer.is_some_and(|p| self.label_hit(p, rect, &view).is_some()) && self.tool.behavior().picks_labels() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        self.handle_mouse(ui, &resp, rect, &view);

        // analizy zależne od kursora i Słońca
        self.maybe_shadows();
        self.maybe_landcover();
        self.maybe_trees(&view);
        self.maybe_roads(ctx.pixels_per_point());
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
        // zamknięcia sezonowe zależą od daty – dzień w roku w kluczu
        let (day, _) = self.when();
        if let Some(r) = &mut self.renderer {
            let key = {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                (self.snap.version, self.frame_version, self.prefs.trails, dim, self.prefs.chains, self.prefs.boundaries).hash(&mut h);
                (self.prefs.closures, self.prefs.pistes, day.m, day.d, ppp.to_bits()).hash(&mut h);
                h.finish()
            };
            if r.static_version() != key {
                let mut tl = Vec::new();
                // granice pod szlakami, łańcuchy nad nimi
                if self.prefs.boundaries {
                    tl.extend(boundary_lines(&self.snap, &self.frame, ppp));
                }
                if self.prefs.trails {
                    tl.extend(trail_lines(&self.snap, &self.frame, ppp, dim));
                }
                if self.prefs.pistes {
                    tl.extend(piste_lines(&self.snap, &self.frame, ppp));
                }
                if self.prefs.chains && self.prefs.trails {
                    tl.extend(chain_lines(&self.snap, &self.frame, ppp));
                }
                if self.prefs.closures {
                    tl.extend(closure_lines(&self.snap, &self.frame, ppp, (day.m, day.d)));
                }
                r.set_static_lines(key, &tl);
            }
            if r.road_version() != self.roads_ver {
                r.set_road_lines(self.roads_ver, &self.road_lines);
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
            let pick_at = pointer.map(local_px);
            let (blit, _) = r.render(&view, &scene, &lines, ppp, &glass, pick_at);
            // punkt pod kursorem odczytany z właśnie narysowanej głębi – dokładnie tam, gdzie widać teren
            if let (Some(at), Some(pk)) = (pick_at, r.pick) {
                if pk.at == at && pk.eye == view.eye {
                    self.hover = pk.world.map(|w| {
                        let (lon, lat) = self.frame.to_lonlat(w.x, w.z);
                        (lon, lat, (w.y / view.exag) as f32)
                    });
                }
            }
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
}
