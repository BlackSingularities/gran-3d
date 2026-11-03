//! Panele atlasu: pasek narzędzi, panel boczny (informacje, mapa i światło, jakość), profil i pomoc.

use eframe::egui::{self, Color32, RichText};

use crate::fmt;
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
    Map,
    Quality,
}

fn metric(ui: &mut egui::Ui, k: &str, v: String) {
    ui.vertical(|ui| {
        ui.label(RichText::new(k.to_uppercase()).size(9.5).color(theme::PAPER_3).extra_letter_spacing(1.0));
        ui.label(RichText::new(v).size(15.0).strong());
    });
}

fn grid2(ui: &mut egui::Ui, items: Vec<(&str, String)>) {
    egui::Grid::new(ui.next_auto_id()).num_columns(2).spacing([18.0, 8.0]).show(ui, |ui| {
        for (i, (k, v)) in items.into_iter().enumerate() {
            metric(ui, k, v);
            if i % 2 == 1 {
                ui.end_row();
            }
        }
    });
}

fn big(ui: &mut egui::Ui, v: String, sub: &str, color: Color32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(v).size(30.0).strong().color(color));
        ui.label(RichText::new(sub).color(theme::PAPER_3));
    });
}

/// Pionowy pasek narzędzi po lewej.
pub fn toolbar(a: &mut Atlas, ctx: &egui::Context) {
    egui::SidePanel::left("narzedzia")
        .resizable(false)
        .exact_width(64.0)
        .frame(egui::Frame::new().fill(theme::INK_0).inner_margin(egui::Margin::symmetric(6, 12)))
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                let mut tool = |ui: &mut egui::Ui, on: bool, icon: &str, cap: &str, tip: &str| -> bool {
                    let b = egui::Button::new(RichText::new(format!("{icon}\n{cap}")).size(11.5))
                        .min_size(egui::vec2(52.0, 46.0))
                        .selected(on);
                    ui.add(b).on_hover_text(tip).clicked()
                };
                if tool(ui, a.tool == Tool::Explore, theme::icon::COMPASS, "Teren", "Eksploracja (1)") {
                    a.tool = Tool::Explore;
                }
                if tool(ui, a.tool == Tool::Route, theme::icon::PATH, "Trasa", "Planowanie trasy (2)") {
                    a.tool = Tool::Route;
                    a.prefs.panel_open = true;
                    a.tab = Tab::Info;
                }
                if tool(ui, a.tool == Tool::Measure, theme::icon::RULER, "Pomiar", "Pomiar i linia wzroku (3)") {
                    a.tool = Tool::Measure;
                    a.prefs.panel_open = true;
                    a.tab = Tab::Info;
                }
                ui.add_space(10.0);
                ui.label(theme::kicker("Soczewka"));
                if tool(ui, a.lens == Lens::Band, theme::icon::WAVES, "Wys.", "Ta sama wysokość (H): pas albo poziomica na wysokości kursora") {
                    a.set_lens(Lens::Band);
                }
                if tool(ui, a.lens == Lens::Rel, theme::icon::PLUS_MINUS, "Wzgl.", "Wysokość względna (W): teren barwiony różnicą wysokości") {
                    a.set_lens(Lens::Rel);
                }
                if tool(ui, a.lens == Lens::Vis, theme::icon::EYE, "Widać", "Widoczność (O): co widać z miejsca pod kursorem") {
                    a.set_lens(Lens::Vis);
                }
                ui.add_space(10.0);
                if tool(ui, a.panorama.is_some(), theme::icon::MOUNTAINS, "Pano", "Panorama z miejsca pod środkiem widoku") {
                    if a.panorama.is_some() {
                        a.exit_panorama();
                    } else {
                        let (lon, lat) = (a.cam.lon, a.cam.lat);
                        a.enter_panorama(lon, lat);
                    }
                }
                if tool(ui, a.flight.is_some(), theme::icon::AIRPLANE_TILT, "Przelot", "Przelot nad trasą (F)") {
                    a.toggle_flight();
                }
            });
        });
}

