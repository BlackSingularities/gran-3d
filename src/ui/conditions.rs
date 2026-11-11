//! Zakładka „Warunki”: pogoda w punkcie (teraz, 48 godzin, 7 dni), pogoda na trasie w porze
//! przejścia i komunikaty lawinowe dla Tatr (TOPR, SLP HZS).

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use crate::avalanche::{self, Bulletin};
use crate::fmt;
use crate::i18n::tr;
use crate::osm::PoiKind;
use crate::ui::atlas::{Atlas, Selection};
use crate::ui::theme;
use crate::weather::{self, Forecast, Hour};

/// Miejsce, dla którego pokazujemy warunki.
#[derive(Clone, Debug)]
pub struct Place {
    pub lon: f64,
    pub lat: f64,
    pub ele: f32,
    pub name: String,
}

/// Miejsce wybrane ręcznie albo zaznaczenie, albo środek widoku.
fn place(a: &Atlas) -> Place {
    if let Some(p) = &a.cond_at {
        return p.clone();
    }
    match &a.selection {
        Selection::Poi(i) => {
            if let Some(p) = a.snap.pois.get(*i) {
                return Place { lon: p.poi.lon, lat: p.poi.lat, ele: p.ele, name: p.poi.name.clone() };
            }
        }
        Selection::Point { lon, lat } => {
            let ele = a.heights.sample_cached(*lon, *lat).unwrap_or(0.0);
            let name = a.name_near(*lon, *lat, 400.0).unwrap_or_else(|| fmt::coords(*lon, *lat));
            return Place { lon: *lon, lat: *lat, ele, name };
        }
        _ => {}
    }
    let (lon, lat) = (a.cam.lon, a.cam.lat);
    let ele = a.heights.sample_cached(lon, lat).unwrap_or(0.0);
    let name = a
        .snap
        .pois
        .iter()
        .filter(|p| p.poi.kind == PoiKind::Peak)
        .map(|p| (crate::geo::haversine(lon, lat, p.poi.lon, p.poi.lat), p))
        .filter(|(d, _)| *d < 5000.0)
        .min_by(|x, y| x.0.total_cmp(&y.0))
        .map(|(_, p)| tf!("Okolice: {}", p.poi.name))
        .unwrap_or_else(|| tr("Środek widoku").to_string());
    Place { lon, lat, ele, name }
}

/// Czas lokalny (polski) z sekund uniksowych: (dzień tygodnia 0 = poniedziałek, godzina).
fn local(t: i64) -> (i64, f64) {
    let off = crate::sun::warsaw_offset_min(t as f64) as i64 * 60;
    let l = t + off;
    let days = l.div_euclid(86_400);
    ((days + 3).rem_euclid(7), l.rem_euclid(86_400) as f64 / 3600.0)
}

fn weekday(i: i64) -> &'static str {
    ["pon.", "wt.", "śr.", "czw.", "pt.", "sob.", "niedz."][i as usize % 7]
}

fn temp(v: f32) -> String {
    format!("{}°", fmt::dec(v as f64, 0).replace('-', "−"))
}

fn kmh(v: f32) -> String {
    tf!("{} km/h", fmt::dec(v as f64, 0))
}

fn temp_color(t: f32) -> Color32 {
    if t <= -10.0 {
        Color32::from_rgb(150, 170, 255)
    } else if t <= 0.0 {
        Color32::from_rgb(140, 200, 255)
    } else if t < 15.0 {
        theme::PAPER
    } else if t < 25.0 {
        Color32::from_rgb(255, 205, 120)
    } else {
        Color32::from_rgb(255, 140, 90)
    }
}

fn gust_color(g: f32) -> Color32 {
    if g >= 90.0 {
        theme::SIGNAL
    } else if g >= 60.0 {
        theme::WARN
    } else {
        theme::paper_2()
    }
}

fn section(ui: &mut egui::Ui, glyph: &str, title: &str) {
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(RichText::new(glyph).size(14.0).color(theme::BRASS));
        ui.label(theme::kicker(title));
    });
}

