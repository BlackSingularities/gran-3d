//! Profil wysokości trasy: wypełnienie barwione nachyleniem, pas kolorów szlaków, mijane punkty,
//! warianty w tle i kursor zsynchronizowany z mapą.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Vec2};

use crate::fmt;
use crate::graph::Route;
use crate::metrics::{grade_class, GRADE_RGB};
use crate::ui::theme;

/// Statystyki odcinka trasy między dwoma dystansami (m).
#[derive(Clone, Copy, Debug, Default)]
pub struct SegStats {
    pub dist: f64,
    pub up: f64,
    pub down: f64,
    pub time: f64,
    pub min_e: f32,
    pub max_e: f32,
    /// Średnie i największe nachylenie (%) – to drugie na odcinkach ~80 m.
    pub avg_grade: f64,
    pub max_grade: f64,
}

pub fn segment(route: &Route, d0: f32, d1: f32) -> SegStats {
    let tr = &route.track;
    let total = tr.d2.last().copied().unwrap_or(0.0);
    let (a0, a1) = (d0.clamp(0.0, total), d1.clamp(0.0, total));
    let (d0, d1) = (a0.min(a1), a0.max(a1));
    let at = |d: f32| {
        let (i, f) = tr.at_dist(d);
        let j = (i + 1).min(tr.len() - 1);
        (tr.lerp(i, f).2, tr.t[i] + (tr.t[j] - tr.t[i]) * f)
    };
    let (e0, t0) = at(d0);
    let (e1, t1) = at(d1);
    let mut es = vec![e0];
    es.extend((0..tr.len()).filter(|&i| tr.d2[i] > d0 && tr.d2[i] < d1).map(|i| tr.e[i]));
    es.push(e1);
    let (up, down) = crate::metrics::climb(&es);
    let mut max_grade = 0.0f64;
    let mut d = d0;
    while d < d1 {
        let b = (d + 80.0).min(d1);
        let span = (b - d).max(1.0);
        max_grade = max_grade.max(((at(b).0 - at(d).0) / span * 100.0).abs() as f64);
        d += 40.0;
    }
    let dist = (d1 - d0) as f64;
    SegStats {
        dist,
        up,
        down,
        time: (t1 - t0).max(0.0) as f64,
        min_e: es.iter().cloned().fold(f32::INFINITY, f32::min),
        max_e: es.iter().cloned().fold(f32::NEG_INFINITY, f32::max),
        avg_grade: (up + down) / dist.max(1.0) * 100.0,
        max_grade,
    }
}

/// Barwa trasy porównywanej (profil i mapa).
pub const COMPARE: Color32 = Color32::from_rgb(214, 120, 255);

pub struct Mark {
    pub dist: f32,
    pub label: String,
}

