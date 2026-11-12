//! Panele trasy, porównania tras, pomiaru i soczewek.

use super::*;

/// Czy zdążysz przed zmrokiem, wychodząc o wybranej porze.
pub(super) fn daylight(a: &Atlas, ui: &mut egui::Ui, r: &Route) {
    let (day, start) = a.when();
    let t = sun::times(day, a.cam.lat, a.cam.lon);
    let end = start + r.stats.time;
    section(ui, theme::icon::SUN_HORIZON, crate::i18n::tr("Plan dnia"));
    grid2(ui, vec![(crate::i18n::tr("Wyjście"), fmt::hour(start)), (crate::i18n::tr("Dojście"), fmt::hour(end.rem_euclid(24.0))), (crate::i18n::tr("Wschód słońca"), fmt::hour(t.rise)), (crate::i18n::tr("Zachód słońca"), fmt::hour(t.set))]);
    let spare = t.set - end;
    let (txt, col) = if !spare.is_finite() {
        (crate::i18n::tr("Nie da się wyliczyć zapasu światła").to_string(), theme::paper_3())
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

/// Barwy podejść i zejść w danych trasy.
const UP_COLOR: Color32 = Color32::from_rgb(240, 160, 110);
const DOWN_COLOR: Color32 = Color32::from_rgb(130, 190, 240);

/// Szczyty, przełęcze i schroniska przy śladzie trasy: (indeks punktu śladu, indeks miejsca), w kolejności marszu.
fn passed_pois(a: &Atlas, r: &Route) -> Vec<(usize, usize)> {
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
    hits.sort();
    hits
}

/// Szczyty, przełęcze i schroniska mijane po drodze: dystans, wysokość i godzina dojścia.
/// Najechanie na wiersz pokazuje to miejsce na trasie (mapa i profil), klik przenosi widok.
pub(super) fn on_route(a: &mut Atlas, ui: &mut egui::Ui, r: &Route) {
    let hits = passed_pois(a, r);
    if hits.is_empty() {
        return;
    }
    if !fold(ui, "po-drodze-zw", theme::icon::MAP_PIN, &tf!("Po drodze ({})", hits.len()), false) {
        return;
    }
    let tr = &r.track;
    let (_, start) = a.when();
    for (i, pi) in hits.into_iter().take(16) {
        let q = a.snap.pois[pi].clone();
        let (glyph, color) = kind_style(q.poi.kind);
        let t = tr.t.get(i).copied().unwrap_or(0.0) as f64;
        let d = tr.d2.get(i).copied().unwrap_or(0.0);
        let sub = format!("{} · {}", fmt::dist(d as f64), fmt::ele(q.ele as f64));
        let resp = rich_row(ui, glyph, color, &q.poi.name, &sub, &fmt::hour((start + t).rem_euclid(24.0)), &tf!("po {}", fmt::time(t)));
        if resp.hovered() {
            a.panel_cursor = Some(d);
        }
        if resp.clicked() {
            a.fly_to(q.poi.lon, q.poi.lat, None);
        }
    }
}

pub(super) fn time_model(a: &mut Atlas, ui: &mut egui::Ui) {
    let (mut m, mut pace) = metrics::model();
    let before = (m, pace);
    ui.horizontal(|ui| {
        ui.label(RichText::new(crate::i18n::tr("Model")).color(theme::paper_3()));
        ui.selectable_value(&mut m, TimeModel::Pttk, crate::i18n::tr("reguła PTTK"));
        ui.selectable_value(&mut m, TimeModel::Din, "DIN 33466");
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new(crate::i18n::tr("Tempo")).color(theme::paper_3()));
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

/// Karta trasy: czas z godzinami wyjścia i dojścia, trudność, dystans i przewyższenia oraz mały profil.
/// Kursor nad profilem pokazuje to miejsce na mapie; punkty pośrednie są na nim zaznaczone kropkami.
fn route_hero(a: &mut Atlas, ui: &mut egui::Ui, r: &Route) {
    let s = r.stats;
    let (_, start) = a.when();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 172.0), egui::Sense::hover());
    let p = ui.painter().with_clip_rect(rect);
    p.rect_filled(rect, 14.0, CARD_FILL);
    p.rect_stroke(rect, 14.0, egui::Stroke::new(1.0, CARD_EDGE), egui::StrokeKind::Inside);
    let inner = rect.shrink2(egui::vec2(14.0, 12.0));
    // czas dużym pismem, pod nim godziny wyjścia i dojścia
    p.text(inner.left_top(), egui::Align2::LEFT_TOP, fmt::time(s.time), egui::FontId::proportional(28.0), theme::PAPER);
    let hours = format!("{}  {} → {}", theme::icon::CLOCK, fmt::hour(start), fmt::hour((start + s.time).rem_euclid(24.0)));
    p.text(inner.left_top() + egui::vec2(0.0, 37.0), egui::Align2::LEFT_TOP, hours, egui::FontId::proportional(12.0), theme::paper_3());
    // trudność w prawym górnym rogu
    let dc = DIFF_COLORS[s.difficulty.min(3)];
    let dg = p.layout_no_wrap(crate::i18n::tr(DIFFICULTY[s.difficulty.min(3)]).to_string(), egui::FontId::proportional(12.0), dc);
    let pill = egui::Rect::from_min_size(egui::pos2(inner.right() - dg.size().x - 18.0, inner.top() + 3.0), dg.size() + egui::vec2(18.0, 8.0));
    p.rect_filled(pill, 10.0, dc.gamma_multiply(0.16));
    p.galley(pill.min + egui::vec2(9.0, 4.0), dg, dc);
    // dystans, podejścia, zejścia
    let y = inner.top() + 72.0;
    let cw = inner.width() / 3.0;
    let cells = [
        (theme::icon::RULER, fmt::dist(s.len2), theme::BRASS),
        (theme::icon::TREND_UP, fmt::ele(s.up), UP_COLOR),
        (theme::icon::TREND_DOWN, fmt::ele(s.down), DOWN_COLOR),
    ];
    for (k, (g, v, c)) in cells.into_iter().enumerate() {
        let x = inner.left() + k as f32 * cw;
        p.text(egui::pos2(x, y), egui::Align2::LEFT_CENTER, g, egui::FontId::proportional(15.0), c);
        p.text(egui::pos2(x + 22.0, y), egui::Align2::LEFT_CENTER, v, egui::FontId::proportional(15.5), theme::PAPER);
    }
    // mały profil wysokości w osi dystansu
    let tr = &r.track;
    if tr.len() < 2 {
        return;
    }
    let plot = egui::Rect::from_min_max(egui::pos2(inner.left(), y + 22.0), inner.right_bottom());
    let total = tr.d2.last().copied().unwrap_or(1.0).max(1.0);
    let (lo, hi) = tr.e.iter().fold((f32::MAX, f32::MIN), |(l, h), &v| (l.min(v), h.max(v)));
    let span = (hi - lo).max(1.0);
    let px = |d: f32| plot.left() + d / total * plot.width();
    let py = |e: f32| plot.bottom() - (e - lo) / span * (plot.height() - 6.0);
    let ele = |d: f32| {
        let (i, f) = tr.at_dist(d);
        tr.lerp(i, f).2
    };
    let n = (plot.width() / 2.0).max(2.0) as usize;
    let pts: Vec<egui::Pos2> = (0..=n)
        .map(|k| {
            let d = total * k as f32 / n as f32;
            egui::pos2(px(d), py(ele(d)))
        })
        .collect();
    for q in &pts {
        p.line_segment([*q, egui::pos2(q.x, plot.bottom())], egui::Stroke::new(2.0, theme::BRASS.gamma_multiply(0.2)));
    }
    p.add(egui::Shape::line(pts, egui::Stroke::new(1.8, theme::BRASS)));
    // punkty pośrednie trasy
    for &li in r.leg_idx.iter().filter(|&&li| li > 0 && li + 1 < tr.len()) {
        p.circle(egui::pos2(px(tr.d2[li]), py(tr.e[li])), 3.5, theme::PAPER, egui::Stroke::new(1.5, theme::INK_0));
    }
    let hover = resp.hover_pos().filter(|q| plot.contains(*q)).map(|q| ((q.x - plot.left()) / plot.width() * total).clamp(0.0, total));
    if hover.is_some() {
        a.panel_cursor = hover;
    }
    if let Some(d) = hover.or(a.profile_cursor) {
        let d = d.clamp(0.0, total);
        let e = ele(d);
        let x = px(d);
        p.line_segment([egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())], egui::Stroke::new(1.0, theme::PAPER.gamma_multiply(0.5)));
        p.circle(egui::pos2(x, py(e)), 4.5, theme::BRASS, egui::Stroke::new(2.0, theme::INK_0));
        if hover.is_some() {
            let (i, f) = tr.at_dist(d);
            let j = (i + 1).min(tr.len() - 1);
            let t = (tr.t[i] + (tr.t[j] - tr.t[i]) * f) as f64;
            let txt = format!("{} · {} · {}", fmt::dist(d as f64), fmt::ele(e as f64), fmt::hour((start + t).rem_euclid(24.0)));
            let g = p.layout_no_wrap(txt, egui::FontId::proportional(11.5), theme::PAPER);
            let gx = (x + 8.0).min(plot.right() - g.size().x - 5.0).max(plot.left() + 5.0);
            let at = egui::pos2(gx, plot.top());
            p.rect_filled(egui::Rect::from_min_size(at, g.size()).expand2(egui::vec2(5.0, 2.0)), 6.0, theme::POPUP_FILL);
            p.galley(at, g, theme::PAPER);
        }
    }
}

/// Zmiana na liście punktów trasy wybrana w tej klatce.
enum WaypointEdit {
    Remove(usize),
    Swap(usize, usize),
    Show(usize),
}

/// Lista punktów trasy: znaczniki jak na mapie, nazwy, godziny dojścia, zmiana kolejności
/// i dane odcinków między kolejnymi punktami.
fn waypoint_list(a: &mut Atlas, ui: &mut egui::Ui) {
    use crate::i18n::tr;
    let wps = a.waypoints.clone();
    let n = wps.len();
    // odcinki między punktami – gdy wyznaczona trasa odpowiada bieżącym punktom
    let route = a
        .routes
        .get(a.route_sel)
        .filter(|r| r.leg_idx.len() == n && r.track.len() >= 2 && r.leg_idx.iter().all(|&i| i < r.track.len()))
        .cloned();
    let (_, start) = a.when();
    let mut edit: Option<WaypointEdit> = None;
    let mut hover_d: Option<f32> = None;
    for (i, w) in wps.iter().enumerate() {
        if i > 0 {
            // odcinek od poprzedniego punktu: kropki łączące znaczniki i dane odcinka
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 20.0), egui::Sense::hover());
            let p = ui.painter().with_clip_rect(rect);
            for k in 0..3 {
                p.circle_filled(egui::pos2(rect.left() + 13.0, rect.top() + 4.0 + k as f32 * 6.0), 1.5, theme::paper_3());
            }
            if let Some(r) = &route {
                let st = profile::segment(r, r.track.d2[r.leg_idx[i - 1]], r.track.d2[r.leg_idx[i]]);
                let txt = format!("{} · ↑ {} · ↓ {} · {}", fmt::dist(st.dist), fmt::ele(st.up), fmt::ele(st.down), fmt::time(st.time));
                p.text(egui::pos2(rect.left() + 34.0, rect.center().y), egui::Align2::LEFT_CENTER, txt, egui::FontId::proportional(11.5), theme::paper_3());
            }
        }
        let row = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let (fill, tag) = if i == 0 {
                (Color32::from_rgb(90, 200, 110), "A".to_string())
            } else if i == n - 1 {
                (theme::SIGNAL, "B".to_string())
            } else {
                (theme::BRASS, i.to_string())
            };
            let (badge, _) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::hover());
            ui.painter().circle_filled(badge.center(), 11.0, fill);
            ui.painter().text(badge.center(), egui::Align2::CENTER_CENTER, tag, egui::FontId::proportional(12.0), Color32::from_rgb(20, 16, 8));
            ui.add_space(4.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(theme::icon::X).on_hover_text(tr("Usuń punkt")).clicked() {
                    edit = Some(WaypointEdit::Remove(i));
                }
                if ui.add_enabled(i + 1 < n, egui::Button::new(theme::icon::ARROW_DOWN).small()).on_hover_text(tr("Przesuń niżej")).clicked() {
                    edit = Some(WaypointEdit::Swap(i, i + 1));
                }
                if ui.add_enabled(i > 0, egui::Button::new(theme::icon::ARROW_UP).small()).on_hover_text(tr("Przesuń wyżej")).clicked() {
                    edit = Some(WaypointEdit::Swap(i, i.saturating_sub(1)));
                }
                // godzina dojścia (gdy trasa wyznaczona) i wysokość
                let info = match &route {
                    Some(r) => format!("{} · {}", fmt::hour((start + r.track.t[r.leg_idx[i]] as f64).rem_euclid(24.0)), fmt::ele(w.e as f64)),
                    None => fmt::ele(w.e as f64),
                };
                ui.label(RichText::new(info).small().color(theme::paper_3()));
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let name = w.name.clone().unwrap_or_else(|| fmt::coords(w.lon, w.lat));
                    if ui.add(egui::Label::new(RichText::new(name).strong()).truncate().sense(egui::Sense::click())).on_hover_text(tr("Pokaż na mapie")).clicked() {
                        edit = Some(WaypointEdit::Show(i));
                    }
                });
            });
        });
        if let Some(r) = &route {
            if ui.rect_contains_pointer(row.response.rect) {
                hover_d = Some(r.track.d2[r.leg_idx[i]]);
            }
        }
    }
    if hover_d.is_some() {
        a.panel_cursor = hover_d;
    }
    match edit {
        Some(WaypointEdit::Remove(i)) => a.remove_waypoint(i),
        Some(WaypointEdit::Swap(i, j)) => a.swap_waypoints(i, j),
        Some(WaypointEdit::Show(i)) => a.fly_to(wps[i].lon, wps[i].lat, None),
        None => {}
    }
}