pub fn show(a: &mut Atlas, ui: &mut egui::Ui) {
    let pl = place(a);
    let ctx = ui.ctx().clone();
    ui.horizontal(|ui| {
        ui.label(theme::kicker(&tf!("{} · {} m n.p.m.", fmt::coords(pl.lon, pl.lat), fmt::int(pl.ele as f64))));
        if a.cond_at.is_some() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(theme::icon::X).on_hover_text(tr("Wróć do środka widoku")).clicked() {
                    a.cond_at = None;
                }
            });
        }
    });
    ui.heading(&pl.name);
    theme::para(ui, "Kliknij punkt albo szczyt na mapie, żeby zobaczyć warunki w tym miejscu. Prognoza jest liczona dla wysokości punktu.");

    match a.weather.get(&ctx, pl.lon, pl.lat, pl.ele) {
        weather::State::Loading => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(tr("pobieram prognozę…")).color(theme::paper_3()));
            });
        }
        weather::State::Failed(e) => {
            ui.label(RichText::new(tf!("Prognoza niedostępna: {}", e)).color(theme::WARN));
        }
        weather::State::Ready(f) => forecast_ui(ui, &f, pl.ele),
    }

    route_weather(a, ui);
    notices_ui(a, ui);
    avalanche_ui(a, ui);

    ui.add_space(12.0);
    theme::para(ui, "Prognoza: Open-Meteo (modele ICON, ECMWF i inne). Komunikaty lawinowe: TOPR (lawiny.topr.pl) i SLP HZS (laviny.sk) – zawsze sprawdź pełny komunikat u źródła przed wyjściem.");
}

fn forecast_ui(ui: &mut egui::Ui, f: &Forecast, ele: f32) {
    let now = weather::now();
    let Some(h) = f.at(now).cloned() else {
        ui.label(tr("Brak prognozy na tę chwilę."));
        return;
    };
    // teraz
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let (glyph, desc) = weather::describe(h.code, h.day);
        ui.label(RichText::new(glyph).size(44.0).color(theme::BRASS));
        ui.vertical(|ui| {
            ui.label(RichText::new(temp(h.temp)).size(30.0).strong().color(temp_color(h.temp)));
            ui.label(RichText::new(tf!("{} · odczuwalna {}", tr(desc), temp(h.feels))).color(theme::paper_2()));
        });
    });
    let vis = if h.vis.is_finite() { fmt::dist(h.vis as f64) } else { "—".into() };
    grid(ui, &[
        (theme::icon::WIND, tr("Wiatr"), tf!("{} {}", kmh(h.wind), weather::wind_from(h.dir)), theme::PAPER),
        (theme::icon::WIND, tr("Porywy"), kmh(h.gust), gust_color(h.gust)),
        (theme::icon::CLOUD, tr("Zachmurzenie"), tf!("{}% (niskie {}%)", h.cloud.round(), h.cloud_low.round()), theme::PAPER),
        (theme::icon::DROP, tr("Szansa opadu"), tf!("{}%", h.prob.round()), theme::PAPER),
        (theme::icon::THERMOMETER, tr("Izoterma 0 °C"), fmt::ele(h.freezing as f64), if h.freezing < ele { Color32::from_rgb(140, 200, 255) } else { theme::PAPER }),
        (theme::icon::EYE, tr("Widzialność"), vis, theme::PAPER),
    ]);
    // ostrzeżenia na najbliższe 24 godziny
    let mut warn: Vec<(&str, i64)> = Vec::new();
    for x in f.next(now, 24) {
        for w in weather::hazards(x, ele) {
            if !warn.iter().any(|(k, _)| *k == w) {
                warn.push((w, x.t));
            }
        }
    }
    if !warn.is_empty() {
        ui.add_space(6.0);
        for (w, t) in warn {
            let (_, hh) = local(t);
            ui.label(RichText::new(tf!("{}  {} od {}", theme::icon::WARNING, tr(w), fmt::hour(hh.floor()))).color(theme::WARN));
        }
    }
    section(ui, theme::icon::CHART_LINE, "Najbliższe 48 godzin");
    chart(ui, f.next(now, 48), ele);
    section(ui, theme::icon::CALENDAR, "7 dni");
    for d in &f.days {
        let (wd, _) = local(d.t + 12 * 3600);
        ui.horizontal(|ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(tr(weekday(wd))).strong());
            let (glyph, desc) = weather::describe(d.code, true);
            ui.label(RichText::new(glyph).size(16.0).color(theme::BRASS)).on_hover_text(tr(desc));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(kmh(d.gust)).monospace().small().color(gust_color(d.gust)));
                let wet = if d.snow > 0.2 { tf!("{} cm", fmt::dec(d.snow as f64, 0)) } else { tf!("{} mm", fmt::dec(d.precip as f64, 1)) };
                ui.label(RichText::new(wet).monospace().small().color(theme::paper_3()));
                ui.label(RichText::new(format!("{} / {}", temp(d.tmin), temp(d.tmax))).monospace().color(temp_color(d.tmax)));
            });
        });
    }
}