/// Rysuje profil; zwraca dystans pod kursorem (m), jeśli kursor jest nad wykresem.
/// Przeciągnięcie zaznacza odcinek (`sel`), klik bez przeciągania zaznaczenie usuwa.
/// `compare` – druga trasa do porównania, rysowana na tej samej osi.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    route: &Route,
    others: &[&Route],
    marks: &[Mark],
    cursor: Option<f32>,
    obstacle: Option<f32>,
    sel: &mut Option<(f32, f32)>,
    compare: Option<&Route>,
) -> Option<f32> {
    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let p = ui.painter_at(rect);
    let tr = &route.track;
    let n = tr.len();
    if n < 2 {
        return None;
    }
    // oś poziomu: zawsze cała wybrana trasa (warianty przycięte do niej)
    let own_total = tr.d2.last().copied().unwrap_or(1.0).max(1.0);
    // przy porównaniu oś obejmuje dłuższą z dwóch tras
    let total = compare.and_then(|c| c.track.d2.last().copied()).unwrap_or(0.0).max(own_total);
    let mut lo = tr.e.iter().cloned().fold(f32::INFINITY, f32::min);
    let mut hi = tr.e.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let (min_e, max_e) = (lo, hi);
    if let Some(c) = compare {
        for &e in &c.track.e {
            lo = lo.min(e);
            hi = hi.max(e);
        }
    }
    for o in others {
        for (k, &e) in o.track.e.iter().enumerate() {
            if o.track.d2[k] <= total {
                lo = lo.min(e);
                hi = hi.max(e);
            }
        }
    }
    let pad = ((hi - lo) * 0.14).max(20.0);
    lo -= pad * 0.5;
    hi += pad;
    let plot = Rect::from_min_max(rect.min + Vec2::new(46.0, 18.0), rect.max - Vec2::new(12.0, 24.0));
    let x = |d: f32| plot.left() + d / total * plot.width();
    let y = |e: f32| plot.bottom() - (e - lo) / (hi - lo).max(1.0) * plot.height();
    let pp = ui.painter_at(plot.expand(2.0));

    // siatka wysokości i dystansu
    let step = nice_step((hi - lo) as f64 / 4.0) as f32;
    let mut e = (lo / step).ceil() * step;
    while e <= hi {
        let yy = y(e);
        p.line_segment([Pos2::new(plot.left(), yy), Pos2::new(plot.right(), yy)], Stroke::new(1.0, Color32::from_white_alpha(14)));
        p.text(Pos2::new(plot.left() - 6.0, yy), Align2::RIGHT_CENTER, fmt::int(e as f64), FontId::monospace(10.5), theme::PAPER_3);
        e += step;
    }
    let dstep = nice_step(total as f64 / 7.0) as f32;
    let mut d = 0.0;
    while d <= total + 1.0 {
        let xx = x(d);
        p.line_segment([Pos2::new(xx, plot.bottom()), Pos2::new(xx, plot.bottom() + 4.0)], Stroke::new(1.0, theme::PAPER_3));
        p.text(Pos2::new(xx, plot.bottom() + 6.0), Align2::CENTER_TOP, fmt::dist(d as f64), FontId::monospace(10.5), theme::PAPER_3);
        d += dstep;
    }

    // warianty w tle, przerywane
    for o in others {
        let pts: Vec<Pos2> = (0..o.track.len()).step_by(2).filter(|&i| o.track.d2[i] <= total).map(|i| Pos2::new(x(o.track.d2[i]), y(o.track.e[i]))).collect();
        pp.extend(Shape::dashed_line(&pts, Stroke::new(1.2, theme::PAPER_3.gamma_multiply(0.6)), 5.0, 4.0));
    }

    // wypełnienie: nachylenie wygładzone na odcinkach, barwa przechodzi ku dołowi w przezroczystość
    let elev = |d: f32| {
        let (i, f) = tr.at_dist(d.clamp(0.0, own_total));
        tr.lerp(i, f).2
    };
    let seg = (own_total / 160.0).max(40.0);
    let mut mesh = egui::Mesh::default();
    let mut d0 = 0.0f32;
    while d0 < own_total {
        let d1 = (d0 + seg).min(own_total);
        let g = (elev(d1) - elev(d0)) as f64 / (d1 - d0).max(0.1) as f64;
        let c = GRADE_RGB[grade_class(g)];
        let top = Color32::from_rgba_unmultiplied(c[0], c[1], c[2], 190);
        let bottom = Color32::from_rgba_unmultiplied(c[0], c[1], c[2], 28);
        let k = 4;
        for s in 0..k {
            let a = d0 + (d1 - d0) * s as f32 / k as f32;
            let b = d0 + (d1 - d0) * (s + 1) as f32 / k as f32;
            let base = mesh.vertices.len() as u32;
            mesh.colored_vertex(Pos2::new(x(a), y(elev(a))), top);
            mesh.colored_vertex(Pos2::new(x(b), y(elev(b))), top);
            mesh.colored_vertex(Pos2::new(x(b), plot.bottom()), bottom);
            mesh.colored_vertex(Pos2::new(x(a), plot.bottom()), bottom);
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base, base + 2, base + 3);
        }
        d0 = d1;
    }
    pp.add(Shape::mesh(mesh));
    // pas kolorów szlaku pod wykresem
    let mut i = 0;
    while i < n - 1 {
        let mut j = i + 1;
        while j < n - 1 && x(tr.d2[j]) - x(tr.d2[i]) < 2.0 {
            j += 1;
        }
        let tc = tr.color[i].rgb();
        p.rect_filled(
            Rect::from_min_max(Pos2::new(x(tr.d2[i]), plot.bottom() + 1.0), Pos2::new(x(tr.d2[j]) + 0.5, plot.bottom() + 4.0)),
            0.0,
            Color32::from_rgb(tc[0], tc[1], tc[2]),
        );
        i = j;
    }
    // trasa porównywana: fioletowa linia na tej samej osi
    if let Some(c) = compare {
        let pts: Vec<Pos2> = (0..c.track.len()).map(|i| Pos2::new(x(c.track.d2[i]), y(c.track.e[i]))).collect();
        pp.add(Shape::line(pts, Stroke::new(2.0, COMPARE)));
    }
    let line: Vec<Pos2> = (0..n).map(|i| Pos2::new(x(tr.d2[i]), y(tr.e[i]))).collect();
    let shadow: Vec<Pos2> = line.iter().map(|q| *q + Vec2::new(0.0, 1.5)).collect();
    pp.add(Shape::line(shadow, Stroke::new(3.0, Color32::from_black_alpha(110))));
    pp.add(Shape::line(line, Stroke::new(2.0, theme::PAPER)));

    // najwyższy i najniższy punkt
    let imax = (0..n).max_by(|&a, &b| tr.e[a].total_cmp(&tr.e[b])).unwrap_or(0);
    let imin = (0..n).min_by(|&a, &b| tr.e[a].total_cmp(&tr.e[b])).unwrap_or(0);
    let tag = |pos: Pos2, txt: String, col: Color32, above: bool| {
        let g = p.layout_no_wrap(txt, FontId::proportional(10.5), col);
        let sz = g.size();
        let off = if above { -sz.y - 8.0 } else { 6.0 };
        let left = (pos.x - sz.x / 2.0).clamp(plot.left(), (plot.right() - sz.x).max(plot.left()));
        let r = Rect::from_min_size(Pos2::new(left, pos.y + off), sz).expand2(Vec2::new(5.0, 2.0));
        // bez tła: tekst z ciemnym obrysem
        let at = r.min + Vec2::new(5.0, 2.0);
        for o in [Vec2::new(1.0, 1.0), Vec2::new(-1.0, 1.0), Vec2::new(1.0, -1.0), Vec2::new(-1.0, -1.0)] {
            p.galley_with_override_text_color(at + o, g.clone(), Color32::from_black_alpha(170));
        }
        p.galley(at, g, col);
    };
    p.circle(Pos2::new(x(tr.d2[imax]), y(max_e)), 3.5, theme::PAPER, Stroke::new(1.5, theme::INK_0));
    tag(Pos2::new(x(tr.d2[imax]), y(max_e)), format!("▲ {}", fmt::ele(max_e as f64)), theme::PAPER, true);
    if max_e - min_e > 30.0 {
        tag(Pos2::new(x(tr.d2[imin]), y(min_e)), format!("▼ {}", fmt::ele(min_e as f64)), theme::PAPER_2, false);
    }

    // mijane punkty
    let mut last_x = -1000.0;
    for m in marks {
        let xx = x(m.dist);
        if xx - last_x < 70.0 || (xx - x(tr.d2[imax])).abs() < 40.0 {
            continue;
        }
        last_x = xx;
        let (i, f) = tr.at_dist(m.dist);
        let (_, _, e) = tr.lerp(i, f);
        let top = plot.top() + 2.0;
        pp.extend(Shape::dashed_line(&[Pos2::new(xx, y(e)), Pos2::new(xx, top + 12.0)], Stroke::new(1.0, theme::BRASS.gamma_multiply(0.6)), 3.0, 3.0));
        p.circle_filled(Pos2::new(xx, y(e)), 3.0, theme::BRASS);
        p.text(Pos2::new(xx, top), Align2::CENTER_TOP, &m.label, FontId::proportional(10.5), theme::PAPER_2);
    }
    if let Some(od) = obstacle {
        let xx = x(od);
        p.line_segment([Pos2::new(xx, plot.top()), Pos2::new(xx, plot.bottom())], Stroke::new(2.0, theme::SIGNAL));
        p.text(Pos2::new(xx + 4.0, plot.top()), Align2::LEFT_TOP, crate::i18n::tr("przeszkoda"), FontId::proportional(10.5), theme::SIGNAL);
    }

    // zaznaczanie odcinka przeciąganiem, klik usuwa zaznaczenie
    let to_d = |px: f32| ((px - plot.left()) / plot.width() * total).clamp(0.0, own_total);
    let anchor_id = ui.id().with("profil-kotwica");
    if resp.drag_started() {
        if let Some(pos) = resp.interact_pointer_pos() {
            ui.data_mut(|m| m.insert_temp(anchor_id, to_d(pos.x)));
        }
    }
    if resp.dragged() {
        if let (Some(pos), Some(a)) = (resp.interact_pointer_pos(), ui.data(|m| m.get_temp::<f32>(anchor_id))) {
            let b = to_d(pos.x);
            *sel = ((b - a).abs() > total * 0.004).then_some((a.min(b), a.max(b)));
        }
    }
    if resp.clicked() {
        *sel = None;
    }
    if let Some((s0, s1)) = *sel {
        let cyan = Color32::from_rgb(80, 220, 255);
        let band = Rect::from_x_y_ranges(x(s0)..=x(s1), plot.y_range());
        p.rect_filled(band, 0.0, Color32::from_rgba_unmultiplied(80, 220, 255, 26));
        for xx in [x(s0), x(s1)] {
            p.line_segment([Pos2::new(xx, plot.top()), Pos2::new(xx, plot.bottom())], Stroke::new(1.5, cyan));
        }
        let st = segment(route, s0, s1);
        let rows = [
            tf!("Odcinek {}", fmt::dist(st.dist)),
            tf!("↑ {}  ↓ {}", fmt::ele(st.up), fmt::ele(st.down)),
            tf!("czas {}", fmt::time(st.time)),
            tf!("nachylenie śr. {}% · maks. {}%", fmt::dec(st.avg_grade, 0), fmt::dec(st.max_grade, 0)),
            format!("{} – {}", fmt::ele(st.min_e as f64), fmt::ele(st.max_e as f64)),
        ];
        let gs: Vec<_> = rows.into_iter().map(|t| p.layout_no_wrap(t, FontId::proportional(11.0), theme::PAPER)).collect();
        let w = gs.iter().map(|g| g.size().x).fold(0.0, f32::max) + 16.0;
        let h = gs.iter().map(|g| g.size().y + 1.0).sum::<f32>() + 10.0;
        let left = (band.center().x - w / 2.0).clamp(plot.left(), (plot.right() - w).max(plot.left()));
        let card = Rect::from_min_size(Pos2::new(left, plot.bottom() - h - 4.0), Vec2::new(w, h));
        p.rect(card, 8.0, theme::POPUP_FILL, Stroke::new(1.0, cyan), egui::StrokeKind::Inside);
        let mut yy = card.top() + 5.0;
        for g in gs {
            let gh = g.size().y;
            p.galley(Pos2::new(card.left() + 8.0, yy), g, theme::PAPER);
            yy += gh + 1.0;
        }
    }

    let mut hover = None;
    if let Some(pos) = resp.hover_pos().filter(|p| plot.x_range().contains(p.x)) {
        hover = Some(to_d(pos.x));
    }
    if let Some(cd) = hover.or(cursor) {
        let (i, f) = tr.at_dist(cd);
        let (_, _, e) = tr.lerp(i, f);
        let xx = x(cd);
        p.line_segment([Pos2::new(xx, plot.top()), Pos2::new(xx, plot.bottom())], Stroke::new(1.0, theme::BRASS));
        p.circle(Pos2::new(xx, y(e)), 5.0, theme::BRASS, Stroke::new(2.0, theme::INK_0));
        let j = (i + 1).min(n - 1);
        // nachylenie w oknie ±40 m
        let (a, b) = (tr.at_dist((cd - 40.0).max(0.0)), tr.at_dist((cd + 40.0).min(own_total)));
        let (ea, eb) = (tr.lerp(a.0, a.1).2, tr.lerp(b.0, b.1).2);
        let span = ((cd + 40.0).min(own_total) - (cd - 40.0).max(0.0)).max(1.0);
        let time = tr.t[i] + (tr.t[j] - tr.t[i]) * f;
        let mut up = 0.0f32;
        for k in 1..=i.min(n - 1) {
            up += (tr.e[k] - tr.e[k - 1]).max(0.0);
        }
        let rows = [
            (fmt::dist(cd as f64), 12.5, theme::PAPER),
            (fmt::ele(e as f64), 12.5, theme::PAPER),
            (tf!("{:+.0}% nachylenia", (eb - ea) / span * 100.0), 11.0, theme::PAPER_2),
            (tf!("↑ {} od startu", fmt::ele(up as f64)), 11.0, theme::PAPER_2),
            (tf!("{} od startu", fmt::time(time as f64)), 11.0, theme::PAPER_2),
        ];
        let galleys: Vec<_> = rows.into_iter().map(|(t, size, c)| p.layout_no_wrap(t, FontId::proportional(size), c)).collect();
        let w = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max) + 20.0;
        let h = galleys.iter().map(|g| g.size().y + 1.0).sum::<f32>() + 14.0;
        let left = if xx + 14.0 + w < plot.right() { xx + 14.0 } else { xx - 14.0 - w };
        let card = Rect::from_min_size(Pos2::new(left, plot.top() + 2.0), Vec2::new(w, h));
        p.rect(card, 10.0, theme::GLASS_FILL, Stroke::new(1.0, theme::GLASS_EDGE), egui::StrokeKind::Inside);
        let mut yy = card.top() + 7.0;
        for g in galleys {
            let gh = g.size().y;
            p.galley(Pos2::new(card.left() + 10.0, yy), g, theme::PAPER);
            yy += gh + 1.0;
        }
    }
    hover
}

fn nice_step(raw: f64) -> f64 {
    let p = 10f64.powf(raw.max(1e-9).log10().floor());
    for m in [1.0, 2.0, 2.5, 5.0, 10.0] {
        if m * p >= raw {
            return m * p;
        }
    }
    10.0 * p
}