pub fn side_panel(a: &mut Atlas, ctx: &egui::Context) {
    if !a.prefs.panel_open {
        return;
    }
    egui::SidePanel::right("panel")
        .resizable(false)
        .exact_width(372.0)
        .frame(egui::Frame::new().fill(theme::INK_1).inner_margin(egui::Margin::same(16)))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut a.tab, Tab::Info, "Informacje");
                ui.selectable_value(&mut a.tab, Tab::Map, "Mapa i światło");
                ui.selectable_value(&mut a.tab, Tab::Quality, "Jakość");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(theme::icon::X).on_hover_text("Ukryj panel (P)").clicked() {
                        a.prefs.panel_open = false;
                    }
                });
            });
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match a.tab {
                Tab::Info => info(a, ui),
                Tab::Map => map_tab(a, ui),
                Tab::Quality => quality_tab(a, ui),
            });
        });
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
    ui.label(theme::kicker(&format!("Okolica · {}", fmt::decimal(a.cam.lon, a.cam.lat))));
    let name = a
        .snap
        .pois
        .iter()
        .filter(|p| p.poi.kind == PoiKind::Peak)
        .min_by(|x, y| {
            haversine(a.cam.lon, a.cam.lat, x.poi.lon, x.poi.lat).total_cmp(&haversine(a.cam.lon, a.cam.lat, y.poi.lon, y.poi.lat))
        })
        .map(|p| format!("Okolice: {}", p.poi.name))
        .unwrap_or_else(|| "Atlas szlaków".into());
    ui.heading(name);
    let g = &a.snap.graph;
    if g.edges.is_empty() {
        let (busy, err, _) = a.world.status();
        ui.label(RichText::new(if busy.is_some() {
            "Pobieram szlaki i punkty tej okolicy z OpenStreetMap…"
        } else if a.cam.distance > 90_000.0 {
            "Przybliż widok, żeby wczytać szlaki i punkty okolicy."
        } else if let Some(e) = &err {
            e.as_str()
        } else {
            "W tej okolicy nie ma znakowanych szlaków w OpenStreetMap."
        })
        .color(theme::PAPER_3));
    } else {
        grid2(ui, vec![
            ("Szlaki", fmt::dist(g.total_km * 1000.0)),
            ("Trasy PTTK i inne", fmt::int(g.routes.len() as f64)),
            ("Punkty", fmt::int(a.snap.pois.len() as f64)),
            ("Sektory", a.snap.sectors.len().to_string()),
        ]);
    }
    ui.add_space(10.0);
    ui.label(theme::kicker("Najbardziej wybitne szczyty"));
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
            ui.label(theme::kicker("Wczytany ślad GPX"));
        });
        let s = r.stats;
        ui.label(RichText::new(r.labels[0].clone()).strong());
        grid2(ui, vec![("Dystans", fmt::dist(s.len2)), ("Czas", fmt::time(s.time)), ("Podejścia", fmt::ele(s.up)), ("Zejścia", fmt::ele(s.down))]);
        ui.horizontal(|ui| {
            if ui.button("Pokaż").clicked() {
                a.fit_route(&r);
            }
            if ui.button("Usuń").clicked() {
                a.gpx = None;
            }
        });
    }
    ui.add_space(10.0);
    ui.horizontal_wrapped(|ui| {
        if ui.button(theme::ic(theme::icon::LINK, "Kopiuj link do widoku")).on_hover_text("Adres gran:// otwiera ten widok w GRANI (także u innych)").clicked() {
            let l = a.share_link();
            ui.ctx().copy_text(l);
            a.notify("Skopiowano link do widoku – wklej go do wyszukiwarki GRANI albo otwórz w systemie.");
        }
    });
    if ui.button(theme::ic(theme::icon::FILE_ARROW_UP, "Wczytaj ślad GPX…")).on_hover_text("albo przeciągnij plik .gpx na mapę").clicked() {
        if let Some(p) = rfd::FileDialog::new().add_filter("GPX", &["gpx"]).pick_file() {
            a.import_gpx(&p);
        }
    }
    ui.add_space(14.0);
    author(ui);
}

