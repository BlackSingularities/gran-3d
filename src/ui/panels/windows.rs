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
            ui.horizontal(|ui| {
                ui.label(RichText::new(theme::icon::CHART_LINE_UP).size(17.0).color(theme::BRASS));
                ui.label(RichText::new(crate::i18n::tr(if is_section { "Przekrój terenu" } else { "Profil wysokości" })).strong());
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
                chip(ui, theme::icon::ARROW_LINE_UP, fmt::ele(hi as f64), theme::paper_2());
                chip(ui, theme::icon::ARROW_LINE_DOWN, fmt::ele(lo as f64), theme::paper_2());
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
                        ui.label(RichText::new(name).size(10.5).color(theme::paper_3()));
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
            let compare = a.compare.as_ref().map(|c| &c.1);
            let h = profile::show(ui, &route, &others, &marks, cursor, None, &mut a.profile_sel, compare);
            a.profile_cursor = h.or(a.flight.as_ref().map(|f| f.dist));
            });
        });
}

pub fn help(a: &mut Atlas, ctx: &egui::Context) {
    if !a.help {
        return;
    }
    // sterowanie kamerą i myszą; skróty poleceń – z rejestru poleceń
    let rows = [
        ("Ctrl+K", crate::i18n::tr("paleta poleceń: wszystkie polecenia z wyszukiwaniem")),
        (crate::i18n::tr("strzałki · Q E · PgUp PgDn · Home End"), crate::i18n::tr("przesuwanie · obrót · nachylenie · przybliżanie")),
        (crate::i18n::tr("W A S D · Shift · spacja"), crate::i18n::tr("spacer: chodzenie · bieg · skok (prawy przycisk – rozglądanie)")),
        (crate::i18n::tr("lewy przycisk"), crate::i18n::tr("przesuwanie (chwyć teren)")),
        (crate::i18n::tr("prawy przycisk"), crate::i18n::tr("obrót wokół chwyconego punktu · klik – menu analiz")),
        (crate::i18n::tr("kółko"), crate::i18n::tr("przybliżanie do kursora")),
        (crate::i18n::tr("dwuklik"), crate::i18n::tr("przybliż do punktu")),
        (crate::i18n::tr("klik z soczewką / Esc"), crate::i18n::tr("przypnij / odepnij soczewkę")),
        ("Tab · Shift+Tab · Enter", crate::i18n::tr("przechodzenie po kontrolkach paneli i ich użycie")),
        (crate::i18n::tr("Esc"), crate::i18n::tr("anuluj / wyjdź")),
        ("?", crate::i18n::tr("ta pomoc")),
    ];
    let mut groups: Vec<(crate::ui::atlas::Group, Vec<(String, &'static str)>)> = Vec::new();
    for act in Action::all() {
        let Some(k) = act.keys() else { continue };
        let (_, label, g) = act.info();
        match groups.iter_mut().find(|(x, _)| *x == g) {
            Some((_, v)) => v.push((k.text(), label)),
            None => groups.push((g, vec![(k.text(), label)])),
        }
    }
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
            egui::ScrollArea::vertical().max_height((ctx.screen_rect().height() - 260.0).max(240.0)).show(ui, |ui| {
                ui.label(theme::kicker("Sterowanie"));
                egui::Grid::new("skroty").num_columns(2).striped(true).spacing([24.0, 6.0]).show(ui, |ui| {
                    for (k, v) in rows {
                        ui.label(v);
                        ui.label(RichText::new(k).monospace().color(theme::BRASS));
                        ui.end_row();
                    }
                });
                for (g, list) in &groups {
                    ui.add_space(8.0);
                    ui.label(theme::kicker(g.label()));
                    egui::Grid::new(("skroty", g.label())).num_columns(2).striped(true).spacing([24.0, 6.0]).show(ui, |ui| {
                        for (k, v) in list {
                            ui.label(*v);
                            ui.label(RichText::new(k).monospace().color(theme::BRASS));
                            ui.end_row();
                        }
                    });
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
