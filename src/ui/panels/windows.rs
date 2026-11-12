//! Okna: profil wysokości, skróty klawiszowe, wczytywanie danych.

use super::*;

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
    let is_section = a.current_route().is_none();
    let Some(route) = a.profile_route().cloned() else {
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
            egui::Frame::new().fill(theme::glass_fill()).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
            ui.set_min_size(size);
            ui.set_max_size(size);
            // po prawej zwijanie i legenda; tytuł z danymi trasy rysowany potem na środku paska
            let mut legend_left = f32::INFINITY;
            let header = ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (glyph, hint) = if open { (theme::icon::CARET_DOWN, crate::i18n::tr("Zwiń profil")) } else { (theme::icon::CARET_UP, crate::i18n::tr("Rozwiń profil")) };
                    if ui.add(egui::Button::new(RichText::new(glyph).size(16.0)).frame(false)).on_hover_text(hint).clicked() {
                        a.prefs.profile_open = !open;
                    }
                    legend_left = ui.min_rect().left();
                    if !open {
                        return;
                    }
                    // legenda nachylenia
                    for (k, name) in [(5usize, "> 45%"), (4, "30–45%"), (3, "20–30%"), (2, "12–20%"), (1, "5–12%"), (0, "< 5%")] {
                        let c = metrics::GRADE_RGB[k];
                        ui.label(RichText::new(name).size(10.5).color(theme::paper_3()));
                        ui.label(RichText::new("●").size(11.0).color(Color32::from_rgb(c[0], c[1], c[2])));
                    }
                    legend_left = ui.min_rect().left();
                });
            });
            {
                let s = route.stats;
                let tr = &route.track;
                let hi = tr.e.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                let lo = tr.e.iter().cloned().fold(f32::INFINITY, f32::min);
                let up_c = Color32::from_rgb(240, 160, 110);
                let down_c = Color32::from_rgb(130, 190, 240);
                // (odstęp przed napisem, napis, rozmiar, barwa) – dane bez teł i obramowań
                let pieces: [(f32, String, f32, Color32); 8] = [
                    (0.0, theme::icon::CHART_LINE_UP.to_string(), 17.0, theme::BRASS),
                    (7.0, crate::i18n::tr(if is_section { "Przekrój terenu" } else { "Profil wysokości" }).to_string(), 14.0, theme::PAPER),
                    (22.0, format!("{}  {}", theme::icon::RULER, fmt::dist(s.len2)), 12.5, theme::PAPER),
                    (16.0, format!("{}  {}", theme::icon::TREND_UP, fmt::ele(s.up)), 12.5, up_c),
                    (16.0, format!("{}  {}", theme::icon::TREND_DOWN, fmt::ele(s.down)), 12.5, down_c),
                    (16.0, format!("{}  {}", theme::icon::CLOCK, fmt::time(s.time)), 12.5, theme::PAPER),
                    (16.0, format!("{}  {}", theme::icon::ARROW_LINE_UP, fmt::ele(hi as f64)), 12.5, theme::paper_2()),
                    (16.0, format!("{}  {}", theme::icon::ARROW_LINE_DOWN, fmt::ele(lo as f64)), 12.5, theme::paper_2()),
                ];
                let row = header.response.rect;
                let painter = ui.painter().clone();
                let galleys: Vec<(f32, std::sync::Arc<egui::Galley>, Color32)> =
                    pieces.into_iter().map(|(gap, t, size, c)| (gap, painter.layout_no_wrap(t, egui::FontId::proportional(size), c), c)).collect();
                let width: f32 = galleys.iter().map(|(gap, g, _)| gap + g.size().x).sum();
                // środek paska; gdy okno jest wąskie – przed legendą, nie na niej
                let bar = ui.max_rect();
                let right_limit = legend_left.min(bar.right()) - 14.0;
                let mut x = (bar.center().x - width / 2.0).min(right_limit - width).max(bar.left());
                let clip = egui::Rect::from_min_max(egui::pos2(bar.left(), row.top() - 4.0), egui::pos2(right_limit.max(bar.left()), row.bottom() + 4.0));
                let painter = painter.with_clip_rect(clip);
                for (gap, g, c) in galleys {
                    x += gap;
                    let w = g.size().x;
                    painter.galley(egui::pos2(x, row.center().y - g.size().y / 2.0), g, c);
                    x += w;
                }
            }
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
            let cursor = a.flight.as_ref().map(|f| f.dist).or(a.profile_cursor).or(a.panel_cursor);
            let compare = a.compare.as_ref().map(|c| &c.1);
            let h = profile::show(ui, &route, &others, &marks, cursor, None, &mut a.profile_sel, compare);
            a.profile_cursor = h.or(a.flight.as_ref().map(|f| f.dist));
            });
        });
}