fn author(ui: &mut egui::Ui) {
    ui.separator();
    ui.label(theme::kicker("Autor"));
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Projekt i wykonanie:").color(theme::PAPER_3));
        if ui.link("redzikowski.com").clicked() {
            let _ = open::that("https://redzikowski.com");
        }
    });
    if ui.add(theme::primary("☕ Postaw kawę")).clicked() {
        let _ = open::that("https://buycoffee.to/adamredzikowski");
    }
    ui.label(RichText::new("Kontakt: kontakt@redzikowski.com").small().color(theme::PAPER_3));
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
    ui.horizontal_wrapped(|ui| {
        if ui.button("Trasa stąd").clicked() {
            a.tool = Tool::Route;
            a.add_waypoint(lon, lat, Some(0));
        }
        if ui.button("Trasa tutaj").clicked() {
            a.tool = Tool::Route;
            a.add_waypoint(lon, lat, None);
        }
        if ui.button("Co widać?").clicked() {
            a.pin_lens(lon, lat, Lens::Vis);
        }
        if ui.button("Wysokość względna").clicked() {
            a.pin_lens(lon, lat, Lens::Rel);
        }
        if ui.button("Ta sama wysokość").clicked() {
            a.pin_lens(lon, lat, Lens::Band);
        }
        if ui.button("Panorama").clicked() {
            a.enter_panorama(lon, lat);
        }
        if ui.button("Kopiuj współrzędne").clicked() {
            ui.ctx().copy_text(fmt::decimal(lon, lat));
            a.notify("Skopiowano współrzędne.");
        }
    });
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
    let mut items = vec![("Wysokość", fmt::ele(p.ele as f64)), ("Model terenu", fmt::ele(p.dem as f64))];
    if p.poi.kind == PoiKind::Place && p.poi.pop > 0 {
        items.push(("Mieszkańcy", fmt::int(p.poi.pop as f64)));
    }
    items.push(("Odległość", fmt::dist(haversine(a.cam.lon, a.cam.lat, p.poi.lon, p.poi.lat))));
    grid2(ui, items);
    ui.label(RichText::new(fmt::coords(p.poi.lon, p.poi.lat)).monospace().small().color(theme::PAPER_3));
    if p.poi.ele.is_some() && (p.ele - p.dem).abs() > 25.0 {
        ui.label(RichText::new("Model terenu wygładza najostrzejsze granie – wysokość szczytu pochodzi z OSM.").small().color(theme::PAPER_3));
    }
    ui.horizontal(|ui| {
        if ui.link("OpenStreetMap").clicked() {
            let _ = open::that(format!("https://www.openstreetmap.org/{}", p.poi.osm));
        }
        if !p.poi.wiki.is_empty() {
            if let Some((lang, title)) = p.poi.wiki.split_once(':') {
                if ui.link("Wikipedia").clicked() {
                    let _ = open::that(format!("https://{lang}.wikipedia.org/wiki/{}", crate::osm::urlencode(title)));
                }
            }
        }
    });
    ui.add_space(6.0);
    point_buttons(a, ui, p.poi.lon, p.poi.lat);
    if p.poi.kind == PoiKind::Peak {
        ui.add_space(10.0);
        ui.label(theme::kicker("Sąsiednie szczyty"));
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
    close_row(a, ui, "Punkt terenu");
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
        ("Nachylenie", format!("{}°", fmt::dec(slope, 0))),
        ("Ekspozycja", format!("{} ({}°)", fmt::compass(aspect), aspect.round())),
        ("Szerokość", format!("{lat:.5}°")),
        ("Długość", format!("{lon:.5}°")),
    ]);
    ui.label(RichText::new(fmt::coords(lon, lat)).monospace().small().color(theme::PAPER_3));
    if let Some(n) = a.name_near(lon, lat, 400.0) {
        ui.label(RichText::new(format!("W pobliżu: {n}")).color(theme::PAPER_2));
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
    close_row(a, ui, "Szlak turystyczny");
    let routes: Vec<crate::osm::RouteInfo> = ed.routes.iter().filter_map(|&r| snap.graph.routes.get(r as usize).cloned()).collect();
    let title = routes.first().map(|r| r.name.clone()).unwrap_or_else(|| if ed.name.is_empty() { "Odcinek szlaku".into() } else { ed.name.clone() });
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
        ("Odcinek", fmt::dist(ed.len2)),
        ("Po terenie", fmt::dist(ed.len3)),
        ("Podejścia", fmt::ele(ed.up)),
        ("Zejścia", fmt::ele(ed.down)),
        ("Skala SAC", SAC_LABEL[ed.sac.min(6) as usize].to_string()),
        ("Czas (tam)", fmt::time(metrics::walk_time(ed.len2, ed.up, ed.down) * metrics::sac_factor(ed.sac))),
    ]);
    let route_ids = ed.routes.clone();
    ui.horizontal(|ui| {
        if ui.button("Pokaż cały szlak").clicked() {
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
        ui.selectable_value(&mut m, TimeModel::Pttk, "reguła PTTK");
        ui.selectable_value(&mut m, TimeModel::Din, "DIN 33466");
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Tempo").color(theme::PAPER_3));
        ui.selectable_value(&mut pace, 0.8, "szybkie");
        ui.selectable_value(&mut pace, 1.0, "normalne");
        ui.selectable_value(&mut pace, 1.25, "spokojne");
    });
    if (m, pace) != before {
        metrics::set_model(m, pace);
        a.prefs.din = m == TimeModel::Din;
        a.prefs.pace = pace;
        a.mark_route_dirty();
    }
}

