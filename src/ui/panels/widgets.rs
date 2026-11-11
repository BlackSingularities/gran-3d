//! Drobne elementy paneli: kafelki liczb, nagłówki, wiersze, paski nachylenia, karty wariantów.

use super::*;

/// Ikona kafla z parametrem.
pub(super) fn stat_glyph(k: &str) -> &'static str {
    use theme::icon as i;
    match crate::i18n::source(k) {
        "Dystans" | "Odcinek" | "Linia prosta" | "W rzucie" | "Odległość" => i::RULER,
        "Po powierzchni" | "Po terenie" | "Szlaki" => i::PATH,
        "Podejścia" | "Wyżej (5 km)" => i::TREND_UP,
        "Zejścia" | "Niżej (5 km)" => i::TREND_DOWN,
        "Czas" | "Czas (tam)" | "Czas (z powrotem)" | "Wyjście" | "Dojście" => i::CLOCK,
        "Najwyżej" | "Wysokość" | "Model terenu" => i::MOUNTAINS,
        "Najniżej" => i::ARROW_LINE_DOWN,
        "Maks. nachylenie" | "Nachylenie" | "Śr. nachylenie" => i::ANGLE,
        "Ekspozycja" | "Azymut" => i::COMPASS,
        "Krętość" => i::WAVES,
        "Punkty GOT" => i::MEDAL,
        "Skala SAC" => i::WARNING,
        "Śr. tempo" => i::GAUGE,
        "Wschód" | "Wschód słońca" => i::SUN_HORIZON,
        "Zachód" | "Zachód słońca" => i::SUN_DIM,
        "Słońce" | "Górowanie" => i::SUN,
        "Granica śniegu" => i::SNOWFLAKE,
        "Mieszkańcy" => i::USERS,
        "Różnica wysokości" => i::ARROWS_VERTICAL,
        "Szerokość" | "Długość" => i::MAP_PIN,
        "Trasy PTTK i inne" => i::SIGNPOST,
        "Punkty" | "Sektory" => i::STACK,
        _ => i::INFO,
    }
}

/// Kafel: ikona, podpis, wartość. Rysowany ręcznie, więc zawsze mieści się w swojej szerokości.
pub(super) fn stat_tile(ui: &mut egui::Ui, w: f32, k: &str, v: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 54.0), egui::Sense::hover());
    let p = ui.painter().with_clip_rect(rect);
    p.rect_filled(rect, 10.0, theme::INK_2);
    p.text(rect.left_center() + egui::vec2(11.0, 0.0), egui::Align2::LEFT_CENTER, stat_glyph(k), egui::FontId::proportional(19.0), theme::BRASS);
    let x = rect.left() + 38.0;
    p.text(egui::pos2(x, rect.top() + 9.0), egui::Align2::LEFT_TOP, crate::i18n::tr(k), egui::FontId::proportional(11.5), theme::paper_3());
    let room = rect.width() - 44.0;
    let mut g = p.layout_no_wrap(v.to_string(), egui::FontId::proportional(16.0), theme::PAPER);
    if g.size().x > room {
        g = p.layout_no_wrap(v.to_string(), egui::FontId::proportional(12.0), theme::PAPER);
    }
    p.galley(egui::pos2(x, rect.bottom() - 9.0 - g.size().y), g, theme::PAPER);
}

/// Dane bez teł: ikona, podpis i wartość w dwóch kolumnach.
pub(super) fn plain_stats(ui: &mut egui::Ui, items: &[(&str, String)]) {
    let w = (ui.available_width() - 12.0) / 2.0;
    for row in items.chunks(2) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            for (k, v) in row {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 40.0), egui::Sense::hover());
                let p = ui.painter().with_clip_rect(rect);
                p.text(rect.left_center() + egui::vec2(9.0, 0.0), egui::Align2::CENTER_CENTER, stat_glyph(k), egui::FontId::proportional(17.0), theme::BRASS);
                p.text(egui::pos2(rect.left() + 26.0, rect.top() + 3.0), egui::Align2::LEFT_TOP, crate::i18n::tr(k), egui::FontId::proportional(11.0), theme::paper_3());
                p.text(egui::pos2(rect.left() + 26.0, rect.bottom() - 3.0), egui::Align2::LEFT_BOTTOM, v, egui::FontId::proportional(15.0), theme::PAPER);
            }
        });
    }
}

pub(super) fn grid2(ui: &mut egui::Ui, items: Vec<(&str, String)>) {
    let gap = 8.0;
    let w = (ui.available_width() - gap) / 2.0;
    for row in items.chunks(2) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            // ostatni samotny kafel na całą szerokość
            let tw = if row.len() == 1 { w * 2.0 + gap } else { w };
            for (k, v) in row {
                stat_tile(ui, tw, k, v);
            }
        });
    }
}

/// Nagłówek sekcji z ikoną.
pub(super) fn section(ui: &mut egui::Ui, glyph: &str, title: &str) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(RichText::new(glyph).size(14.0).color(theme::BRASS));
        ui.label(theme::kicker(title));
    });
}

