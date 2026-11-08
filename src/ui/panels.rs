//! Interfejs nad mapą: górny pasek (wyszukiwarka, narzędzia, widok, zegar), szuflada (szczegóły, warstwy, światło), sterowanie kamerą, profil i pomoc.

use eframe::egui::{self, Color32, RichText};

use crate::fmt;
use crate::graph::Route;
use crate::geo::{bearing, haversine};
use crate::graph::RouteKind;
use crate::metrics::{self, TimeModel, DIFFICULTY, SAC_LABEL};
use crate::osm::PoiKind;
use crate::sun::{self, Day};
use crate::ui::atlas::{Atlas, Lens, Quality, RouteMode, Selection, Style, Tool};
use crate::ui::profile;
use crate::ui::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Info,
    Library,
}

/// Ikona kafla z parametrem.
fn stat_glyph(k: &str) -> &'static str {
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
fn stat_tile(ui: &mut egui::Ui, w: f32, k: &str, v: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 54.0), egui::Sense::hover());
    let p = ui.painter().with_clip_rect(rect);
    p.rect_filled(rect, 10.0, theme::INK_2);
    p.text(rect.left_center() + egui::vec2(11.0, 0.0), egui::Align2::LEFT_CENTER, stat_glyph(k), egui::FontId::proportional(19.0), theme::BRASS);
    let x = rect.left() + 38.0;
    p.text(egui::pos2(x, rect.top() + 9.0), egui::Align2::LEFT_TOP, crate::i18n::tr(k), egui::FontId::proportional(11.5), theme::PAPER_3);
    let room = rect.width() - 44.0;
    let mut g = p.layout_no_wrap(v.to_string(), egui::FontId::proportional(16.0), theme::PAPER);
    if g.size().x > room {
        g = p.layout_no_wrap(v.to_string(), egui::FontId::proportional(12.0), theme::PAPER);
    }
    p.galley(egui::pos2(x, rect.bottom() - 9.0 - g.size().y), g, theme::PAPER);
}

/// Dane bez teł: ikona, podpis i wartość w dwóch kolumnach.
fn plain_stats(ui: &mut egui::Ui, items: &[(&str, String)]) {
    let w = (ui.available_width() - 12.0) / 2.0;
    for row in items.chunks(2) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            for (k, v) in row {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 40.0), egui::Sense::hover());
                let p = ui.painter().with_clip_rect(rect);
                p.text(rect.left_center() + egui::vec2(9.0, 0.0), egui::Align2::CENTER_CENTER, stat_glyph(k), egui::FontId::proportional(17.0), theme::BRASS);
                p.text(egui::pos2(rect.left() + 26.0, rect.top() + 3.0), egui::Align2::LEFT_TOP, crate::i18n::tr(k), egui::FontId::proportional(11.0), theme::PAPER_3);
                p.text(egui::pos2(rect.left() + 26.0, rect.bottom() - 3.0), egui::Align2::LEFT_BOTTOM, v, egui::FontId::proportional(15.0), theme::PAPER);
            }
        });
    }
}

fn grid2(ui: &mut egui::Ui, items: Vec<(&str, String)>) {
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
fn section(ui: &mut egui::Ui, glyph: &str, title: &str) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(RichText::new(glyph).size(14.0).color(theme::BRASS));
        ui.label(theme::kicker(title));
    });
}

/// Profil wysokości jako mały wykres z wypełnieniem.
fn spark(painter: &egui::Painter, rect: egui::Rect, e: &[f32], color: Color32) {
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

const DIFF_COLORS: [Color32; 4] = [theme::OK, theme::WARN, theme::SIGNAL, Color32::from_rgb(190, 80, 200)];

/// Pigułka z tekstem w kolorze.
fn pill(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new().fill(color.gamma_multiply(0.16)).corner_radius(9).inner_margin(egui::Margin::symmetric(9, 3)).show(ui, |ui| {
        ui.label(RichText::new(text).size(12.0).strong().color(color));
    });
}

/// Karta wariantu trasy.
fn variant_card(ui: &mut egui::Ui, r: &Route, title: &str, sub: &str, sel: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 78.0), egui::Sense::click());
    let p = ui.painter().with_clip_rect(rect);
    p.rect_filled(rect, 12.0, if sel { theme::BRASS_DIM } else if resp.hovered() { theme::INK_3 } else { theme::INK_2 });
    let inner = rect.shrink2(egui::vec2(12.0, 9.0));
    let time = fmt::time(r.stats.time);
    p.text(inner.right_top(), egui::Align2::RIGHT_TOP, &time, egui::FontId::proportional(15.0), theme::PAPER);
    let title_clip = egui::Rect::from_min_max(inner.left_top(), egui::pos2(inner.right() - 90.0, inner.top() + 20.0));
    p.with_clip_rect(title_clip).text(inner.left_top(), egui::Align2::LEFT_TOP, title, egui::FontId::proportional(14.0), theme::PAPER);
    p.text(inner.left_top() + egui::vec2(0.0, 21.0), egui::Align2::LEFT_TOP, sub, egui::FontId::proportional(11.5), theme::PAPER_2);
    let chart = egui::Rect::from_min_max(egui::pos2(inner.left(), inner.bottom() - 16.0), inner.right_bottom());
    spark(&p, chart, &r.track.e, if sel { theme::BRASS } else { theme::PAPER_3 });
    resp.clicked()
}

/// Udział dystansu w klasach nachylenia (odcinki co ~100 m).
fn slope_shares(r: &Route) -> [f32; 4] {
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

fn slope_bar(ui: &mut egui::Ui, r: &Route) {
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
            ui.label(RichText::new(format!("{} {}%", crate::i18n::tr(NAMES[i]), fmt::dec(*s as f64 * 100.0, 0))).small().color(theme::PAPER_2));
        }
    });
}

/// Czy zdążysz przed zmrokiem, wychodząc o wybranej porze.
fn daylight(a: &Atlas, ui: &mut egui::Ui, r: &Route) {
    let (day, start) = a.when();
    let t = sun::times(day, a.cam.lat, a.cam.lon);
    let end = start + r.stats.time;
    section(ui, theme::icon::SUN_HORIZON, crate::i18n::tr("Plan dnia"));
    grid2(ui, vec![(crate::i18n::tr("Wyjście"), fmt::hour(start)), (crate::i18n::tr("Dojście"), fmt::hour(end.rem_euclid(24.0))), (crate::i18n::tr("Wschód słońca"), fmt::hour(t.rise)), (crate::i18n::tr("Zachód słońca"), fmt::hour(t.set))]);
    let spare = t.set - end;
    let (txt, col) = if !spare.is_finite() {
        (crate::i18n::tr("Nie da się wyliczyć zapasu światła").to_string(), theme::PAPER_3)
    } else if spare >= 2.0 {
        (tf!("{} zapasu przed zachodem", fmt::time(spare)), theme::OK)
    } else if spare >= 0.0 {
        (tf!("Tylko {} zapasu przed zachodem", fmt::time(spare)), theme::WARN)
    } else {
        (tf!("Dojście {} po zachodzie – weź czołówkę", fmt::time(-spare)), theme::SIGNAL)
    };
    pill(ui, &txt, col);
    theme::para(ui, crate::i18n::tr("Wyjście to pora ustawiona na zegarze u góry; zmienisz ją w menu Słońce."));
}

/// Szczyty, przełęcze i schroniska mijane po drodze, z czasem dojścia.
fn on_route(a: &mut Atlas, ui: &mut egui::Ui, r: &Route) {
    let tr = &r.track;
    let mut hits: Vec<(usize, usize)> = Vec::new();
    for (pi, p) in a.snap.pois.iter().enumerate().filter(|(_, p)| matches!(p.poi.kind, PoiKind::Peak | PoiKind::Saddle | PoiKind::Hut | PoiKind::Shelter)).take(800) {
        let best = (0..tr.len()).step_by(3).map(|i| (haversine(tr.lon[i], tr.lat[i], p.poi.lon, p.poi.lat), i)).min_by(|x, y| x.0.total_cmp(&y.0));
        if let Some((d, i)) = best {
            if d < 90.0 {
                hits.push((i, pi));
            }
        }
    }
    if hits.is_empty() {
        return;
    }
    hits.sort();
    section(ui, theme::icon::MAP_PIN, &tf!("Po drodze ({})", hits.len()));
    for (i, pi) in hits.into_iter().take(12) {
        let q = a.snap.pois[pi].clone();
        let glyph = match q.poi.kind {
            PoiKind::Peak => theme::icon::MOUNTAINS,
            PoiKind::Saddle => theme::icon::ARROWS_VERTICAL,
            _ => theme::icon::HOUSE,
        };
        let eta = tr.t.get(i).map(|t| fmt::time(*t as f64)).unwrap_or_default();
        let clicked = ui.horizontal(|ui| {
            ui.label(RichText::new(glyph).size(15.0).color(theme::PAPER_3));
            row(ui, &q.poi.name, &eta, &fmt::ele(q.ele as f64))
        }).inner;
        if clicked {
            a.fly_to(q.poi.lon, q.poi.lat, None);
        }
    }
}

fn big(ui: &mut egui::Ui, v: String, sub: &str, color: Color32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(v).size(30.0).strong().color(color));
        ui.label(RichText::new(sub).color(theme::PAPER_3));
    });
}

/// Szkło pod obszarem (okno, panel, menu) z poprzedniej klatki: obraz pod nim jest rozmyty.
pub fn glass_area(a: &mut Atlas, ctx: &egui::Context, id: egui::Id, alpha: f32) {
    if let Some(r) = ctx.memory(|m| m.area_rect(id)) {
        a.glass.push((r, alpha));
    }
}

/// Widoczność kontrolek na mapie (autoukrywanie): 1 – widoczne, 0 – schowane.
pub fn controls_alpha(a: &Atlas, ctx: &egui::Context) -> f32 {
    if !a.prefs.autohide || a.search.is_some() || egui::Popup::is_any_open(ctx) {
        return 1.0;
    }
    let screen = ctx.screen_rect();
    let (idle, top) = ctx.input(|i| {
        let over = i.pointer.hover_pos().map_or(false, |p| {
            // nad paskiem u góry, nad panelem bocznym albo nad profilem – nic się nie chowa
            p.y < 80.0 || p.x > screen.right() - right_inset(a) || p.y > screen.bottom() - bottom_inset(a)
        });
        (i.pointer.time_since_last_movement(), over)
    });
    if top {
        return 1.0;
    }
    const HOLD: f32 = 2.2;
    const FADE: f32 = 0.35;
    if idle < HOLD {
        ctx.request_repaint_after(std::time::Duration::from_secs_f32(HOLD - idle + 0.01));
        return 1.0;
    }
    let t = ((idle - HOLD) / FADE).min(1.0);
    if t < 1.0 {
        ctx.request_repaint();
    }
    1.0 - t
}