/// Wiersz pomocy: opis po lewej, po prawej klawisze narysowane jak klawisze (rozdzielone „ · ”).
fn help_row(ui: &mut egui::Ui, desc: &str, keys: &str, line: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 31.0), egui::Sense::hover());
    let p = ui.painter().with_clip_rect(rect);
    if line {
        p.line_segment([rect.left_top(), rect.right_top()], egui::Stroke::new(1.0, CARD_EDGE));
    }
    let y = rect.center().y;
    let mut x = rect.right();
    for key in keys.split(" · ").collect::<Vec<_>>().into_iter().rev() {
        let g = p.layout_no_wrap(key.to_string(), egui::FontId::monospace(12.0), theme::PAPER);
        let w = (g.size().x + 14.0).max(26.0);
        let cap = egui::Rect::from_min_size(egui::pos2(x - w, y - 11.0), egui::vec2(w, 22.0));
        p.rect_filled(cap, 6.0, Color32::from_white_alpha(14));
        p.rect_stroke(cap, 6.0, egui::Stroke::new(1.0, Color32::from_white_alpha(34)), egui::StrokeKind::Inside);
        p.galley(egui::pos2(cap.center().x - g.size().x / 2.0, y - g.size().y / 2.0), g, theme::PAPER);
        x -= w + 5.0;
    }
    let desc_clip = egui::Rect::from_min_max(rect.min, egui::pos2((x - 6.0).max(rect.left()), rect.bottom()));
    p.with_clip_rect(desc_clip).text(egui::pos2(rect.left(), y), egui::Align2::LEFT_CENTER, desc, egui::FontId::proportional(13.5), theme::paper_2());
}

/// Karta pomocy: ikona, tytuł i wiersze „opis – klawisze”.
fn help_card(ui: &mut egui::Ui, glyph: &str, title: &str, rows: &[(String, String)]) {
    egui::Frame::new()
        .fill(CARD_FILL)
        .stroke(egui::Stroke::new(1.0, CARD_EDGE))
        .corner_radius(egui::CornerRadius::same(14))
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.horizontal(|ui| {
                ui.label(RichText::new(glyph).size(17.0).color(theme::BRASS));
                ui.label(RichText::new(title).size(14.5).strong());
            });
            ui.add_space(6.0);
            for (i, (desc, keys)) in rows.iter().enumerate() {
                help_row(ui, desc, keys, i > 0);
            }
        });
}

