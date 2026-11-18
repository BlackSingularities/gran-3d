//! Rysowanie punktów społeczności, szkicu nowego punktu i obszarów offline nad widokiem 3D.

use super::*;

/// Najwięcej znaczników punktów naraz (najbliższe środkowi widoku).
const MAX_PINS: usize = 160;

impl Atlas {
    /// Prostokąt geograficzny w przybliżeniu widoczny w kadrze (do pobierania punktów).
    pub fn view_bbox(&self) -> (f64, f64, f64, f64) {
        let r = (self.cam.distance * 2.5).clamp(2_000.0, 120_000.0);
        let dlat = r / 110_574.0;
        let dlon = r / (111_320.0 * self.cam.lat.to_radians().cos().max(0.05));
        (self.cam.lon - dlon, self.cam.lat - dlat, self.cam.lon + dlon, self.cam.lat + dlat)
    }

    /// Punkty społeczności, szkic punktu i obszary offline.
    pub(super) fn community_overlays(&mut self, ui: &egui::Ui, p: &egui::Painter, rect: Rect, visible: Rect, view: &View) {
        self.point_hits.clear();
        let at = |s: [f32; 2]| Pos2::new(s[0] + rect.left(), s[1] + rect.top());
        let big = crate::platform::MOBILE;
        // obszary offline (zawsze przy narzędziu obszaru, a szkic – także poza nim)
        if self.tool == Tool::Area {
            for a in self.offline.areas() {
                let c = if a.complete { theme::OK } else { theme::WARN };
                self.geo_rect(p, rect, view, a.bbox, c.gamma_multiply(0.8), 1.5);
            }
        }
        if let Some(d) = &self.area_draft {
            let corners: Vec<(f64, f64)> = [d.a, d.b].into_iter().flatten().collect();
            if let [a, b] = corners[..] {
                let bb = [a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)];
                self.geo_rect(p, rect, view, bb, theme::BRASS, 2.5);
            }
            for (lon, lat) in corners {
                let e = self.heights.sample_cached(lon, lat).unwrap_or(0.0);
                if let Some(s) = view.project(self.local(lon, lat, e)) {
                    p.circle_filled(at(s), if big { 11.0 } else { 7.0 }, theme::BRASS);
                    p.circle_stroke(at(s), if big { 11.0 } else { 7.0 }, Stroke::new(2.0, theme::INK_0));
                }
            }
        }
        // szkic nowego punktu
        if let Some(d) = &self.point_draft {
            if let Some(s) = view.project(self.local(d.lon, d.lat, d.ele)) {
                let c = at(s);
                pin(p, c, d.category.icon(), d.category.color(), true, 0, big);
            }
        }
        if !self.prefs.user_points {
            return;
        }
        let (w, s, e, n) = self.view_bbox();
        self.points.want(ui.ctx(), w, s, e, n);
        let sel = match &self.selection {
            Selection::UserPoint(id) => Some(id.clone()),
            _ => None,
        };
        let mut list: Vec<(f64, crate::points::Point)> = self
            .points
            .all()
            .into_iter()
            .filter(|pt| pt.lon >= w && pt.lon <= e && pt.lat >= s && pt.lat <= n)
            .map(|pt| (haversine(self.cam.lon, self.cam.lat, pt.lon, pt.lat), pt))
            .collect();
        list.sort_by(|a, b| a.0.total_cmp(&b.0));
        list.truncate(MAX_PINS);
        // dalsze najpierw, bliższe i zaznaczony na wierzchu
        list.reverse();
        let mut selected = None;
        for (_, pt) in list {
            let ele = pt.ele.or_else(|| self.heights.sample_cached(pt.lon, pt.lat)).unwrap_or(0.0);
            let pos = self.local(pt.lon, pt.lat, ele);
            let Some(sc) = view.project(pos) else { continue };
            let c = at(sc);
            if !visible.contains(c) || self.occluded(view, pos) {
                continue;
            }
            if sel.as_deref() == Some(pt.id.as_str()) {
                selected = Some((c, pt));
                continue;
            }
            let dim = pt.status != crate::points::Status::Approved;
            pin(p, c, pt.category.icon(), if dim { pt.category.color().gamma_multiply(0.55) } else { pt.category.color() }, false, pt.comments.len(), big);
            let r = if big { 20.0 } else { 14.0 };
            self.point_hits.push((Rect::from_center_size(c - Vec2::new(0.0, r + 6.0), Vec2::splat(r * 2.2)), pt.id.clone()));
        }
        if let Some((c, pt)) = selected {
            pin(p, c, pt.category.icon(), pt.category.color(), true, pt.comments.len(), big);
            let r = if big { 20.0 } else { 14.0 };
            self.point_hits.push((Rect::from_center_size(c - Vec2::new(0.0, r + 6.0), Vec2::splat(r * 2.2)), pt.id.clone()));
            // podpis zaznaczonego punktu
            let g = p.layout_no_wrap(pt.title.clone(), FontId::proportional(13.0), theme::PAPER);
            let tr = Rect::from_center_size(c - Vec2::new(0.0, 2.0 * r + 24.0), g.size() + Vec2::new(14.0, 6.0));
            p.rect_filled(tr, 6.0, theme::INK_1.gamma_multiply(0.92));
            p.galley(tr.min + Vec2::new(7.0, 3.0), g, theme::PAPER);
        }
    }

    /// Prostokąt geograficzny (w, s, e, n) położony na terenie.
    fn geo_rect(&self, p: &egui::Painter, rect: Rect, view: &View, bb: [f64; 4], color: Color32, width: f32) {
        let [w, s, e, n] = bb;
        let mut pts: Vec<(f64, f64)> = Vec::new();
        const K: usize = 24;
        for i in 0..=K {
            pts.push((w + (e - w) * i as f64 / K as f64, n));
        }
        for i in 0..=K {
            pts.push((e, n - (n - s) * i as f64 / K as f64));
        }
        for i in 0..=K {
            pts.push((e - (e - w) * i as f64 / K as f64, s));
        }
        for i in 0..=K {
            pts.push((w, s + (n - s) * i as f64 / K as f64));
        }
        // odcinki widoczne w kadrze (punkt za kamerą przerywa linię)
        let mut segs: Vec<Vec<Pos2>> = vec![Vec::new()];
        for (lon, lat) in pts {
            let el = self.heights.sample_cached(lon, lat).unwrap_or(0.0);
            match view.project(self.local(lon, lat, el + 4.0)) {
                Some(sc) => segs.last_mut().unwrap().push(Pos2::new(sc[0] + rect.left(), sc[1] + rect.top())),
                None => segs.push(Vec::new()),
            }
        }
        for l in segs.into_iter().filter(|l| l.len() > 1) {
            p.add(egui::Shape::line(l.clone(), Stroke::new(width + 2.0, Color32::from_black_alpha(120))));
            p.add(egui::Shape::line(l, Stroke::new(width, color)));
        }
    }

    /// Drążek chodzenia w spacerze (ekran dotykowy): lewy dolny róg widocznej części mapy.
    pub(super) fn walk_stick_ui(&mut self, ui: &mut egui::Ui, visible: Rect) {
        const R: f32 = 56.0;
        let c = Pos2::new(visible.left() + R + 24.0, visible.bottom() - R - 24.0);
        let area = Rect::from_center_size(c, Vec2::splat(R * 2.6));
        let resp = ui.interact(area, ui.id().with("drazek-spaceru"), Sense::drag());
        let p = ui.painter();
        p.circle_filled(c, R, Color32::from_black_alpha(90));
        p.circle_stroke(c, R, Stroke::new(1.5, Color32::from_white_alpha(60)));
        if let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.is_pointer_button_down_on()) {
            let mut v = (pos - c) / R;
            if v.length() > 1.0 {
                v = v.normalized();
            }
            self.walk_stick = v;
            ui.ctx().request_repaint();
        } else {
            self.walk_stick = Vec2::ZERO;
        }
        let knob = c + self.walk_stick * R;
        p.circle_filled(knob, 22.0, theme::BRASS.gamma_multiply(0.85));
        p.text(c - Vec2::new(0.0, R + 14.0), Align2::CENTER_CENTER, crate::i18n::tr("chodzenie"), FontId::proportional(11.0), theme::paper_2());
    }

    /// Punkt społeczności pod kursorem (klik, dotknięcie).
    pub(super) fn point_hit(&self, at: Pos2) -> Option<String> {
        self.point_hits
            .iter()
            .filter(|(r, _)| r.contains(at))
            .min_by(|a, b| (a.0.center() - at).length().total_cmp(&(b.0.center() - at).length()))
            .map(|(_, id)| id.clone())
    }
}

/// Szpilka punktu: koło z ikoną kategorii na nóżce i licznik komentarzy.
fn pin(p: &egui::Painter, foot: Pos2, glyph: &str, color: Color32, selected: bool, comments: usize, big: bool) {
    let r = if big { 15.0 } else { 11.0 } * if selected { 1.2 } else { 1.0 };
    let head = foot - Vec2::new(0.0, r + 8.0);
    p.line_segment([foot, head], Stroke::new(2.0, Color32::from_black_alpha(160)));
    p.circle_filled(foot, 2.5, color);
    p.circle_filled(head, r + 2.0, Color32::from_black_alpha(150));
    p.circle_filled(head, r, color);
    if selected {
        p.circle_stroke(head, r + 3.5, Stroke::new(2.0, theme::PAPER));
    }
    p.text(head, Align2::CENTER_CENTER, glyph, FontId::proportional(r * 1.15), theme::INK_0);
    if comments > 1 {
        let b = head + Vec2::new(r * 0.8, -r * 0.8);
        p.circle_filled(b, 7.5, theme::INK_0);
        p.text(b, Align2::CENTER_CENTER, comments.min(99).to_string(), FontId::proportional(9.5), theme::PAPER);
    }
}
