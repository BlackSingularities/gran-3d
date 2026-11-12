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

/// Tło i krawędź kart z danymi w panelu.
pub(super) const CARD_FILL: Color32 = Color32::from_rgba_premultiplied(10, 10, 10, 10);
pub(super) const CARD_EDGE: Color32 = Color32::from_rgba_premultiplied(22, 22, 22, 22);

/// Komórka danych: ikona i podpis w jednym wierszu, pod nimi wartość. Rysowana ręcznie w swoim polu.
fn stat_cell(p: &egui::Painter, cell: egui::Rect, k: &str, v: &str) {
    let p = p.with_clip_rect(cell.shrink2(egui::vec2(6.0, 0.0)));
    let x = cell.left() + 14.0;
    p.text(egui::pos2(x, cell.top() + 9.0), egui::Align2::LEFT_TOP, stat_glyph(k), egui::FontId::proportional(13.5), theme::BRASS);
    p.text(egui::pos2(x + 20.0, cell.top() + 9.5), egui::Align2::LEFT_TOP, crate::i18n::tr(k), egui::FontId::proportional(11.5), theme::paper_3());
    let room = cell.width() - 26.0;
    let mut g = p.layout_no_wrap(v.to_string(), egui::FontId::proportional(16.5), theme::PAPER);
    if g.size().x > room {
        g = p.layout_no_wrap(v.to_string(), egui::FontId::proportional(13.0), theme::PAPER);
    }
    p.galley(egui::pos2(x, cell.bottom() - 8.0 - g.size().y), g, theme::PAPER);
}

/// Dane bez teł: ikona, podpis i wartość w dwóch kolumnach.
pub(super) fn plain_stats(ui: &mut egui::Ui, items: &[(&str, String)]) {
    let w = (ui.available_width() / 2.0).floor();
    for row in items.chunks(2) {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w * 2.0, 48.0), egui::Sense::hover());
        let p = ui.painter().with_clip_rect(rect);
        for (c, (k, v)) in row.iter().enumerate() {
            stat_cell(&p, egui::Rect::from_min_size(egui::pos2(rect.left() + c as f32 * w - 8.0, rect.top()), egui::vec2(w, 48.0)), k, v);
        }
    }
}

/// Karta danych: komórki w dwóch kolumnach, wiersze rozdzielone cienką linią.
pub(super) fn grid2(ui: &mut egui::Ui, items: Vec<(&str, String)>) {
    if items.is_empty() {
        return;
    }
    const H: f32 = 52.0;
    let rows = items.len().div_ceil(2);
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, rows as f32 * H), egui::Sense::hover());
    let p = ui.painter().with_clip_rect(rect);
    p.rect_filled(rect, 12.0, CARD_FILL);
    p.rect_stroke(rect, 12.0, egui::Stroke::new(1.0, CARD_EDGE), egui::StrokeKind::Inside);
    for (r, row) in items.chunks(2).enumerate() {
        let top = rect.top() + r as f32 * H;
        if r > 0 {
            p.line_segment([egui::pos2(rect.left() + 14.0, top), egui::pos2(rect.right() - 14.0, top)], egui::Stroke::new(1.0, CARD_EDGE));
        }
        // ostatnia samotna komórka na całą szerokość
        let cw = if row.len() == 1 { w } else { w / 2.0 };
        for (c, (k, v)) in row.iter().enumerate() {
            stat_cell(&p, egui::Rect::from_min_size(egui::pos2(rect.left() + c as f32 * cw, top), egui::vec2(cw, H)), k, v);
        }
    }
}

