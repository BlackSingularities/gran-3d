//! Wejście: klawiatura (skróty przez [`super::actions`]) i mysz nad widokiem 3D.

use super::*;

impl Atlas {
    pub(super) fn keyboard(&mut self, ctx: &egui::Context) {
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

    pub(super) fn handle_mouse(&mut self, ui: &mut egui::Ui, resp: &egui::Response, rect: Rect, view: &View) {
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

    /// Narysowana etykieta pod kursorem (napis albo kropka); najbliższa, gdy kilka się nakłada.
    pub(super) fn label_hit(&self, p: Pos2, _rect: Rect, _view: &View) -> Option<usize> {
        self.label_hits
            .iter()
            .filter(|(r, _)| r.contains(p))
            .min_by(|a, b| (a.0.center() - p).length().total_cmp(&(b.0.center() - p).length()))
            .map(|(_, i)| *i)
    }
}