fn route_info(a: &mut Atlas, ui: &mut egui::Ui) {
    ui.label(theme::kicker("Planowanie trasy"));
    ui.heading(if a.route_mode == RouteMode::Trails { "Trasa szlakami" } else { "Trasa przez teren" });
    let before = a.route_mode;
    ui.horizontal(|ui| {
        ui.selectable_value(&mut a.route_mode, RouteMode::Trails, "Szlakami");
        ui.selectable_value(&mut a.route_mode, RouteMode::Terrain, "Przez teren")
            .on_hover_text("A* po modelu terenu z funkcją Toblera i limitem nachylenia – analiza rzeźby, nie zachęta do schodzenia ze szlaków w parkach narodowych.");
    });
    if before != a.route_mode {
        a.mark_route_dirty();
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(theme::kicker("Punkty"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !a.waypoints.is_empty() {
                if ui.small_button("Wyczyść").clicked() {
                    a.clear_route();
                }
                if ui.small_button("Odwróć").clicked() {
                    a.reverse_route();
                }
            }
        });
    });
    if a.waypoints.is_empty() {
        ui.label(RichText::new("Klikaj na mapie, żeby dodać punkty trasy (start, przez, cel). Punkty można przeciągać; prawy przycisk daje więcej opcji.").color(theme::PAPER_3));
    }
    let mut remove = None;
    let n = a.waypoints.len();
    for (i, w) in a.waypoints.iter().enumerate() {
        ui.horizontal(|ui| {
            let tag = if i == 0 { "A".to_string() } else if i == n - 1 { "B".to_string() } else { i.to_string() };
            ui.label(RichText::new(tag).strong().color(theme::BRASS));
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
    ui.add_space(6.0);
    ui.label(theme::kicker("Czas przejścia"));
    time_model(a, ui);
    if a.routing {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new("wyznaczam trasę…").color(theme::PAPER_3));
        });
    }
    if let Some(e) = &a.route_error {
        ui.label(RichText::new(e).color(theme::WARN));
    }
    if !a.routes.is_empty() {
        ui.add_space(8.0);
        ui.label(theme::kicker(&format!("Warianty ({})", a.routes.len())));
        let base_t = a.routes[0].stats.time;
        let mut pick = None;
        for (i, r) in a.routes.iter().enumerate() {
            let sel = i == a.route_sel;
            let resp = theme::card(sel).inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(r.labels.join(" · ")).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(fmt::time(r.stats.time)).strong());
                    });
                });
                let d = r.stats.time - base_t;
                let diff = if i > 0 && d.abs() > 0.04 { format!(" · {}{}", if d > 0.0 { "+" } else { "−" }, fmt::time(d.abs())) } else { String::new() };
                ui.label(RichText::new(format!("{} · ↑ {} · ↓ {}{diff}", fmt::dist(r.stats.len2), fmt::ele(r.stats.up), fmt::ele(r.stats.down))).small().color(theme::PAPER_3));
            });
            if resp.response.interact(egui::Sense::click()).clicked() {
                pick = Some(i);
            }
        }
        if let Some(i) = pick {
            a.route_sel = i;
        }
        if let Some(r) = a.routes.get(a.route_sel).cloned() {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(theme::kicker("Szczegóły wariantu"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let c = [theme::OK, theme::WARN, theme::SIGNAL, Color32::from_rgb(190, 80, 200)][r.stats.difficulty.min(3)];
                    ui.label(RichText::new(DIFFICULTY[r.stats.difficulty.min(3)]).color(c));
                });
            });
            let s = r.stats;
            let mut items = vec![
                ("Dystans", fmt::dist(s.len2)),
                ("Po powierzchni", fmt::dist(s.len3)),
                ("Podejścia", fmt::ele(s.up)),
                ("Zejścia", fmt::ele(s.down)),
                ("Czas", fmt::time(s.time)),
                ("Najwyżej", fmt::ele(s.max_ele)),
                ("Najniżej", fmt::ele(s.min_ele)),
                ("Maks. nachylenie", format!("{}°", s.max_slope.round())),
                ("Linia prosta", fmt::dist(s.straight)),
                ("Krętość", format!("×{}", fmt::dec(s.len2 / s.straight.max(1.0), 2))),
            ];
            if r.kind == RouteKind::Trail {
                items.push(("Punkty GOT", fmt::int(s.got)));
                items.push(("Skala SAC", SAC_LABEL[s.sac_max.min(6) as usize].split(' ').next().unwrap_or("—").to_string()));
            }
            grid2(ui, items);
            if !r.route_refs.is_empty() {
                ui.add_space(6.0);
                ui.label(theme::kicker("Szlaki na trasie"));
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
    }
    if a.current_route().is_some() {
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button("Pokaż całą").clicked() {
                if let Some(r) = a.current_route().cloned() {
                    a.fit_route(&r);
                }
            }
            if ui.button(if a.prefs.profile_open { "Ukryj profil" } else { "Profil" }).clicked() {
                a.prefs.profile_open = !a.prefs.profile_open;
            }
            if ui.button(theme::ic(theme::icon::LINK, "Kopiuj link")).on_hover_text("Link gran:// z widokiem i punktami trasy").clicked() {
                let l = a.share_link();
                ui.ctx().copy_text(l);
                a.notify("Skopiowano link z trasą.");
            }
            if ui.button(theme::ic(theme::icon::EXPORT, "Eksport GPX")).on_hover_text("G").clicked() {
                a.export_gpx();
            }
            if ui.button(if a.flight.is_some() { "Zatrzymaj przelot" } else { "Przelot" }).on_hover_text("F").clicked() {
                a.toggle_flight();
            }
        });
    }
    ui.add_space(8.0);
    if ui.button("Wczytaj ślad GPX…").clicked() {
        if let Some(p) = rfd::FileDialog::new().add_filter("GPX", &["gpx"]).pick_file() {
            a.import_gpx(&p);
        }
    }
}