/// Wiersz nagłówka sekcji: ikona i tytuł na środku, po obu stronach cienka linia
/// (w sekcji zwijanej po prawej strzałka).
fn section_row(ui: &mut egui::Ui, glyph: &str, title: &str, fold: Option<bool>) -> egui::Response {
    ui.add_space(10.0);
    let sense = if fold.is_some() { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), sense);
    let p = ui.painter().with_clip_rect(rect);
    if fold.is_some() {
        theme::focus_ring(ui, &resp, rect, 8.0);
        if resp.hovered() {
            p.rect_filled(rect, 8.0, Color32::from_white_alpha(10));
        }
    }
    let y = rect.center().y;
    let mut end = rect.right();
    if let Some(open) = fold {
        let caret = if open { theme::icon::CARET_UP } else { theme::icon::CARET_DOWN };
        p.text(egui::pos2(rect.right() - 6.0, y), egui::Align2::RIGHT_CENTER, caret, egui::FontId::proportional(13.0), theme::paper_3());
        end = rect.right() - 28.0;
    }
    // ikona z tytułem na środku wiersza (środek całego wiersza, także gdy po prawej jest strzałka)
    let gg = p.layout_no_wrap(glyph.to_string(), egui::FontId::proportional(15.0), theme::BRASS);
    let tg = p.layout_no_wrap(crate::i18n::tr(title).to_string(), egui::FontId::proportional(13.0), theme::PAPER);
    let w = gg.size().x + 7.0 + tg.size().x;
    let x0 = (rect.center().x - w / 2.0).min(end - 10.0 - w).max(rect.left() + 2.0);
    let x1 = x0 + w;
    let title_x = x0 + gg.size().x + 7.0;
    p.galley(egui::pos2(x0, y - gg.size().y / 2.0), gg, theme::BRASS);
    p.galley(egui::pos2(title_x, y - tg.size().y / 2.0), tg, theme::PAPER);
    let line = egui::Stroke::new(1.0, Color32::from_white_alpha(18));
    if rect.left() + 2.0 < x0 - 10.0 {
        p.line_segment([egui::pos2(rect.left() + 2.0, y), egui::pos2(x0 - 10.0, y)], line);
    }
    if x1 + 10.0 < end {
        p.line_segment([egui::pos2(x1 + 10.0, y), egui::pos2(end, y)], line);
    }
    resp
}

/// Nagłówek sekcji z ikoną.
pub(super) fn section(ui: &mut egui::Ui, glyph: &str, title: &str) {
    section_row(ui, glyph, title, None);
}

/// Zwijana sekcja: klik w nagłówek chowa albo pokazuje treść (stan pamiętany między uruchomieniami).
/// Zwraca, czy sekcja jest rozwinięta.
pub(super) fn fold(ui: &mut egui::Ui, key: &'static str, glyph: &str, title: &str, default_open: bool) -> bool {
    let id = egui::Id::new(("sekcja-panelu", key));
    let mut open = ui.data_mut(|d| *d.get_persisted_mut_or(id, default_open));
    let resp = section_row(ui, glyph, title, Some(open));
    if resp.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_persisted(id, open));
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    open
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
    // rysowana ręcznie na środku wiersza
    let g = ui.painter().layout_no_wrap(text.to_string(), egui::FontId::proportional(12.0), color);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), g.size().y + 8.0), egui::Sense::hover());
    let p = ui.painter().with_clip_rect(rect);
    let bg = egui::Rect::from_center_size(rect.center(), g.size() + egui::vec2(20.0, 8.0)).intersect(rect);
    p.rect_filled(bg, 10.0, color.gamma_multiply(0.16));
    p.galley(egui::pos2(bg.center().x - g.size().x / 2.0, rect.center().y - g.size().y / 2.0), g, color);
}

/// Karta wariantu trasy.
pub(super) fn variant_card(ui: &mut egui::Ui, r: &Route, title: &str, sub: &str, sel: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 78.0), egui::Sense::click());
    theme::focus_ring(ui, &resp, rect, 12.0);
    let p = ui.painter().with_clip_rect(rect);
    p.rect_filled(rect, 12.0, if sel { Color32::from_rgba_unmultiplied(122, 176, 255, 40) } else if resp.hovered() { Color32::from_white_alpha(18) } else { CARD_FILL });
    p.rect_stroke(rect, 12.0, if sel { egui::Stroke::new(1.5, theme::BRASS) } else { egui::Stroke::new(1.0, CARD_EDGE) }, egui::StrokeKind::Inside);
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