/// Górny pasek na całą szerokość okna, ze szkła nad mapą: od lewej narzędzia (zaczyna je przycisk
/// zaznaczania), wyszukiwarka na środku, po prawej pora dnia, styl mapy, warstwy i `trailing`.
pub fn top_bar(a: &mut Atlas, ctx: &egui::Context, trailing: impl FnOnce(&mut Atlas, &mut egui::Ui)) {
    // zegar przy porze „teraz” ma iść także przy nieruchomym widoku
    ctx.request_repaint_after(std::time::Duration::from_secs(20));
    let alpha = controls_alpha(a, ctx);
    let screen = ctx.screen_rect();
    egui::Area::new(egui::Id::new("gorny-pasek"))
        .fixed_pos(screen.min)
        .order(egui::Order::Middle)
        .interactable(alpha > 0.05)
        .show(ctx, |ui| {
            ui.multiply_opacity(alpha);
            let bar = egui::Rect::from_min_size(screen.min, egui::vec2(screen.width(), TOP_BAR_H));
            let p = ui.painter();
            p.rect_filled(bar, 0.0, theme::GLASS_FILL);
            p.line_segment([bar.left_bottom(), bar.right_bottom()], egui::Stroke::new(1.0, theme::GLASS_EDGE));
            ui.allocate_rect(bar, egui::Sense::hover());
            a.glass.push((bar, alpha));
            let inner = bar.shrink2(egui::vec2(12.0, 7.0));
            let row = |ui: &mut egui::Ui, rect: egui::Rect, layout: egui::Layout, body: &mut dyn FnMut(&mut egui::Ui)| {
                ui.scope_builder(egui::UiBuilder::new().max_rect(rect).layout(layout), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    body(ui);
                });
            };
            row(ui, inner, egui::Layout::left_to_right(egui::Align::Center), &mut |ui| tools(a, ui));
            if a.search.is_none() {
                let w = (screen.width() * 0.28).clamp(220.0, 448.0);
                let r = egui::Rect::from_center_size(inner.center(), egui::vec2(w, inner.height()));
                row(ui, r, egui::Layout::left_to_right(egui::Align::Center), &mut |ui| search_field(a, ui));
            }
            let mut trailing = Some(trailing);
            row(ui, inner, egui::Layout::right_to_left(egui::Align::Center), &mut |ui| {
                if let Some(t) = trailing.take() {
                    t(a, ui);
                }
                ui.separator();
                layers_button(a, ui);
                style_button(a, ui);
                sun_button(a, ui);
            });
        });
}

/// Wysokość górnego paska.
pub const TOP_BAR_H: f32 = 56.0;
/// Wysokość paska stanu na dole mapy.
pub const STATUS_H: f32 = 24.0;

/// Odsunięcie od dołu okna: pasek stanu i profil trasy (gdy otwarty).
pub fn bottom_inset(a: &Atlas) -> f32 {
    STATUS_H + match (a.current_route().is_some(), a.prefs.profile_open) {
        (false, _) => 0.0,
        (true, true) => PROFILE_H + 20.0,
        (true, false) => PROFILE_BAR_H + 20.0,
    }
}

/// Pole wyszukiwarki; po kliknięciu okno wyników rozwija się w tym samym miejscu.
fn search_field(a: &mut Atlas, ui: &mut egui::Ui) {
    let w = (ui.ctx().screen_rect().width() * 0.28).clamp(220.0, 448.0);
    ui.allocate_ui(egui::vec2(w, 42.0), |ui| {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 42.0), egui::Sense::click());
        let p = ui.painter();
        p.rect_filled(rect, 12.0, if resp.hovered() { Color32::from_white_alpha(18) } else { Color32::from_white_alpha(8) });
        p.text(rect.left_center() + egui::vec2(16.0, 0.0), egui::Align2::LEFT_CENTER, theme::icon::MAGNIFYING_GLASS, egui::FontId::proportional(18.0), theme::PAPER_2);
        p.text(rect.left_center() + egui::vec2(44.0, 0.0), egui::Align2::LEFT_CENTER, crate::i18n::tr("Szukaj szczytu, szlaku, miejscowości"), egui::FontId::proportional(14.0), theme::PAPER_3);
        p.text(rect.right_center() - egui::vec2(14.0, 0.0), egui::Align2::RIGHT_CENTER, "/", egui::FontId::monospace(12.0), theme::PAPER_3);
        if resp.clicked() {
            a.search = Some(Default::default());
        }
    });
}

/// Zwykły tryb: klik zaznacza, bez narzędzia, soczewki i panoramy.
fn select_mode(a: &mut Atlas) {
    if a.tool == Tool::Route {
        a.end_planning();
    }
    a.tool = Tool::Explore;
    if a.lens_pin.is_some() {
        a.unpin_lens();
    }
    a.lens = Lens::None;
    if a.panorama.is_some() {
        a.exit_panorama();
    }
}

fn tools(a: &mut Atlas, ui: &mut egui::Ui) {
    let plain = a.tool == Tool::Explore && a.lens == Lens::None && a.panorama.is_none();
    if theme::icon_button(ui, theme::icon::CURSOR, plain).on_hover_text(crate::i18n::tr("Zaznaczanie – zwykły tryb (1)")).clicked() {
        select_mode(a);
    }
    if theme::icon_button(ui, theme::icon::COMPASS, a.prefs.panel_open && a.tab == Tab::Info).on_hover_text(crate::i18n::tr("Szczegóły okolicy (P)")).clicked() {
        a.prefs.panel_open = !(a.prefs.panel_open && a.tab == Tab::Info);
        a.tab = Tab::Info;
    }
    if theme::icon_button(ui, theme::icon::BOOKMARKS, a.prefs.panel_open && a.tab == Tab::Library).on_hover_text(crate::i18n::tr("Biblioteka: zapisane trasy, miejsca i widoki (B)")).clicked() {
        a.prefs.panel_open = !(a.prefs.panel_open && a.tab == Tab::Library);
        a.tab = Tab::Library;
    }
    ui.separator();
    if theme::icon_button(ui, theme::icon::PATH, a.tool == Tool::Route).on_hover_text(crate::i18n::tr("Planowanie trasy (2)")).clicked() {
        if a.tool == Tool::Route {
            a.end_planning();
        } else {
            a.tool = Tool::Route;
        }
        a.prefs.panel_open = true;
        a.tab = Tab::Info;
    }
    if theme::icon_button(ui, theme::icon::RULER, a.tool == Tool::Measure).on_hover_text(crate::i18n::tr("Pomiar i linia wzroku (3)")).clicked() {
        a.tool = if a.tool == Tool::Measure { Tool::Explore } else { Tool::Measure };
        a.prefs.panel_open = true;
        a.tab = Tab::Info;
    }
    ui.separator();
    if theme::icon_button(ui, theme::icon::WAVES, a.lens == Lens::Band).on_hover_text(crate::i18n::tr("Ta sama wysokość (H)")).clicked() {
        a.set_lens(Lens::Band);
    }
    if theme::icon_button(ui, theme::icon::PLUS_MINUS, a.lens == Lens::Rel).on_hover_text(crate::i18n::tr("Wysokość względna (W)")).clicked() {
        a.set_lens(Lens::Rel);
    }
    if theme::icon_button(ui, theme::icon::EYE, a.lens == Lens::Vis).on_hover_text(crate::i18n::tr("Co widać stąd (O)")).clicked() {
        a.set_lens(Lens::Vis);
    }
    ui.separator();
    if theme::icon_button(ui, theme::icon::PERSON_SIMPLE_WALK, a.panorama.is_some()).on_hover_text(crate::i18n::tr("Spacer ze środka widoku: W A S D, spacja – skok, prawy przycisk – rozglądanie, Esc – wyjście")).clicked() {
        if a.panorama.is_some() {
            a.exit_panorama();
        } else {
            let (lon, lat) = (a.cam.lon, a.cam.lat);
            a.enter_panorama(lon, lat);
        }
    }
}

/// Pora dnia: ikona słońca lub księżyca, godzina i data; rozwija panel światła.
fn sun_button(a: &mut Atlas, ui: &mut egui::Ui) {
    let (day, hour) = a.when();
    let custom = a.prefs.custom_time.is_some();
    let (_, alt) = a.sun();
    let glyph = if alt > 6.0 {
        theme::icon::SUN
    } else if alt > -6.0 {
        theme::icon::SUN_HORIZON
    } else {
        theme::icon::MOON_STARS
    };
    let accent = if custom { theme::BRASS } else { theme::PAPER };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(118.0, 42.0), egui::Sense::click());
    let p = ui.painter();
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    p.rect_filled(rect, 12.0, if open { Color32::from_rgba_unmultiplied(122, 176, 255, 40) } else if resp.hovered() { Color32::from_white_alpha(16) } else { Color32::TRANSPARENT });
    let sun_col = if alt > 6.0 { theme::WARN } else if alt > -6.0 { Color32::from_rgb(255, 150, 90) } else { Color32::from_rgb(160, 180, 255) };
    p.text(rect.left_center() + egui::vec2(18.0, 0.0), egui::Align2::CENTER_CENTER, glyph, egui::FontId::proportional(22.0), sun_col);
    p.text(rect.left_top() + egui::vec2(38.0, 5.0), egui::Align2::LEFT_TOP, fmt::hour(hour), egui::FontId::proportional(16.0), accent);
    p.text(rect.left_bottom() + egui::vec2(38.0, -5.0), egui::Align2::LEFT_BOTTOM, if custom { day.label() } else { crate::i18n::tr("teraz").into() }, egui::FontId::proportional(10.5), theme::PAPER_3);
    let resp = resp.on_hover_text(if custom { crate::i18n::tr("Pora ustawiona ręcznie") } else { crate::i18n::tr("Oświetlenie jak teraz") });
    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).frame(theme::glass().inner_margin(egui::Margin::same(14))).show(|ui| {
        ui.set_width(360.0);
        light_tab(a, ui);
    });
    let pid = egui::Popup::default_response_id(&resp);
    if egui::Popup::is_id_open(ui.ctx(), pid) {
        let ctx = ui.ctx().clone();
        glass_area(a, &ctx, pid, 1.0);
    }
}

/// Przycisk paska z ikoną, podpisem i podtytułem; rozwija panel z kafelkami.
#[allow(clippy::too_many_arguments)]
fn bar_menu(ui: &mut egui::Ui, glyph: &str, glyph_color: Color32, title: &str, sub: &str, w: f32, hint: &str, width: f32, body: impl FnOnce(&mut egui::Ui)) -> Option<egui::Rect> {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 42.0), egui::Sense::click());
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    let p = ui.painter();
    p.rect_filled(rect, 12.0, if open { Color32::from_rgba_unmultiplied(122, 176, 255, 40) } else if resp.hovered() { Color32::from_white_alpha(16) } else { Color32::TRANSPARENT });
    p.text(rect.left_center() + egui::vec2(18.0, 0.0), egui::Align2::CENTER_CENTER, glyph, egui::FontId::proportional(20.0), glyph_color);
    p.text(rect.left_top() + egui::vec2(38.0, 5.0), egui::Align2::LEFT_TOP, title, egui::FontId::proportional(14.0), theme::PAPER);
    p.text(rect.left_bottom() + egui::vec2(38.0, -5.0), egui::Align2::LEFT_BOTTOM, sub, egui::FontId::proportional(10.5), theme::PAPER_3);
    p.text(rect.right_center() - egui::vec2(10.0, 0.0), egui::Align2::RIGHT_CENTER, theme::icon::CARET_DOWN, egui::FontId::proportional(12.0), theme::PAPER_3);
    let resp = resp.on_hover_text(hint);
    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).frame(theme::glass().inner_margin(egui::Margin::same(14))).show(|ui| {
        ui.set_width(width);
        body(ui);
    });
    let pid = egui::Popup::default_response_id(&resp);
    if egui::Popup::is_id_open(ui.ctx(), pid) {
        ui.ctx().memory(|m| m.area_rect(pid))
    } else {
        None
    }
}