/// Profil wysokości jako mały wykres z wypełnieniem.
pub(super) fn spark(painter: &egui::Painter, rect: egui::Rect, e: &[f32], color: Color32) {
    if e.len() < 2 {
        return;
    }
    let (lo, hi) = e.iter().fold((f32::MAX, f32::MIN), |(l, h), &v| (l.min(v), h.max(v)));
    let span = (hi - lo).max(1.0);
    let n = (rect.width() / 2.0).max(2.0) as usize;
    let pts: Vec<egui::Pos2> = (0..=n)
        .map(|i| {
            let f = i as f32 / n as f32;
            let v = e[(f * (e.len() - 1) as f32) as usize];
            egui::pos2(rect.left() + f * rect.width(), rect.bottom() - (v - lo) / span * rect.height())
        })
        .collect();
    let fill = color.gamma_multiply(0.22);
    for q in &pts {
        painter.line_segment([*q, egui::pos2(q.x, rect.bottom())], egui::Stroke::new(2.0, fill));
    }
    painter.add(egui::Shape::line(pts, egui::Stroke::new(1.5, color)));
}

pub(super) const DIFF_COLORS: [Color32; 4] = [theme::OK, theme::WARN, theme::SIGNAL, Color32::from_rgb(190, 80, 200)];

/// Pigułka z tekstem w kolorze.
pub(super) fn pill(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new().fill(color.gamma_multiply(0.16)).corner_radius(9).inner_margin(egui::Margin::symmetric(9, 3)).show(ui, |ui| {
        ui.label(RichText::new(text).size(12.0).strong().color(color));
    });
}

/// Karta wariantu trasy.
pub(super) fn variant_card(ui: &mut egui::Ui, r: &Route, title: &str, sub: &str, sel: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 78.0), egui::Sense::click());
    theme::focus_ring(ui, &resp, rect, 12.0);
    let p = ui.painter().with_clip_rect(rect);
    p.rect_filled(rect, 12.0, if sel { theme::BRASS_DIM } else if resp.hovered() { theme::INK_3 } else { theme::INK_2 });
    let inner = rect.shrink2(egui::vec2(12.0, 9.0));
    let time = fmt::time(r.stats.time);
    p.text(inner.right_top(), egui::Align2::RIGHT_TOP, &time, egui::FontId::proportional(15.0), theme::PAPER);
    let title_clip = egui::Rect::from_min_max(inner.left_top(), egui::pos2(inner.right() - 90.0, inner.top() + 20.0));
    p.with_clip_rect(title_clip).text(inner.left_top(), egui::Align2::LEFT_TOP, title, egui::FontId::proportional(14.0), theme::PAPER);
    p.text(inner.left_top() + egui::vec2(0.0, 21.0), egui::Align2::LEFT_TOP, sub, egui::FontId::proportional(11.5), theme::paper_2());
    let chart = egui::Rect::from_min_max(egui::pos2(inner.left(), inner.bottom() - 16.0), inner.right_bottom());
    spark(&p, chart, &r.track.e, if sel { theme::BRASS } else { theme::paper_3() });
    resp.clicked()
}

/// Udział dystansu w klasach nachylenia (odcinki co ~100 m).
pub(super) fn slope_shares(r: &Route) -> [f32; 4] {
    let tr = &r.track;
    let mut out = [0f32; 4];
    let mut start = 0;
    for i in 1..tr.len() {
        let d = tr.d2[i] - tr.d2[start];
        if d >= 100.0 || i == tr.len() - 1 {
            if d > 0.0 {
                let g = (tr.e[i] - tr.e[start]).abs() / d * 100.0;
                out[if g < 5.0 { 0 } else if g < 15.0 { 1 } else if g < 30.0 { 2 } else { 3 }] += d;
            }
            start = i;
        }
    }
    let sum: f32 = out.iter().sum::<f32>().max(1.0);
    out.map(|v| v / sum)
}

pub(super) fn slope_bar(ui: &mut egui::Ui, r: &Route) {
    const NAMES: [&str; 4] = ["płasko", "łagodnie", "stromo", "bardzo stromo"];
    let shares = slope_shares(r);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 12.0), egui::Sense::hover());
    let p = ui.painter().with_clip_rect(rect);
    let mut x = rect.left();
    for (i, s) in shares.iter().enumerate() {
        let w = s * rect.width();
        p.rect_filled(egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2((w - 1.0).max(0.0), 12.0)), 3.0, DIFF_COLORS[i]);
        x += w;
    }
    ui.horizontal_wrapped(|ui| {
        for (i, s) in shares.iter().enumerate() {
            let (dot, _) = ui.allocate_exact_size(egui::vec2(9.0, 14.0), egui::Sense::hover());
            ui.painter().circle_filled(dot.center(), 4.0, DIFF_COLORS[i]);
            ui.label(RichText::new(format!("{} {}%", crate::i18n::tr(NAMES[i]), fmt::dec(*s as f64 * 100.0, 0))).small().color(theme::paper_2()));
        }
    });
}

pub(super) fn big(ui: &mut egui::Ui, v: String, sub: &str, color: Color32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(v).size(30.0).strong().color(color));
        ui.label(RichText::new(sub).color(theme::paper_3()));
    });
}

