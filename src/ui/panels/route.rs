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

/// Szczyty, przełęcze i schroniska mijane po drodze, z czasem dojścia.
pub(super) fn on_route(a: &mut Atlas, ui: &mut egui::Ui, r: &Route) {
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
            ui.label(RichText::new(glyph).size(15.0).color(theme::paper_3()));
            row(ui, &q.poi.name, &eta, &fmt::ele(q.ele as f64))
        }).inner;
        if clicked {
            a.fly_to(q.poi.lon, q.poi.lat, None);
        }
    }
}

pub(super) fn time_model(a: &mut Atlas, ui: &mut egui::Ui) {
    let (mut m, mut pace) = metrics::model();
    let before = (m, pace);
    ui.horizontal(|ui| {
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

pub fn route_info(a: &mut Atlas, ui: &mut egui::Ui) {
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
            ui.label(w.name.clone().unwrap_or_else(|| fmt::coords(w.lon, w.lat)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(theme::icon::X).clicked() {
                    remove = Some(i);
                }
                ui.label(RichText::new(fmt::ele(w.e as f64)).monospace().small().color(theme::paper_3()));
            });
        });
    }
    if let Some(i) = remove {
        a.remove_waypoint(i);
    }
    closures_warning(ui, &a.route_closures());
    section(ui, theme::icon::CLOCK, crate::i18n::tr("Czas przejścia"));
    time_model(a, ui);
    if a.routing {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new(crate::i18n::tr("wyznaczam trasę…")).color(theme::paper_3()));
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
        // najważniejsze liczby dużym pismem, w jednym rzędzie
        summary(ui, &[
            (theme::icon::CLOCK, fmt::time(s.time), theme::PAPER),
            (theme::icon::RULER, fmt::dist(s.len2), theme::PAPER),
            (theme::icon::TREND_UP, fmt::ele(s.up), Color32::from_rgb(240, 160, 110)),
            (theme::icon::TREND_DOWN, fmt::ele(s.down), Color32::from_rgb(130, 190, 240)),
        ]);
        ui.add_space(4.0);
        pill(ui, crate::i18n::tr(DIFFICULTY[s.difficulty.min(3)]), DIFF_COLORS[s.difficulty.min(3)]);
        ui.add_space(2.0);
        let mut items = vec![
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
        compare_section(a, ui);
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
        if theme::row_button(ui, theme::icon::WATCH, crate::i18n::tr("Kurs na zegarek (FIT)"), false)
            .on_hover_text(crate::i18n::tr("Garmin, Suunto, Wahoo, Coros: ślad z planowanym czasem i punktami trasy"))
            .clicked()
        {
            a.export_route_as("fit");
        }
        if a.waypoints.len() >= 2
            && theme::row_button(ui, theme::icon::ARROW_SQUARE_OUT, crate::i18n::tr("Otwórz w Mapy.com"), false)
                .on_hover_text(crate::i18n::tr("Ta sama trasa w planerze Mapy.com (także w aplikacji na telefonie)"))
                .clicked()
        {
            a.open_in_mapy();
        }
        let flying = a.flight.is_some();
        if theme::row_button(ui, theme::icon::AIRPLANE_TILT, if flying { crate::i18n::tr("Zatrzymaj przelot") } else { crate::i18n::tr("Przelot") }, flying).on_hover_text("F").clicked() {
            a.toggle_flight();
        }
        access_section(a, ui);
    }
    ui.add_space(8.0);
    if theme::row_button(ui, theme::icon::FILE_ARROW_UP, crate::i18n::tr("Wczytaj ślad (GPX, KML, GeoJSON)…"), false).clicked() {
        if let Some(p) = pick_track_file() {
            a.import_track(&p);
        }
    }
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
    ui.horizontal_wrapped(|ui| {
        if ui.button(theme::ic(theme::icon::TRAIN, "Rozkład jazdy")).on_hover_text(crate::i18n::tr("Połączenia komunikacją publiczną do początku trasy (Google Maps)")).clicked() {
            crate::transit::open_planner(start.lon, start.lat);
        }
        if ui.button(theme::ic(theme::icon::CAR, "Dojazd autem")).clicked() {
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
    egui::Grid::new("porownanie-tabela").num_columns(4).spacing([14.0, 4.0]).show(ui, |ui| {
        ui.label("");
        ui.label(RichText::new(tr("ta trasa")).small().color(theme::paper_3()));
        ui.label(RichText::new(tr("porównywana")).small().color(crate::ui::profile::COMPARE));
        ui.label(RichText::new(tr("różnica")).small().color(theme::paper_3()));
        ui.end_row();
        for (k, a1, b1, f, lower_better) in rows {
            ui.label(RichText::new(tr(k)).color(theme::paper_2()));
            ui.label(RichText::new(f(a1)).monospace());
            ui.label(RichText::new(f(b1)).monospace());
            let d = b1 - a1;
            let tiny = d.abs() < a1.abs().max(1.0) * 0.01;
            let color = if tiny { theme::paper_3() } else if (d < 0.0) == lower_better { theme::OK } else { theme::WARN };
            let sign = if d > 0.0 { "+" } else if d < 0.0 { "−" } else { "" };
            ui.label(RichText::new(if tiny { "≈".to_string() } else { format!("{sign}{}", f(d.abs())) }).monospace().color(color));
            ui.end_row();
        }
    });
    theme::para(ui, "Zielona różnica: porównywana trasa jest pod tym względem łatwiejsza lub krótsza.");
}