/// Styl mapy: osobny przycisk z bieżącym stylem.
fn style_button(a: &mut Atlas, ui: &mut egui::Ui) {
    let label = a.prefs.style.label();
    let shown = bar_menu(ui, style_glyph(a.prefs.style), theme::BRASS, crate::i18n::tr("Styl mapy"), label, 128.0, crate::i18n::tr("Styl mapy (M)"), 340.0, |ui| style_tab(a, ui));
    if let Some(r) = shown {
        a.glass.push((r, 1.0));
    }
}

/// Warstwy: osobny przycisk z liczbą włączonych.
fn layers_button(a: &mut Atlas, ui: &mut egui::Ui) {
    let p = &a.prefs;
    let n = [p.labels, p.trails, p.contours, p.km_grid, p.shadows, p.detail, p.snow, p.landcover].iter().filter(|v| **v).count();
    let shown = bar_menu(ui, theme::icon::STACK, theme::BRASS, crate::i18n::tr("Warstwy"), &tf!("włączone: {}", n), 118.0, crate::i18n::tr("Warstwy na mapie"), 380.0, |ui| layers_tab(a, ui));
    if let Some(r) = shown {
        a.glass.push((r, 1.0));
    }
}

fn style_glyph(st: Style) -> &'static str {
    match st {
        Style::Satellite => theme::icon::GLOBE_HEMISPHERE_EAST,
        Style::Terrain => theme::icon::MOUNTAINS,
        Style::Map => theme::icon::MAP_TRIFOLD,
        Style::Hypso => theme::icon::CHART_BAR,
        Style::Slope => theme::icon::ANGLE,
        Style::Aspect => theme::icon::COMPASS,
    }
}

/// Szerokość prawego panelu szczegółów.
const SIDEBAR_W: f32 = 440.0;

fn sidebar_open(a: &Atlas) -> bool {
    a.prefs.panel_open
}

/// Odsunięcie elementów przy prawej krawędzi, gdy panel szczegółów jest otwarty.
pub fn right_inset(a: &Atlas) -> f32 {
    if sidebar_open(a) { SIDEBAR_W } else { 0.0 }
}

/// Szczegóły w prawym panelu bocznym (zwęża widok mapy); wywoływać przed panelem środkowym.
pub fn sidebar(a: &mut Atlas, ctx: &egui::Context) {
    if !sidebar_open(a) {
        // schowany panel: uchwyt przy prawej krawędzi
        let alpha = controls_alpha(a, ctx);
        egui::Area::new(egui::Id::new("szczegoly-uchwyt"))
            .anchor(egui::Align2::RIGHT_CENTER, [-6.0, 0.0])
            .interactable(alpha > 0.05)
            .show(ctx, |ui| {
                ui.multiply_opacity(alpha);
                theme::glass().inner_margin(egui::Margin::symmetric(4, 10)).show(ui, |ui| {
                    if ui.small_button(theme::icon::CARET_LEFT).on_hover_text(crate::i18n::tr("Pokaż szczegóły (P)")).clicked() {
                        a.prefs.panel_open = true;
                        a.tab = Tab::Info;
                    }
                });
            });
        glass_area(a, ctx, egui::Id::new("szczegoly-uchwyt"), alpha);
        return;
    }
    // panel boczny przy prawej krawędzi, ze szkła: od górnego paska do dolnego (profil, pasek stanu)
    let screen = ctx.screen_rect();
    let height = (screen.height() - TOP_BAR_H - bottom_inset(a)).max(200.0);
    let alpha = controls_alpha(a, ctx);
    a.glass.push((egui::Rect::from_min_size(egui::pos2(screen.right() - SIDEBAR_W, screen.top() + TOP_BAR_H), egui::vec2(SIDEBAR_W, height)), alpha));
    egui::Area::new(egui::Id::new("szczegoly"))
        .fixed_pos([screen.right() - SIDEBAR_W, screen.top() + TOP_BAR_H])
        .order(egui::Order::Middle)
        .interactable(alpha > 0.05)
        .show(ctx, |ui| {
          ui.multiply_opacity(alpha);
          egui::Frame::new().fill(theme::GLASS_FILL).inner_margin(egui::Margin::symmetric(18, 16)).show(ui, |ui| {
            ui.set_width(SIDEBAR_W - 36.0);
            ui.set_height(height - 32.0);
            // większe pismo niż w szufladzie
            let st = ui.style_mut();
            st.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
            st.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
            st.text_styles.insert(egui::TextStyle::Heading, egui::FontId::proportional(27.0));
            st.text_styles.insert(egui::TextStyle::Monospace, egui::FontId::monospace(14.0));
            st.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(12.5));
            st.spacing.item_spacing.y = 7.0;
            ui.horizontal(|ui| {
                let (glyph, title) = match (a.tab, a.tool) {
                    (Tab::Library, _) => (theme::icon::BOOKMARKS, crate::i18n::tr("Biblioteka")),
                    (_, Tool::Route) => (theme::icon::PATH, crate::i18n::tr("Trasa")),
                    (_, Tool::Measure) => (theme::icon::RULER, crate::i18n::tr("Pomiar")),
                    (_, Tool::Explore) => (theme::icon::COMPASS, crate::i18n::tr("Szczegóły")),
                };
                ui.label(RichText::new(glyph).size(19.0).color(theme::BRASS));
                ui.label(RichText::new(title).size(17.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(theme::icon::CARET_RIGHT).on_hover_text(crate::i18n::tr("Ukryj panel (P)")).clicked() {
                        a.prefs.panel_open = false;
                    }
                    if a.tab == Tab::Info && a.tool == Tool::Route && ui.button(theme::ic(theme::icon::X, "Zakończ planowanie")).on_hover_text(crate::i18n::tr("Usuwa trasę z mapy i wraca do zwykłego trybu")).clicked() {
                        a.end_planning();
                    }
                });
            });
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                // nic nie rozpycha panelu poza jego szerokość
                ui.set_max_width(ui.available_width());
                if a.tab == Tab::Library {
                    crate::ui::library::show(a, ui);
                } else {
                    info(a, ui);
                }
            });
          });
        });
}


/// Sterowanie kamerą w prawym dolnym rogu.
pub fn nav(a: &mut Atlas, ctx: &egui::Context) {
    let alpha = controls_alpha(a, ctx);
    // nad profilem trasy, gdy jest otwarty
    egui::Area::new(egui::Id::new("kamera"))
        .anchor(egui::Align2::RIGHT_BOTTOM, [-16.0 - right_inset(a), -16.0 - bottom_inset(a)])
        .interactable(alpha > 0.05)
        .show(ctx, |ui| {
            ui.multiply_opacity(alpha);
            theme::glass().show(ui, |ui| {
                ui.vertical(|ui| {
                    let zoom = |a: &mut Atlas, k: f64| {
                        let mut to = a.cam;
                        to.distance = (a.cam.distance * k).clamp(a.min_dist(), 900_000.0);
                        a.start_fly(to, 0.35);
                    };
                    if theme::icon_button(ui, theme::icon::PLUS, false).on_hover_text(crate::i18n::tr("Przybliż")).clicked() {
                        zoom(a, 0.55);
                    }
                    if theme::icon_button(ui, theme::icon::MINUS, false).on_hover_text(crate::i18n::tr("Oddal")).clicked() {
                        zoom(a, 1.8);
                    }
                    ui.separator();
                    let full = ui.ctx().input(|i| i.viewport().fullscreen.unwrap_or(false));
                    if theme::icon_button(ui, if full { theme::icon::CORNERS_IN } else { theme::icon::CORNERS_OUT }, full).on_hover_text(crate::i18n::tr("Pełny ekran (F11)")).clicked() {
                        let ctx = ui.ctx().clone();
                        a.toggle_fullscreen(&ctx);
                    }
                    if theme::icon_button(ui, theme::icon::PERSPECTIVE, a.cam.pitch < 75.0).on_hover_text(crate::i18n::tr("Widok ukośny / z góry (V)")).clicked() {
                        let mut to = a.cam;
                        to.pitch = if a.cam.pitch > 75.0 { 40.0 } else { 89.0 };
                        a.start_fly(to, 0.7);
                    }
                });
            });
        });
    glass_area(a, ctx, egui::Id::new("kamera"), alpha);
}

fn info(a: &mut Atlas, ui: &mut egui::Ui) {
    if a.lens_pin.is_some() {
        return lens_info(a, ui);
    }
    match a.tool {
        Tool::Route => return route_info(a, ui),
        Tool::Measure => return measure_info(a, ui),
        Tool::Explore => {}
    }
    match a.selection.clone() {
        Selection::Poi(i) => poi_info(a, ui, i),
        Selection::Point { lon, lat } => point_info(a, ui, lon, lat),
        Selection::Trail(e) => trail_info(a, ui, e),
        Selection::None => region_info(a, ui),
    }
}