fn grid(ui: &mut egui::Ui, items: &[(&str, &str, String, Color32)]) {
    let w = (ui.available_width() - 12.0) / 2.0;
    for row in items.chunks(2) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            for (g, k, v, c) in row {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 40.0), Sense::hover());
                let p = ui.painter().with_clip_rect(rect);
                p.text(rect.left_center() + Vec2::new(9.0, 0.0), Align2::CENTER_CENTER, *g, FontId::proportional(16.0), theme::BRASS);
                p.text(Pos2::new(rect.left() + 26.0, rect.top() + 3.0), Align2::LEFT_TOP, *k, FontId::proportional(11.0), theme::paper_3());
                p.text(Pos2::new(rect.left() + 26.0, rect.bottom() - 3.0), Align2::LEFT_BOTTOM, v, FontId::proportional(14.5), *c);
            }
        });
    }
}

/// Wykres godzinowy: temperatura (linia), opad (słupki), porywy (linia przerywana), noc (tło), izoterma.
fn chart(ui: &mut egui::Ui, hours: &[Hour], ele: f32) {
    if hours.len() < 2 {
        return;
    }
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 170.0), Sense::hover());
    let p = ui.painter_at(rect);
    let plot = Rect::from_min_max(rect.min + Vec2::new(30.0, 8.0), rect.max - Vec2::new(30.0, 22.0));
    let n = hours.len();
    let x = |i: f32| plot.left() + i / (n - 1) as f32 * plot.width();
    let tmin = hours.iter().map(|h| h.temp).fold(f32::INFINITY, f32::min) - 2.0;
    let tmax = hours.iter().map(|h| h.temp).fold(f32::NEG_INFINITY, f32::max) + 2.0;
    let yt = |t: f32| plot.bottom() - (t - tmin) / (tmax - tmin).max(1.0) * plot.height();
    let gmax = hours.iter().map(|h| h.gust).fold(40.0f32, f32::max);
    let yg = |g: f32| plot.bottom() - g / gmax * plot.height();
    let pmax = hours.iter().map(|h| h.precip).fold(2.0f32, f32::max);
    // noc
    for (i, h) in hours.iter().enumerate() {
        if !h.day {
            let r = Rect::from_x_y_ranges(x(i as f32 - 0.5).max(plot.left())..=x(i as f32 + 0.5).min(plot.right()), plot.y_range());
            p.rect_filled(r, 0.0, Color32::from_black_alpha(55));
        }
    }
    // 0 °C
    if tmin < 0.0 && tmax > 0.0 {
        p.line_segment([Pos2::new(plot.left(), yt(0.0)), Pos2::new(plot.right(), yt(0.0))], Stroke::new(1.0, Color32::from_rgb(90, 140, 220)));
        p.text(Pos2::new(plot.left() - 4.0, yt(0.0)), Align2::RIGHT_CENTER, "0°", FontId::monospace(10.0), Color32::from_rgb(120, 170, 240));
    }
    // opad
    for (i, h) in hours.iter().enumerate() {
        if h.precip > 0.05 {
            let top = plot.bottom() - (h.precip / pmax).min(1.0) * plot.height() * 0.5;
            let c = if h.snow > 0.05 { Color32::from_rgb(220, 230, 255) } else { Color32::from_rgb(90, 160, 255) };
            p.rect_filled(Rect::from_min_max(Pos2::new(x(i as f32) - 2.5, top), Pos2::new(x(i as f32) + 2.5, plot.bottom())), 1.0, c.gamma_multiply(0.75));
        }
    }
    // porywy
    let gust: Vec<Pos2> = hours.iter().enumerate().map(|(i, h)| Pos2::new(x(i as f32), yg(h.gust))).collect();
    p.extend(egui::Shape::dashed_line(&gust, Stroke::new(1.3, theme::WARN.gamma_multiply(0.8)), 4.0, 3.0));
    // temperatura
    let line: Vec<Pos2> = hours.iter().enumerate().map(|(i, h)| Pos2::new(x(i as f32), yt(h.temp))).collect();
    p.add(egui::Shape::line(line, Stroke::new(2.2, Color32::from_rgb(255, 170, 110))));
    // osie
    p.text(Pos2::new(plot.left() - 4.0, plot.top()), Align2::RIGHT_TOP, temp(tmax - 2.0), FontId::monospace(10.0), theme::paper_3());
    p.text(Pos2::new(plot.left() - 4.0, plot.bottom()), Align2::RIGHT_BOTTOM, temp(tmin + 2.0), FontId::monospace(10.0), theme::paper_3());
    p.text(Pos2::new(plot.right() + 4.0, plot.top()), Align2::LEFT_TOP, fmt::dec(gmax as f64, 0), FontId::monospace(10.0), theme::WARN);
    for (i, h) in hours.iter().enumerate() {
        let (_, hh) = local(h.t);
        if (hh.round() as i64) % 6 == 0 {
            let xx = x(i as f32);
            p.line_segment([Pos2::new(xx, plot.bottom()), Pos2::new(xx, plot.bottom() + 3.0)], Stroke::new(1.0, theme::paper_3()));
            let label = if hh.round() as i64 == 0 { let (wd, _) = local(h.t); tr(weekday(wd)).to_string() } else { format!("{:02}", hh.round() as i64) };
            p.text(Pos2::new(xx, plot.bottom() + 5.0), Align2::CENTER_TOP, label, FontId::monospace(10.0), theme::paper_3());
        }
    }
    // odczyt pod kursorem
    if let Some(pos) = resp.hover_pos().filter(|q| plot.x_range().contains(q.x)) {
        let i = (((pos.x - plot.left()) / plot.width() * (n - 1) as f32).round() as usize).min(n - 1);
        let h = &hours[i];
        let xx = x(i as f32);
        p.line_segment([Pos2::new(xx, plot.top()), Pos2::new(xx, plot.bottom())], Stroke::new(1.0, theme::BRASS));
        let (wd, hh) = local(h.t);
        let (_, desc) = weather::describe(h.code, h.day);
        let mut rows = vec![
            format!("{} {:02}:00 · {}", tr(weekday(wd)), hh.round() as i64, tr(desc)),
            tf!("{} (odczuwalna {})", temp(h.temp), temp(h.feels)),
            tf!("wiatr {} {}, porywy {}", kmh(h.wind), weather::wind_from(h.dir), kmh(h.gust)),
            tf!("opad {} mm ({}%) · chmury {}%", fmt::dec(h.precip as f64, 1), h.prob.round(), h.cloud.round()),
        ];
        let hz = weather::hazards(h, ele);
        if !hz.is_empty() {
            rows.push(hz.iter().map(|w| tr(w)).collect::<Vec<_>>().join(", "));
        }
        let gs: Vec<_> = rows.into_iter().map(|t| p.layout_no_wrap(t, FontId::proportional(11.5), theme::PAPER)).collect();
        let w = gs.iter().map(|g| g.size().x).fold(0.0, f32::max) + 16.0;
        let hgt = gs.iter().map(|g| g.size().y + 1.0).sum::<f32>() + 10.0;
        let left = if xx + 10.0 + w < rect.right() { xx + 10.0 } else { xx - 10.0 - w };
        let card = Rect::from_min_size(Pos2::new(left.max(rect.left()), plot.top()), Vec2::new(w, hgt));
        p.rect(card, 8.0, theme::POPUP_FILL, Stroke::new(1.0, theme::glass_edge()), egui::StrokeKind::Inside);
        let mut yy = card.top() + 5.0;
        for g in gs {
            let gh = g.size().y;
            p.galley(Pos2::new(card.left() + 8.0, yy), g, theme::PAPER);
            yy += gh + 1.0;
        }
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        ui.label(RichText::new(tr("— temperatura")).small().color(Color32::from_rgb(255, 170, 110)));
        ui.label(RichText::new(tr("- - porywy (km/h)")).small().color(theme::WARN));
        ui.label(RichText::new(tr("▮ opad")).small().color(Color32::from_rgb(90, 160, 255)));
        ui.label(RichText::new(tr("▮ śnieg")).small().color(Color32::from_rgb(220, 230, 255)));
    });
}