fn measure_info(a: &mut Atlas, ui: &mut egui::Ui) {
    ui.label(theme::kicker("Pomiar i linia wzroku"));
    ui.heading("Odległość po terenie");
    if a.measure.is_empty() {
        ui.label(RichText::new("Klikaj na mapie, żeby dodać punkty pomiaru. Dla dwóch punktów program sprawdzi też, czy jeden widać z drugiego.").color(theme::PAPER_3));
        return;
    }
    ui.horizontal(|ui| {
        ui.label(theme::kicker(&format!("Pomiar · {} pkt", a.measure.len())));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Wyczyść").clicked() {
                a.measure.clear();
                a.los = None;
            }
            if ui.small_button("Cofnij").clicked() {
                a.measure.pop();
                a.los = None;
            }
        });
    });
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
    big(ui, fmt::dist(d3), "po powierzchni", theme::PAPER);
    grid2(ui, vec![
        ("W rzucie", fmt::dist(d2)),
        ("Różnica wysokości", fmt::signed((last.2 - first.2) as f64, "m")),
        ("Azymut", format!("{}° {}", bearing(first.0, first.1, last.0, last.1).round(), fmt::compass(bearing(first.0, first.1, last.0, last.1)))),
        ("Linia prosta", fmt::dist(haversine(first.0, first.1, last.0, last.1))),
    ]);
    if a.measure.len() == 2 {
        ui.add_space(8.0);
        ui.label(theme::kicker("Linia wzroku (oczy 1,7 m, krzywizna Ziemi)"));
        match a.los {
            Some((None, _)) => {
                big(ui, "Widać".into(), "", theme::OK);
                ui.label(RichText::new("Z punktu 1 widać punkt 2 – teren nie przesłania linii wzroku.").color(theme::PAPER_3));
            }
            Some((Some(f), d)) => {
                big(ui, "Nie widać".into(), "", theme::SIGNAL);
                ui.label(RichText::new(format!("Teren przesłania linię wzroku po {} od punktu 1.", fmt::dist(f * d))).color(theme::PAPER_3));
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
            ui.label(theme::kicker("Analiza widoczności"));
            ui.heading("Co widać stąd?");
            ui.label(RichText::new(format!("{} · {}", fmt::decimal(lon, lat), fmt::ele(e as f64))).monospace().small().color(theme::PAPER_3));
            ui.label(theme::kicker("Wysokość obserwatora nad terenem"));
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
                    ui.label(RichText::new("liczę promienie widzenia…").color(theme::PAPER_3));
                });
            }
            if let Some(v) = &a.vs {
                big(ui, fmt::dec(v.area_km2, 1), "km² widocznych", theme::TEAL);
                ui.label(RichText::new(format!("w promieniu 32 km · uwzględnia krzywiznę Ziemi i refrakcję · oko {} m n.p.m.", fmt::int(v.observer as f64))).small().color(theme::PAPER_3));
                ui.add_space(6.0);
                ui.label(theme::kicker(&format!("Widoczne szczyty i schroniska ({})", v.visible.len())));
                let list: Vec<(usize, f64)> = v.visible.iter().take(40).copied().collect();
                for (i, d) in list {
                    let p = a.snap.pois[i].clone();
                    if row(ui, &p.poi.name, &fmt::ele(p.ele as f64), &fmt::dist(d)) {
                        a.selection = Selection::Poi(i);
                    }
                }
            }
        }
        Lens::Rel => {
            ui.label(theme::kicker("Wysokość względna"));
            ui.heading("Co jest wyżej?");
            ui.label(RichText::new(format!("Poziom 0: {} · {}", fmt::ele(e as f64), fmt::decimal(lon, lat))).small().color(theme::PAPER_3));
            ui.add(egui::Slider::new(&mut a.prefs.rel_range, 50.0..=1500.0).text("zakres barw (m)").logarithmic(true));
            if let Some(r) = &a.rel {
                grid2(ui, vec![("Wyżej (5 km)", format!("{}%", fmt::dec(r.above * 100.0, 0))), ("Niżej (5 km)", format!("{}%", fmt::dec(r.below * 100.0, 0)))]);
            }
            ui.add_space(6.0);
            ui.label(theme::kicker("Szczyty względem punktu"));
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
            ui.label(theme::kicker(if a.prefs.band_tol > 0.0 { "Ta sama wysokość" } else { "Poziomica przez punkt" }));
            ui.heading(fmt::ele(e as f64));
            ui.add(egui::Slider::new(&mut a.prefs.band_tol, 0.0..=50.0).text("pas ± m (0 – sama poziomica)"));
            let tol = a.prefs.band_tol.max(15.0);
            ui.label(theme::kicker(&format!("Na tej samej wysokości (±{} m)", tol.round())));
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
    if ui.button("Odepnij soczewkę (Esc)").clicked() {
        unpin = true;
    }
    if unpin {
        a.unpin_lens();
    }
}