fn region_info(a: &mut Atlas, ui: &mut egui::Ui) {
    ui.label(theme::kicker(&tf!("Okolica · {}", fmt::decimal(a.cam.lon, a.cam.lat))));
    let name = a
        .snap
        .pois
        .iter()
        .filter(|p| p.poi.kind == PoiKind::Peak)
        .min_by(|x, y| {
            haversine(a.cam.lon, a.cam.lat, x.poi.lon, x.poi.lat).total_cmp(&haversine(a.cam.lon, a.cam.lat, y.poi.lon, y.poi.lat))
        })
        .map(|p| tf!("Okolice: {}", p.poi.name))
        .unwrap_or_else(|| crate::i18n::tr("Atlas szlaków").into());
    ui.heading(name);
    let g = &a.snap.graph;
    if g.edges.is_empty() {
        let (busy, err) = a.world.status();
        theme::para(ui, if busy.is_some() {
            crate::i18n::tr("Pobieram szlaki i punkty tej okolicy z OpenStreetMap…")
        } else if a.cam.distance > 90_000.0 {
            crate::i18n::tr("Przybliż widok, żeby wczytać szlaki i punkty okolicy.")
        } else if let Some(e) = &err {
            e.as_str()
        } else {
            crate::i18n::tr("W tej okolicy nie ma znakowanych szlaków w OpenStreetMap.")
        });
    } else {
        grid2(ui, vec![
            (crate::i18n::tr("Szlaki"), fmt::dist(g.total_km * 1000.0)),
            (crate::i18n::tr("Trasy PTTK i inne"), fmt::int(g.routes.len() as f64)),
            (crate::i18n::tr("Punkty"), fmt::int(a.snap.pois.len() as f64)),
            (crate::i18n::tr("Sektory"), a.snap.sectors.len().to_string()),
        ]);
    }
    ui.add_space(10.0);
    ui.label(theme::kicker(crate::i18n::tr("Najbardziej wybitne szczyty")));
    let peaks: Vec<(usize, String, f32)> = a
        .snap
        .pois
        .iter()
        .enumerate()
        .filter(|(_, p)| p.poi.kind == PoiKind::Peak)
        .take(14)
        .map(|(i, p)| (i, p.poi.name.clone(), p.ele))
        .collect();
    for (i, n, e) in peaks {
        if row(ui, &n, &fmt::ele(e as f64), "") {
            let p = &a.snap.pois[i].poi;
            let (lon, lat) = (p.lon, p.lat);
            a.selection = Selection::Poi(i);
            a.fly_to(lon, lat, Some(a.cam.distance.clamp(2500.0, 9000.0)));
        }
    }
    if let Some(r) = a.gpx.clone() {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label(theme::kicker(crate::i18n::tr("Wczytany ślad GPX")));
        });
        let s = r.stats;
        ui.label(RichText::new(r.labels[0].clone()).strong());
        grid2(ui, vec![(crate::i18n::tr("Dystans"), fmt::dist(s.len2)), (crate::i18n::tr("Czas"), fmt::time(s.time)), (crate::i18n::tr("Podejścia"), fmt::ele(s.up)), (crate::i18n::tr("Zejścia"), fmt::ele(s.down))]);
        ui.horizontal(|ui| {
            if ui.button(crate::i18n::tr("Pokaż")).clicked() {
                a.fit_route(&r);
            }
            if ui.button(crate::i18n::tr("Usuń")).clicked() {
                a.gpx = None;
            }
        });
    }
    ui.add_space(10.0);
    if theme::row_button(ui, theme::icon::LINK, crate::i18n::tr("Kopiuj link do widoku"), false).clicked() {
        let l = a.share_link();
        ui.ctx().copy_text(l);
        a.notify(crate::i18n::tr("Skopiowano link do widoku – wklej go do wyszukiwarki GRANI albo otwórz w systemie."));
    }
    if theme::row_button(ui, theme::icon::BOOKMARK_SIMPLE, crate::i18n::tr("Zapisz ten widok"), false).on_hover_text(crate::i18n::tr("Zakładka w bibliotece (Ctrl+D)")).clicked() {
        a.save_view();
    }
    if theme::row_button(ui, theme::icon::FILE_ARROW_UP, crate::i18n::tr("Wczytaj ślad (GPX, KML, GeoJSON)…"), false).on_hover_text(crate::i18n::tr("albo przeciągnij plik na mapę")).clicked() {
        if let Some(p) = pick_track_file() {
            a.import_track(&p);
        }
    }
}

fn row(ui: &mut egui::Ui, name: &str, right: &str, sub: &str) -> bool {
    let r = ui
        .horizontal(|ui| {
            ui.set_width(ui.available_width());
            let l = ui.add(egui::Label::new(RichText::new(name).strong()).sense(egui::Sense::click()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(right).monospace().color(theme::PAPER_2));
                if !sub.is_empty() {
                    ui.label(RichText::new(sub).small().color(theme::PAPER_3));
                }
            });
            l.clicked()
        })
        .inner;
    r
}

fn point_buttons(a: &mut Atlas, ui: &mut egui::Ui, lon: f64, lat: f64) {
    if theme::row_button(ui, theme::icon::FLAG, crate::i18n::tr("Trasa stąd"), false).clicked() {
        a.tool = Tool::Route;
        a.add_waypoint(lon, lat, Some(0));
    }
    if theme::row_button(ui, theme::icon::MAP_PIN, crate::i18n::tr("Trasa tutaj"), false).clicked() {
        a.tool = Tool::Route;
        a.add_waypoint(lon, lat, None);
    }
    if theme::row_button(ui, theme::icon::EYE, crate::i18n::tr("Co widać stąd?"), false).clicked() {
        a.pin_lens(lon, lat, Lens::Vis);
    }
    if theme::row_button(ui, theme::icon::PLUS_MINUS, crate::i18n::tr("Wysokość względna"), false).clicked() {
        a.pin_lens(lon, lat, Lens::Rel);
    }
    if theme::row_button(ui, theme::icon::WAVES, crate::i18n::tr("Ta sama wysokość"), false).clicked() {
        a.pin_lens(lon, lat, Lens::Band);
    }
    if theme::row_button(ui, theme::icon::PERSON_SIMPLE_WALK, crate::i18n::tr("Spacer stąd"), false).clicked() {
        a.enter_panorama(lon, lat);
    }
    if theme::row_button(ui, theme::icon::STAR, crate::i18n::tr("Dodaj do ulubionych"), false).clicked() {
        let name = a.selection_name();
        a.save_place(lon, lat, name);
    }
    if theme::row_button(ui, theme::icon::COPY, crate::i18n::tr("Kopiuj współrzędne"), false).clicked() {
        ui.ctx().copy_text(fmt::decimal(lon, lat));
        a.notify(crate::i18n::tr("Skopiowano współrzędne."));
    }
}

fn close_row(a: &mut Atlas, ui: &mut egui::Ui, kicker: &str) {
    ui.horizontal(|ui| {
        ui.label(theme::kicker(kicker));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button(theme::icon::X).clicked() {
                a.selection = Selection::None;
            }
        });
    });
}

fn poi_info(a: &mut Atlas, ui: &mut egui::Ui, i: usize) {
    let Some(p) = a.snap.pois.get(i).cloned() else {
        a.selection = Selection::None;
        return;
    };
    close_row(a, ui, p.poi.kind.label());
    ui.heading(&p.poi.name);
    let mut items = vec![(crate::i18n::tr("Wysokość"), fmt::ele(p.ele as f64)), (crate::i18n::tr("Model terenu"), fmt::ele(p.dem as f64))];
    if p.poi.kind == PoiKind::Place && p.poi.pop > 0 {
        items.push((crate::i18n::tr("Mieszkańcy"), fmt::int(p.poi.pop as f64)));
    }
    items.push((crate::i18n::tr("Odległość"), fmt::dist(haversine(a.cam.lon, a.cam.lat, p.poi.lon, p.poi.lat))));
    grid2(ui, items);
    ui.label(RichText::new(fmt::coords(p.poi.lon, p.poi.lat)).monospace().small().color(theme::PAPER_3));
    if p.poi.ele.is_some() && (p.ele - p.dem).abs() > 25.0 {
        theme::para(ui, crate::i18n::tr("Model terenu wygładza najostrzejsze granie – wysokość szczytu pochodzi z OSM."));
    }
    ui.horizontal(|ui| {
        if !p.poi.osm.is_empty() && ui.link("OpenStreetMap").clicked() {
            let _ = open::that(format!("https://www.openstreetmap.org/{}", p.poi.osm));
        }
        if !p.poi.wiki.is_empty() {
            if let Some((lang, title)) = p.poi.wiki.split_once(':') {
                if ui.link(crate::i18n::tr("Wikipedia")).clicked() {
                    let _ = open::that(format!("https://{lang}.wikipedia.org/wiki/{}", crate::osm::urlencode(title)));
                }
            }
        }
    });
    ui.add_space(6.0);
    point_buttons(a, ui, p.poi.lon, p.poi.lat);
    if p.poi.kind == PoiKind::Peak {
        ui.add_space(10.0);
        ui.label(theme::kicker(crate::i18n::tr("Sąsiednie szczyty")));
        let mut near: Vec<(usize, f64)> = a
            .snap
            .pois
            .iter()
            .enumerate()
            .filter(|(j, q)| *j != i && q.poi.kind == PoiKind::Peak)
            .map(|(j, q)| (j, haversine(p.poi.lon, p.poi.lat, q.poi.lon, q.poi.lat)))
            .filter(|(_, d)| *d < 6000.0)
            .collect();
        near.sort_by(|x, y| x.1.total_cmp(&y.1));
        for (j, d) in near.into_iter().take(8) {
            let q = a.snap.pois[j].clone();
            let dir = fmt::compass(bearing(p.poi.lon, p.poi.lat, q.poi.lon, q.poi.lat));
            if row(ui, &q.poi.name, &fmt::ele(q.ele as f64), &format!("{} {}", fmt::dist(d), dir)) {
                a.selection = Selection::Poi(j);
                a.fly_to(q.poi.lon, q.poi.lat, None);
            }
        }
    }
}

fn point_info(a: &mut Atlas, ui: &mut egui::Ui, lon: f64, lat: f64) {
    close_row(a, ui, crate::i18n::tr("Punkt terenu"));
    let e = a.heights.sample_cached(lon, lat).unwrap_or(0.0) as f64;
    ui.heading(fmt::ele(e));
    let g = |dx: f64, dy: f64| a.heights.sample_cached(lon + dx, lat + dy).map(|v| v as f64).unwrap_or(e);
    let kx = 111_320.0 * lat.to_radians().cos();
    let st = 15.0;
    let gx = (g(st / kx, 0.0) - g(-st / kx, 0.0)) / (2.0 * st);
    let gy = (g(0.0, st / 110_574.0) - g(0.0, -st / 110_574.0)) / (2.0 * st);
    let slope = gx.hypot(gy).atan().to_degrees();
    let aspect = (-gx).atan2(-gy).to_degrees().rem_euclid(360.0);
    grid2(ui, vec![
        (crate::i18n::tr("Nachylenie"), format!("{}°", fmt::dec(slope, 0))),
        (crate::i18n::tr("Ekspozycja"), format!("{} ({}°)", fmt::compass(aspect), aspect.round())),
        (crate::i18n::tr("Szerokość"), format!("{lat:.5}°")),
        (crate::i18n::tr("Długość"), format!("{lon:.5}°")),
    ]);
    let (day, _) = a.when();
    let t = sun::times(day, lat, lon);
    section(ui, theme::icon::SUN, crate::i18n::tr("Słońce w tym miejscu"));
    grid2(ui, vec![(crate::i18n::tr("Wschód"), fmt::hour(t.rise)), (crate::i18n::tr("Zachód"), fmt::hour(t.set)), (crate::i18n::tr("Górowanie"), format!("{} · {}°", fmt::hour(t.noon), t.max_alt.round())), (crate::i18n::tr("Granica śniegu"), fmt::ele(sun::snowline(day)))]);
    ui.label(RichText::new(fmt::coords(lon, lat)).monospace().small().color(theme::PAPER_3));
    if let Some(n) = a.name_near(lon, lat, 400.0) {
        ui.label(RichText::new(tf!("W pobliżu: {}", n)).color(theme::PAPER_2));
    }
    ui.add_space(6.0);
    point_buttons(a, ui, lon, lat);
}