/// Pogoda na trasie: punkty charakterystyczne w porze, o której się do nich dojdzie.
fn route_weather(a: &mut Atlas, ui: &mut egui::Ui) {
    let Some(r) = a.current_route().cloned() else { return };
    let tr_ = &r.track;
    let n = tr_.len();
    if n < 2 {
        return;
    }
    section(ui, theme::icon::PATH, "Pogoda na trasie");
    let (day, hour) = a.when();
    let dep = crate::sun::warsaw_unix(day, hour) as i64;
    let imax = (0..n).max_by(|&x, &y| tr_.e[x].total_cmp(&tr_.e[y])).unwrap_or(0);
    let mut idx = vec![0, n / 3, imax, 2 * n / 3, n - 1];
    idx.sort();
    idx.dedup_by(|x, y| (tr_.d2[*x] - tr_.d2[*y]).abs() < 800.0);
    theme::para(ui, &tf!("Wyjście o {} (pora z zegara u góry); czasy dojścia według modelu czasu przejścia.", fmt::hour(hour)));
    let ctx = ui.ctx().clone();
    let mut worst: Vec<&'static str> = Vec::new();
    for i in idx {
        let (lon, lat, e) = (tr_.lon[i], tr_.lat[i], tr_.e[i]);
        let t = dep + (tr_.t[i] as f64 * 3600.0) as i64;
        let label = if i == 0 {
            tr("Start").to_string()
        } else if i == n - 1 {
            tr("Cel").to_string()
        } else if i == imax {
            a.name_near(lon, lat, 300.0).unwrap_or_else(|| tr("Najwyżej").to_string())
        } else {
            a.name_near(lon, lat, 300.0).unwrap_or_else(|| fmt::dist(tr_.d2[i] as f64))
        };
        let state = a.weather.get(&ctx, lon, lat, e);
        ui.horizontal(|ui| {
            ui.set_width(ui.available_width());
            let (_, hh) = local(t);
            ui.label(RichText::new(fmt::hour(hh)).monospace().color(theme::paper_3()));
            ui.label(RichText::new(&label).strong());
            ui.label(RichText::new(fmt::ele(e as f64)).small().color(theme::paper_3()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match &state {
                weather::State::Ready(f) => match f.at(t) {
                    Some(h) => {
                        ui.label(RichText::new(kmh(h.gust)).monospace().small().color(gust_color(h.gust)));
                        ui.label(RichText::new(tf!("{}%", h.prob.round())).monospace().small().color(theme::paper_3()));
                        ui.label(RichText::new(temp(h.temp)).monospace().color(temp_color(h.temp)));
                        let (glyph, desc) = weather::describe(h.code, h.day);
                        ui.label(RichText::new(glyph).size(16.0).color(theme::BRASS)).on_hover_text(tr(desc));
                        for w in weather::hazards(h, e) {
                            if !worst.contains(&w) {
                                worst.push(w);
                            }
                        }
                    }
                    None => {
                        ui.label(RichText::new(tr("poza zakresem prognozy")).small().color(theme::paper_3()));
                    }
                },
                weather::State::Loading => {
                    ui.spinner();
                }
                weather::State::Failed(_) => {
                    ui.label(RichText::new("—").color(theme::paper_3()));
                }
            });
        });
    }
    if !worst.is_empty() {
        let list: Vec<&str> = worst.iter().map(|w| tr(w)).collect();
        ui.label(RichText::new(tf!("{}  Na trasie: {}", theme::icon::WARNING, list.join(", "))).color(theme::WARN));
    }
}

fn in_tatras(lon: f64, lat: f64) -> bool {
    (19.4..=20.6).contains(&lon) && (48.75..=49.45).contains(&lat)
}

/// Komunikat turystyczny TPN: warunki, zagrożenia, zamknięte szlaki (także informacja o TANAP).
fn notices_ui(a: &mut Atlas, ui: &mut egui::Ui) {
    let pl = place(a);
    if !in_tatras(pl.lon, pl.lat) {
        return;
    }
    section(ui, theme::icon::SIGNPOST, "Komunikat turystyczny TPN");
    let ctx = ui.ctx().clone();
    match a.notices.get(&ctx) {
        crate::notices::State::Loading => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(tr("pobieram komunikat TPN…")).color(theme::paper_3()));
            });
        }
        crate::notices::State::Failed(e) => {
            ui.label(RichText::new(tf!("Komunikat niedostępny: {}", e)).color(theme::WARN));
        }
        crate::notices::State::Ready(n) => {
            if !n.headline.is_empty() {
                ui.add(egui::Label::new(RichText::new(&n.headline).strong()).wrap());
            }
            if !n.date.is_empty() {
                ui.label(RichText::new(&n.date).small().color(theme::paper_3()));
            }
            let list = |ui: &mut egui::Ui, glyph: &str, color: Color32, title: &str, items: &[String]| {
                if items.is_empty() {
                    return;
                }
                ui.add_space(6.0);
                ui.label(RichText::new(format!("{glyph}  {}", tr(title))).strong().color(color));
                for it in items {
                    ui.horizontal_top(|ui| {
                        ui.label(RichText::new("•").color(color));
                        ui.add(egui::Label::new(RichText::new(it).color(theme::paper_2())).wrap());
                    });
                }
            };
            list(ui, theme::icon::PROHIBIT, Color32::from_rgb(255, 120, 110), "Zamknięte szlaki", &n.closures);
            list(ui, theme::icon::WARNING, theme::WARN, "Zagrożenia", &n.hazards);
            egui::CollapsingHeader::new(tr("Informacje dodatkowe")).id_salt("tpn-dodatkowe").show(ui, |ui| {
                for it in &n.extra {
                    ui.add(egui::Label::new(RichText::new(format!("• {it}")).small().color(theme::paper_2())).wrap());
                }
            });
        }
    }
    ui.horizontal_wrapped(|ui| {
        if ui.small_button(theme::ic(theme::icon::ARROW_SQUARE_OUT, "Pełny komunikat TPN")).clicked() {
            let _ = open::that(crate::notices::TPN_URL);
        }
        if ui.small_button(theme::ic(theme::icon::ARROW_SQUARE_OUT, "TANAP (Słowacja)")).clicked() {
            let _ = open::that(crate::notices::TANAP_URL);
        }
    });
    theme::para(ui, "Zamknięte odcinki zapisane w OpenStreetMap program rysuje na mapie na czerwono (Warstwy → Zamknięte szlaki, klawisz Z) i ostrzega o nich przy trasie.");
}