fn map_tab(a: &mut Atlas, ui: &mut egui::Ui) {
    ui.label(theme::kicker("Styl terenu"));
    ui.horizontal_wrapped(|ui| {
        for s in Style::ALL {
            ui.selectable_value(&mut a.prefs.style, s, s.label());
        }
    });
    let legend = match a.prefs.style {
        Style::Slope => Some("Klasy nachylenia jak na mapach lawinowych: 15–25° · 25–30° · 30–35° · 35–40° · 40–45° · ponad 45°."),
        Style::Aspect => Some("Barwa – kierunek, w który opada stok; nasycenie – nachylenie."),
        Style::Hypso => Some("Barwy wysokości: niziny zielone, piętro hal brązowe, najwyżej biel."),
        Style::Satellite => Some("Sentinel-2 cloudless 2020 (EOX) – 10 m; z bliska ortofotomapa GUGiK, ČÚZK i ZBGIS."),
        _ => None,
    };
    if let Some(l) = legend {
        ui.label(RichText::new(l).small().color(theme::PAPER_3));
    }
    ui.add_space(8.0);
    ui.label(theme::kicker("Warstwy"));
    ui.horizontal_wrapped(|ui| {
        ui.toggle_value(&mut a.prefs.trails, "Szlaki (T)");
        ui.toggle_value(&mut a.prefs.labels, "Nazwy (L)");
        ui.toggle_value(&mut a.prefs.contours, "Poziomice (C)");
        ui.toggle_value(&mut a.prefs.km_grid, "Siatka km");
        ui.toggle_value(&mut a.prefs.shadows, "Cienie rzeźby");
        ui.toggle_value(&mut a.prefs.snow, "Śnieg");
        ui.toggle_value(&mut a.prefs.ortho, "Ortofoto (PL, CZ, SK)")
            .on_hover_text("W stylu „Satelita”: zdjęcia lotnicze GUGiK, ČÚZK i ZBGIS do ok. 0,6 m na bliskich kaflach");
        ui.toggle_value(&mut a.prefs.landcover, "Pokrycie terenu OSM")
            .on_hover_text("W stylu „Teren”: wody, lasy, kosodrzewina i piargi z OpenStreetMap");
    });
    ui.add(egui::Slider::new(&mut a.prefs.label_density, 0.3..=2.0).text("gęstość nazw"));
    ui.add(egui::Slider::new(&mut a.prefs.exag, 1.0..=3.0).step_by(0.1).text("przewyższenie ×"));
    ui.add_space(8.0);
    ui.label(theme::kicker("Słońce i pora roku"));
    let (day, hour) = a.when();
    let mut h = hour;
    let mut d = day;
    ui.horizontal(|ui| {
        if ui.small_button("◀").on_hover_text("dzień wcześniej").clicked() {
            d = d.add_days(-1);
        }
        ui.label(RichText::new(d.label()).strong());
        if ui.small_button("▶").on_hover_text("dzień później").clicked() {
            d = d.add_days(1);
        }
        if ui.small_button("−1 mies.").clicked() {
            d = d.add_days(-30);
        }
        if ui.small_button("+1 mies.").clicked() {
            d = d.add_days(30);
        }
    });
    let r = ui.add(egui::Slider::new(&mut h, 0.0..=23.99).text("godzina").custom_formatter(|v, _| fmt::hour(v)));
    if r.changed() || d != day {
        a.prefs.custom_time = Some((d.y, d.m, d.d, h));
    }
    let t = sun::times(d, a.cam.lat, a.cam.lon);
    let (az, alt) = a.sun();
    grid2(ui, vec![
        ("Wschód", fmt::hour(t.rise)),
        ("Zachód", fmt::hour(t.set)),
        ("Słońce teraz", format!("{}° nad horyzontem", fmt::dec(alt, 0))),
        ("Azymut", format!("{}° {}", az.round(), fmt::compass(az))),
        ("Granica śniegu", fmt::ele(sun::snowline(d))),
        ("Górowanie", format!("{} · {}°", fmt::hour(t.noon), t.max_alt.round())),
    ]);
    ui.horizontal(|ui| {
        if ui.button("Teraz").clicked() {
            a.prefs.custom_time = None;
        }
        if ui.button("Złota godzina").on_hover_text("godzina przed zachodem").clicked() {
            a.prefs.custom_time = Some((d.y, d.m, d.d, (t.set - 1.0).max(0.0)));
        }
        if ui.button("Wschód").clicked() {
            a.prefs.custom_time = Some((d.y, d.m, d.d, t.rise + 0.25));
        }
        if ui.button("Południe").clicked() {
            a.prefs.custom_time = Some((d.y, d.m, d.d, t.noon));
        }
    });
    let _ = Day::today;
    ui.add_space(10.0);
    ui.label(theme::kicker("Źródła danych"));
    ui.label(
        RichText::new("Teren: Terrarium/AWS Open Data (SRTM, EU-DEM), LiDAR: NMT GUGiK, DMR 5G ČÚZK (import z wersji 1.x) · Zdjęcia: Sentinel-2 cloudless 2020 © EOX (CC BY-NC-SA 4.0) · Szlaki i punkty: © OpenStreetMap (ODbL) · Wyszukiwanie: Nominatim")
            .small()
            .color(theme::PAPER_3),
    );
}