/// Okno pomocy: logo i nazwa programu, obsługa myszą i klawiaturą oraz skróty poleceń w kartach.
pub fn help(a: &mut Atlas, ctx: &egui::Context) {
    use crate::i18n::tr;
    use crate::ui::atlas::Group;
    if !a.help {
        return;
    }
    let row = |desc: &str, keys: &str| (tr(desc).to_string(), tr(keys).to_string());
    // obsługa mapy; skróty poleceń niżej – z rejestru poleceń
    let mut cards: Vec<(&'static str, String, Vec<(String, String)>)> = vec![
        (
            theme::icon::MOUSE,
            tr("Mysz").to_string(),
            vec![
                row("przesuwanie (chwyć teren)", "lewy przycisk"),
                row("obrót wokół chwyconego punktu", "prawy przycisk"),
                row("menu analiz", "prawy klik"),
                row("przybliżanie do kursora", "kółko"),
                row("przybliż do punktu", "dwuklik"),
                row("przypnij soczewkę", "klik z soczewką"),
            ],
        ),
        (
            theme::icon::KEYBOARD,
            tr("Kamera z klawiatury").to_string(),
            vec![
                row("przesuwanie widoku", "strzałki"),
                row("obrót", "Q · E"),
                row("nachylenie", "PgUp · PgDn"),
                row("przybliżanie i oddalanie", "Home · End"),
                row("spacer: chodzenie", "W · A · S · D"),
                row("spacer: bieg i skok", "Shift · spacja"),
            ],
        ),
        (
            theme::icon::INFO,
            tr("Ogólne").to_string(),
            vec![
                row("paleta poleceń", "Ctrl+K"),
                row("przechodzenie po kontrolkach", "Tab · Shift+Tab · Enter"),
                row("anuluj / wyjdź", "Esc"),
                row("ta pomoc", "?"),
            ],
        ),
    ];
    let mut groups: Vec<(Group, Vec<(String, String)>)> = Vec::new();
    for act in Action::all() {
        let Some(k) = act.keys() else { continue };
        let (_, label, g) = act.info();
        let item = (label.to_string(), k.text());
        match groups.iter_mut().find(|(x, _)| *x == g) {
            Some((_, v)) => v.push(item),
            None => groups.push((g, vec![item])),
        }
    }
    for (g, rows) in groups {
        let glyph = match g {
            Group::Tools => theme::icon::WRENCH,
            Group::View => theme::icon::EYE,
            Group::Layers => theme::icon::STACK,
            Group::Panels => theme::icon::SQUARES_FOUR,
            Group::Route => theme::icon::PATH,
            Group::Interface => theme::icon::GEAR,
        };
        cards.push((glyph, g.label().to_string(), rows));
    }
    let screen = ctx.screen_rect();
    let width = (screen.width() - 80.0).clamp(420.0, 900.0);
    let n_cols = if width < 700.0 { 1 } else { 2 };
    // karty po kolei do krótszej kolumny
    let mut cols: Vec<Vec<usize>> = vec![Vec::new(); n_cols];
    let mut heights = vec![0usize; n_cols];
    for (i, c) in cards.iter().enumerate() {
        let k = (0..n_cols).min_by_key(|&k| heights[k]).unwrap_or(0);
        cols[k].push(i);
        heights[k] += c.2.len() + 2;
    }
    let id = egui::Id::new("okno-pomocy");
    glass_area(a, ctx, id, 1.0);
    let mut close = false;
    egui::Window::new("pomoc")
        .id(id)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .frame(theme::glass().inner_margin(egui::Margin::same(22)))
        .show(ctx, |ui| {
            ui.set_width(width);
            // nagłówek: logo, nazwa z wersją, krótki opis, zamknięcie
            ui.horizontal(|ui| {
                let logo = theme::logo(ctx);
                ui.add(egui::Image::new((logo.id(), egui::vec2(64.0, 64.0))));
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("GRAŃ").size(30.0).strong());
                        ui.label(RichText::new(env!("CARGO_PKG_VERSION")).size(13.0).color(theme::paper_3()));
                    });
                    ui.label(RichText::new(tr("Atlas gór w 3D: teren, szlaki, trasy i warunki")).size(14.0).color(theme::paper_2()));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if ui.add(egui::Button::new(RichText::new(theme::icon::X).size(18.0)).frame(false)).on_hover_text(tr("Zamknij (Esc)")).clicked() {
                        close = true;
                    }
                });
            });
            ui.add_space(8.0);
            theme::para(ui, tr("Atlas łączy rzeczywisty model terenu z siecią szlaków OSM. Wszystkie obliczenia – trasy, widoczność, cienie – odbywają się lokalnie na tym komputerze."));
            ui.add_space(10.0);
            egui::ScrollArea::vertical().max_height((screen.height() - 300.0).max(220.0)).show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                ui.columns(n_cols, |c| {
                    for (k, list) in cols.iter().enumerate() {
                        for &i in list {
                            let (glyph, title, rows) = &cards[i];
                            help_card(&mut c[k], glyph, title, rows);
                            c[k].add_space(12.0);
                        }
                    }
                });
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(theme::icon::FILE_ARROW_UP).size(15.0).color(theme::BRASS));
                ui.label(RichText::new(tr("Pliki GPX, KML i GeoJSON możesz przeciągnąć bezpośrednio na mapę.")).small().color(theme::paper_3()));
            });
        });
    if close {
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

/// Okno „Wczytywanie danych”: mapka sektorów okolicy w barwach stanu i paski postępu.
pub fn load_window(a: &mut Atlas, ctx: &egui::Context) {
    use crate::i18n::tr;
    if !a.load_details {
        return;
    }
    let mut open = true;
    let title = tr("Wczytywanie danych");
    glass_area(a, ctx, egui::Id::new(title), 1.0);
    let ok_c = theme::OK;
    let busy_c = theme::BRASS;
    let wait_c = theme::INK_4;
    let err_c = theme::SIGNAL;
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::RIGHT_BOTTOM, [-80.0 - right_inset(a), -16.0 - bottom_inset(a)])
        .open(&mut open)
        .show(ctx, |ui| {
            ui.set_width(330.0);
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
            let grid = a.world.sector_grid();
            let done = grid.iter().filter(|g| g.2 == 0).count();
            // 1. nazwy i szlaki: mapka 3 × 3 + postęp
            ui.horizontal(|ui| {
                let cell = 30.0;
                let (rect, _) = ui.allocate_exact_size(egui::vec2(cell * 3.0 + 8.0, cell * 3.0 + 8.0), egui::Sense::hover());
                let t = ui.input(|i| i.time) as f32;
                for (dx, dy, st, txt) in &grid {
                    if dx.abs() > 1 || dy.abs() > 1 {
                        continue;
                    }
                    let r = egui::Rect::from_min_size(
                        rect.min + egui::vec2((*dx + 1) as f32 * (cell + 4.0), (*dy + 1) as f32 * (cell + 4.0)),
                        egui::vec2(cell, cell),
                    );
                    let c = match st {
                        0 => ok_c,
                        1 => busy_c.gamma_multiply(0.55 + 0.45 * (t * 4.0).sin().abs()),
                        3 => err_c,
                        _ => wait_c,
                    };
                    ui.painter().rect_filled(r, 6.0, c.gamma_multiply(if *st == 0 { 0.8 } else { 1.0 }));
                    if *dx == 0 && *dy == 0 {
                        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, theme::icon::CROSSHAIR, egui::FontId::proportional(15.0), theme::INK_0);
                    }
                    ui.interact(r, ui.id().with(("sektor", dx, dy)), egui::Sense::hover()).on_hover_text(txt);
                }
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(tr("Nazwy i szlaki")).strong());
                    ui.label(RichText::new(tf!("{} z {} sektorów gotowe", done, grid.len())).color(theme::paper_2()));
                    theme::bar(ui, done as f32 / grid.len().max(1) as f32, ok_c);
                    if let Some(w) = a.world.progress().2 {
                        ui.label(RichText::new(tf!("w toku: {}", w)).small().color(busy_c));
                    }
                    ui.horizontal(|ui| {
                        for (c, t) in [(ok_c, "gotowe"), (busy_c, "w toku"), (wait_c, "kolejka"), (err_c, "błąd")] {
                            ui.label(RichText::new("■").color(c));
                            ui.label(RichText::new(tr(t)).small().color(theme::paper_3()));
                        }
                    });
                });
            });
            ui.add_space(8.0);
            ui.separator();
            // 2. teren i zdjęcia
            let row = |ui: &mut egui::Ui, glyph: &str, name: &str, state: String, ok: bool| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(glyph).color(if ok { ok_c } else { busy_c }));
                    ui.label(RichText::new(tr(name)).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(state).color(if ok { ok_c } else { theme::paper_2() }));
                    });
                });
            };
            if let Some(r) = &a.renderer {
                let [dem, sat, ortho, local] = r.tiles.loading_kinds;
                let left = dem + sat + ortho + local;
                let st = if left == 0 { tr("gotowe").to_string() } else { tf!("{} kafli w kolejce", left) };
                row(ui, theme::icon::MOUNTAINS, "Teren i zdjęcia w kadrze", st, left == 0);
            }
            let land = a.world.land_busy();
            row(ui, theme::icon::TREE_EVERGREEN, "Lasy, woda, skały", if land { tr("pobieranie…").to_string() } else { tr("gotowe").to_string() }, !land);
            ui.add_space(4.0);
            ui.label(RichText::new(tr("Pobrane dane zapisują się na dysku – przy następnym uruchomieniu wczytują się od razu.")).small().color(theme::paper_3()));
        });
    if !open {
        a.load_details = false;
    }
}