fn avalanche_ui(a: &mut Atlas, ui: &mut egui::Ui) {
    section(ui, theme::icon::SNOWFLAKE, "Zagrożenie lawinowe");
    let pl = place(a);
    if !in_tatras(pl.lon, pl.lat) {
        theme::para(ui, "Komunikaty lawinowe są dostępne dla Tatr (TOPR – strona polska, SLP HZS – Słowacja). Przesuń widok w Tatry, żeby je zobaczyć.");
        return;
    }
    let ctx = ui.ctx().clone();
    match a.avalanche.get(&ctx) {
        avalanche::State::Loading => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(tr("pobieram komunikaty lawinowe…")).color(theme::paper_3()));
            });
        }
        avalanche::State::Failed(e) => {
            ui.label(RichText::new(tf!("Komunikaty niedostępne: {}", e)).color(theme::WARN));
        }
        avalanche::State::Ready(list) => {
            if list.is_empty() {
                theme::para(ui, "Brak aktualnych komunikatów (poza sezonem zimowym są wydawane rzadko albo wcale).");
            }
            // najpierw komunikat dla strony, na której jest punkt
            let slovak = pl.lat < 49.205 || (pl.lon > 19.95 && pl.lat < 49.235 && pl.lon < 20.25);
            let mut order: Vec<&Bulletin> = list.iter().collect();
            order.sort_by_key(|b| (b.source == "TOPR") == slovak);
            for b in order {
                bulletin_ui(ui, b, pl.ele);
            }
        }
    }
}