fn trail_info(a: &mut Atlas, ui: &mut egui::Ui, e: u32) {
    let snap = a.snap.clone();
    let Some(ed) = snap.graph.edges.get(e as usize) else {
        a.selection = Selection::None;
        return;
    };
    close_row(a, ui, crate::i18n::tr("Szlak turystyczny"));
    let routes: Vec<crate::osm::RouteInfo> = ed.routes.iter().filter_map(|&r| snap.graph.routes.get(r as usize).cloned()).collect();
    let title = routes.first().map(|r| r.name.clone()).unwrap_or_else(|| if ed.name.is_empty() { crate::i18n::tr("Odcinek szlaku").into() } else { ed.name.clone() });
    ui.heading(title);
    for r in &routes {
        ui.horizontal(|ui| {
            let c = crate::metrics::TrailColor::parse(&r.color).rgb();
            let (rect, _) = ui.allocate_exact_size(egui::vec2(26.0, 7.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 3.0, Color32::from_rgb(c[0], c[1], c[2]));
            ui.label(RichText::new(&r.name).strong());
            if !r.r#ref.is_empty() {
                ui.label(RichText::new(&r.r#ref).small().color(theme::PAPER_3));
            }
            if ui.small_button("OSM").clicked() {
                let _ = open::that(format!("https://www.openstreetmap.org/relation/{}", r.id));
            }
        });
    }
    grid2(ui, vec![
        (crate::i18n::tr("Odcinek"), fmt::dist(ed.len2)),
        (crate::i18n::tr("Po terenie"), fmt::dist(ed.len3)),
        (crate::i18n::tr("Podejścia"), fmt::ele(ed.up)),
        (crate::i18n::tr("Zejścia"), fmt::ele(ed.down)),
        (crate::i18n::tr("Skala SAC"), crate::i18n::tr(SAC_LABEL[ed.sac.min(6) as usize]).to_string()),
        (crate::i18n::tr("Czas (tam)"), fmt::time(metrics::walk_time(ed.len2, ed.up, ed.down) * metrics::sac_factor(ed.sac))),
        (crate::i18n::tr("Czas (z powrotem)"), fmt::time(metrics::walk_time(ed.len2, ed.down, ed.up) * metrics::sac_factor(ed.sac))),
        (crate::i18n::tr("Śr. nachylenie"), format!("{}%", fmt::dec((ed.up + ed.down) / ed.len2.max(1.0) * 100.0, 0))),
    ]);
    let route_ids = ed.routes.clone();
    ui.horizontal(|ui| {
        if ui.button(crate::i18n::tr("Pokaż cały szlak")).clicked() {
            let edges: Vec<u32> = a
                .snap
                .graph
                .edges
                .iter()
                .enumerate()
                .filter(|(_, x)| x.routes.iter().any(|r| route_ids.contains(r)))
                .map(|(i, _)| i as u32)
                .collect();
            if let Some(b) = crate::ui::search::edges_bbox(a, &edges) {
                a.fit_bbox(&b);
            }
        }
    });
}

fn time_model(a: &mut Atlas, ui: &mut egui::Ui) {
    let (mut m, mut pace) = metrics::model();
    let before = (m, pace);
    ui.horizontal(|ui| {
        ui.selectable_value(&mut m, TimeModel::Pttk, crate::i18n::tr("reguła PTTK"));
        ui.selectable_value(&mut m, TimeModel::Din, "DIN 33466");
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new(crate::i18n::tr("Tempo")).color(theme::PAPER_3));
        ui.selectable_value(&mut pace, 0.8, crate::i18n::tr("szybkie"));
        ui.selectable_value(&mut pace, 1.0, crate::i18n::tr("normalne"));
        ui.selectable_value(&mut pace, 1.25, crate::i18n::tr("spokojne"));
    });
    if (m, pace) != before {
        metrics::set_model(m, pace);
        a.prefs.din = m == TimeModel::Din;
        a.prefs.pace = pace;
        a.mark_route_dirty();
    }
}

fn route_info(a: &mut Atlas, ui: &mut egui::Ui) {
    ui.label(theme::kicker(crate::i18n::tr("Planowanie trasy")));
    ui.heading(if a.route_mode == RouteMode::Trails { crate::i18n::tr("Trasa szlakami") } else { crate::i18n::tr("Trasa przez teren") });
    let before = a.route_mode;
    ui.horizontal(|ui| {
        ui.selectable_value(&mut a.route_mode, RouteMode::Trails, theme::ic(theme::icon::SIGNPOST, "Szlakami"));
        ui.selectable_value(&mut a.route_mode, RouteMode::Terrain, theme::ic(theme::icon::MOUNTAINS, "Przez teren"))
            .on_hover_text(crate::i18n::tr("A* po modelu terenu z funkcją Toblera i limitem nachylenia – analiza rzeźby, nie zachęta do schodzenia ze szlaków w parkach narodowych."));
    });
    if before != a.route_mode {
        a.mark_route_dirty();
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(theme::kicker(crate::i18n::tr("Punkty")));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !a.waypoints.is_empty() {
                if ui.small_button(crate::i18n::tr("Wyczyść")).clicked() {
                    a.clear_route();
                }
                if ui.small_button(crate::i18n::tr("Odwróć")).clicked() {
                    a.reverse_route();
                }
            }
            if ui.add_enabled(a.can_redo(), egui::Button::new(theme::icon::ARROW_CLOCKWISE).small()).on_hover_text(crate::i18n::tr("Ponów (Ctrl+Y)")).clicked() {
                a.redo_route();
            }
            if ui.add_enabled(a.can_undo(), egui::Button::new(theme::icon::ARROW_COUNTER_CLOCKWISE).small()).on_hover_text(crate::i18n::tr("Cofnij (Ctrl+Z)")).clicked() {
                a.undo_route();
            }
        });
    });
    if a.waypoints.is_empty() {
        theme::para(ui, crate::i18n::tr("Klikaj na mapie, żeby dodać punkty trasy (start, przez, cel). Punkty można przeciągać; prawy przycisk daje więcej opcji."));
    }
    let mut remove = None;
    let n = a.waypoints.len();
    for (i, w) in a.waypoints.iter().enumerate() {
        ui.horizontal(|ui| {
            let tag = if i == 0 { "A".to_string() } else if i == n - 1 { "B".to_string() } else { i.to_string() };
            let (badge, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
            ui.painter().circle_filled(badge.center(), 11.0, theme::BRASS_DIM);
            ui.painter().text(badge.center(), egui::Align2::CENTER_CENTER, tag, egui::FontId::proportional(12.0), theme::BRASS);
            ui.label(w.name.clone().unwrap_or_else(|| fmt::decimal(w.lon, w.lat)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(theme::icon::X).clicked() {
                    remove = Some(i);
                }
                ui.label(RichText::new(fmt::ele(w.e as f64)).monospace().small().color(theme::PAPER_3));
            });
        });
    }
    if let Some(i) = remove {
        a.remove_waypoint(i);
    }
    section(ui, theme::icon::CLOCK, crate::i18n::tr("Czas przejścia"));
    time_model(a, ui);
    if a.routing {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new(crate::i18n::tr("wyznaczam trasę…")).color(theme::PAPER_3));
        });
    }
    if let Some(e) = &a.route_error {
        ui.label(RichText::new(e).color(theme::WARN));
    }
    if !a.routes.is_empty() {
        section(ui, theme::icon::ARROWS_SPLIT, &tf!("Warianty ({})", a.routes.len()));
        let base_t = a.routes[0].stats.time;
        let routes = a.routes.clone();
        for (i, r) in routes.iter().enumerate() {
            let d = r.stats.time - base_t;
            let diff = if i > 0 && d.abs() > 0.04 { format!(" · {}{}", if d > 0.0 { "+" } else { "−" }, fmt::time(d.abs())) } else { String::new() };
            let sub = tf!("{} · ↑ {} · ↓ {}{}", fmt::dist(r.stats.len2), fmt::ele(r.stats.up), fmt::ele(r.stats.down), diff);
            if variant_card(ui, r, &r.labels.join(" · "), &sub, i == a.route_sel) {
                a.route_sel = i;
            }
        }
    }
    if let Some(r) = a.current_route().cloned() {
        let s = r.stats;
        section(ui, theme::icon::CHART_LINE, crate::i18n::tr("Szczegóły"));
        pill(ui, crate::i18n::tr(DIFFICULTY[s.difficulty.min(3)]), DIFF_COLORS[s.difficulty.min(3)]);
        ui.add_space(2.0);
        let mut items = vec![
            (crate::i18n::tr("Czas"), fmt::time(s.time)),
            (crate::i18n::tr("Dystans"), fmt::dist(s.len2)),
            (crate::i18n::tr("Podejścia"), fmt::ele(s.up)),
            (crate::i18n::tr("Zejścia"), fmt::ele(s.down)),
            (crate::i18n::tr("Najwyżej"), fmt::ele(s.max_ele)),
            (crate::i18n::tr("Najniżej"), fmt::ele(s.min_ele)),
            (crate::i18n::tr("Po powierzchni"), fmt::dist(s.len3)),
            (crate::i18n::tr("Linia prosta"), fmt::dist(s.straight)),
            (crate::i18n::tr("Maks. nachylenie"), format!("{}°", s.max_slope.round())),
            (crate::i18n::tr("Śr. tempo"), tf!("{} km/h", fmt::dec(s.len3 / 1000.0 / s.time.max(0.01), 1))),
            (crate::i18n::tr("Krętość"), format!("×{}", fmt::dec(s.len2 / s.straight.max(1.0), 2))),
        ];
        if r.kind == RouteKind::Trail {
            items.push((crate::i18n::tr("Punkty GOT"), fmt::int(s.got)));
            items.push((crate::i18n::tr("Skala SAC"), crate::i18n::tr(SAC_LABEL[s.sac_max.min(6) as usize]).split(' ').next().unwrap_or("—").to_string()));
        }
        grid2(ui, items);
        section(ui, theme::icon::ANGLE, crate::i18n::tr("Stromizna trasy"));
        slope_bar(ui, &r);
        daylight(a, ui, &r);
        on_route(a, ui, &r);
        if !r.route_refs.is_empty() {
            section(ui, theme::icon::SIGNPOST, crate::i18n::tr("Szlaki na trasie"));
            for &ri in r.route_refs.iter().take(10) {
                if let Some(info) = a.snap.graph.routes.get(ri as usize) {
                    ui.horizontal(|ui| {
                        let c = crate::metrics::TrailColor::parse(&info.color).rgb();
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 6.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 3.0, Color32::from_rgb(c[0], c[1], c[2]));
                        ui.label(RichText::new(&info.name).small());
                    });
                }
            }
        }
    }
    if a.current_route().is_some() {
        section(ui, theme::icon::LIGHTNING, crate::i18n::tr("Działania"));
        if theme::row_button(ui, theme::icon::ARROWS_OUT, crate::i18n::tr("Pokaż całą"), false).clicked() {
            if let Some(r) = a.current_route().cloned() {
                a.fit_route(&r);
            }
        }
        let profile_open = a.prefs.profile_open;
        if theme::row_button(ui, theme::icon::CHART_LINE, crate::i18n::tr("Profil wysokości"), profile_open).clicked() {
            a.prefs.profile_open = !profile_open;
        }
        if theme::row_button(ui, theme::icon::LINK, crate::i18n::tr("Kopiuj link"), false).on_hover_text(crate::i18n::tr("Link gran:// z widokiem i punktami trasy")).clicked() {
            let l = a.share_link();
            ui.ctx().copy_text(l);
            a.notify(crate::i18n::tr("Skopiowano link z trasą."));
        }
        if theme::row_button(ui, theme::icon::BOOKMARKS, crate::i18n::tr("Zapisz w bibliotece"), false).on_hover_text("Ctrl+S").clicked() {
            a.save_route_to_library();
        }
        if theme::row_button(ui, theme::icon::EXPORT, crate::i18n::tr("Eksport (GPX, KML, GeoJSON)"), false).on_hover_text("G").clicked() {
            a.export_route();
        }
        let flying = a.flight.is_some();
        if theme::row_button(ui, theme::icon::AIRPLANE_TILT, if flying { crate::i18n::tr("Zatrzymaj przelot") } else { crate::i18n::tr("Przelot") }, flying).on_hover_text("F").clicked() {
            a.toggle_flight();
        }
    }
    ui.add_space(8.0);
    if theme::row_button(ui, theme::icon::FILE_ARROW_UP, crate::i18n::tr("Wczytaj ślad (GPX, KML, GeoJSON)…"), false).clicked() {
        if let Some(p) = pick_track_file() {
            a.import_track(&p);
        }
    }
}

