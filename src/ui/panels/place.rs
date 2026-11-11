//! Szczegóły okolicy, punktu, miejsca z OSM i odcinka szlaku.

use super::*;

pub(super) fn region_info(a: &mut Atlas, ui: &mut egui::Ui) {
    ui.label(theme::kicker(&tf!("Okolica · {}", fmt::coords(a.cam.lon, a.cam.lat))));
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

pub(super) fn point_buttons(a: &mut Atlas, ui: &mut egui::Ui, lon: f64, lat: f64) {
    section(ui, theme::icon::LIGHTNING, crate::i18n::tr("Działania"));
    let mut act: Option<u8> = None;
    let items: [(&str, &str); 9] = [
        (theme::icon::FLAG, "Trasa stąd"),
        (theme::icon::MAP_PIN, "Trasa tutaj"),
        (theme::icon::PERSON_SIMPLE_WALK, "Spacer stąd"),
        (theme::icon::CLOUD_SUN, "Pogoda"),
        (theme::icon::EYE, "Co widać stąd?"),
        (theme::icon::PLUS_MINUS, "Wysokość względna"),
        (theme::icon::WAVES, "Ta sama wysokość"),
        (theme::icon::STAR, "Do ulubionych"),
        (theme::icon::COPY, "Współrzędne"),
    ];
    theme::tile_grid(ui, 3, |ui, w| {
        for (k, (g, t)) in items.iter().enumerate() {
            if theme::action(ui, g, t, w).clicked() {
                act = Some(k as u8);
            }
        }
    });
    match act {
        Some(0) => {
            a.tool = Tool::Route;
            a.add_waypoint(lon, lat, Some(0));
        }
        Some(1) => {
            a.tool = Tool::Route;
            a.add_waypoint(lon, lat, None);
        }
        Some(2) => a.enter_panorama(lon, lat),
        Some(3) => {
            let ele = a.heights.sample_cached(lon, lat).unwrap_or(0.0);
            let name = a.selection_name().or_else(|| a.name_near(lon, lat, 400.0)).unwrap_or_else(|| fmt::coords(lon, lat));
            a.cond_at = Some(crate::ui::conditions::Place { lon, lat, ele, name });
            a.tab = Tab::Conditions;
            a.prefs.panel_open = true;
        }
        Some(4) => a.pin_lens(lon, lat, Lens::Vis),
        Some(5) => a.pin_lens(lon, lat, Lens::Rel),
        Some(6) => a.pin_lens(lon, lat, Lens::Band),
        Some(7) => {
            let name = a.selection_name();
            a.save_place(lon, lat, name);
        }
        Some(8) => {
            ui.ctx().copy_text(fmt::coords(lon, lat));
            a.notify(crate::i18n::tr("Skopiowano współrzędne."));
        }
        _ => {}
    }
}

pub(super) fn poi_info(a: &mut Atlas, ui: &mut egui::Ui, i: usize) {
    let Some(p) = a.snap.pois.get(i).cloned() else {
        a.selection = Selection::None;
        return;
    };
    let (glyph, color) = kind_style(p.poi.kind);
    let dist = haversine(a.cam.lon, a.cam.lat, p.poi.lon, p.poi.lat);
    let sub = format!("{} · {}", fmt::ele(p.ele as f64), fmt::coords(p.poi.lon, p.poi.lat));
    header(a, ui, glyph, color, p.poi.kind.label(), &p.poi.name, &sub);
    let _ = dist;
    let mut items = vec![(crate::i18n::tr("Wysokość"), fmt::ele(p.ele as f64))];
    if (p.ele - p.dem).abs() > 10.0 {
        items.push((crate::i18n::tr("Model terenu"), fmt::ele(p.dem as f64)));
    }
    if p.poi.kind == PoiKind::Place && p.poi.pop > 0 {
        items.push((crate::i18n::tr("Mieszkańcy"), fmt::int(p.poi.pop as f64)));
    }
    items.push((crate::i18n::tr("Odległość"), fmt::dist(haversine(a.cam.lon, a.cam.lat, p.poi.lon, p.poi.lat))));
    grid2(ui, items);
    if p.poi.ele.is_some() && (p.ele - p.dem).abs() > 25.0 {
        theme::para(ui, crate::i18n::tr("Model terenu wygładza najostrzejsze granie – wysokość szczytu pochodzi z OSM."));
    }
    poi_details(a, ui, &p.poi);
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

pub(super) fn point_info(a: &mut Atlas, ui: &mut egui::Ui, lon: f64, lat: f64) {
    let e = a.heights.sample_cached(lon, lat).unwrap_or(0.0) as f64;
    let near = a.name_near(lon, lat, 400.0).map(|n| tf!("w pobliżu: {}", n)).unwrap_or_else(|| fmt::coords(lon, lat));
    header(a, ui, theme::icon::MAP_PIN, theme::TEAL, crate::i18n::tr("Punkt terenu"), &fmt::ele(e), &near);
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
    coords_section(a, ui, lon, lat);
    ui.add_space(6.0);
    point_buttons(a, ui, lon, lat);
}

/// Współrzędne punktu we wszystkich zapisach, każdy z przyciskiem kopiowania.
pub(super) fn coords_section(a: &mut Atlas, ui: &mut egui::Ui, lon: f64, lat: f64) {
    use crate::coords::CoordFormat;
    section(ui, theme::icon::MAP_PIN, crate::i18n::tr("Współrzędne"));
    let mut copied = false;
    egui::Grid::new(("wspolrzedne", (lon * 1e6) as i64, (lat * 1e6) as i64)).num_columns(3).spacing([10.0, 4.0]).show(ui, |ui| {
        for (f, label) in [(CoordFormat::Dms, "°′″"), (CoordFormat::Decimal, "°"), (CoordFormat::Utm, "UTM"), (CoordFormat::Mgrs, "MGRS")] {
            let txt = crate::coords::format_as(f, lon, lat);
            let mut rt = RichText::new(&txt).monospace();
            if f == crate::coords::format() {
                rt = rt.color(theme::PAPER);
            } else {
                rt = rt.color(theme::paper_2());
            }
            ui.label(RichText::new(label).small().color(theme::paper_3()));
            ui.label(rt);
            if ui.small_button(theme::icon::COPY).on_hover_text(crate::i18n::tr("Kopiuj")).clicked() {
                ui.ctx().copy_text(txt);
                copied = true;
            }
            ui.end_row();
        }
    });
    if copied {
        a.notify(crate::i18n::tr("Skopiowano współrzędne."));
    }
}

pub(super) fn trail_info(a: &mut Atlas, ui: &mut egui::Ui, e: u32) {
    let snap = a.snap.clone();
    let Some(ed) = snap.graph.edges.get(e as usize) else {
        a.selection = Selection::None;
        return;
    };
    let routes: Vec<crate::osm::RouteInfo> = ed.routes.iter().filter_map(|&r| snap.graph.routes.get(r as usize).cloned()).collect();
    let title = routes.first().map(|r| r.name.clone()).unwrap_or_else(|| if ed.name.is_empty() { crate::i18n::tr("Odcinek szlaku").into() } else { ed.name.clone() });
    let colors: Vec<&str> = ed.colors.iter().map(|c| c.name()).collect();
    let sub = format!("{} · {}", fmt::dist(ed.len2), colors.join(", "));
    let c0 = ed.colors.first().map(|c| c.rgb()).unwrap_or([122, 176, 255]);
    header(a, ui, theme::icon::PATH, Color32::from_rgb(c0[0], c0[1], c0[2]), crate::i18n::tr("Szlak turystyczny"), &title, &sub);
    for r in &routes {
        ui.horizontal(|ui| {
            let c = crate::metrics::TrailColor::parse(&r.color).rgb();
            let (rect, _) = ui.allocate_exact_size(egui::vec2(26.0, 7.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 3.0, Color32::from_rgb(c[0], c[1], c[2]));
            ui.label(RichText::new(&r.name).strong());
            if !r.r#ref.is_empty() {
                ui.label(RichText::new(&r.r#ref).small().color(theme::paper_3()));
            }
            if ui.small_button("OSM").clicked() {
                let _ = open::that(format!("https://www.openstreetmap.org/relation/{}", r.id));
            }
        });
    }
    let (day, _) = a.when();
    let closed: Vec<crate::osm::Closure> = snap.closures_on(&ed.lon, &ed.lat, (day.m, day.d)).into_iter().cloned().collect();
    closures_warning(ui, &closed);
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

/// Informacje o punkcie z OSM (godziny, telefon, strona, noclegi…) i podgląd kamery internetowej.
pub(super) fn poi_details(a: &mut Atlas, ui: &mut egui::Ui, p: &crate::osm::Poi) {
    use crate::i18n::tr;
    if p.info.is_empty() {
        if matches!(p.kind, PoiKind::Hut | PoiKind::Shelter) {
            theme::para(ui, "OpenStreetMap nie ma dla tego miejsca godzin otwarcia ani kontaktu – możesz je tam uzupełnić.");
        }
        return;
    }
    section(ui, theme::icon::INFO, tr("Informacje"));
    let get = |k: &str| p.info.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone());
    let row = |ui: &mut egui::Ui, glyph: &str, label: &str, value: &str| {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(glyph).color(theme::BRASS));
            ui.label(RichText::new(tr(label)).color(theme::paper_3()));
            ui.label(RichText::new(value).color(theme::PAPER));
        });
    };
    if let Some(h) = get("hours") {
        // stan teraz (otwarte / zamknięte i najbliższa zmiana), gdy program rozumie zapis godzin
        if let Some(hours) = crate::hours::Hours::parse(&h) {
            let (day, minute) = crate::hours::now_local(p.lon, p.lat);
            let st = hours.status(day, minute);
            let color = if st.open { theme::OK } else { theme::SIGNAL };
            ui.horizontal(|ui| {
                ui.label(RichText::new(theme::icon::CLOCK).color(color));
                ui.label(RichText::new(crate::hours::describe(st)).strong().color(color));
            });
        }
        let h = if h == "24/7" { tr("całą dobę").to_string() } else { h };
        row(ui, theme::icon::CALENDAR_BLANK, "Godziny:", &h);
    }
    if let Some(b) = get("beds").or_else(|| get("capacity")) {
        row(ui, theme::icon::BED, "Miejsca noclegowe:", &b);
    }
    if let Some(r) = get("reservation") {
        let r = match r.as_str() {
            "required" => tr("wymagana").to_string(),
            "recommended" => tr("zalecana").to_string(),
            "yes" => tr("możliwa").to_string(),
            "no" => tr("brak").to_string(),
            _ => r,
        };
        row(ui, theme::icon::CALENDAR, "Rezerwacja:", &r);
    }
    if let Some(o) = get("operator") {
        row(ui, theme::icon::USERS, "Prowadzi:", &o);
    }
    if let Some(f) = get("fee") {
        let f = match f.as_str() {
            "yes" => tr("płatne").to_string(),
            "no" => tr("bezpłatne").to_string(),
            _ => f,
        };
        row(ui, theme::icon::COINS, "Opłata:", &f);
    }
    if let Some(d) = get("description") {
        ui.add(egui::Label::new(RichText::new(d).small().color(theme::paper_2())).wrap());
    }
    ui.horizontal_wrapped(|ui| {
        if let Some(ph) = get("phone") {
            let first = ph.split(';').next().unwrap_or("").trim().to_string();
            if ui.button(theme::ic(theme::icon::PHONE, &first)).on_hover_text(tr("Kopiuj numer")).clicked() {
                ui.ctx().copy_text(first.clone());
                a.notify(tf!("Skopiowano numer {}", first));
            }
        }
        if let Some(w) = get("website") {
            let url = if w.starts_with("http") { w } else { format!("https://{w}") };
            if ui.button(theme::ic(theme::icon::GLOBE, "Strona")).on_hover_text(&url).clicked() {
                let _ = open::that(&url);
            }
        }
        if let Some(m) = get("email") {
            if ui.button(theme::ic(theme::icon::ENVELOPE, "E-mail")).on_hover_text(&m).clicked() {
                let _ = open::that(format!("mailto:{m}"));
            }
        }
    });
    if let Some(cam) = get("webcam") {
        let url = if cam.starts_with("http") { cam } else { format!("https://{cam}") };
        section(ui, theme::icon::CAMERA, tr("Kamera"));
        webcam(a, ui, &url);
    }
}

/// Podgląd obrazu kamery (gdy adres prowadzi do zdjęcia) albo odnośnik do strony kamery.
pub(super) fn webcam(a: &mut Atlas, ui: &mut egui::Ui, url: &str) {
    use crate::i18n::tr;
    let state = a.webcam_image(ui.ctx(), url);
    match state {
        crate::ui::atlas::CamState::Loading => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(tr("pobieram obraz kamery…")).color(theme::paper_3()));
            });
        }
        crate::ui::atlas::CamState::Image(tex) => {
            let w = ui.available_width();
            let size = tex.size_vec2();
            let h = w * size.y / size.x.max(1.0);
            ui.add(egui::Image::new((tex.id(), egui::vec2(w, h))).corner_radius(8.0));
        }
        crate::ui::atlas::CamState::NotImage => {}
        crate::ui::atlas::CamState::Failed(e) => {
            ui.label(RichText::new(tf!("Obraz niedostępny: {}", e)).small().color(theme::paper_3()));
        }
    }
    if ui.link(tr("Otwórz stronę kamery")).clicked() {
        let _ = open::that(url);
    }
}
