//! Profil wysokości trasy: wypełnienie barwione nachyleniem, pas kolorów szlaków, mijane punkty,
//! warianty w tle i kursor zsynchronizowany z mapą.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Vec2};

use crate::fmt;
use crate::graph::Route;
use crate::metrics::{grade_class, GRADE_RGB};
use crate::ui::theme;

pub struct Mark {
    pub dist: f32,
    pub label: String,
}

/// Rysuje profil; zwraca dystans pod kursorem (m), jeśli kursor jest nad wykresem.
pub fn show(
    ui: &mut egui::Ui,
    route: &Route,
    others: &[&Route],
    marks: &[Mark],
    cursor: Option<f32>,
    obstacle: Option<f32>,
) -> Option<f32> {
    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    let p = ui.painter_at(rect);
    let tr = &route.track;
    let n = tr.len();
    if n < 2 {
        return None;
    }
    let total = *tr.d2.last().unwrap();
    let mut lo = tr.e.iter().cloned().fold(f32::INFINITY, f32::min);
    let mut hi = tr.e.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    for o in others {
        for &e in &o.track.e {
            lo = lo.min(e);
            hi = hi.max(e);
        }
    }
    let pad = ((hi - lo) * 0.12).max(20.0);
    lo -= pad;
    hi += pad * 0.6;
    let plot = Rect::from_min_max(rect.min + Vec2::new(52.0, 16.0), rect.max - Vec2::new(14.0, 26.0));
    let max_d = others.iter().map(|o| *o.track.d2.last().unwrap()).fold(total, f32::max);
    let x = |d: f32| plot.left() + d / max_d.max(1.0) * plot.width();
    let y = |e: f32| plot.bottom() - (e - lo) / (hi - lo).max(1.0) * plot.height();

    // siatka wysokości
    let step = nice_step((hi - lo) as f64 / 4.0) as f32;
    let mut e = (lo / step).ceil() * step;
    while e <= hi {
        let yy = y(e);
        p.line_segment([Pos2::new(plot.left(), yy), Pos2::new(plot.right(), yy)], Stroke::new(1.0, theme::INK_3));
        p.text(Pos2::new(plot.left() - 6.0, yy), Align2::RIGHT_CENTER, fmt::int(e as f64), FontId::monospace(10.5), theme::PAPER_3);
        e += step;
    }
    let dstep = nice_step(max_d as f64 / 6.0) as f32;
    let mut d = 0.0;
    while d <= max_d {
        let xx = x(d);
        p.line_segment([Pos2::new(xx, plot.top()), Pos2::new(xx, plot.bottom())], Stroke::new(1.0, theme::INK_2));
        p.text(Pos2::new(xx, plot.bottom() + 4.0), Align2::CENTER_TOP, fmt::dist(d as f64), FontId::monospace(10.5), theme::PAPER_3);
        d += dstep;
    }

    // warianty w tle
    for o in others {
        let pts: Vec<Pos2> = (0..o.track.len()).step_by(2).map(|i| Pos2::new(x(o.track.d2[i]), y(o.track.e[i]))).collect();
        p.add(Shape::line(pts, Stroke::new(1.2, theme::PAPER_3.gamma_multiply(0.5))));
    }

    // wypełnienie barwione nachyleniem (odcinki ~ co 2 px)
    let mut i = 0;
    while i < n - 1 {
        let mut j = i + 1;
        while j < n - 1 && x(tr.d2[j]) - x(tr.d2[i]) < 2.0 {
            j += 1;
        }
        let dd = (tr.d2[j] - tr.d2[i]).max(0.1);
        let g = (tr.e[j] - tr.e[i]) as f64 / dd as f64;
        let c = GRADE_RGB[grade_class(g)];
        let col = Color32::from_rgba_unmultiplied(c[0], c[1], c[2], 150);
        let (x0, x1) = (x(tr.d2[i]), x(tr.d2[j]));
        p.add(Shape::convex_polygon(
            vec![
                Pos2::new(x0, plot.bottom()),
                Pos2::new(x0, y(tr.e[i])),
                Pos2::new(x1, y(tr.e[j])),
                Pos2::new(x1, plot.bottom()),
            ],
            col,
            Stroke::NONE,
        ));
        // pas kolorów szlaku pod wykresem
        let tc = tr.color[i].rgb();
        p.rect_filled(
            Rect::from_min_max(Pos2::new(x0, plot.bottom() + 0.5), Pos2::new(x1 + 0.5, plot.bottom() + 3.5)),
            0.0,
            Color32::from_rgb(tc[0], tc[1], tc[2]),
        );
        i = j;
    }
    let line: Vec<Pos2> = (0..n).map(|i| Pos2::new(x(tr.d2[i]), y(tr.e[i]))).collect();
    p.add(Shape::line(line, Stroke::new(1.8, theme::PAPER)));

    // mijane punkty
    let mut last_x = -1000.0;
    for m in marks {
        let xx = x(m.dist);
        if xx - last_x < 54.0 {
            continue;
        }
        last_x = xx;
        let (i, f) = tr.at_dist(m.dist);
        let (_, _, e) = tr.lerp(i, f);
        p.circle_filled(Pos2::new(xx, y(e)), 2.5, theme::BRASS);
        p.text(Pos2::new(xx, y(e) - 6.0), Align2::CENTER_BOTTOM, &m.label, FontId::proportional(10.5), theme::PAPER_2);
    }
    if let Some(od) = obstacle {
        let xx = x(od);
        p.line_segment([Pos2::new(xx, plot.top()), Pos2::new(xx, plot.bottom())], Stroke::new(2.0, theme::SIGNAL));
        p.text(Pos2::new(xx + 4.0, plot.top()), Align2::LEFT_TOP, "przeszkoda", FontId::proportional(10.5), theme::SIGNAL);
    }

    let mut hover = None;
    if let Some(pos) = resp.hover_pos().filter(|p| plot.x_range().contains(p.x)) {
        hover = Some(((pos.x - plot.left()) / plot.width() * max_d).clamp(0.0, total));
    }
    if let Some(cd) = hover.or(cursor) {
        let (i, f) = tr.at_dist(cd);
        let (_, _, e) = tr.lerp(i, f);
        let xx = x(cd);
        p.line_segment([Pos2::new(xx, plot.top()), Pos2::new(xx, plot.bottom())], Stroke::new(1.0, theme::BRASS));
        p.circle_filled(Pos2::new(xx, y(e)), 4.0, theme::BRASS);
        let j = (i + 1).min(n - 1);
        let dd = (tr.d2[j] - tr.d2[i]).max(0.1);
        // nachylenie w oknie ±40 m
        let (a, b) = (tr.at_dist((cd - 40.0).max(0.0)), tr.at_dist((cd + 40.0).min(total)));
        let (ea, eb) = (tr.lerp(a.0, a.1).2, tr.lerp(b.0, b.1).2);
        let span = ((cd + 40.0).min(total) - (cd - 40.0).max(0.0)).max(1.0);
        let _ = dd;
        let time = tr.t[i] + (tr.t[j] - tr.t[i]) * f;
        let txt = format!(
            "{}  ·  {}  ·  {:+.0}%  ·  {}",
            fmt::dist(cd as f64),
            fmt::ele(e as f64),
            (eb - ea) / span * 100.0,
            fmt::time(time as f64)
        );
        let at = if xx > plot.center().x { Align2::RIGHT_TOP } else { Align2::LEFT_TOP };
        let tx = if xx > plot.center().x { xx - 8.0 } else { xx + 8.0 };
        p.text(Pos2::new(tx, plot.top() + 2.0), at, txt, FontId::monospace(11.0), theme::PAPER);
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