fn measure_info(a: &mut Atlas, ui: &mut egui::Ui) {
    ui.label(theme::kicker(crate::i18n::tr("Pomiar i linia wzroku")));
    ui.heading(crate::i18n::tr("Odległość po terenie"));
    if a.measure.is_empty() {
        theme::para(ui, crate::i18n::tr("Klikaj na mapie, żeby dodać punkty pomiaru. Dla dwóch punktów program sprawdzi też, czy jeden widać z drugiego."));
        return;
    }
    ui.horizontal(|ui| {
        ui.label(theme::kicker(&tf!("Pomiar · {} pkt", a.measure.len())));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button(crate::i18n::tr("Wyczyść")).clicked() {
                a.measure.clear();
                a.los = None;
            }
            if ui.small_button(crate::i18n::tr("Cofnij")).clicked() {
                a.measure.pop();
                a.los = None;
            }
        });
    });
    if a.measure.is_empty() {
        return;
    }
    let (mut d2, mut d3) = (0.0, 0.0);
    for w in a.measure.windows(2) {
        let n = (haversine(w[0].0, w[0].1, w[1].0, w[1].1) / 20.0).ceil().clamp(2.0, 2000.0) as usize;
        let mut prev = (w[0].0, w[0].1, a.heights.sample_cached(w[0].0, w[0].1).unwrap_or(w[0].2) as f64);
        for k in 1..=n {
            let f = k as f64 / n as f64;
            let lon = w[0].0 + (w[1].0 - w[0].0) * f;
            let lat = w[0].1 + (w[1].1 - w[0].1) * f;
            let e = a.heights.sample_cached(lon, lat).unwrap_or(0.0) as f64;
            let h = haversine(prev.0, prev.1, lon, lat);
            d2 += h;
            d3 += h.hypot(e - prev.2);
            prev = (lon, lat, e);
        }
    }
    let first = a.measure[0];
    let last = *a.measure.last().unwrap();
    big(ui, fmt::dist(d3), crate::i18n::tr("po powierzchni"), theme::PAPER);
    grid2(ui, vec![
        (crate::i18n::tr("W rzucie"), fmt::dist(d2)),
        (crate::i18n::tr("Różnica wysokości"), fmt::signed((last.2 - first.2) as f64, "m")),
        (crate::i18n::tr("Azymut"), format!("{}° {}", bearing(first.0, first.1, last.0, last.1).round(), fmt::compass(bearing(first.0, first.1, last.0, last.1)))),
        (crate::i18n::tr("Linia prosta"), fmt::dist(haversine(first.0, first.1, last.0, last.1))),
    ]);
    if a.measure.len() == 2 {
        ui.add_space(8.0);
        ui.label(theme::kicker(crate::i18n::tr("Linia wzroku (oczy 1,7 m, krzywizna Ziemi)")));
        match a.los {
            Some((None, _)) => {
                big(ui, crate::i18n::tr("Widać").into(), "", theme::OK);
                theme::para(ui, crate::i18n::tr("Z punktu 1 widać punkt 2 – teren nie przesłania linii wzroku."));
            }
            Some((Some(f), d)) => {
                big(ui, crate::i18n::tr("Nie widać").into(), "", theme::SIGNAL);
                theme::para(ui, tf!("Teren przesłania linię wzroku po {} od punktu 1.", fmt::dist(f * d)));
            }
            None => {
                ui.spinner();
            }
        }
    }
}

fn lens_info(a: &mut Atlas, ui: &mut egui::Ui) {
    let Some((lon, lat, e)) = a.lens_pin else { return };
    let mut unpin = false;
    match a.lens {
        Lens::Vis => {
            ui.label(theme::kicker(crate::i18n::tr("Analiza widoczności")));
            ui.heading(crate::i18n::tr("Co widać stąd?"));
            ui.label(RichText::new(format!("{} · {}", fmt::decimal(lon, lat), fmt::ele(e as f64))).monospace().small().color(theme::PAPER_3));
            ui.label(theme::kicker(crate::i18n::tr("Wysokość obserwatora nad terenem")));
            let before = a.prefs.vs_eye;
            ui.horizontal(|ui| {
                for v in [1.7f32, 10.0, 30.0, 100.0] {
                    ui.selectable_value(&mut a.prefs.vs_eye, v, format!("{} m", fmt::dec(v as f64, if v < 2.0 { 1 } else { 0 })));
                }
            });
            if before != a.prefs.vs_eye {
                a.start_viewshed(lon, lat);
            }
            if a.vs_busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(crate::i18n::tr("liczę promienie widzenia…")).color(theme::PAPER_3));
                });
            }
            if let Some(v) = &a.vs {
                big(ui, fmt::dec(v.area_km2, 1), crate::i18n::tr("km² widocznych"), theme::TEAL);
                theme::para(ui, tf!("w promieniu 32 km · uwzględnia krzywiznę Ziemi i refrakcję · oko {} m n.p.m.", fmt::int(v.observer as f64)));
                ui.add_space(6.0);
                ui.label(theme::kicker(&tf!("Widoczne szczyty i schroniska ({})", v.visible.len())));
                let list: Vec<(usize, f64)> = v.visible.iter().take(40).copied().collect();
                for (i, d) in list {
                    let Some(p) = a.snap.pois.get(i).cloned() else { continue };
                    if row(ui, &p.poi.name, &fmt::ele(p.ele as f64), &fmt::dist(d)) {
                        a.selection = Selection::Poi(i);
                    }
                }
            }
        }
        Lens::Rel => {
            ui.label(theme::kicker(crate::i18n::tr("Wysokość względna")));
            ui.heading(crate::i18n::tr("Co jest wyżej?"));
            theme::para(ui, tf!("Poziom 0: {} · {}", fmt::ele(e as f64), fmt::decimal(lon, lat)));
            ui.add(egui::Slider::new(&mut a.prefs.rel_range, 50.0..=1500.0).text(crate::i18n::tr("zakres barw (m)")).logarithmic(true));
            if let Some(r) = &a.rel {
                grid2(ui, vec![(crate::i18n::tr("Wyżej (5 km)"), format!("{}%", fmt::dec(r.above * 100.0, 0))), (crate::i18n::tr("Niżej (5 km)"), format!("{}%", fmt::dec(r.below * 100.0, 0)))]);
            }
            ui.add_space(6.0);
            ui.label(theme::kicker(crate::i18n::tr("Szczyty względem punktu")));
            let mut peaks: Vec<(usize, f32, f64)> = a
                .snap
                .pois
                .iter()
                .enumerate()
                .filter(|(_, p)| p.poi.kind == PoiKind::Peak)
                .map(|(i, p)| (i, p.ele - e, haversine(lon, lat, p.poi.lon, p.poi.lat)))
                .filter(|x| x.2 < 15_000.0 && x.2 > 30.0)
                .collect();
            peaks.sort_by(|x, y| x.2.total_cmp(&y.2));
            for (i, dh, d) in peaks.into_iter().take(16) {
                let p = a.snap.pois[i].clone();
                let col = if dh > 0.0 { Color32::from_rgb(255, 176, 140) } else { Color32::from_rgb(143, 211, 255) };
                let r = ui
                    .horizontal(|ui| {
                        let l = ui.add(egui::Label::new(RichText::new(&p.poi.name).strong()).sense(egui::Sense::click()));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(fmt::dist(d)).small().color(theme::PAPER_3));
                            ui.label(RichText::new(fmt::signed(dh as f64, "m")).monospace().color(col));
                        });
                        l.clicked()
                    })
                    .inner;
                if r {
                    a.selection = Selection::Poi(i);
                }
            }
        }
        Lens::Band | Lens::None => {
            ui.label(theme::kicker(if a.prefs.band_tol > 0.0 { crate::i18n::tr("Ta sama wysokość") } else { crate::i18n::tr("Poziomica przez punkt") }));
            ui.heading(fmt::ele(e as f64));
            ui.add(egui::Slider::new(&mut a.prefs.band_tol, 0.0..=50.0).text(crate::i18n::tr("pas ± m (0 – sama poziomica)")));
            let tol = a.prefs.band_tol.max(15.0);
            ui.label(theme::kicker(&tf!("Na tej samej wysokości (±{} m)", tol.round())));
            let mut same: Vec<(usize, f64)> = a
                .snap
                .pois
                .iter()
                .enumerate()
                .filter(|(_, p)| matches!(p.poi.kind, PoiKind::Peak | PoiKind::Saddle | PoiKind::Hut | PoiKind::Shelter | PoiKind::Lake) && (p.ele - e).abs() <= tol)
                .map(|(i, p)| (i, haversine(lon, lat, p.poi.lon, p.poi.lat)))
                .filter(|x| x.1 > 30.0)
                .collect();
            same.sort_by(|x, y| x.1.total_cmp(&y.1));
            for (i, d) in same.into_iter().take(20) {
                let p = a.snap.pois[i].clone();
                if row(ui, &p.poi.name, &fmt::ele(p.ele as f64), &fmt::dist(d)) {
                    a.selection = Selection::Poi(i);
                }
            }
        }
    }
    ui.add_space(8.0);
    if ui.button(crate::i18n::tr("Odepnij soczewkę (Esc)")).clicked() {
        unpin = true;
    }
    if unpin {
        a.unpin_lens();
    }
}