/// Działania na trasie jako kafelki: widok, profil, przelot, zapis, eksport i udostępnianie.
fn route_actions(a: &mut Atlas, ui: &mut egui::Ui) {
    use crate::i18n::tr;
    let flying = a.flight.is_some();
    let mut act: Option<u8> = None;
    ui.scope(|ui| {
        theme::tile_grid(ui, 3, |ui, w| {
            if theme::action(ui, theme::icon::ARROWS_OUT, "Pokaż całą", w).clicked() {
                act = Some(0);
            }
            if theme::action(ui, theme::icon::CHART_LINE, "Profil", w).on_hover_text(tr("Profil wysokości na dole okna")).clicked() {
                act = Some(1);
            }
            if theme::action(ui, theme::icon::AIRPLANE_TILT, if flying { "Zatrzymaj" } else { "Przelot" }, w).on_hover_text("F").clicked() {
                act = Some(2);
            }
            if theme::action(ui, theme::icon::BOOKMARKS, "Zapisz", w).on_hover_text(tr("Zapisz w bibliotece (Ctrl+S)")).clicked() {
                act = Some(3);
            }
            if theme::action(ui, theme::icon::EXPORT, "Eksport", w).on_hover_text(tr("Eksport (GPX, KML, GeoJSON)")).clicked() {
                act = Some(4);
            }
            if theme::action(ui, theme::icon::WATCH, "Na zegarek", w).on_hover_text(tr("Garmin, Suunto, Wahoo, Coros: ślad z planowanym czasem i punktami trasy")).clicked() {
                act = Some(5);
            }
            if theme::action(ui, theme::icon::LINK, "Kopiuj link", w).on_hover_text(tr("Link gran:// z widokiem i punktami trasy")).clicked() {
                act = Some(6);
            }
            if theme::action(ui, theme::icon::ARROW_SQUARE_OUT, "Mapy.com", w).on_hover_text(tr("Ta sama trasa w planerze Mapy.com (także w aplikacji na telefonie)")).clicked() {
                act = Some(7);
            }
            if theme::action(ui, theme::icon::FILE_ARROW_UP, "Wczytaj ślad", w).on_hover_text(tr("Wczytaj ślad (GPX, KML, GeoJSON)…")).clicked() {
                act = Some(8);
            }
        });
    });
    match act {
        Some(0) => {
            if let Some(r) = a.current_route().cloned() {
                a.fit_route(&r);
            }
        }
        Some(1) => a.prefs.profile_open = !a.prefs.profile_open,
        Some(2) => a.toggle_flight(),
        Some(3) => a.save_route_to_library(),
        Some(4) => a.export_route(),
        Some(5) => a.export_route_as("fit"),
        Some(6) => {
            let l = a.share_link();
            ui.ctx().copy_text(l);
            a.notify(tr("Skopiowano link z trasą."));
        }
        Some(7) => a.open_in_mapy(),
        Some(8) => {
            if let Some(p) = pick_track_file() {
                a.import_track(&p);
            }
        }
        _ => {}
    }
}