fn bulletin_ui(ui: &mut egui::Ui, b: &Bulletin, ele: f32) {
    ui.add_space(8.0);
    theme::card(false).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            // stopień jako kwadrat w barwie skali europejskiej
            let (r, _) = ui.allocate_exact_size(egui::vec2(46.0, 46.0), Sense::hover());
            let c = avalanche::level_rgb(b.level);
            ui.painter().rect_filled(r, 8.0, Color32::from_rgb(c[0], c[1], c[2]));
            ui.painter().text(r.center(), Align2::CENTER_CENTER, b.level.to_string(), FontId::proportional(26.0), Color32::from_rgb(20, 20, 20));
            ui.vertical(|ui| {
                ui.label(RichText::new(tr(avalanche::level_name(b.level))).size(17.0).strong());
                ui.label(RichText::new(format!("{} · {}", b.source, b.region)).small().color(theme::paper_3()));
            });
        });
        if let Some((h, up, lo)) = b.split {
            let here = if ele as u32 >= h { up } else { lo };
            ui.label(tf!("Powyżej {} m: {} · poniżej: {} (tutaj: {})", h, up, lo, here));
        }
        if let Some((am, pm)) = b.am_pm {
            ui.label(tf!("Rano: {} · po południu: {}", am, pm));
        }
        if !b.highlight.is_empty() {
            ui.label(RichText::new(&b.highlight).strong().color(theme::paper_2()));
        }
        for p in &b.problems {
            ui.horizontal(|ui| {
                rose(ui, &p.aspects);
                ui.vertical(|ui| {
                    ui.label(RichText::new(tr(p.kind.label())).strong());
                    let mut where_: Vec<String> = Vec::new();
                    if let Some(x) = p.above {
                        where_.push(tf!("powyżej {} m", x));
                    }
                    if let Some(x) = p.below {
                        where_.push(tf!("poniżej {} m", x));
                    }
                    if !p.period.is_empty() {
                        where_.push(tr(&p.period).to_string());
                    }
                    let dirs: Vec<&str> = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"].iter().zip(p.aspects).filter(|(_, on)| *on).map(|(d, _)| *d).collect();
                    if !dirs.is_empty() {
                        where_.push(tf!("wystawy {}", dirs.join(" ")));
                    }
                    if !where_.is_empty() {
                        ui.label(RichText::new(where_.join(" · ")).small().color(theme::paper_3()));
                    }
                });
            });
        }
        let utc = if b.source == "TOPR" { "" } else { " UTC" };
        ui.label(RichText::new(tf!("Wydano {}{} · ważny do {}{}", b.issued, utc, b.valid_until, utc)).small().color(theme::paper_3()));
        let long = [b.comment.as_str(), b.snowpack.as_str(), b.tendency.as_str()].iter().any(|s| !s.is_empty());
        if long {
            egui::CollapsingHeader::new(tr("Komentarz do komunikatu")).id_salt(("lawiny", b.source, &b.region)).show(ui, |ui| {
                for s in [&b.comment, &b.snowpack, &b.tendency] {
                    if !s.is_empty() {
                        ui.add(egui::Label::new(RichText::new(s.as_str()).small().color(theme::paper_2())).wrap());
                    }
                }
            });
        }
        if ui.link(tf!("Pełny komunikat: {}", b.url.trim_start_matches("https://").trim_end_matches('/'))).clicked() {
            let _ = open::that(b.url);
        }
    });
}

/// Róża wystaw: zaznaczone kierunki, z których stoki są groźne.
fn rose(ui: &mut egui::Ui, aspects: &[bool; 8]) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), Sense::hover());
    let p = ui.painter();
    let c = r.center();
    let rad = 18.0;
    for (i, on) in aspects.iter().enumerate() {
        let a0 = (i as f32 * 45.0 - 22.5 - 90.0).to_radians();
        let a1 = (i as f32 * 45.0 + 22.5 - 90.0).to_radians();
        let pts = vec![c, c + Vec2::new(a0.cos(), a0.sin()) * rad, c + Vec2::new(((a0 + a1) / 2.0).cos(), ((a0 + a1) / 2.0).sin()) * rad, c + Vec2::new(a1.cos(), a1.sin()) * rad];
        let fill = if *on { Color32::from_rgb(255, 120, 60) } else { theme::INK_3 };
        p.add(egui::Shape::convex_polygon(pts, fill, Stroke::new(1.0, theme::INK_0)));
    }
    p.text(c - Vec2::new(0.0, rad + 1.0), Align2::CENTER_BOTTOM, "N", FontId::proportional(8.5), theme::paper_3());
}