/// Styl mapy jako kafelki z ikonami.
fn style_tab(a: &mut Atlas, ui: &mut egui::Ui) {
    ui.label(theme::kicker(crate::i18n::tr("Styl mapy")));
    theme::tile_grid(ui, 3, |ui, w| {
        for st in Style::ALL {
            if theme::tile(ui, style_glyph(st), st.label(), a.prefs.style == st, w).clicked() {
                a.prefs.style = st;
            }
        }
    });
    let legend = match a.prefs.style {
        Style::Slope => Some(crate::i18n::tr("Klasy nachylenia jak na mapach lawinowych: 15–25° · 25–30° · 30–35° · 35–40° · 40–45° · ponad 45°.")),
        Style::Aspect => Some(crate::i18n::tr("Barwa – kierunek, w który opada stok; nasycenie – nachylenie.")),
        Style::Hypso => Some(crate::i18n::tr("Barwy wysokości: niziny zielone, piętro hal brązowe, najwyżej biel.")),
        Style::Satellite => Some(crate::i18n::tr("Zdjęcia lotnicze i satelitarne z całego świata (Esri). Ortofoto krajowe włączysz w ustawieniach.")),
        _ => None,
    };
    if let Some(l) = legend {
        theme::para(ui, l);
    }
    ui.add_space(10.0);
    ui.add(egui::Slider::new(&mut a.prefs.exag, 1.0..=3.0).step_by(0.1).text(crate::i18n::tr("przewyższenie ×")));
}

/// Nakładki na mapie jako kafelki z ikonami.
fn layers_tab(a: &mut Atlas, ui: &mut egui::Ui) {
    // co naprawdę zmienia obraz w danym stylu: śnieg tylko w barwach terenu (na zdjęciu jest
    // zdjęcie), pokrycie (woda, lasy, drzewa) w stylach Teren, Satelita i Mapa – nie w analitycznych
    let snow_off = (a.prefs.style != Style::Terrain).then_some(crate::i18n::tr("widać tylko w stylu Teren (zdjęcie satelitarne ma własny śnieg)"));
    let land_off = matches!(a.prefs.style, Style::Hypso | Style::Slope | Style::Aspect).then_some(crate::i18n::tr("w stylach analitycznych nie ma wody, lasów ani drzew"));
    fn grid(ui: &mut egui::Ui, items: Vec<(&str, &str, &mut bool, Option<&str>)>) {
        theme::tile_grid(ui, 4, |ui, w| {
            for (g, t, v, off) in items {
                let on = *v && off.is_none();
                if theme::tile_ex(ui, g, t, on, w, off).clicked() {
                    *v = !*v;
                }
            }
        });
    }
    names_section(a, ui, 4);
    ui.add_space(10.0);
    let p = &mut a.prefs;
    ui.label(theme::kicker(crate::i18n::tr("Linie")));
    grid(ui, vec![
        (theme::icon::PATH, crate::i18n::tr("Szlaki (T)"), &mut p.trails, None),
        (theme::icon::WAVE_SINE, crate::i18n::tr("Poziomice (C)"), &mut p.contours, None),
        (theme::icon::GRID_FOUR, crate::i18n::tr("Siatka km"), &mut p.km_grid, None),
    ]);
    ui.add_space(10.0);
    ui.label(theme::kicker(crate::i18n::tr("Teren")));
    grid(ui, vec![
        (theme::icon::CLOUD_SUN, crate::i18n::tr("Cienie"), &mut p.shadows, None),
        (theme::icon::DROP_HALF, crate::i18n::tr("Rzeźba"), &mut p.detail, None),
        (theme::icon::SNOWFLAKE, crate::i18n::tr("Śnieg"), &mut p.snow, snow_off),
        (theme::icon::TREE_EVERGREEN, crate::i18n::tr("Lasy i woda"), &mut p.landcover, land_off),
    ]);
}

/// Nazwy na mapie: główny przełącznik, rodzaje (kafelki), wysokości i gęstość.
pub fn names_section(a: &mut Atlas, ui: &mut egui::Ui, cols: usize) {
    let p = &mut a.prefs;
    ui.label(theme::kicker("Nazwy na mapie"));
    theme::switch(ui, &mut p.labels, "Pokaż nazwy (L)");
    let off = (!p.labels).then_some("nazwy są wyłączone (L)");
    theme::tile_grid(ui, cols, |ui, w| {
        for (g, t, v) in p.names.kinds() {
            let on = *v && off.is_none();
            if theme::tile_ex(ui, g, t, on, w, off).clicked() {
                *v = !*v;
            }
        }
    });
    theme::switch(ui, &mut p.names.elevation, "Wysokość przy szczytach i przełęczach");
    ui.add(egui::Slider::new(&mut p.label_density, 0.3..=2.0).text(crate::i18n::tr("gęstość nazw")));
    theme::para(ui, "Nazwy pokazują się tylko dla miejsc, które widać – te za graniami są ukryte. Granie, doliny, hale i źródła doczytują się razem ze szlakami okolicy.");
}

/// Strona „Mapa” w ustawieniach.
pub fn map_settings(a: &mut Atlas, ui: &mut egui::Ui) {
    ui.label(theme::kicker(crate::i18n::tr("Zdjęcia")));
    theme::switch(ui, &mut a.prefs.national_ortho, crate::i18n::tr("Ortofoto krajowe (PL, CZ, SK)"));
    theme::para(ui, crate::i18n::tr("Z bliska zamiast zdjęć Esri – ostrzejsze zdjęcia lotnicze z geoportali GUGiK, ČÚZK i ÚGKK SR. Wolniej się wczytują."));
    ui.add_space(12.0);
    ui.label(theme::kicker(crate::i18n::tr("Kontrolki")));
    theme::switch(ui, &mut a.prefs.autohide, crate::i18n::tr("Autoukrywanie kontrolek na mapie"));
    theme::para(ui, crate::i18n::tr("Pasek narzędzi, wyszukiwarka i przyciski kamery chowają się, gdy mysz stoi; wracają przy ruchu myszy."));
    ui.add_space(12.0);
    names_section(a, ui, 4);
}

fn light_tab(a: &mut Atlas, ui: &mut egui::Ui) {
    let (day, hour) = a.when();
    let mut h = hour;
    let mut d = day;
    let t = sun::times(d, a.cam.lat, a.cam.lon);
    let (az, alt) = a.sun();
    ui.horizontal(|ui| {
        ui.label(RichText::new(fmt::hour(hour)).size(34.0).strong());
        ui.vertical(|ui| {
            ui.label(RichText::new(if a.prefs.custom_time.is_some() { crate::i18n::tr("pora ustawiona") } else { crate::i18n::tr("teraz") }).small().color(theme::PAPER_3));
            ui.label(RichText::new(tf!("Słońce {}° · {} {}", fmt::dec(alt, 0), az.round(), fmt::compass(az))).small().color(theme::PAPER_2));
        });
    });
    // oś dnia: noc, świt, dzień, zmierzch i bieżąca godzina
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 14.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 7.0, Color32::from_rgb(28, 34, 58));
    let x = |hh: f64| rect.left() + rect.width() * (hh / 24.0).clamp(0.0, 1.0) as f32;
    if t.set > t.rise {
        let day_r = egui::Rect::from_x_y_ranges(x(t.rise)..=x(t.set), rect.y_range());
        p.rect_filled(day_r, 7.0, Color32::from_rgb(232, 196, 96));
        for (a0, a1) in [(t.rise - 0.6, t.rise + 0.6), (t.set - 0.6, t.set + 0.6)] {
            p.rect_filled(egui::Rect::from_x_y_ranges(x(a0)..=x(a1), rect.y_range()), 4.0, Color32::from_rgb(240, 130, 80));
        }
    }
    let cx = x(hour);
    p.circle(egui::pos2(cx, rect.center().y), 7.0, theme::PAPER, egui::Stroke::new(2.0, theme::INK_0));
    // suwak godziny i data na całą szerokość, data wyśrodkowana
    ui.spacing_mut().slider_width = ui.available_width();
    let r = ui.add(egui::Slider::new(&mut h, 0.0..=23.99).show_value(false));
    let (row, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), egui::Sense::hover());
    let bw = 30.0;
    let btn = |ui: &mut egui::Ui, x: f32, glyph: &str, hint: &str| {
        let rr = egui::Rect::from_min_size(egui::pos2(x, row.top()), egui::vec2(bw, row.height()));
        ui.put(rr, egui::Button::new(glyph).frame(false)).on_hover_text(hint).clicked()
    };
    if btn(ui, row.left(), theme::icon::CARET_DOUBLE_LEFT, crate::i18n::tr("miesiąc wcześniej")) {
        d = d.add_days(-30);
    }
    if btn(ui, row.left() + bw, theme::icon::CARET_LEFT, crate::i18n::tr("dzień wcześniej")) {
        d = d.add_days(-1);
    }
    if btn(ui, row.right() - 2.0 * bw, theme::icon::CARET_RIGHT, crate::i18n::tr("dzień później")) {
        d = d.add_days(1);
    }
    if btn(ui, row.right() - bw, theme::icon::CARET_DOUBLE_RIGHT, crate::i18n::tr("miesiąc później")) {
        d = d.add_days(30);
    }
    ui.painter().text(row.center(), egui::Align2::CENTER_CENTER, d.label(), egui::FontId::proportional(15.0), theme::PAPER);
    if r.changed() || d != day {
        a.prefs.custom_time = Some((d.y, d.m, d.d, h));
    }
    ui.add_space(6.0);
    let mut pick = None;
    theme::tile_grid(ui, 5, |ui, w| {
        if theme::tile(ui, theme::icon::CLOCK, crate::i18n::tr("Teraz"), a.prefs.custom_time.is_none(), w).clicked() {
            pick = Some(None);
        }
        if theme::tile(ui, theme::icon::SUN_HORIZON, crate::i18n::tr("Wschód"), false, w).clicked() {
            pick = Some(Some(t.rise + 0.25));
        }
        if theme::tile(ui, theme::icon::SUN, crate::i18n::tr("Południe"), false, w).clicked() {
            pick = Some(Some(t.noon));
        }
        if theme::tile(ui, theme::icon::SUN_DIM, crate::i18n::tr("Złota godz."), false, w).clicked() {
            pick = Some(Some((t.set - 1.0).max(0.0)));
        }
        if theme::tile(ui, theme::icon::MOON_STARS, crate::i18n::tr("Zachód"), false, w).clicked() {
            pick = Some(Some(t.set));
        }
    });
    match pick {
        Some(None) => a.prefs.custom_time = None,
        Some(Some(hh)) => a.prefs.custom_time = Some((d.y, d.m, d.d, hh)),
        None => {}
    }
    ui.add_space(6.0);
    plain_stats(ui, &[
        (crate::i18n::tr("Wschód"), fmt::hour(t.rise)),
        (crate::i18n::tr("Zachód"), fmt::hour(t.set)),
        (crate::i18n::tr("Długość dnia"), fmt::time((t.set - t.rise).max(0.0))),
        (crate::i18n::tr("Górowanie"), format!("{} · {}°", fmt::hour(t.noon), t.max_alt.round())),
        (crate::i18n::tr("Granica śniegu"), fmt::ele(sun::snowline(d))),
    ]);
    let _ = Day::today;
}