pub fn route_info(a: &mut Atlas, ui: &mut egui::Ui) {
    use crate::i18n::tr;
    let n = a.waypoints.len();
    // nazwa trasy („start – cel”) albo zachęta do rozpoczęcia
    let title = if a.current_route().is_some() { a.route_name() } else { tr("Nowa trasa").to_string() };
    ui.add(egui::Label::new(RichText::new(title).size(21.0).strong()).truncate());
    // sposób wyznaczania: dwa równe przyciski
    let before = a.route_mode;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let w = ((ui.available_width() - 6.0) / 2.0).floor();
        if ui.add_sized([w, 34.0], egui::Button::new(theme::ic(theme::icon::SIGNPOST, "Szlakami")).selected(a.route_mode == RouteMode::Trails)).clicked() {
            a.route_mode = RouteMode::Trails;
        }
        if ui
            .add_sized([w, 34.0], egui::Button::new(theme::ic(theme::icon::MOUNTAINS, "Przez teren")).selected(a.route_mode == RouteMode::Terrain))
            .on_hover_text(tr("A* po modelu terenu z funkcją Toblera i limitem nachylenia – analiza rzeźby, nie zachęta do schodzenia ze szlaków w parkach narodowych."))
            .clicked()
        {
            a.route_mode = RouteMode::Terrain;
        }
    });
    if before != a.route_mode {
        a.mark_route_dirty();
    }
    if a.routing {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new(tr("wyznaczam trasę…")).color(theme::paper_3()));
        });
    }
    if let Some(e) = &a.route_error {
        ui.add(egui::Label::new(RichText::new(e).color(theme::WARN)).wrap());
    }
    let current = a.current_route().cloned();
    if let Some(r) = &current {
        ui.add_space(2.0);
        route_hero(a, ui, r);
    }

    // punkty trasy
    section(ui, theme::icon::MAP_PIN, &tf!("Punkty ({})", n));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if ui.add_enabled(a.can_undo(), egui::Button::new(theme::icon::ARROW_COUNTER_CLOCKWISE).small()).on_hover_text(tr("Cofnij (Ctrl+Z)")).clicked() {
            a.undo_route();
        }
        if ui.add_enabled(a.can_redo(), egui::Button::new(theme::icon::ARROW_CLOCKWISE).small()).on_hover_text(tr("Ponów (Ctrl+Y)")).clicked() {
            a.redo_route();
        }
        if n >= 2 {
            if ui.small_button(theme::ic(theme::icon::ARROWS_DOWN_UP, "Odwróć")).on_hover_text(tr("Odwróć kierunek trasy")).clicked() {
                a.reverse_route();
            }
            let closed = a.waypoints.first().zip(a.waypoints.last()).is_some_and(|(f, l)| haversine(f.lon, f.lat, l.lon, l.lat) < 30.0);
            if !closed {
                if ui.small_button(theme::ic(theme::icon::REPEAT, "Pętla")).on_hover_text(tr("Dodaje powrót do punktu startu")).clicked() {
                    a.close_loop();
                }
                if ui.small_button(theme::ic(theme::icon::ARROW_U_DOWN_LEFT, "Tam i z powrotem")).on_hover_text(tr("Powrót do startu przez te same punkty")).clicked() {
                    a.there_and_back();
                }
            }
        }
        if n > 0 && ui.small_button(theme::ic(theme::icon::TRASH, "Wyczyść")).clicked() {
            a.clear_route();
        }
    });
    if a.waypoints.is_empty() {
        theme::para(ui, tr("Klikaj na mapie, żeby dodać punkty trasy (start, przez, cel). Punkty można przeciągać; prawy przycisk daje więcej opcji."));
    } else {
        waypoint_list(a, ui);
    }
    closures_warning(ui, &a.route_closures());

    // warianty
    if a.routes.len() > 1 {
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

    let Some(r) = current else {
        ui.add_space(8.0);
        if theme::row_button(ui, theme::icon::FILE_ARROW_UP, tr("Wczytaj ślad (GPX, KML, GeoJSON)…"), false).clicked() {
            if let Some(p) = pick_track_file() {
                a.import_track(&p);
            }
        }
        return;
    };
    section(ui, theme::icon::LIGHTNING, tr("Działania"));
    route_actions(a, ui);

    if fold(ui, "czas-przejscia", theme::icon::CLOCK, tr("Czas przejścia"), true) {
        time_model(a, ui);
    }
    daylight(a, ui, &r);

    if fold(ui, "dane-trasy", theme::icon::CHART_LINE, tr("Dane trasy"), true) {
        let s = r.stats;
        let mut items = vec![
            (tr("Najwyżej"), fmt::ele(s.max_ele)),
            (tr("Najniżej"), fmt::ele(s.min_ele)),
            (tr("Po powierzchni"), fmt::dist(s.len3)),
            (tr("Linia prosta"), fmt::dist(s.straight)),
            (tr("Maks. nachylenie"), format!("{}°", s.max_slope.round())),
            (tr("Śr. tempo"), tf!("{} km/h", fmt::dec(s.len3 / 1000.0 / s.time.max(0.01), 1))),
            (tr("Krętość"), format!("×{}", fmt::dec(s.len2 / s.straight.max(1.0), 2))),
        ];
        if r.kind == RouteKind::Trail {
            items.push((tr("Punkty GOT"), fmt::int(s.got)));
            items.push((tr("Skala SAC"), tr(SAC_LABEL[s.sac_max.min(6) as usize]).split(' ').next().unwrap_or("—").to_string()));
        }
        grid2(ui, items);
        ui.add_space(6.0);
        ui.label(theme::kicker(tr("Stromizna trasy")));
        slope_bar(ui, &r);
    }
    on_route(a, ui, &r);
    if !r.route_refs.is_empty() && fold(ui, "szlaki-na-trasie-zw", theme::icon::SIGNPOST, tr("Szlaki na trasie"), false) {
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
    compare_section(a, ui);
    access_section(a, ui);
}

/// Ostrzeżenie o zamkniętych odcinkach (z OSM) na trasie albo szlaku.
pub fn closures_warning(ui: &mut egui::Ui, closures: &[crate::osm::Closure]) {
    if closures.is_empty() {
        return;
    }
    egui::Frame::new().fill(Color32::from_rgb(70, 22, 22)).corner_radius(egui::CornerRadius::same(10)).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(theme::ic(theme::icon::PROHIBIT, "Zamknięty odcinek")).strong().color(Color32::from_rgb(255, 140, 130)));
        for c in closures {
            let what = if c.name.is_empty() { crate::i18n::tr("odcinek szlaku").to_string() } else { c.name.clone() };
            let when = if c.periods.is_empty() {
                crate::i18n::tr("zamknięty").to_string()
            } else {
                let p: Vec<String> = c.periods.iter().map(|&((m1, d1), (m2, d2))| format!("{d1}.{m1:02}–{d2}.{m2:02}")).collect();
                tf!("zamknięty sezonowo ({})", p.join(", "))
            };
            ui.add(egui::Label::new(RichText::new(format!("{what}: {when}")).color(theme::PAPER)).wrap());
            ui.label(RichText::new(&c.rule).small().monospace().color(theme::paper_3()));
        }
        theme::para(ui, "Na podstawie OpenStreetMap. Aktualne zamknięcia sprawdź w komunikacie parku (zakładka Warunki).");
    });
}