/// Wiersz listy: nazwa po lewej, wartość po prawej, przed nią drobny dopisek; klikalny w całości.
pub(super) fn row(ui: &mut egui::Ui, name: &str, right: &str, sub: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::click());
    theme::focus_ring(ui, &resp, rect, 8.0);
    let p = ui.painter().with_clip_rect(rect);
    if resp.hovered() {
        p.rect_filled(rect, 8.0, Color32::from_white_alpha(14));
    }
    let y = rect.center().y;
    let rg = p.layout_no_wrap(right.to_string(), egui::FontId::proportional(13.5), theme::paper_2());
    let mut limit = rect.right() - 8.0 - rg.size().x;
    p.galley(egui::pos2(limit, y - rg.size().y / 2.0), rg, theme::paper_2());
    if !sub.is_empty() {
        let sg = p.layout_no_wrap(sub.to_string(), egui::FontId::proportional(11.5), theme::paper_3());
        limit -= 10.0 + sg.size().x;
        p.galley(egui::pos2(limit, y - sg.size().y / 2.0), sg, theme::paper_3());
    }
    let name_clip = egui::Rect::from_min_max(rect.min, egui::pos2((limit - 10.0).max(rect.left()), rect.bottom()));
    p.with_clip_rect(name_clip).text(egui::pos2(rect.left() + 8.0, y), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(14.0), theme::PAPER);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Wiersz z ikoną i dwiema liniami: nazwa i dopisek po lewej, wartość i jej dopisek po prawej.
pub(super) fn rich_row(ui: &mut egui::Ui, glyph: &str, glyph_color: Color32, name: &str, sub: &str, right: &str, right_sub: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 42.0), egui::Sense::click());
    theme::focus_ring(ui, &resp, rect, 10.0);
    let p = ui.painter().with_clip_rect(rect);
    if resp.hovered() {
        p.rect_filled(rect, 10.0, Color32::from_white_alpha(14));
    }
    let icon_c = egui::pos2(rect.left() + 19.0, rect.center().y);
    p.circle_filled(icon_c, 14.0, glyph_color.gamma_multiply(0.14));
    p.text(icon_c, egui::Align2::CENTER_CENTER, glyph, egui::FontId::proportional(15.0), glyph_color);
    let rg = p.layout_no_wrap(right.to_string(), egui::FontId::proportional(13.5), theme::PAPER);
    let sg = p.layout_no_wrap(right_sub.to_string(), egui::FontId::proportional(11.5), theme::paper_3());
    let rw = rg.size().x.max(sg.size().x);
    p.galley(egui::pos2(rect.right() - 8.0 - rg.size().x, rect.top() + 5.0), rg, theme::PAPER);
    p.galley(egui::pos2(rect.right() - 8.0 - sg.size().x, rect.bottom() - 6.0 - sg.size().y), sg, theme::paper_3());
    let left = rect.left() + 42.0;
    let text_clip = egui::Rect::from_min_max(egui::pos2(left, rect.top()), egui::pos2((rect.right() - 18.0 - rw).max(left), rect.bottom()));
    let pt = p.with_clip_rect(text_clip);
    if sub.is_empty() {
        pt.text(egui::pos2(left, rect.center().y), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(14.0), theme::PAPER);
    } else {
        pt.text(egui::pos2(left, rect.top() + 4.0), egui::Align2::LEFT_TOP, name, egui::FontId::proportional(14.0), theme::PAPER);
        pt.text(egui::pos2(left, rect.bottom() - 5.0), egui::Align2::LEFT_BOTTOM, sub, egui::FontId::proportional(11.5), theme::paper_3());
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
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
        ui.painter().rect_filled(r.shrink(1.0), 14.0, color.gamma_multiply(0.16));
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
