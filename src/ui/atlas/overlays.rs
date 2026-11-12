//! Nakładki na widoku: etykiety, znaczniki, kompas, podziałka, pasek stanu, menu i komunikaty.

use super::*;

impl Atlas {
    /// Etykiety, znaczniki, kompas, podziałka, odczyt kursora i menu kontekstowe.
    pub(super) fn overlays(&mut self, ui: &mut egui::Ui, rect: Rect, view: &View) {
        let p_full = ui.painter_at(rect);
        // część mapy niezasłonięta interfejsem: nazwy i znaczniki nie są rysowane pod panelem bocznym,
        // górnym paskiem ani pod profilem i paskiem stanu na dole
        let shown = crate::ui::panels::controls_alpha(self, ui.ctx()) > 0.05;
        let side = if shown { crate::ui::panels::right_inset(self) } else { 0.0 };
        let top = if shown { crate::ui::panels::TOP_BAR_H } else { 0.0 };
        // profil i pasek stanu nie chowają się razem z kontrolkami
        let bottom = crate::ui::panels::bottom_inset(self);
        let visible = Rect::from_min_max(
            Pos2::new(rect.left(), (rect.top() + top).min(rect.bottom())),
            Pos2::new((rect.right() - side).max(rect.left()), (rect.bottom() - bottom).max(rect.top())),
        );
        let p = ui.painter_at(visible);
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
            // zaznaczony punkt najpierw i zawsze z nazwą
            let sel = match self.selection {
                Selection::Poi(i) if i < snap.pois.len() => Some(i),
                _ => None,
            };
            let first = sel.map(|i| (i, &snap.pois[i]));
            // kandydaci w kadrze, osobno dla każdego rodzaju (w kolejności ważności z migawki);
            // bez tego szczyty zajmowały cały przydział, a jeziora i przełęcze nie dochodziły do głosu
            let class = |k: PoiKind| -> usize {
                match k {
                    PoiKind::Place => 0,
                    PoiKind::Peak => 1,
                    PoiKind::Saddle => 2,
                    PoiKind::Lake | PoiKind::Waterfall | PoiKind::Spring | PoiKind::DrinkingWater => 3,
                    PoiKind::Hut | PoiKind::Shelter => 4,
                    PoiKind::Valley | PoiKind::Ridge | PoiKind::Locality => 5,
                    _ => 6,
                }
            };
            let mut lists: Vec<Vec<usize>> = vec![Vec::new(); 7];
            for (i, poi) in snap.pois.iter().enumerate() {
                if Some(i) == sel || !names.shows(&poi.poi) {
                    continue;
                }
                let pos = self.local(poi.poi.lon, poi.poi.lat, poi.dem);
                let Some(s) = view.project(pos) else { continue };
                if !visible.shrink(4.0).contains(at(s)) {
                    continue;
                }
                let is_place = poi.poi.kind == PoiKind::Place;
                let min_prom = (pos.distance(view.eye) / 9000.0) as f32 * if is_place { 0.35 } else { 1.0 };
                if poi.prom < min_prom.min(30.0) * 0.6 {
                    continue;
                }
                lists[class(poi.poi.kind)].push(i);
            }
            // przeplatanie: w każdej kolejce miejscowość, dwa szczyty i po jednym punkcie innych rodzajów
            let mut order: Vec<usize> = Vec::with_capacity(lists.iter().map(Vec::len).sum());
            let mut next = [0usize; 7];
            loop {
                let mut any = false;
                for (c, list) in lists.iter().enumerate() {
                    let take = if c == 1 { 2 } else { 1 };
                    for _ in 0..take {
                        if let Some(&i) = list.get(next[c]) {
                            order.push(i);
                            next[c] += 1;
                            any = true;
                        }
                    }
                }
                if !any {
                    break;
                }
            }
            let mut hits: Vec<(Rect, usize)> = Vec::new();
            for (idx, poi) in first.into_iter().chain(order.into_iter().map(|i| (i, &snap.pois[i]))) {
                let is_sel = Some(idx) == sel;
                let is_place = poi.poi.kind == PoiKind::Place;
                if !is_sel && (shown >= max || (is_place && shown_places >= max_places)) {
                    if is_place {
                        continue;
                    }
                    break;
                }
                let pos = self.local(poi.poi.lon, poi.poi.lat, poi.dem);
                let Some(s) = view.project(pos) else { continue };
                let sp = at(s);
                if !visible.shrink(4.0).contains(sp) {
                    continue;
                }
                let dist = pos.distance(view.eye);
                // drobne punkty tylko z bliska
                let min_prom = (dist / 9000.0) as f32 * if is_place { 0.35 } else { 1.0 };
                if !is_sel && poi.prom < min_prom.min(30.0) * 0.6 {
                    continue;
                }
                if !is_sel && self.occluded(view, pos) {
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
                    PoiKind::Station => (poi.poi.name.clone(), Color32::from_rgb(176, 206, 255), 11.5),
                    PoiKind::BusStop => (poi.poi.name.clone(), Color32::from_rgb(176, 206, 255), 10.5),
                    _ => (poi.poi.name.clone(), theme::paper_2(), 11.0),
                };
                let (color, size) = if is_sel { (theme::BRASS, size + 1.5) } else { (color, size) };
                let font = FontId::proportional(size);
                let galley = p.layout_no_wrap(label, font, color);
                let lr = Rect::from_min_size(sp + Vec2::new(7.0, -galley.size().y - 3.0), galley.size()).expand(2.0);
                // w panoramie pod nazwą jest jeszcze odległość – miejsce zajmuje też ona
                let busy = if self.panorama.is_some() { Rect::from_min_max(lr.min, lr.max + Vec2::new(0.0, 13.0)) } else { lr };
                // napis, który wchodziłby pod panel boczny, górny pasek albo profil, nie jest pokazywany wcale
                // (zamiast uciętego)
                if (side > 0.0 && busy.right() > visible.right() - 2.0) || (top > 0.0 && busy.top() < visible.top() + 2.0) || busy.bottom() > visible.bottom() - 2.0 {
                    continue;
                }
                if !is_sel && taken.iter().any(|t| t.intersects(busy)) {
                    continue;
                }
                taken.push(busy);
                // klikalny obszar: napis i kropka
                hits.push((busy.union(Rect::from_center_size(sp, Vec2::splat(16.0))), idx));
                shown += 1;
                shown_places += is_place as usize;
                let dot = match poi.poi.kind {
                    PoiKind::Peak => theme::PAPER,
                    PoiKind::Hut | PoiKind::Shelter => Color32::from_rgb(255, 170, 70),
                    PoiKind::Lake | PoiKind::Waterfall | PoiKind::Spring => Color32::from_rgb(110, 180, 230),
                    PoiKind::Locality => Color32::from_rgb(150, 205, 110),
                    PoiKind::Valley | PoiKind::Ridge => Color32::from_rgb(170, 180, 150),
                    PoiKind::Place => Color32::from_rgb(200, 190, 170),
                    PoiKind::Station | PoiKind::BusStop => Color32::from_rgb(110, 160, 255),
                    _ => theme::paper_2(),
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
                    let g = p.layout_no_wrap(txt, FontId::proportional(11.0), theme::paper_2());
                    let dp = lr.left_bottom() + Vec2::new(2.0, -2.0);
                    for o in [Vec2::new(1.0, 1.0), Vec2::new(-1.0, 1.0), Vec2::new(1.0, -1.0), Vec2::new(-1.0, -1.0)] {
                        p.galley_with_override_text_color(dp + o, g.clone(), Color32::from_black_alpha(170));
                    }
                    p.galley(dp, g, theme::paper_2());
                }
            }
            self.label_hits = hits;
        } else {
            self.label_hits.clear();
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
                    let c = at(s);
                    p.line_segment([c, c - Vec2::new(0.0, 22.0)], Stroke::new(2.0, theme::TEAL));
                    p.circle_stroke(c, 8.0, Stroke::new(4.0, Color32::from_black_alpha(160)));
                    p.circle_stroke(c, 8.0, Stroke::new(2.0, theme::TEAL));
                    p.circle_filled(c - Vec2::new(0.0, 22.0), 3.5, theme::TEAL);
                }
            }
            Selection::Poi(i) => {
                if let Some(poi) = self.snap.pois.get(*i) {
                    if let Some(s) = view.project(self.local(poi.poi.lon, poi.poi.lat, poi.dem)) {
                        // wyraźny znacznik: ciemna obwódka, jasny pierścień i pionowa szpilka
                        let c = at(s);
                        p.line_segment([c, c - Vec2::new(0.0, 26.0)], Stroke::new(2.0, theme::BRASS));
                        p.circle_stroke(c, 11.0, Stroke::new(4.0, Color32::from_black_alpha(160)));
                        p.circle_stroke(c, 11.0, Stroke::new(2.2, theme::BRASS));
                        p.circle_filled(c - Vec2::new(0.0, 26.0), 4.0, theme::BRASS);
                    }
                }
            }
            _ => {}
        }
        // kursor profilu na trasie
        if let (Some(d), Some(r)) = (self.profile_cursor.or(self.panel_cursor), self.current_route()) {
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

        // reszta nakładek (kompas, podziałka, pasek stanu) – na całej szerokości
        let p = p_full;
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
            Stroke::new(1.0, if hot { theme::paper_3() } else { theme::INK_4 }),
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
                _ if i % 2 == 0 => (5.0, 1.2, theme::paper_3()),
                _ => (3.0, 1.0, theme::INK_4),
            };
            if len > 0.0 {
                pc.line_segment([c + d * (r - 2.0), c + d * (r - 2.0 - len)], Stroke::new(w, col));
            }
        }
        for (deg, s) in [(90.0, "E"), (180.0, "S"), (270.0, "W")] {
            pc.text(c + rot(deg) * (r - 9.0), Align2::CENTER_CENTER, s, FontId::proportional(9.5), theme::paper_3());
        }
        pc.text(c + rot(0.0) * (r - 9.0), Align2::CENTER_CENTER, "N", FontId::proportional(11.0), theme::SIGNAL);
        // igła: czerwona połowa na północ, jasna na południe, pochylona razem z kamerą
        let n = rot(0.0);
        let side = Vec2::new(-n.y, n.x) * 3.5;
        let tip = 15.0 * (0.45 + 0.55 * (self.cam.pitch as f32).to_radians().sin().abs()).min(1.0);
        pc.add(egui::Shape::convex_polygon(vec![c + n * tip, c + side, c - side], theme::SIGNAL, Stroke::NONE));
        pc.add(egui::Shape::convex_polygon(vec![c - n * tip, c - side, c + side], theme::paper_3(), Stroke::NONE));
        pc.circle(c, 2.5, Color32::from_rgb(26, 29, 35), Stroke::new(1.0, theme::paper_3()));
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
        p.rect_filled(bar, 0.0, theme::glass_fill());
        // cały pasek jednym pismem i jedną barwą: odczyt kursora, stan wczytywania, źródła danych
        let bar_font = FontId::proportional(11.5);
        let bar_color = theme::paper_2();
        // źródła danych przy prawej krawędzi; pozostałe napisy nie wchodzą na siebie
        let attr_g = p.layout_no_wrap(attr_text(self.prefs.style).to_string(), bar_font.clone(), bar_color);
        let attr_left = bar.right() - 12.0 - attr_g.size().x;
        p.galley(Pos2::new(attr_left, bar.center().y - attr_g.size().y / 2.0), attr_g, bar_color);
        let info_g = p.layout_no_wrap(info, bar_font.clone(), bar_color);
        let info_right = bar.left() + 12.0 + info_g.size().x;
        p.with_clip_rect(Rect::from_min_max(bar.min, Pos2::new((attr_left - 16.0).max(bar.left()), bar.bottom())))
            .galley(Pos2::new(bar.left() + 12.0, bar.center().y - info_g.size().y / 2.0), info_g, bar_color);

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
        if self.world.roads_busy() {
            chips.push(crate::i18n::tr("pobieranie dróg i ulic…").into());
        }
        if self.land_busy {
            chips.push(crate::i18n::tr("wypiekanie pokrycia terenu i poziomów wody…").into());
        }
        if self.shadow_busy {
            chips.push(if self.prefs.style == Style::Insolation { crate::i18n::tr("liczenie nasłonecznienia…") } else { crate::i18n::tr("liczenie cieni…") }.into());
        }
        if self.iso_busy {
            chips.push(crate::i18n::tr("liczenie czasu dojścia…").into());
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
            Lens::Time => Some(crate::i18n::tr("soczewka: czas dojścia")),
            Lens::None => None,
        };
        if let Some(t) = lens_txt {
            chips.push(format!("{t}{}", if self.lens_pin.is_some() { " (przypięta · Esc)" } else { " · klik przypina" }));
        }
        // statusy na środku dolnego paska (nad odczytem kursora i atrybucją)
        if !chips.is_empty() {
            // zwykły tekst na pasku, bez tła i obwódki; klik otwiera szczegóły wczytywania
            let g = p.layout_no_wrap(chips.join("  ·  "), bar_font, bar_color);
            // na środku paska, a gdy brakuje miejsca – między odczytem kursora a źródłami danych
            let (lo, hi) = (info_right + 16.0, attr_left - 16.0);
            let left = (bar.center().x - g.size().x / 2.0).min(hi - g.size().x).max(lo);
            let r = Rect::from_min_size(Pos2::new(left, bar.center().y - g.size().y / 2.0), g.size());
            let resp = ui.interact(r.expand2(Vec2::new(4.0, 2.0)), ui.id().with("stan-wczytywania"), Sense::click());
            let color = if resp.hovered() { theme::PAPER } else { bar_color };
            p.with_clip_rect(Rect::from_min_max(Pos2::new(lo.min(hi), bar.top()), Pos2::new(hi.max(lo), bar.bottom()))).galley_with_override_text_color(r.min, g, color);
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
                    ui.label(egui::RichText::new(format!("{}  ·  {}", fmt::coords(lon, lat), fmt::ele(e as f64))).small().monospace().color(theme::paper_3()));
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
                    if item(ui, crate::i18n::tr("Czas dojścia stąd")) {
                        self.pin_lens(lon, lat, Lens::Time);
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
                        ui.ctx().copy_text(fmt::coords(lon, lat));
                        self.notify(crate::i18n::tr("Skopiowano współrzędne."));
                        close = true;
                    }
                    // kopiowanie w innym zapisie
                    let mut copy_as = None;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.label(egui::RichText::new(crate::i18n::tr("jako")).small().color(theme::paper_3()));
                        for (f, short) in [
                            (crate::coords::CoordFormat::Dms, "°′″"),
                            (crate::coords::CoordFormat::Decimal, "0.00°"),
                            (crate::coords::CoordFormat::Utm, "UTM"),
                            (crate::coords::CoordFormat::Mgrs, "MGRS"),
                        ] {
                            if ui.small_button(short).on_hover_text(crate::coords::format_as(f, lon, lat)).clicked() {
                                copy_as = Some(f);
                            }
                        }
                    });
                    if let Some(f) = copy_as {
                        ui.ctx().copy_text(crate::coords::format_as(f, lon, lat));
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
    pub(super) fn occluded(&self, view: &View, p: DVec3) -> bool {
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

    pub(super) fn occluded_now(&self, view: &View, p: DVec3) -> bool {
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

/// Źródła danych widocznego stylu mapy (prawa strona paska stanu).
fn attr_text(style: Style) -> &'static str {
    match style {
        Style::Satellite => crate::i18n::tr("Zdjęcia © Esri, Maxar, Earthstar Geographics · teren © Mapterhorn · © OpenStreetMap"),
        _ => crate::i18n::tr("Teren © Mapterhorn (ÚGKK SR, GUGiK, ČÚZK), AWS Open Data · © OpenStreetMap"),
    }
}

pub(super) fn short(s: &str) -> String {
    let mut out: String = s.chars().take(60).collect();
    if s.chars().count() > 60 {
        out.push('…');
    }
    out
}