pub(super) fn row(ui: &mut egui::Ui, name: &str, right: &str, sub: &str) -> bool {
    let r = ui
        .horizontal(|ui| {
            ui.set_width(ui.available_width());
            let l = ui.add(egui::Label::new(RichText::new(name).strong()).sense(egui::Sense::click()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(right).monospace().color(theme::paper_2()));
                if !sub.is_empty() {
                    ui.label(RichText::new(sub).small().color(theme::paper_3()));
                }
            });
            l.clicked()
        })
        .inner;
    r
}

/// Ikona i barwa rodzaju punktu.
pub(super) fn kind_style(k: PoiKind) -> (&'static str, Color32) {
    use theme::icon as i;
    match k {
        PoiKind::Peak => (i::MOUNTAINS, theme::PAPER),
        PoiKind::Saddle => (i::ARROWS_VERTICAL, Color32::from_rgb(215, 222, 235)),
        PoiKind::Ridge => (i::TREND_UP, Color32::from_rgb(205, 214, 190)),
        PoiKind::Valley => (i::TREND_DOWN, Color32::from_rgb(205, 214, 190)),
        PoiKind::Hut => (i::HOUSE_LINE, Color32::from_rgb(255, 190, 110)),
        PoiKind::Shelter => (i::TENT, Color32::from_rgb(255, 190, 110)),
        PoiKind::Lake | PoiKind::Waterfall | PoiKind::Spring | PoiKind::DrinkingWater => (i::DROP, Color32::from_rgb(130, 195, 240)),
        PoiKind::Cave => (i::CIRCLE_HALF, theme::paper_2()),
        PoiKind::Viewpoint => (i::BINOCULARS, theme::PAPER),
        PoiKind::Locality => (i::PLANT, Color32::from_rgb(170, 220, 130)),
        PoiKind::Place => (i::BUILDINGS, Color32::from_rgb(230, 222, 200)),
        PoiKind::Parking => (i::CAR, theme::paper_2()),
        PoiKind::BusStop => (i::BUS, theme::paper_2()),
        PoiKind::Station => (i::TRAIN, theme::paper_2()),
        PoiKind::Cableway => (i::ARROW_UP_RIGHT, theme::paper_2()),
        PoiKind::Shrine => (i::CROSS, theme::paper_2()),
        PoiKind::Webcam => (i::CAMERA, Color32::from_rgb(130, 195, 240)),
    }
}

/// Nagłówek panelu zaznaczenia: ikona w kółku, rodzaj, nazwa, podtytuł i zamknięcie.
pub(super) fn header(a: &mut Atlas, ui: &mut egui::Ui, glyph: &str, color: Color32, kicker: &str, title: &str, sub: &str) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(46.0, 46.0), egui::Sense::hover());
        ui.painter().circle_filled(r.center(), 22.0, Color32::from_white_alpha(14));
        ui.painter().circle_stroke(r.center(), 22.0, egui::Stroke::new(1.5, color.gamma_multiply(0.7)));
        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, glyph, egui::FontId::proportional(22.0), color);
        ui.vertical(|ui| {
            ui.set_width(ui.available_width() - 30.0);
            ui.label(theme::kicker(kicker));
            ui.add(egui::Label::new(RichText::new(title).size(22.0).strong()).wrap());
            if !sub.is_empty() {
                ui.label(RichText::new(sub).small().color(theme::paper_3()));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            if ui.small_button(theme::icon::X).on_hover_text(crate::i18n::tr("Zamknij (Esc)")).clicked() {
                a.selection = Selection::None;
            }
        });
    });
    ui.add_space(6.0);
}

/// Rząd dużych wartości z ikonami (podsumowanie trasy).
pub(super) fn summary(ui: &mut egui::Ui, items: &[(&str, String, Color32)]) {
    let n = items.len().max(1) as f32;
    let w = (ui.available_width() - 6.0 * (n - 1.0)) / n;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (g, v, c) in items {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 52.0), egui::Sense::hover());
            let p = ui.painter().with_clip_rect(rect);
            p.rect_filled(rect, 10.0, theme::INK_2);
            p.text(rect.center_top() + egui::vec2(0.0, 14.0), egui::Align2::CENTER_CENTER, *g, egui::FontId::proportional(14.0), theme::BRASS);
            let mut gal = p.layout_no_wrap(v.clone(), egui::FontId::proportional(17.0), *c);
            if gal.size().x > w - 6.0 {
                gal = p.layout_no_wrap(v.clone(), egui::FontId::proportional(13.0), *c);
            }
            p.galley(egui::pos2(rect.center().x - gal.size().x / 2.0, rect.bottom() - 8.0 - gal.size().y), gal, *c);
        }
    });
}

#[allow(dead_code)]
pub(super) fn close_row(a: &mut Atlas, ui: &mut egui::Ui, kicker: &str) {
    ui.horizontal(|ui| {
        ui.label(theme::kicker(kicker));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button(theme::icon::X).clicked() {
                a.selection = Selection::None;
            }
        });
    });
}