fn quality_tab(a: &mut Atlas, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(theme::kicker(&format!("Jakość renderowania · {}", a.prefs.quality.label())));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(format!("{:.0} kl./s", a.fps())).small().color(theme::TEAL));
        });
    });
    ui.horizontal_wrapped(|ui| {
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
    let mut changed = false;
    changed |= ui.add(egui::Slider::new(&mut a.prefs.render_scale, 0.25..=1.0).text("rozdzielczość renderu").custom_formatter(|v, _| format!("{:.0}%", v * 100.0))).changed();
    let mut detail = 700.0 - a.prefs.detail_px;
    if ui.add(egui::Slider::new(&mut detail, 100.0..=520.0).text("szczegółowość terenu").show_value(false)).changed() {
        a.prefs.detail_px = 700.0 - detail;
        changed = true;
    }
    changed |= ui.add(egui::Slider::new(&mut a.prefs.max_dem, 150..=1200).text("kafle terenu w pamięci GPU")).changed();
    changed |= ui.add(egui::Slider::new(&mut a.prefs.max_img, 150..=1500).text("kafle zdjęć w pamięci GPU")).changed();
    if changed {
        a.prefs.quality = Quality::Custom;
    }
    if let Some(r) = &a.renderer {
        let (d, i) = r.tiles.resident();
        ui.label(RichText::new(format!("Rysowane kafle: {} · w pamięci: teren {d}, zdjęcia {i}", r.drawn)).small().monospace().color(theme::PAPER_3));
    }
    ui.add_space(12.0);
    ui.label(theme::kicker("Dane i pamięć"));
    ui.label(RichText::new("Folder danych, limit rozmiaru i obszary do pracy bez internetu.").small().color(theme::PAPER_3));
    ui.horizontal(|ui| {
        if ui.button(theme::ic(theme::icon::MAP_TRIFOLD, "Biblioteka map (D)")).clicked() {
            a.requests.open_data = true;
        }
        if ui.button(theme::ic(theme::icon::GEAR, "Folder i limit (Ctrl+,)")).clicked() {
            a.requests.open_settings = true;
        }
    });
    ui.add_space(12.0);
    ui.label(theme::kicker("Program"));
    ui.label(RichText::new(format!("GRAŃ {}", env!("CARGO_PKG_VERSION"))).strong());
    if ui.button(theme::ic(theme::icon::ARROWS_CLOCKWISE, "Sprawdź aktualizacje")).clicked() {
        a.requests.check_updates = true;
    }
    if ui.button("Skróty klawiszowe (?)").clicked() {
        a.help = true;
    }
}

/// Profil wysokości na dole okna.
pub fn profile_panel(a: &mut Atlas, ctx: &egui::Context) {
    if !a.prefs.profile_open {
        a.profile_cursor = a.flight.as_ref().map(|f| f.dist);
        return;
    }
    let Some(route) = a.current_route().cloned() else {
        // profil pomiaru (2+ punktów)
        a.profile_cursor = None;
        return;
    };
    egui::TopBottomPanel::bottom("profil")
        .exact_height(190.0)
        .frame(egui::Frame::new().fill(theme::INK_1).inner_margin(egui::Margin::symmetric(12, 8)))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::kicker("Profil wysokości"));
                let s = route.stats;
                ui.label(RichText::new(format!("{} · ↑ {} · ↓ {} · {}", fmt::dist(s.len2), fmt::ele(s.up), fmt::ele(s.down), fmt::time(s.time))).small().color(theme::PAPER_3));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(theme::icon::X).clicked() {
                        a.prefs.profile_open = false;
                    }
                });
            });
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
}