/// Dojazd do początku trasy: najbliższe stacje, przystanki i parkingi z odnośnikami do planerów.
fn access_section(a: &mut Atlas, ui: &mut egui::Ui) {
    let Some(start) = a.waypoints.first().cloned() else { return };
    section(ui, theme::icon::BUS, crate::i18n::tr("Dojazd do początku"));
    let snap = a.snap.clone();
    let near = crate::transit::nearby(&snap.pois, start.lon, start.lat);
    if near.is_empty() {
        theme::para(ui, "W pobliżu początku trasy nie ma w danych OSM stacji, przystanków ani parkingów.");
    }
    for (i, d) in near {
        let p = &snap.pois[i].poi;
        let (glyph, color) = kind_style(p.kind);
        ui.horizontal(|ui| {
            ui.label(RichText::new(glyph).color(color));
            let r = ui.add(egui::Label::new(RichText::new(&p.name)).sense(egui::Sense::click()));
            ui.label(RichText::new(format!("{} · {}", crate::i18n::tr(p.kind.label()), fmt::dist(d))).small().color(theme::paper_3()));
            if r.on_hover_text(crate::i18n::tr("Pokaż na mapie")).clicked() {
                a.selection = Selection::Poi(i);
                a.fly_to(p.lon, p.lat, Some(1500.0));
            }
        });
    }
    // dwa równe przyciski na całą szerokość panelu (napisy na środku)
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let w = ((ui.available_width() - 6.0) / 2.0).floor();
        if ui
            .add_sized([w, 34.0], egui::Button::new(theme::ic(theme::icon::TRAIN, "Rozkład jazdy")))
            .on_hover_text(crate::i18n::tr("Połączenia komunikacją publiczną do początku trasy (Google Maps)"))
            .clicked()
        {
            crate::transit::open_planner(start.lon, start.lat);
        }
        if ui.add_sized([w, 34.0], egui::Button::new(theme::ic(theme::icon::CAR, "Dojazd autem"))).clicked() {
            let _ = open::that(crate::transit::drive_url(start.lon, start.lat));
        }
    });
}