/// Strona „Grafika” w ustawieniach.
pub fn graphics(a: &mut Atlas, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(theme::kicker(&tf!("Jakość · {}", a.prefs.quality.label())));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(tf!("{:.0} kl./s · {:.1} ms", a.fps(), 1000.0 / a.fps().max(1.0))).small().color(theme::TEAL));
        });
    });
    ui.horizontal(|ui| {
        for q in Quality::PRESETS {
            if ui.selectable_label(a.prefs.quality == q, q.label()).clicked() {
                a.prefs.quality = q;
                let (s, d, md, mi) = q.values();
                a.prefs.render_scale = s;
                a.prefs.detail_px = d;
                a.prefs.max_dem = md;
                a.prefs.max_img = mi;
            }
        }
    });
    ui.add_space(6.0);
    let mut changed = false;
    changed |= ui.add(egui::Slider::new(&mut a.prefs.render_scale, 0.25..=1.0).text(crate::i18n::tr("rozdzielczość renderu")).custom_formatter(|v, _| format!("{:.0}%", v * 100.0))).changed();
    let mut detail = 700.0 - a.prefs.detail_px;
    if ui.add(egui::Slider::new(&mut detail, 100.0..=520.0).text(crate::i18n::tr("szczegółowość terenu")).show_value(false)).changed() {
        a.prefs.detail_px = 700.0 - detail;
        changed = true;
    }
    changed |= ui.add(egui::Slider::new(&mut a.prefs.max_dem, 150..=1200).text(crate::i18n::tr("kafle terenu w pamięci GPU"))).changed();
    changed |= ui.add(egui::Slider::new(&mut a.prefs.max_img, 150..=1500).text(crate::i18n::tr("kafle zdjęć w pamięci GPU"))).changed();
    if changed {
        a.prefs.quality = Quality::Custom;
    }
    if let Some(r) = &a.renderer {
        let (d, i) = r.tiles.resident();
        ui.label(RichText::new(tf!("Rysowane kafle: {} · w pamięci: teren {}, zdjęcia {}", r.drawn, d, i)).small().monospace().color(theme::PAPER_3));
    }
}

/// Wysokość panelu profilu.
pub const PROFILE_H: f32 = 196.0;
/// Wysokość zwiniętego profilu (sam nagłówek z danymi).
pub const PROFILE_BAR_H: f32 = 22.0;

/// Profil wysokości na dole okna.
pub fn profile_panel(a: &mut Atlas, ctx: &egui::Context) {
    let open = a.prefs.profile_open;
    if !open {
        a.profile_cursor = a.flight.as_ref().map(|f| f.dist);
    }
    let Some(route) = a.current_route().cloned() else {
        // profil pomiaru (2+ punktów)
        a.profile_cursor = None;
        return;
    };
    let screen = ctx.screen_rect();
    // dolny pasek wbudowany w okno, na całą szerokość nad paskiem stanu; zwinięty – sam nagłówek
    let h = if open { PROFILE_H } else { PROFILE_BAR_H };
    let size = egui::vec2((screen.width() - 28.0).max(320.0), h);
    glass_area(a, ctx, egui::Id::new("profil"), 1.0);
    egui::Area::new(egui::Id::new("profil"))
        .fixed_pos([screen.left(), screen.bottom() - STATUS_H - h - 20.0])
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            egui::Frame::new().fill(theme::GLASS_FILL).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
            ui.set_min_size(size);
            ui.set_max_size(size);
            ui.horizontal(|ui| {
                ui.label(RichText::new(theme::icon::CHART_LINE_UP).size(17.0).color(theme::BRASS));
                ui.label(RichText::new(crate::i18n::tr("Profil wysokości")).strong());
                ui.add_space(10.0);
                let s = route.stats;
                let tr = &route.track;
                let hi = tr.e.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                let lo = tr.e.iter().cloned().fold(f32::INFINITY, f32::min);
                // dane bez teł i obramowań
                let chip = |ui: &mut egui::Ui, g: &str, v: String, c: Color32| {
                    ui.add_space(8.0);
                    ui.label(RichText::new(format!("{g}  {v}")).size(12.5).color(c));
                };
                chip(ui, theme::icon::RULER, fmt::dist(s.len2), theme::PAPER);
                chip(ui, theme::icon::TREND_UP, fmt::ele(s.up), Color32::from_rgb(240, 160, 110));
                chip(ui, theme::icon::TREND_DOWN, fmt::ele(s.down), Color32::from_rgb(130, 190, 240));
                chip(ui, theme::icon::CLOCK, fmt::time(s.time), theme::PAPER);
                chip(ui, theme::icon::ARROW_LINE_UP, fmt::ele(hi as f64), theme::PAPER_2);
                chip(ui, theme::icon::ARROW_LINE_DOWN, fmt::ele(lo as f64), theme::PAPER_2);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (glyph, hint) = if open { (theme::icon::CARET_DOWN, crate::i18n::tr("Zwiń profil")) } else { (theme::icon::CARET_UP, crate::i18n::tr("Rozwiń profil")) };
                    if ui.add(egui::Button::new(RichText::new(glyph).size(16.0)).frame(false)).on_hover_text(hint).clicked() {
                        a.prefs.profile_open = !open;
                    }
                    if !open {
                        return;
                    }
                    // legenda nachylenia
                    for (k, name) in [(5usize, "> 45%"), (4, "30–45%"), (3, "20–30%"), (2, "12–20%"), (1, "5–12%"), (0, "< 5%")] {
                        let c = metrics::GRADE_RGB[k];
                        ui.label(RichText::new(name).size(10.5).color(theme::PAPER_3));
                        ui.label(RichText::new("●").size(11.0).color(Color32::from_rgb(c[0], c[1], c[2])));
                    }
                });
            });
            if !open {
                return;
            }
            let others: Vec<&crate::graph::Route> = a.routes.iter().enumerate().filter(|(i, _)| *i != a.route_sel).map(|(_, r)| r).collect();
            // mijane punkty (szczyty, przełęcze, schroniska w pobliżu śladu)
            let tr = &route.track;
            let mut marks = Vec::new();
            for p in a.snap.pois.iter().filter(|p| matches!(p.poi.kind, PoiKind::Peak | PoiKind::Saddle | PoiKind::Hut | PoiKind::Shelter)).take(800) {
                let mut best = (f64::INFINITY, 0usize);
                for i in (0..tr.len()).step_by(3) {
                    let d = haversine(tr.lon[i], tr.lat[i], p.poi.lon, p.poi.lat);
                    if d < best.0 {
                        best = (d, i);
                    }
                }
                if best.0 < 90.0 {
                    marks.push(profile::Mark { dist: tr.d2[best.1], label: p.poi.name.clone() });
                }
            }
            marks.sort_by(|x, y| x.dist.total_cmp(&y.dist));
            let cursor = a.flight.as_ref().map(|f| f.dist).or(a.profile_cursor);
            let h = profile::show(ui, &route, &others, &marks, cursor, None);
            a.profile_cursor = h.or(a.flight.as_ref().map(|f| f.dist));
            });
        });
}

pub fn help(a: &mut Atlas, ctx: &egui::Context) {
    if !a.help {
        return;
    }
    let rows = [
        ("1 – 3", crate::i18n::tr("eksploracja · trasa · pomiar")),
        ("H  W  O", crate::i18n::tr("soczewka: ta sama wysokość · względna · widoczność")),
        ("/", "wyszukiwarka"),
        ("M", crate::i18n::tr("następny styl mapy")),
        ("P", crate::i18n::tr("pokaż / ukryj panel szczegółów")),
        ("T  L  C", crate::i18n::tr("szlaki · nazwy · poziomice")),
        (crate::i18n::tr("klik z soczewką / Esc"), crate::i18n::tr("przypnij / odepnij soczewkę")),
        ("+  −", crate::i18n::tr("przewyższenie terenu")),
        ("[  ]", crate::i18n::tr("pora dnia −/+ 30 min")),
        ("F", crate::i18n::tr("przelot nad trasą")),
        (crate::i18n::tr("W A S D · Shift · spacja"), crate::i18n::tr("spacer: chodzenie · bieg · skok (prawy przycisk – rozglądanie)")),
        ("N", crate::i18n::tr("północ u góry")),
        ("V", crate::i18n::tr("widok z góry / ukośny")),
        ("R", crate::i18n::tr("widok początkowy")),
        ("G", crate::i18n::tr("eksport trasy (GPX, KML, GeoJSON)")),
        (crate::i18n::tr("Ctrl+Z  Ctrl+Y"), crate::i18n::tr("cofnij / ponów zmianę punktów trasy")),
        ("Ctrl+S", crate::i18n::tr("zapisz trasę w bibliotece")),
        ("Ctrl+D", crate::i18n::tr("zapisz widok w bibliotece")),
        ("B", "biblioteka"),
        ("F11", crate::i18n::tr("pełny ekran")),
        ("S", crate::i18n::tr("zrzut ekranu PNG")),
        ("⌫", crate::i18n::tr("usuń ostatni punkt")),
        (crate::i18n::tr("Esc"), crate::i18n::tr("anuluj / wyjdź")),
        (crate::i18n::tr("strzałki · Q E · PgUp PgDn"), crate::i18n::tr("przesuwanie · obrót · nachylenie")),
        (crate::i18n::tr("lewy przycisk"), crate::i18n::tr("przesuwanie (chwyć teren)")),
        (crate::i18n::tr("prawy przycisk"), crate::i18n::tr("obrót wokół chwyconego punktu · klik – menu analiz")),
        (crate::i18n::tr("kółko"), crate::i18n::tr("przybliżanie do kursora")),
        ("dwuklik", crate::i18n::tr("przybliż do punktu")),
        ("Ctrl+,", "ustawienia"),
    ];
    let mut open = true;
    glass_area(a, ctx, egui::Id::new(crate::i18n::tr("Jak korzystać z Grani")), 1.0);
    egui::Window::new(crate::i18n::tr("Jak korzystać z Grani"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .open(&mut open)
        .show(ctx, |ui| {
            ui.set_width(560.0);
            theme::para(ui, crate::i18n::tr("Atlas łączy rzeczywisty model terenu z siecią szlaków OSM. Wszystkie obliczenia – trasy, widoczność, cienie – odbywają się lokalnie na tym komputerze."));
            ui.add_space(8.0);
            egui::Grid::new("skroty").num_columns(2).striped(true).spacing([24.0, 6.0]).show(ui, |ui| {
                for (k, v) in rows {
                    ui.label(crate::i18n::tr(v));
                    ui.label(RichText::new(crate::i18n::tr(k)).monospace().color(theme::BRASS));
                    ui.end_row();
                }
            });
            ui.add_space(8.0);
            theme::para(ui, crate::i18n::tr("Pliki GPX, KML i GeoJSON możesz przeciągnąć bezpośrednio na mapę."));
        });
    if !open {
        a.help = false;
    }
}

/// Okno wyboru pliku śladu (GPX, KML, GeoJSON).
pub fn pick_track_file() -> Option<std::path::PathBuf> {
    rfd::FileDialog::new()
        .add_filter(crate::i18n::tr("Ślady (GPX, KML, GeoJSON)"), &crate::formats::EXTENSIONS)
        .add_filter("GPX", &["gpx"])
        .add_filter("KML", &["kml"])
        .add_filter("GeoJSON", &["geojson", "json"])
        .pick_file()
}