pub fn help(a: &mut Atlas, ctx: &egui::Context) {
    if !a.help {
        return;
    }
    let rows = [
        ("1 – 3", "eksploracja · trasa · pomiar"),
        ("H  W  O", "soczewka: ta sama wysokość · względna · widoczność"),
        ("/", "wyszukiwarka"),
        ("M", "mapa i światło"),
        ("P", "pokaż / ukryj panel"),
        ("D", "mapy do pobrania (bez internetu)"),
        ("T  L  C", "szlaki · nazwy · poziomice"),
        ("klik z soczewką / Esc", "przypnij / odepnij soczewkę"),
        ("+  −", "przewyższenie terenu"),
        ("[  ]", "pora dnia −/+ 30 min"),
        ("F", "przelot nad trasą"),
        ("N", "północ u góry"),
        ("V", "widok z góry / ukośny"),
        ("R", "widok początkowy"),
        ("G", "eksport GPX"),
        ("S", "zrzut ekranu PNG"),
        ("⌫", "usuń ostatni punkt"),
        ("Esc", "anuluj / wyjdź"),
        ("strzałki · Q E · PgUp PgDn", "przesuwanie · obrót · nachylenie"),
        ("lewy przycisk", "przesuwanie (chwyć teren)"),
        ("prawy przycisk", "obrót i nachylenie · klik – menu analiz"),
        ("kółko", "przybliżanie do kursora"),
        ("dwuklik", "przybliż do punktu"),
        ("Ctrl+,", "folder i limit danych"),
    ];
    let mut open = true;
    egui::Window::new("Jak korzystać z Grani")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .open(&mut open)
        .show(ctx, |ui| {
            ui.set_width(560.0);
            ui.label(RichText::new("Atlas łączy rzeczywisty model terenu z siecią szlaków OSM. Wszystkie obliczenia – trasy, widoczność, cienie – odbywają się lokalnie na tym komputerze.").color(theme::PAPER_2));
            ui.add_space(8.0);
            egui::Grid::new("skroty").num_columns(2).striped(true).spacing([24.0, 6.0]).show(ui, |ui| {
                for (k, v) in rows {
                    ui.label(v);
                    ui.label(RichText::new(k).monospace().color(theme::BRASS));
                    ui.end_row();
                }
            });
            ui.add_space(8.0);
            ui.label(RichText::new("Pliki .gpx możesz przeciągnąć bezpośrednio na mapę.").small().color(theme::PAPER_3));
        });
    if !open {
        a.help = false;
    }
}