pub fn measure_info(a: &mut Atlas, ui: &mut egui::Ui) {
    ui.label(theme::kicker(crate::i18n::tr("Pomiar, przekrój i linia wzroku")));
    ui.heading(crate::i18n::tr("Odległość po terenie"));
    if a.measure.is_empty() {
        theme::para(ui, crate::i18n::tr("Klikaj na mapie, żeby dodać punkty pomiaru. Przekrój terenu wzdłuż linii pokaże się na dole, a dla dwóch punktów program sprawdzi też, czy jeden widać z drugiego."));
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

pub(super) fn lens_info(a: &mut Atlas, ui: &mut egui::Ui) {
    let Some((lon, lat, e)) = a.lens_pin else { return };
    let mut unpin = false;
    match a.lens {
        Lens::Vis => {
            ui.label(theme::kicker(crate::i18n::tr("Analiza widoczności")));
            ui.heading(crate::i18n::tr("Co widać stąd?"));
            ui.label(RichText::new(format!("{} · {}", fmt::coords(lon, lat), fmt::ele(e as f64))).monospace().small().color(theme::paper_3()));
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
                    ui.label(RichText::new(crate::i18n::tr("liczę promienie widzenia…")).color(theme::paper_3()));
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
            theme::para(ui, tf!("Poziom 0: {} · {}", fmt::ele(e as f64), fmt::coords(lon, lat)));
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
                            ui.label(RichText::new(fmt::dist(d)).small().color(theme::paper_3()));
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

/// Porównanie bieżącej trasy z wariantem, wczytanym śladem albo trasą z biblioteki.
pub(super) fn compare_section(a: &mut Atlas, ui: &mut egui::Ui) {
    use crate::i18n::tr;
    section(ui, theme::icon::ARROWS_LEFT_RIGHT, tr("Porównanie tras"));
    // kandydaci: inne warianty, ślad GPX, trasy z biblioteki ze śladem
    let mut opts: Vec<(String, Route)> = Vec::new();
    for (i, r) in a.routes.iter().enumerate() {
        if i != a.route_sel {
            opts.push((tf!("Wariant: {}", r.labels.join(" · ")), r.clone()));
        }
    }
    if let Some(g) = &a.gpx {
        if a.current_route().map(|c| !std::ptr::eq(c, g)).unwrap_or(false) {
            opts.push((tf!("Ślad: {}", g.labels.first().cloned().unwrap_or_default()), g.clone()));
        }
    }
    let lib: Vec<(String, Vec<(f64, f64, f32)>)> = a.library.routes.iter().filter(|r| r.track.len() >= 2).map(|r| (r.name.clone(), r.track.clone())).collect();
    let current = a.compare.as_ref().map(|c| c.0.clone());
    let mut pick: Option<Option<(String, Route)>> = None;
    egui::ComboBox::from_id_salt("porownanie")
        .width(ui.available_width() - 8.0)
        .selected_text(current.clone().unwrap_or_else(|| tr("Porównaj z…").to_string()))
        .show_ui(ui, |ui| {
            if current.is_some() && ui.selectable_label(false, tr("Bez porównania")).clicked() {
                pick = Some(None);
            }
            for (name, r) in &opts {
                if ui.selectable_label(current.as_deref() == Some(name.as_str()), name).clicked() {
                    pick = Some(Some((name.clone(), r.clone())));
                }
            }
            for (name, track) in &lib {
                let label = tf!("Biblioteka: {}", name);
                if ui.selectable_label(current.as_deref() == Some(label.as_str()), &label).clicked() {
                    let r = crate::graph::route_from_points(track, RouteKind::Gpx, name, 1.0);
                    pick = Some(Some((label.clone(), r)));
                }
            }
            if opts.is_empty() && lib.is_empty() {
                ui.label(RichText::new(tr("Brak innych tras – zapisz trasy w bibliotece albo wczytaj ślad.")).small().color(theme::paper_3()));
            }
        });
    if let Some(p) = pick {
        a.compare = p;
        a.prefs.profile_open = true;
    }
    let (Some(cur), Some((name, other))) = (a.current_route().cloned(), a.compare.clone()) else {
        theme::para(ui, "Wybierz drugą trasę: pojawi się na mapie i na profilu (fioletowa), a tu – zestawienie liczb.");
        return;
    };
    let (s, o) = (cur.stats, other.stats);
    ui.label(RichText::new(name).small().color(crate::ui::profile::COMPARE));
    let rows: [(&str, f64, f64, fn(f64) -> String, bool); 7] = [
        ("Czas", s.time, o.time, fmt::time, true),
        ("Dystans", s.len2, o.len2, fmt::dist, true),
        ("Podejścia", s.up, o.up, fmt::ele, true),
        ("Zejścia", s.down, o.down, fmt::ele, true),
        ("Najwyżej", s.max_ele, o.max_ele, fmt::ele, false),
        ("Maks. nachylenie", s.max_slope, o.max_slope, |v| format!("{}°", v.round()), true),
        ("Punkty GOT", s.got, o.got, |v| fmt::int(v), false),
    ];
    // tabela na całą szerokość panelu, rysowana ręcznie: stałe kolumny, nic się nie ucina
    const ROW_H: f32 = 26.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H * (rows.len() as f32 + 1.0)), egui::Sense::hover());
    let p = ui.painter().with_clip_rect(rect);
    p.rect_filled(rect, 12.0, CARD_FILL);
    p.rect_stroke(rect, 12.0, egui::Stroke::new(1.0, CARD_EDGE), egui::StrokeKind::Inside);
    let inner = rect.shrink2(egui::vec2(12.0, 0.0));
    // kolumny: nazwa, ta trasa, porównywana, różnica
    let col = |k: usize| inner.left() + inner.width() * [0.0, 0.34, 0.56, 0.78][k];
    // napis w komórce; gdy za szeroki – mniejszym pismem
    let cell = |k: usize, y: f32, text: String, size: f32, color: Color32| {
        let room = if k == 3 { inner.right() - col(3) } else { col(k + 1) - col(k) - 6.0 };
        let mut g = p.layout_no_wrap(text.clone(), egui::FontId::proportional(size), color);
        if g.size().x > room {
            g = p.layout_no_wrap(text, egui::FontId::proportional(size - 2.5), color);
        }
        p.galley(egui::pos2(col(k), y - g.size().y / 2.0), g, color);
    };
    let y0 = rect.top() + ROW_H / 2.0;
    cell(1, y0, tr("ta trasa").to_string(), 11.5, theme::paper_3());
    cell(2, y0, tr("porównywana").to_string(), 11.5, crate::ui::profile::COMPARE);
    cell(3, y0, tr("różnica").to_string(), 11.5, theme::paper_3());
    for (i, (k, a1, b1, f, lower_better)) in rows.into_iter().enumerate() {
        let top = rect.top() + ROW_H * (i as f32 + 1.0);
        p.line_segment([egui::pos2(inner.left(), top), egui::pos2(inner.right(), top)], egui::Stroke::new(1.0, CARD_EDGE));
        let y = top + ROW_H / 2.0;
        cell(0, y, tr(k).to_string(), 13.0, theme::paper_2());
        cell(1, y, f(a1), 13.5, theme::PAPER);
        cell(2, y, f(b1), 13.5, theme::PAPER);
        let d = b1 - a1;
        let tiny = d.abs() < a1.abs().max(1.0) * 0.01;
        let color = if tiny { theme::paper_3() } else if (d < 0.0) == lower_better { theme::OK } else { theme::WARN };
        let sign = if d > 0.0 { "+" } else if d < 0.0 { "−" } else { "" };
        cell(3, y, if tiny { "≈".to_string() } else { format!("{sign}{}", f(d.abs())) }, 13.5, color);
    }
    theme::para(ui, "Zielona różnica: porównywana trasa jest pod tym względem łatwiejsza lub krótsza.");
}
