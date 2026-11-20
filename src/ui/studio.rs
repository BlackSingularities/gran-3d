//! Okno Studio (tylko komputer, tylko konta z dostępem): kroki u góry (Obszar → Dane → wyniki),
//! ustawienia w kolumnie po lewej, duży podgląd na żywo po prawej (schemat obszaru, model 3D
//! z materiałami, arkusz mapy albo przekroju narysowany z prawdziwego pliku SVG), pasek stanu
//! i główne działanie na dole.
//!
//! Obszar wskazuje się na mapie tylko na zakładce Obszar (na pozostałych kliknięcia mapy go nie
//! zmieniają, a linia A–B przekroju – tylko na zakładce Przekrój).
//!
//! Silnik jest w [`crate::studio`]; tu są tylko kontrolki, podglądy i rysowanie obszaru na mapie.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};

use crate::i18n::tr;
use crate::studio::color::{Mode, Palette};
use crate::studio::draw::Font;
use crate::studio::geom::Shape;
use crate::studio::mapdraw::{MapStyle, Orient, Paper, Relief};
use crate::studio::mesh::{Mat, MatPreset, Walls};
use crate::studio::section::Plane;
use crate::studio::{Pick, SheetKind, Studio, Tab};
use crate::ui::atlas::{Atlas, Tool};
use crate::ui::theme;

/// Domyślny rozmiar okna Studio (pt).
const SIZE: Vec2 = Vec2::new(1180.0, 760.0);
/// Szerokość kolumny ustawień (pt).
const LEFT: f32 = 372.0;
/// Wysokość paska na dole (pt).
const FOOTER: f32 = 48.0;

/// Okno Studio – wywoływać co klatkę (samo sprawdza dostęp i czy narzędzie jest włączone).
pub fn window(a: &mut Atlas, ctx: &egui::Context) {
    let allowed = crate::studio::allowed(a.points.account());
    if !allowed {
        if a.tool == Tool::Studio {
            a.tool = Tool::Explore;
        }
        return;
    }
    a.studio.poll(ctx);
    a.studio.open = a.tool == Tool::Studio;
    if !a.studio.open {
        return;
    }
    // kliknięcia mapy: obszar tylko na zakładce Obszar, linia A–B tylko na zakładce Przekrój
    a.studio.pick = match a.studio.o.tab {
        Tab::Area => Pick::Area,
        Tab::Section if a.studio.o.section.plane != Plane::Horizontal => Pick::Section,
        _ => Pick::None,
    };
    let cam = (a.cam.lon, a.cam.lat, a.cam.distance);
    let cache = a.heights.store().root().to_path_buf();
    let mut open = true;
    let screen = ctx.screen_rect();
    let size = Vec2::new(SIZE.x.min(screen.width() - 32.0), SIZE.y.min(screen.height() - 96.0));
    egui::Window::new(format!("{}  {}", theme::icon::CUBE, tr("GRAŃ Studio")))
        .id(egui::Id::new("gran-studio-2"))
        .open(&mut open)
        .default_size(size)
        .min_size(Vec2::new(theme::fit(ctx, 640.0), 480.0))
        .default_pos(Pos2::new((screen.right() - size.x - 24.0).max(8.0), 64.0))
        .resizable(true)
        .collapsible(true)
        .frame(egui::Frame::window(&ctx.style()).fill(theme::POPUP_FILL))
        .show(ctx, |ui| {
            let s = &mut a.studio;
            steps(s, ui);
            ui.add_space(4.0);
            let avail = ui.available_size();
            let body_h = (avail.y - FOOTER).max(260.0);
            if avail.x >= 820.0 {
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(Vec2::new(LEFT, body_h), egui::Layout::top_down(egui::Align::Min), |ui| {
                        egui::ScrollArea::vertical().id_salt("studio-settings").auto_shrink([false, false]).max_height(body_h).show(ui, |ui| {
                            ui.set_width(LEFT - 16.0);
                            settings(s, ui, cam, cache.clone());
                        });
                    });
                    ui.add_space(6.0);
                    let rw = ui.available_width();
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(rw, body_h), Sense::hover());
                    stage(s, ui, rect);
                });
            } else {
                // wąskie okno: podgląd nad ustawieniami
                let ph = (body_h * 0.42).clamp(200.0, 360.0);
                let (rect, _) = ui.allocate_exact_size(Vec2::new(avail.x, ph), Sense::hover());
                stage(s, ui, rect);
                ui.add_space(6.0);
                egui::ScrollArea::vertical().id_salt("studio-settings").auto_shrink([false, false]).max_height(body_h - ph - 6.0).show(ui, |ui| {
                    settings(s, ui, cam, cache.clone());
                });
            }
            footer(s, ui, cache.clone());
        });
    if !open {
        a.close_tool();
    }
    a.studio.persist();
}

// ---------------------------------------------------------------- kroki

/// Czy zakładka jest „zrobiona” (obszar wskazany, dane przygotowane).
fn done(s: &Studio, t: Tab) -> bool {
    match t {
        Tab::Area => s.region().is_some(),
        Tab::Terrain => s.scene.is_some() && !s.stale(),
        _ => false,
    }
}

/// Pasek kroków: numer, ikona, nazwa i krótki opis; wyniki dostępne po przygotowaniu danych.
fn steps(s: &mut Studio, ui: &mut egui::Ui) {
    let n = Tab::ALL.len() as f32;
    let gap = 6.0;
    let w = ((ui.available_width() - gap * (n - 1.0)) / n).max(80.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (i, t) in Tab::ALL.into_iter().enumerate() {
            let needs_scene = !matches!(t, Tab::Area | Tab::Terrain);
            let enabled = !needs_scene || s.scene.is_some();
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 46.0), if enabled { Sense::click() } else { Sense::hover() });
            let resp = if enabled { resp } else { resp.on_hover_text(tr("Najpierw przygotuj dane obszaru (zakładka Dane).")) };
            let on = s.o.tab == t;
            let p = ui.painter();
            let fill = if on {
                theme::BRASS_DIM
            } else if resp.hovered() && enabled {
                theme::INK_3
            } else {
                theme::INK_2
            };
            p.rect_filled(rect, 10.0, fill);
            if on {
                // górna kreska wyróżnienia
                p.rect_filled(Rect::from_min_size(rect.left_top() + Vec2::new(10.0, 0.0), Vec2::new(rect.width() - 20.0, 2.5)), 1.5, theme::BRASS);
            }
            let alpha = if enabled { 1.0 } else { 0.45 };
            let badge = rect.left_center() + Vec2::new(18.0, 0.0);
            let is_done = done(s, t);
            p.circle_filled(badge, 10.0, if is_done { theme::OK } else if on { theme::BRASS } else { theme::INK_0 });
            let badge_text = if is_done { theme::icon::CHECK.to_string() } else { (i + 1).to_string() };
            p.text(badge, Align2::CENTER_CENTER, badge_text, FontId::proportional(11.5), if is_done || on { theme::INK_0 } else { theme::paper_2().gamma_multiply(alpha) });
            let tx = rect.left() + 34.0;
            p.text(Pos2::new(tx, rect.center().y - 8.0), Align2::LEFT_CENTER, format!("{}  {}", t.icon(), t.label()), FontId::proportional(13.5), theme::PAPER.gamma_multiply(alpha));
            if w > 120.0 {
                p.text(Pos2::new(tx, rect.center().y + 9.0), Align2::LEFT_CENTER, t.hint(), FontId::proportional(11.0), theme::paper_3().gamma_multiply(alpha));
            }
            if resp.clicked() {
                s.o.tab = t;
            }
        }
    });
}

// ---------------------------------------------------------------- pasek na dole

fn footer(s: &mut Studio, ui: &mut egui::Ui, cache: std::path::PathBuf) {
    ui.separator();
    ui.horizontal(|ui| {
        ui.set_min_height(FOOTER - 14.0);
        let action_w = 210.0;
        let status_w = (ui.available_width() - action_w - 12.0).max(120.0);
        ui.allocate_ui_with_layout(Vec2::new(status_w, FOOTER - 14.0), egui::Layout::left_to_right(egui::Align::Center), |ui| status(s, ui, status_w));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let free = s.exporting().is_none();
            match s.o.tab {
                Tab::Area => {
                    if ui.add_enabled(s.region().is_some(), theme::primary("Dalej: dane terenu")).clicked() {
                        s.o.tab = Tab::Terrain;
                    }
                }
                Tab::Terrain => {
                    let can = s.region().is_some() && s.busy().is_none();
                    let label = if s.scene.is_some() && !s.stale() { "Przygotuj dane ponownie" } else { "Przygotuj dane obszaru" };
                    if ui.add_enabled(can, theme::primary(label)).clicked() {
                        s.prepare(cache, ui.ctx());
                    }
                }
                Tab::Look => {
                    if ui.add_enabled(free, theme::primary("Zapisz obraz terenu…")).clicked() {
                        s.export_raster(false);
                    }
                }
                Tab::Model => {
                    if ui.add_enabled(free, theme::primary("Zapisz model 3D…")).clicked() {
                        s.export_model();
                    }
                }
                Tab::Section => {
                    if ui.add_enabled(free, theme::primary("Zapisz przekrój…")).clicked() {
                        s.export_sheet(SheetKind::Section);
                    }
                }
                Tab::Map => {
                    if ui.add_enabled(free, theme::primary("Zapisz mapę…")).clicked() {
                        s.export_sheet(SheetKind::Map);
                    }
                }
            }
        });
    });
}

/// Pasek stanu: postęp zadań, błąd, komunikat, ostatni plik.
fn status(s: &mut Studio, ui: &mut egui::Ui, w: f32) {
    if let Some((f, msg)) = s.busy() {
        ui.add(egui::ProgressBar::new(f).text(msg).desired_width(w - 100.0));
        if ui.button(tr("Przerwij")).clicked() {
            s.cancel();
        }
        return;
    }
    if let Some((f, msg)) = s.exporting() {
        ui.add(egui::ProgressBar::new(f).text(msg).desired_width(w - 10.0));
        return;
    }
    if let Some(e) = s.error.clone() {
        ui.add(egui::Label::new(egui::RichText::new(format!("{}  {e}", theme::icon::WARNING)).color(theme::SIGNAL)).truncate()).on_hover_text(e.clone());
        if ui.small_button(theme::icon::X).clicked() {
            s.error = None;
        }
    } else if let Some(n) = s.notice.clone() {
        ui.add(egui::Label::new(egui::RichText::new(format!("{}  {n}", theme::icon::CHECK_CIRCLE)).color(theme::OK)).truncate()).on_hover_text(n.clone());
        if let Some(f) = s.last_file.clone() {
            if ui.small_button(theme::ic(theme::icon::FOLDER_OPEN, "Pokaż w folderze")).clicked() {
                crate::platform::show_log(&f);
            }
        }
        if ui.small_button(theme::icon::X).clicked() {
            s.notice = None;
        }
    } else {
        ui.label(egui::RichText::new(tr("Wyniki zawierają podpisy źródeł danych (GUGiK, ČÚZK, Copernicus, OpenStreetMap).")).small().color(theme::paper_3()));
    }
}

// ---------------------------------------------------------------- kolumna ustawień

fn settings(s: &mut Studio, ui: &mut egui::Ui, cam: (f64, f64, f64), cache: std::path::PathBuf) {
    match s.o.tab {
        Tab::Area => area_tab(s, ui, cam),
        Tab::Terrain => terrain_tab(s, ui, cache),
        Tab::Look => look_tab(s, ui),
        Tab::Model => model_tab(s, ui),
        Tab::Section => section_tab(s, ui),
        Tab::Map => map_tab(s, ui),
    }
}

/// Karta sekcji ustawień z nagłówkiem.
fn card(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(theme::INK_1).corner_radius(10.0).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::kicker(title));
        ui.add_space(2.0);
        body(ui);
    });
    ui.add_space(8.0);
}

/// Karta zwijana (ustawienia zaawansowane).
fn fold(ui: &mut egui::Ui, id: &str, title: &str, open: bool, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(theme::INK_1).corner_radius(10.0).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::CollapsingHeader::new(theme::kicker(title)).id_salt(id).default_open(open).show(ui, body);
    });
    ui.add_space(8.0);
}

fn para(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(egui::RichText::new(text).color(theme::paper_2())).wrap());
}

fn note(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(egui::RichText::new(text).small().color(theme::paper_3())).wrap());
}

fn km(m: f64) -> String {
    if m >= 1000.0 {
        format!("{:.2} km", m / 1000.0)
    } else {
        format!("{m:.0} m")
    }
}

fn need_scene(s: &Studio, ui: &mut egui::Ui) -> bool {
    if s.scene.is_none() {
        para(ui, tr("Najpierw przygotuj dane obszaru (zakładka Dane)."));
        return false;
    }
    if s.stale() {
        ui.colored_label(theme::WARN, format!("{}  {}", theme::icon::WARNING, tr("Obszar zmienił się od przygotowania danych – przygotuj je ponownie.")));
        ui.add_space(6.0);
    }
    true
}

/// Siatka dwóch kolumn: etykieta i kontrolka.
fn grid(ui: &mut egui::Ui, id: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Grid::new(id).num_columns(2).spacing([10.0, 6.0]).show(ui, body);
}

// ---------------------------------------------------------------- obszar

const M_LAT: f64 = 110_574.0;
fn m_lon(lat: f64) -> f64 {
    111_320.0 * lat.to_radians().cos().max(0.05)
}

fn area_tab(s: &mut Studio, ui: &mut egui::Ui, (lon, lat, dist): (f64, f64, f64)) {
    card(ui, "KSZTAŁT WYCINKA", |ui| {
        ui.horizontal_wrapped(|ui| {
            for sh in Shape::ALL {
                if ui.add(egui::Button::selectable(s.o.area.shape == sh, format!("{}  {}", sh.icon(), sh.label()))).clicked() && s.o.area.shape != sh {
                    // prostokąt i koło mają inne znaczenie punktów niż wielokąt i pas
                    let keep = matches!((s.o.area.shape, sh), (Shape::Polygon, Shape::Ridge) | (Shape::Ridge, Shape::Polygon));
                    if !keep {
                        s.o.area.pts.clear();
                    }
                    s.o.area.shape = sh;
                }
            }
        });
        ui.add_space(4.0);
        para(ui, s.o.area.shape.hint());
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(tf!("Punkty: {}", s.o.area.pts.len()));
            if ui.add_enabled(!s.o.area.pts.is_empty(), egui::Button::new(theme::ic(theme::icon::ARROW_U_UP_LEFT, "Cofnij punkt"))).clicked() {
                s.o.area.pts.pop();
            }
            if ui.add_enabled(!s.o.area.pts.is_empty(), egui::Button::new(theme::ic(theme::icon::TRASH, "Wyczyść"))).clicked() {
                s.o.area.pts.clear();
            }
        });
        if ui.button(theme::ic(theme::icon::FRAME_CORNERS, "Kwadrat ze środka widoku")).on_hover_text(tr("Kwadrat wokół środka mapy, wielkością dopasowany do przybliżenia.")).clicked() {
            let half = (dist * 0.6).clamp(300.0, 25_000.0);
            s.o.area.shape = Shape::Rect;
            s.o.area.pts = vec![(lon - half / m_lon(lat), lat - half / M_LAT), (lon + half / m_lon(lat), lat + half / M_LAT)];
        }
    });
    // wymiary liczbowo (prostokąt, koło) i szerokość pasa
    let complete = s.o.area.complete();
    let shape = s.o.area.shape;
    match shape {
        Shape::Rect if complete => card(ui, "WYMIARY", |ui| {
            let (a, b) = (s.o.area.pts[0], s.o.area.pts[1]);
            let (clon, clat) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            let mut w = (b.0 - a.0).abs() * m_lon(clat);
            let mut h = (b.1 - a.1).abs() * M_LAT;
            let mut changed = false;
            grid(ui, "studio-rect", |ui| {
                ui.label(tr("Szerokość (W–E)"));
                changed |= ui.add(egui::DragValue::new(&mut w).range(100.0..=120_000.0).speed(10.0).suffix(" m")).changed();
                ui.end_row();
                ui.label(tr("Wysokość (N–S)"));
                changed |= ui.add(egui::DragValue::new(&mut h).range(100.0..=120_000.0).speed(10.0).suffix(" m")).changed();
                ui.end_row();
            });
            if changed {
                let (dx, dy) = (w / 2.0 / m_lon(clat), h / 2.0 / M_LAT);
                s.o.area.pts = vec![(clon - dx, clat - dy), (clon + dx, clat + dy)];
            }
            note(ui, tr("Środek obszaru zostaje na miejscu."));
        }),
        Shape::Circle if complete => card(ui, "WYMIARY", |ui| {
            let (c, e) = (s.o.area.pts[0], s.o.area.pts[1]);
            let mut r = (((e.0 - c.0) * m_lon(c.1)).powi(2) + ((e.1 - c.1) * M_LAT).powi(2)).sqrt();
            grid(ui, "studio-circle", |ui| {
                ui.label(tr("Promień"));
                if ui.add(egui::DragValue::new(&mut r).range(50.0..=60_000.0).speed(10.0).suffix(" m")).changed() {
                    s.o.area.pts[1] = (c.0 + r / m_lon(c.1), c.1);
                }
                ui.end_row();
            });
        }),
        Shape::Ridge => card(ui, "PAS WZDŁUŻ LINII", |ui| {
            ui.add(egui::Slider::new(&mut s.o.area.width, 100.0..=10_000.0).logarithmic(true).suffix(" m").text(tr("Szerokość pasa")));
        }),
        _ => {}
    }
    card(ui, "ODCIĘCIE WYSOKOŚCIĄ", |ui| {
        let mut on = s.o.area.above.is_some();
        if ui.checkbox(&mut on, tr("Tylko teren powyżej wysokości (np. sam masyw nad doliną)")).changed() {
            s.o.area.above = on.then(|| s.scene.as_ref().map(|sc| sc.grid.range()).map(|(lo, hi)| lo + (hi - lo) * 0.4).unwrap_or(1200.0));
        }
        if let Some(v) = s.o.area.above.as_mut() {
            ui.add(egui::DragValue::new(v).range(-400.0..=8800.0).speed(5.0).suffix(tr(" m n.p.m.")));
        }
    });
    if let Some((_, r)) = s.region() {
        let b = r.bounds();
        card(ui, "WIELKOŚĆ", |ui| {
            ui.label(egui::RichText::new(tf!("{} × {} · {:.2} km²", km(b[2] - b[0]), km(b[3] - b[1]), r.area_m2() / 1e6)).strong());
            if let Some(c) = s.cell() {
                note(ui, &tf!("Siatka terenu ok. {} × {} węzłów (oczko {:.1} m).", ((b[2] - b[0]) / c).round(), ((b[3] - b[1]) / c).round(), c));
            }
        });
    } else {
        note(ui, tr("Wskaż obszar na mapie – punkty możesz potem przeciągać, Backspace usuwa ostatni."));
    }
}

// ---------------------------------------------------------------- dane

fn terrain_tab(s: &mut Studio, ui: &mut egui::Ui, _cache: std::path::PathBuf) {
    use crate::studio::terrain::Source;
    card(ui, "ŹRÓDŁO TERENU", |ui| {
        for src in [Source::Best, Source::Copernicus] {
            ui.radio_value(&mut s.o.source, src, src.label());
        }
    });
    card(ui, "SZCZEGÓŁOWOŚĆ", |ui| {
        let mut auto = s.o.cell <= 0.0;
        ui.horizontal(|ui| {
            if ui.checkbox(&mut auto, tr("Oczko siatki dobierane samo")).changed() {
                s.o.cell = if auto { 0.0 } else { s.cell().unwrap_or(10.0) as f32 };
            }
            if !auto {
                ui.add(egui::DragValue::new(&mut s.o.cell).range(1.0..=90.0).speed(0.2).suffix(" m"));
            }
        });
        if let (Some((_, r)), Some(cell)) = (s.region(), s.cell()) {
            let b = r.bounds();
            let (nx, ny) = ((b[2] - b[0]) / cell, (b[3] - b[1]) / cell);
            note(ui, &tf!("Oczko {:.1} m · siatka {} × {} węzłów", cell, nx.round(), ny.round()));
            if s.o.source == Source::Best && cell < 30.0 {
                note(ui, tr("Poza Polską i Czechami dane mają ok. 30 m – drobniejsze oczko tylko wygładza teren."));
            }
        }
        ui.add(egui::Slider::new(&mut s.o.smooth, 0..=6).text(tr("Wygładzanie")));
    });
    card(ui, "OBIEKTY", |ui| {
        ui.checkbox(&mut s.o.osm, tr("Lasy, wody, skały, szlaki, drogi i nazwy z OpenStreetMap"));
        ui.add_enabled(s.o.osm, egui::Checkbox::new(&mut s.o.buildings, tr("Budynki (do 150 km²)")));
    });
    if s.region().is_none() {
        para(ui, tr("Najpierw wskaż obszar (zakładka Obszar)."));
    }
    if let Some(sc) = s.scene.clone() {
        card(ui, "PRZYGOTOWANE DANE", |ui| {
            let (lo, hi) = sc.grid.range();
            ui.label(tf!("Wysokości {:.0}–{:.0} m n.p.m. · siatka {} × {} ({:.1} m)", lo, hi, sc.grid.nx, sc.grid.ny, sc.grid.cell));
            if let Some(f) = &sc.feats {
                ui.label(tf!("Obiekty: {} linii, {} obszarów, {} nazw", f.lines.len(), f.polys.len(), f.labels.len()));
            }
            for c in sc.credits() {
                note(ui, &c);
            }
        });
    }
}

// ---------------------------------------------------------------- wygląd

fn look_tab(s: &mut Studio, ui: &mut egui::Ui) {
    if !need_scene(s, ui) {
        return;
    }
    let l = &mut s.o.look;
    card(ui, "KOLORY", |ui| {
        grid(ui, "studio-look", |ui| {
            ui.label(tr("Kolor od"));
            egui::ComboBox::from_id_salt("studio-mode").selected_text(l.mode.label()).show_ui(ui, |ui| {
                for m in Mode::ALL {
                    ui.selectable_value(&mut l.mode, m, m.label());
                }
            });
            ui.end_row();
            if l.mode == Mode::Elevation {
                ui.label(tr("Paleta"));
                egui::ComboBox::from_id_salt("studio-palette").selected_text(l.palette.label()).show_ui(ui, |ui| {
                    for p in Palette::ALL {
                        ui.selectable_value(&mut l.palette, p, p.label());
                    }
                });
                ui.end_row();
            }
            if l.mode == Mode::Plain {
                ui.label(tr("Kolor terenu"));
                ui.color_edit_button_srgb(&mut l.plain);
                ui.end_row();
            }
        });
        if l.mode == Mode::Elevation {
            ui.checkbox(&mut l.relative, tr("Paleta rozciągnięta na wysokości obszaru"));
        }
    });
    card(ui, "CIENIOWANIE", |ui| {
        ui.add(egui::Slider::new(&mut l.shade, 0.0..=1.0).text(tr("Siła")));
        ui.add(egui::Slider::new(&mut l.sun_az, 0.0..=360.0).suffix("°").text(tr("Azymut słońca")));
        ui.add(egui::Slider::new(&mut l.sun_alt, 5.0..=85.0).suffix("°").text(tr("Wysokość słońca")));
        ui.checkbox(&mut l.multi, tr("Światło z kilku kierunków"));
        ui.horizontal(|ui| {
            ui.checkbox(&mut l.contours, tr("Warstwice co"));
            ui.add_enabled(l.contours, egui::DragValue::new(&mut l.contour_step).range(1.0..=500.0).suffix(" m"));
        });
    });
    card(ui, "OBIEKTY NA TEKSTURZE", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut l.forest, tr("Lasy"));
            ui.checkbox(&mut l.water, tr("Wody"));
            ui.checkbox(&mut l.rock, tr("Skały"));
            ui.checkbox(&mut l.trails, tr("Szlaki"));
            ui.checkbox(&mut l.paths, tr("Ścieżki"));
            ui.checkbox(&mut l.roads, tr("Drogi"));
            ui.checkbox(&mut l.buildings, tr("Budynki"));
        });
    });
    card(ui, "OBRAZ", |ui| {
        ui.horizontal(|ui| {
            ui.label(tr("Tekstura modelu i obrazu"));
            egui::ComboBox::from_id_salt("studio-tex").selected_text(format!("{} px", s.o.tex_px)).show_ui(ui, |ui| {
                for px in [1024u32, 2048, 4096, 8192, 16384] {
                    ui.selectable_value(&mut s.o.tex_px, px, format!("{px} px"));
                }
            });
        });
        let free = s.exporting().is_none();
        if ui.add_enabled(free, egui::Button::new(theme::ic(theme::icon::STACK, "Mapa wysokości 16-bit"))).on_hover_text(tr("PNG w skali szarości (16 bitów) z opisem zakresu – do Blendera, silników gier i programów GIS.")).clicked() {
            s.export_raster(true);
        }
    });
}

// ---------------------------------------------------------------- model

fn model_tab(s: &mut Studio, ui: &mut egui::Ui) {
    if !need_scene(s, ui) {
        return;
    }
    let dims = s.scene.as_ref().map(|sc| (sc.region.bounds(), sc.grid.range()));
    let m = &mut s.o.model;
    card(ui, "BRYŁA", |ui| {
        ui.add(egui::Slider::new(&mut m.width_mm, 30.0..=1000.0).logarithmic(true).suffix(" mm").text(tr("Dłuższy bok")));
        ui.add(egui::Slider::new(&mut m.exag, 0.5..=5.0).suffix("×").text(tr("Przewyższenie")));
        ui.add(egui::Slider::new(&mut m.base_mm, 0.0..=40.0).suffix(" mm").text(tr("Podstawa")));
        ui.add(egui::Slider::new(&mut m.cells, 50..=2000).text(tr("Szczegółowość (oczka)")));
        if let Some((b, (lo, hi))) = dims {
            let longest = (b[2] - b[0]).max(b[3] - b[1]).max(1.0);
            let k = m.width_mm as f64 / longest;
            let h = (hi - lo) as f64 * k * m.exag as f64 + m.base_mm as f64;
            note(ui, &tf!("Około {:.0} × {:.0} × {:.0} mm · skala 1:{:.0}", (b[2] - b[0]) * k, (b[3] - b[1]) * k, h, 1000.0 / k));
            if (m.cells as f64).powi(2) * 2.0 > 3e6 {
                ui.label(egui::RichText::new(tr("Bardzo gęsta siatka – plik będzie duży, a zapis potrwa dłużej.")).small().color(theme::WARN));
            }
        }
    });
    card(ui, "MATERIAŁY", |ui| {
        ui.checkbox(&mut m.materials, tr("Osobne materiały dla lasów, skał, wód, śniegu i budynków"));
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(tr("Zestaw:")).color(theme::paper_3()));
            for p in MatPreset::ALL {
                if ui.small_button(p.label()).clicked() {
                    m.preset(p);
                }
            }
        });
        ui.add_space(4.0);
        egui::Grid::new("studio-mats").num_columns(4).spacing([8.0, 5.0]).show(ui, |ui| {
            ui.label(egui::RichText::new(tr("Materiał")).small().color(theme::paper_3()));
            ui.label(egui::RichText::new(tr("Kolor")).small().color(theme::paper_3()));
            ui.label(egui::RichText::new(tr("Tekstura")).small().color(theme::paper_3()));
            ui.label(egui::RichText::new(tr("Szorstkość")).small().color(theme::paper_3()));
            ui.end_row();
            for mat in Mat::ALL {
                if !m.materials && !matches!(mat, Mat::Ground | Mat::Walls | Mat::Bottom) {
                    continue;
                }
                let terrain = !matches!(mat, Mat::Walls | Mat::Bottom);
                let label = if !m.materials && mat == Mat::Ground { tr("Powierzchnia terenu") } else { mat.label() };
                let cfg = &mut m.mats[mat as usize];
                ui.label(label);
                ui.color_edit_button_srgb(&mut cfg.color).on_hover_text(if cfg.texture { tr("Odcień mnożony przez teksturę terenu") } else { tr("Kolor jednolity") });
                if terrain {
                    ui.checkbox(&mut cfg.texture, "");
                } else {
                    ui.label("");
                }
                ui.add(egui::DragValue::new(&mut cfg.roughness).range(0.0..=1.0).speed(0.01).fixed_decimals(2));
                ui.end_row();
            }
        });
        ui.add_space(4.0);
        grid(ui, "studio-mat-rules", |ui| {
            ui.label(tr("Ściany boczne"));
            egui::ComboBox::from_id_salt("studio-walls").selected_text(m.walls.label()).show_ui(ui, |ui| {
                for w in Walls::ALL {
                    ui.selectable_value(&mut m.walls, w, w.label());
                }
            });
            ui.end_row();
            if m.materials {
                ui.label(tr("Skała od nachylenia"));
                ui.add(egui::DragValue::new(&mut m.rock_slope).range(0.0..=89.0).speed(0.5).suffix("°")).on_hover_text(tr("0 – wyłączone"));
                ui.end_row();
                ui.label(tr("Śnieg powyżej"));
                ui.add(egui::DragValue::new(&mut m.snow_line).range(0.0..=8800.0).speed(10.0).suffix(tr(" m n.p.m."))).on_hover_text(tr("0 – wyłączone"));
                ui.end_row();
            }
        });
        note(ui, tr("Materiały trafiają do GLB (PBR: kolor, szorstkość, metaliczność), OBJ/MTL (osobne grupy) i kolorów PLY. STL nie przenosi materiałów."));
    });
    fold(ui, "studio-model-metal", "METALICZNOŚĆ", false, |ui| {
        egui::Grid::new("studio-metal").num_columns(2).spacing([8.0, 5.0]).show(ui, |ui| {
            for mat in Mat::ALL {
                ui.label(mat.label());
                ui.add(egui::Slider::new(&mut m.mats[mat as usize].metallic, 0.0..=1.0));
                ui.end_row();
            }
        });
    });
    card(ui, "ZAPIS", |ui| {
        para(ui, tr("GLB do przeglądarek, Blendera i silników gier (materiały PBR), OBJ z teksturami, STL do druku 3D (szczelna bryła z podstawą), PLY z kolorami wierzchołków."));
    });
}

// ---------------------------------------------------------------- przekrój

fn section_tab(s: &mut Studio, ui: &mut egui::Ui) {
    if !need_scene(s, ui) {
        return;
    }
    card(ui, "PŁASZCZYZNA", |ui| {
        ui.horizontal_wrapped(|ui| {
            for p in Plane::ALL {
                ui.selectable_value(&mut s.o.section.plane, p, p.label());
            }
        });
        match s.o.section.plane {
            Plane::Vertical => para(ui, tr("Kliknij na mapie punkty A i B – płaszczyzna pionowa przetnie teren wzdłuż tej linii. Wynik: profil z opisem wysokości, szlakami i nazwami.")),
            Plane::Horizontal => para(ui, tr("Płaszczyzna pozioma na zadanej wysokości – wynik to dokładna linia przecięcia z terenem (warstwica) w całym obszarze.")),
            Plane::Oblique => para(ui, tr("Kliknij A i B: linia A–B leży na zadanej wysokości, a płaszczyzna opada pod kątem upadu w prawo od kierunku A→B (jak warstwa skalna).")),
        }
        if s.o.section.plane != Plane::Horizontal {
            let n = s.o.section.line.len();
            ui.horizontal(|ui| {
                ui.label(match n {
                    0 => tr("Wskaż punkt A na mapie."),
                    1 => tr("Wskaż punkt B na mapie."),
                    _ => tr("Linia A–B gotowa (kliknięcie zaczyna nową)."),
                });
                if n > 0 && ui.small_button(theme::ic(theme::icon::TRASH, "Wyczyść")).clicked() {
                    s.o.section.line.clear();
                }
            });
            if n >= 2 && s.section_line().is_none() {
                ui.colored_label(theme::WARN, tr("Linia wychodzi poza przygotowany obszar."));
            }
        }
    });
    let (lo, hi) = s.scene.as_ref().map(|sc| sc.grid.range()).unwrap_or((0.0, 3000.0));
    let o = &mut s.o.section;
    card(ui, "RYSUNEK", |ui| {
        if o.plane != Plane::Vertical {
            ui.add(egui::Slider::new(&mut o.level, lo.floor()..=hi.ceil()).suffix(tr(" m n.p.m.")).text(tr("Wysokość płaszczyzny")));
        }
        if o.plane == Plane::Oblique {
            ui.add(egui::Slider::new(&mut o.dip, 1.0..=89.0).suffix("°").text(tr("Kąt upadu")));
        }
        if o.plane == Plane::Vertical {
            ui.add(egui::Slider::new(&mut o.exag, 1.0..=5.0).suffix("×").text(tr("Przewyższenie")));
        }
        ui.horizontal(|ui| {
            ui.label(tr("Skala 1:"));
            ui.add(egui::DragValue::new(&mut o.scale).range(500.0..=1_000_000.0).speed(100.0));
        });
        ui.checkbox(&mut o.labels, tr("Opisy (wysokości, nazwy, szlaki)"));
    });
    if s.o.section.plane == Plane::Vertical {
        if let (Some(sc), Some((a, b))) = (s.scene.clone(), s.section_line()) {
            card(ui, "PROFIL", |ui| profile_chart(ui, &crate::studio::section::profile(&sc.grid, a, b)));
        }
    }
    card(ui, "ZAPIS", |ui| {
        para(ui, tr("SVG z warstwami, PDF z warstwami do druku, DXF do programów CAD albo PNG; profil pionowy także jako CSV."));
    });
}

fn profile_chart(ui: &mut egui::Ui, prof: &[(f64, f32, f64, f64)]) {
    if prof.len() < 2 {
        return;
    }
    let (resp, p) = ui.allocate_painter(Vec2::new(ui.available_width(), 110.0), Sense::hover());
    let r = resp.rect.shrink2(Vec2::new(4.0, 8.0));
    p.rect_filled(resp.rect, 8.0, theme::INK_0);
    let len = prof.last().map(|v| v.0).unwrap_or(1.0).max(1.0);
    let (lo, hi) = prof.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), v| (a.min(v.1), b.max(v.1)));
    let span = (hi - lo).max(1.0);
    let at = |d: f64, z: f32| Pos2::new(r.left() + (d / len) as f32 * r.width(), r.bottom() - (z - lo) / span * r.height());
    let line: Vec<Pos2> = prof.iter().map(|v| at(v.0, v.1)).collect();
    // wielokąt profilu może być wklęsły – wypełnienie pionowymi paskami
    for w in line.windows(2) {
        let q = [w[0], w[1], Pos2::new(w[1].x, r.bottom()), Pos2::new(w[0].x, r.bottom())];
        p.add(egui::Shape::convex_polygon(q.to_vec(), theme::BRASS_DIM, Stroke::NONE));
    }
    p.add(egui::Shape::line(line, Stroke::new(1.8, theme::BRASS)));
    let small = FontId::proportional(11.0);
    p.text(r.left_top() + Vec2::new(2.0, 0.0), Align2::LEFT_TOP, format!("{hi:.0} m"), small.clone(), theme::paper_2());
    p.text(r.left_bottom() + Vec2::new(2.0, 0.0), Align2::LEFT_BOTTOM, format!("{lo:.0} m"), small.clone(), theme::paper_2());
    p.text(r.right_bottom(), Align2::RIGHT_BOTTOM, km(len), small, theme::paper_2());
}

// ---------------------------------------------------------------- mapa

fn map_tab(s: &mut Studio, ui: &mut egui::Ui) {
    if !need_scene(s, ui) {
        return;
    }
    let o = &mut s.o.map;
    card(ui, "ARKUSZ", |ui| {
        grid(ui, "studio-sheet", |ui| {
            ui.label(tr("Tytuł"));
            ui.add(egui::TextEdit::singleline(&mut o.title).hint_text(tr("np. Masyw Śnieżnika")).desired_width(210.0));
            ui.end_row();
            ui.label(tr("Podtytuł"));
            ui.add(egui::TextEdit::singleline(&mut o.subtitle).hint_text(tr("np. Mapa turystyczna")).desired_width(210.0));
            ui.end_row();
            ui.label(tr("Autor"));
            ui.add(egui::TextEdit::singleline(&mut o.author).hint_text(tr("opcjonalnie")).desired_width(210.0));
            ui.end_row();
            ui.label(tr("Papier"));
            egui::ComboBox::from_id_salt("studio-paper").selected_text(o.paper.label()).width(210.0).show_ui(ui, |ui| {
                for p in Paper::ALL {
                    ui.selectable_value(&mut o.paper, p, p.label());
                }
            });
            ui.end_row();
            ui.label(tr("Orientacja"));
            ui.horizontal(|ui| {
                for r in Orient::ALL {
                    ui.selectable_value(&mut o.orient, r, r.label());
                }
            });
            ui.end_row();
            if o.paper == Paper::Scale {
                ui.label(tr("Skala 1:"));
                ui.add(egui::DragValue::new(&mut o.scale).range(1000.0..=1_000_000.0).speed(250.0));
                ui.end_row();
            }
            ui.label(tr("Margines"));
            ui.add(egui::Slider::new(&mut o.margin, 3.0..=40.0).suffix(" mm"));
            ui.end_row();
        });
        if o.paper != Paper::Scale {
            note(ui, tr("Skala zostanie dopasowana tak, żeby obszar zmieścił się na arkuszu."));
        }
    });
    card(ui, "STYL", |ui| {
        grid(ui, "studio-style", |ui| {
            ui.label(tr("Styl mapy"));
            egui::ComboBox::from_id_salt("studio-style").selected_text(o.style.label()).width(210.0).show_ui(ui, |ui| {
                for st in MapStyle::ALL {
                    ui.selectable_value(&mut o.style, st, st.label());
                }
            });
            ui.end_row();
            ui.label(tr("Krój pisma"));
            egui::ComboBox::from_id_salt("studio-font").selected_text(o.font.label()).width(210.0).show_ui(ui, |ui| {
                for f in Font::ALL {
                    ui.selectable_value(&mut o.font, f, f.label());
                }
            });
            ui.end_row();
            ui.label(tr("Napisy"));
            ui.add(egui::Slider::new(&mut o.text_scale, 0.5..=2.5).suffix("×"));
            ui.end_row();
            ui.label(tr("Grubość linii"));
            ui.add(egui::Slider::new(&mut o.line_scale, 0.3..=3.0).suffix("×"));
            ui.end_row();
            ui.label(tr("Podkład"));
            egui::ComboBox::from_id_salt("studio-relief").selected_text(o.relief.label()).width(210.0).show_ui(ui, |ui| {
                for r in Relief::ALL {
                    ui.selectable_value(&mut o.relief, r, r.label());
                }
            });
            ui.end_row();
            if o.relief != Relief::None {
                ui.label(tr("Rozdzielczość"));
                ui.add(egui::DragValue::new(&mut o.dpi).range(72..=600).suffix(" dpi"));
                ui.end_row();
            }
        });
    });
    card(ui, "RZEŹBA TERENU", |ui| {
        ui.horizontal(|ui| {
            ui.checkbox(&mut o.contours, tr("Warstwice co"));
            ui.add_enabled(o.contours, egui::DragValue::new(&mut o.step).range(1.0..=500.0).suffix(" m"));
        });
        ui.add_enabled_ui(o.contours, |ui| {
            ui.horizontal(|ui| {
                ui.label(tr("Główna co"));
                ui.add(egui::DragValue::new(&mut o.index_every).range(2..=10));
                ui.label(tr("warstwice"));
            });
            ui.checkbox(&mut o.smooth, tr("Wygładzone krzywe"));
            ui.horizontal(|ui| {
                ui.checkbox(&mut o.contour_labels, tr("Opisy warstwic, gęstość"));
                ui.add_enabled(o.contour_labels, egui::Slider::new(&mut o.label_density, 0.3..=4.0).show_value(false));
            });
        });
    });
    card(ui, "TREŚĆ MAPY", |ui| {
        ui.columns(2, |c| {
            c[0].checkbox(&mut o.forest, tr("Lasy"));
            c[0].checkbox(&mut o.water, tr("Wody"));
            c[0].checkbox(&mut o.rock, tr("Skały i lodowce"));
            c[0].checkbox(&mut o.buildings, tr("Budynki"));
            c[0].checkbox(&mut o.roads, tr("Drogi i koleje"));
            c[1].checkbox(&mut o.paths, tr("Ścieżki"));
            c[1].checkbox(&mut o.trails, tr("Szlaki"));
            c[1].checkbox(&mut o.peaks, tr("Szczyty i schroniska"));
            c[1].checkbox(&mut o.peak_ele, tr("Wysokości szczytów"));
            c[1].checkbox(&mut o.places, tr("Miejscowości"));
        });
    });
    card(ui, "RAMKA I OPIS", |ui| {
        ui.columns(2, |c| {
            c[0].checkbox(&mut o.graticule, tr("Siatka geograficzna"));
            c[0].checkbox(&mut o.frame, tr("Ramka obszaru"));
            c[0].checkbox(&mut o.legend, tr("Legenda"));
            c[1].checkbox(&mut o.scalebar, tr("Podziałka"));
            c[1].checkbox(&mut o.north, tr("Strzałka północy"));
        });
    });
    layers_card(s, ui);
    card(ui, "ZAPIS", |ui| {
        ui.checkbox(&mut s.o.map.split_layers, tr("Każda warstwa także w osobnym pliku SVG"));
        para(ui, tr("SVG z zagnieżdżonymi warstwami (Inkscape, Illustrator, Affinity), PDF z warstwami do druku, DXF do programów CAD albo PNG w rozdzielczości podkładu."));
        if ui.add_enabled(s.exporting().is_none(), egui::Button::new(theme::ic(theme::icon::WAVE_SINE, "Warstwice (GeoJSON)"))).clicked() {
            s.export_contours();
        }
    });
}

/// Menedżer warstw mapy: widoczność, blokada, kolor, krycie, grubość, pominięcie w pliku.
fn layers_card(s: &mut Studio, ui: &mut egui::Ui) {
    // warstwy z ostatniego podglądu i te pominięte (których w rysunku już nie ma)
    let mut rows: Vec<(String, String, String, usize, [u8; 3])> = s.sheet.layers.iter().map(|l| (l.id.to_string(), l.name.clone(), l.group_name.clone(), l.count, l.color)).collect();
    for (id, c) in &s.o.map.layers {
        if !c.export && !rows.iter().any(|r| r.0 == *id) {
            rows.push((id.clone(), id.clone(), tr("Pominięte").to_string(), 0, [128, 128, 128]));
        }
    }
    let show_kind = s.sheet.kind == Some(SheetKind::Map);
    fold(ui, "studio-layers", "WARSTWY", true, |ui| {
        if rows.is_empty() || !show_kind {
            note(ui, tr("Lista warstw pojawi się po narysowaniu podglądu mapy."));
            return;
        }
        ui.horizontal(|ui| {
            if ui.small_button(theme::ic(theme::icon::EYE, "Pokaż wszystkie")).clicked() {
                for c in s.o.map.layers.values_mut() {
                    c.visible = true;
                    c.export = true;
                }
            }
            if ui.small_button(theme::ic(theme::icon::ARROW_COUNTER_CLOCKWISE, "Przywróć domyślne")).clicked() {
                s.o.map.layers.clear();
            }
        });
        ui.add_space(4.0);
        let mut last_group = String::new();
        for (id, name, group, count, lead) in rows {
            if group != last_group {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(group.to_uppercase()).small().strong().color(theme::paper_3()));
                last_group = group.clone();
            }
            let mut cfg = s.o.map.layers.get(&id).cloned().unwrap_or_default();
            let before = cfg.clone();
            ui.horizontal(|ui| {
                let eye = if cfg.visible { theme::icon::EYE } else { theme::icon::EYE_SLASH };
                if ui.add(egui::Button::new(eye).frame(false)).on_hover_text(tr("Widoczna w pliku")).clicked() {
                    cfg.visible = !cfg.visible;
                }
                let lock = if cfg.locked { theme::icon::LOCK_SIMPLE } else { theme::icon::LOCK_SIMPLE_OPEN };
                if ui.add(egui::Button::new(lock).frame(false)).on_hover_text(tr("Zablokowana do edycji (Inkscape)")).clicked() {
                    cfg.locked = !cfg.locked;
                }
                let mut col = cfg.color.unwrap_or(lead);
                if ui.color_edit_button_srgb(&mut col).on_hover_text(tr("Kolor warstwy")).changed() {
                    cfg.color = Some(col);
                }
                let text = if count > 0 { format!("{name}  ·  {count}") } else { name.clone() };
                let rich = egui::RichText::new(text).color(if cfg.export && cfg.visible { theme::PAPER } else { theme::paper_3() });
                ui.add(egui::Label::new(rich).truncate());
            });
            ui.horizontal(|ui| {
                ui.add_space(26.0);
                ui.label(egui::RichText::new(tr("krycie")).small().color(theme::paper_3()));
                let mut pct = (cfg.opacity * 100.0).round();
                if ui.add(egui::DragValue::new(&mut pct).range(0.0..=100.0).suffix(" %").speed(1.0)).changed() {
                    cfg.opacity = pct / 100.0;
                }
                ui.label(egui::RichText::new(tr("linie")).small().color(theme::paper_3()));
                ui.add(egui::DragValue::new(&mut cfg.width).range(0.1..=5.0).speed(0.02).suffix("×").fixed_decimals(2));
                ui.checkbox(&mut cfg.export, "").on_hover_text(tr("Zapisywana w pliku (odznaczona – pominięta)"));
                if cfg.color.is_some() && ui.small_button(theme::icon::ARROW_COUNTER_CLOCKWISE).on_hover_text(tr("Kolor stylu")).clicked() {
                    cfg.color = None;
                }
            });
            if cfg != before {
                if cfg.is_default() {
                    s.o.map.layers.remove(&id);
                } else {
                    s.o.map.layers.insert(id.clone(), cfg);
                }
            }
        }
        ui.add_space(4.0);
        note(ui, tr("Ukryta warstwa zostaje w pliku (do włączenia w programie graficznym), pominięta – nie trafia do niego wcale."));
    });
}

// ---------------------------------------------------------------- podgląd (prawa strona)

fn stage(s: &mut Studio, ui: &mut egui::Ui, rect: Rect) {
    ui.painter().rect_filled(rect, 12.0, theme::INK_0);
    match s.o.tab {
        Tab::Area => area_canvas(s, ui, rect),
        Tab::Terrain => {
            if s.scene.is_some() && !s.stale() {
                flat_view(s, ui, rect);
            } else {
                area_canvas(s, ui, rect);
            }
        }
        Tab::Look => {
            if s.scene.is_none() {
                empty(ui, rect, tr("Najpierw przygotuj dane obszaru (zakładka Dane)."));
                return;
            }
            let flat_id = egui::Id::new("studio-look-flat");
            let mut flat = ui.ctx().data(|d| d.get_temp::<bool>(flat_id)).unwrap_or(false);
            if flat {
                flat_view(s, ui, rect);
            } else {
                preview_3d(s, ui, rect);
            }
            let r = Rect::from_min_size(rect.right_top() + Vec2::new(-186.0, 10.0), Vec2::new(176.0, 26.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::selectable(!flat, theme::ic(theme::icon::CUBE, "Bryła"))).clicked() {
                        flat = false;
                    }
                    if ui.add(egui::Button::selectable(flat, theme::ic(theme::icon::IMAGE, "Tekstura"))).clicked() {
                        flat = true;
                    }
                });
            });
            ui.ctx().data_mut(|d| d.insert_temp(flat_id, flat));
        }
        Tab::Model => {
            if s.scene.is_none() {
                empty(ui, rect, tr("Najpierw przygotuj dane obszaru (zakładka Dane)."));
                return;
            }
            preview_3d(s, ui, rect);
        }
        Tab::Section => {
            if s.scene.is_none() {
                empty(ui, rect, tr("Najpierw przygotuj dane obszaru (zakładka Dane)."));
                return;
            }
            sheet_view(s, ui, rect, SheetKind::Section);
        }
        Tab::Map => {
            if s.scene.is_none() {
                empty(ui, rect, tr("Najpierw przygotuj dane obszaru (zakładka Dane)."));
                return;
            }
            sheet_view(s, ui, rect, SheetKind::Map);
        }
    }
}

fn empty(ui: &mut egui::Ui, rect: Rect, text: &str) {
    let p = ui.painter_at(rect);
    p.text(rect.center() - Vec2::new(0.0, 14.0), Align2::CENTER_CENTER, theme::icon::CUBE, FontId::proportional(34.0), theme::paper_3());
    p.text(rect.center() + Vec2::new(0.0, 18.0), Align2::CENTER_CENTER, text, FontId::proportional(14.0), theme::paper_2());
}

/// Schemat obszaru w metrach: obrys, siatka kilometrowa, wymiary.
fn area_canvas(s: &mut Studio, ui: &mut egui::Ui, rect: Rect) {
    let p = ui.painter_at(rect);
    let Some((_, r)) = s.region() else {
        p.text(rect.center() - Vec2::new(0.0, 26.0), Align2::CENTER_CENTER, s.o.area.shape.icon(), FontId::proportional(40.0), theme::BRASS);
        p.text(rect.center() + Vec2::new(0.0, 12.0), Align2::CENTER_CENTER, tr("Wskaż obszar na mapie"), FontId::proportional(18.0), theme::PAPER);
        p.text(rect.center() + Vec2::new(0.0, 38.0), Align2::CENTER_CENTER, s.o.area.shape.hint(), FontId::proportional(13.0), theme::paper_2());
        return;
    };
    let b = r.bounds();
    let (wm, hm) = ((b[2] - b[0]).max(1.0), (b[3] - b[1]).max(1.0));
    let inner = rect.shrink(56.0);
    let k = (inner.width() as f64 / wm).min(inner.height() as f64 / hm);
    let c = inner.center();
    let at = |x: f64, y: f64| Pos2::new(c.x + ((x - (b[0] + b[2]) / 2.0) * k) as f32, c.y - ((y - (b[1] + b[3]) / 2.0) * k) as f32);
    // siatka co „ładną” liczbę metrów
    let step = [100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0, 20_000.0, 50_000.0].into_iter().find(|st| wm.max(hm) / st <= 8.0).unwrap_or(100_000.0);
    let grid_c = Color32::from_white_alpha(14);
    let mut x = (b[0] / step).floor() * step;
    while x <= b[2] + step {
        p.line_segment([at(x, b[1] - step), at(x, b[3] + step)], Stroke::new(1.0, grid_c));
        x += step;
    }
    let mut y = (b[1] / step).floor() * step;
    while y <= b[3] + step {
        p.line_segment([at(b[0] - step, y), at(b[2] + step, y)], Stroke::new(1.0, grid_c));
        y += step;
    }
    let pts: Vec<Pos2> = r.outline(240).into_iter().map(|(x, y)| at(x, y)).collect();
    let mut closed = pts.clone();
    if let Some(f) = pts.first().copied() {
        closed.push(f);
    }
    p.add(egui::Shape::line(closed, Stroke::new(2.5, Color32::from_rgb(255, 170, 60))));
    // wymiary
    let (tl, br) = (at(b[0], b[3]), at(b[2], b[1]));
    let dim = Stroke::new(1.0, theme::paper_3());
    p.line_segment([Pos2::new(tl.x, br.y + 18.0), Pos2::new(br.x, br.y + 18.0)], dim);
    p.text(Pos2::new((tl.x + br.x) / 2.0, br.y + 30.0), Align2::CENTER_CENTER, km(wm), FontId::proportional(13.0), theme::PAPER);
    p.line_segment([Pos2::new(br.x + 18.0, tl.y), Pos2::new(br.x + 18.0, br.y)], dim);
    p.text(Pos2::new(br.x + 24.0, (tl.y + br.y) / 2.0), Align2::LEFT_CENTER, km(hm), FontId::proportional(13.0), theme::PAPER);
    p.text(rect.left_top() + Vec2::new(16.0, 14.0), Align2::LEFT_TOP, tf!("{:.2} km² · siatka co {}", r.area_m2() / 1e6, km(step)), FontId::proportional(12.5), theme::paper_2());
    p.text(rect.right_top() + Vec2::new(-16.0, 14.0), Align2::RIGHT_TOP, "N ↑", FontId::proportional(13.0), theme::paper_2());
    if s.o.tab == Tab::Area {
        p.text(rect.center_bottom() - Vec2::new(0.0, 16.0), Align2::CENTER_CENTER, tr("Punkty obszaru przeciągniesz na mapie; Backspace usuwa ostatni."), FontId::proportional(12.0), theme::paper_3());
    }
}

/// Płaski podgląd tekstury terenu z obrysem wycinka.
fn flat_view(s: &mut Studio, ui: &mut egui::Ui, rect: Rect) {
    let key = s.flat_key();
    let stale = s.flat.as_ref().map(|(k, _)| *k != key).unwrap_or(true);
    if stale {
        if let Some((_, tex)) = s.preview_mesh() {
            let img = egui::ColorImage::from_rgba_unmultiplied([tex.width() as usize, tex.height() as usize], tex.as_raw());
            s.flat = Some((key, ui.ctx().load_texture("studio-flat", img, egui::TextureOptions::LINEAR)));
        }
    }
    let (Some((_, t)), Some(sc)) = (&s.flat, &s.scene) else { return };
    let [tw, th] = t.size();
    let inner = rect.shrink(16.0);
    let k = (inner.width() / tw.max(1) as f32).min(inner.height() / th.max(1) as f32);
    let r = Rect::from_center_size(inner.center(), Vec2::new(tw as f32 * k, th as f32 * k));
    let p = ui.painter_at(rect);
    p.image(t.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    // obrys wycinka
    let g = &sc.grid;
    let to = |x: f64, y: f64| Pos2::new(r.left() + ((x - g.x0) / (g.x1() - g.x0)) as f32 * r.width(), r.top() + ((g.y1 - y) / (g.y1 - g.y0())) as f32 * r.height());
    let mut pts: Vec<Pos2> = sc.region.outline(160).into_iter().map(|(x, y)| to(x, y)).collect();
    if let Some(f) = pts.first().copied() {
        pts.push(f);
    }
    p.add(egui::Shape::line(pts, Stroke::new(1.5, Color32::from_white_alpha(210))));
}

/// Podgląd 3D na całą scenę: przeciąganie obraca, kółko przybliża, dwuklik – widok domyślny.
fn preview_3d(s: &mut Studio, ui: &mut egui::Ui, rect: Rect) {
    let id = egui::Id::new("studio-preview");
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    if resp.dragged() {
        let d = resp.drag_delta();
        s.view.yaw += d.x * 0.5;
        s.view.pitch = (s.view.pitch + d.y * 0.4).clamp(5.0, 89.0);
    }
    if resp.hovered() {
        let z = ui.ctx().input(|i| i.smooth_scroll_delta.y);
        if z != 0.0 {
            s.view.zoom = (s.view.zoom * (1.0 + z * 0.002)).clamp(0.3, 6.0);
        }
    }
    if resp.double_clicked() {
        s.view = Default::default();
    }
    let ppp = ui.ctx().pixels_per_point();
    // przy obracaniu mniejszy obraz (płynność), po puszczeniu – pełny
    let q = if resp.dragged() { 0.5 } else { 1.0 };
    let (w, h) = (((rect.width() * ppp * q) as usize).max(16), ((rect.height() * ppp * q) as usize).max(16));
    let view_key = {
        let v = s.view;
        (v.yaw.to_bits() as u64) ^ ((v.pitch.to_bits() as u64) << 21) ^ ((v.zoom.to_bits() as u64) << 42) ^ (w as u64 * 31 + h as u64)
    };
    let key = s.preview_key() ^ view_key.rotate_left(7);
    // przy przeciąganiu suwaków ustawień – dopiero po puszczeniu (chyba że nie ma żadnego obrazu)
    let dragging_ui = ui.ctx().input(|i| i.pointer.any_down()) && !resp.dragged();
    let stale = s.preview.as_ref().map(|(k, _)| *k != key).unwrap_or(true);
    if stale && (!dragging_ui || s.preview.is_none()) {
        if let Some((mesh, tex)) = s.preview_mesh() {
            let l = &s.o.look;
            let (az, alt) = (l.sun_az.to_radians(), l.sun_alt.to_radians());
            let light = [az.sin() * alt.cos(), az.cos() * alt.cos(), alt.sin()];
            let px = crate::studio::preview::render(&mesh, &tex, &s.o.model, s.strata(), light, &s.view, w, h);
            let img = egui::ColorImage::from_rgba_unmultiplied([w, h], px.as_flattened());
            let t = ui.ctx().load_texture("studio-preview", img, egui::TextureOptions::LINEAR);
            s.preview = Some((key, t));
        }
    }
    let p = ui.painter_at(rect);
    if let Some((_, t)) = &s.preview {
        p.image(t.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }
    // opis wymiarów i podpowiedź
    if let Some((_, mesh, _)) = &s.preview_data {
        let sz = mesh.size();
        let k = s.o.model.width_mm / sz[0].max(sz[1]).max(1e-3);
        p.text(rect.left_bottom() + Vec2::new(14.0, -14.0), Align2::LEFT_BOTTOM, tf!("{:.0} × {:.0} × {:.0} mm", sz[0] * k, sz[1] * k, sz[2] * k), FontId::proportional(12.5), theme::paper_2());
    }
    p.text(rect.right_bottom() + Vec2::new(-14.0, -14.0), Align2::RIGHT_BOTTOM, tr("Przeciągnij – obrót · kółko – przybliżenie · dwuklik – widok domyślny"), FontId::proportional(11.5), theme::paper_3());
}

/// Podgląd arkusza: obraz prawdziwego pliku SVG (ten sam silnik co programy graficzne),
/// przybliżanie kółkiem wokół kursora, przesuwanie przeciąganiem, dwuklik – cały arkusz.
fn sheet_view(s: &mut Studio, ui: &mut egui::Ui, rect: Rect, kind: SheetKind) {
    let ppp = ui.ctx().pixels_per_point();
    let resp = ui.interact(rect, egui::Id::new("studio-sheet"), Sense::click_and_drag());
    let sh = &mut s.sheet;
    if sh.zoom <= 0.0 {
        sh.zoom = 1.0;
    }
    if resp.dragged() {
        let d = resp.drag_delta();
        sh.pan.0 += d.x;
        sh.pan.1 += d.y;
    }
    if resp.hovered() {
        let z = ui.ctx().input(|i| i.smooth_scroll_delta.y);
        if z != 0.0 {
            let z0 = sh.zoom;
            let z1 = (z0 * (1.0 + z * 0.0025)).clamp(1.0, 12.0);
            if let Some(cur) = resp.hover_pos() {
                let c = cur - rect.center();
                sh.pan.0 = c.x - (c.x - sh.pan.0) * z1 / z0;
                sh.pan.1 = c.y - (c.y - sh.pan.1) * z1 / z0;
            }
            sh.zoom = z1;
        }
    }
    if resp.double_clicked() {
        sh.zoom = 1.0;
        sh.pan = (0.0, 0.0);
    }
    if sh.zoom <= 1.0001 {
        sh.pan = (0.0, 0.0);
    }
    // rozdzielczość obrazu: do wielkości na ekranie razy przybliżenie (stopniowana co 512 px)
    let want = (rect.width().max(rect.height()) * ppp * sh.zoom.min(4.0)).clamp(800.0, 4096.0);
    let px = ((want / 512.0).ceil() * 512.0) as u32;
    let current_px = sh.tex.as_ref().map(|t| t.size()[0].max(t.size()[1]) as u32).unwrap_or(0);
    // przy samym przybliżaniu nie zmniejszamy już narysowanego obrazu
    let px = if sh.kind == Some(kind) && current_px > px { current_px } else { px };
    let ctx = ui.ctx().clone();
    s.sheet_request(kind, px, &ctx);
    let sh = &s.sheet;
    let p = ui.painter_at(rect);
    let shown = sh.kind == Some(kind);
    if let (true, Some(t)) = (shown, &sh.tex) {
        let (w, h) = sh.size_mm;
        let inner = rect.shrink(18.0);
        let k = (inner.width() / w.max(1.0) as f32).min(inner.height() / h.max(1.0) as f32) * sh.zoom;
        let size = Vec2::new(w as f32 * k, h as f32 * k);
        let r = Rect::from_center_size(rect.center() + Vec2::new(sh.pan.0, sh.pan.1), size);
        p.rect_filled(r.translate(Vec2::new(0.0, 4.0)).expand(2.0), 4.0, Color32::from_black_alpha(90));
        p.image(t.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        // pasek informacji
        let info = tf!("{} × {} mm · SVG {} kB · {} %", w.round(), h.round(), (sh.svg_len / 1024).max(1), (sh.zoom * 100.0).round());
        let ir = Rect::from_min_size(rect.left_bottom() + Vec2::new(12.0, -36.0), Vec2::new(300.0, 24.0));
        p.rect_filled(ir, 8.0, Color32::from_black_alpha(150));
        p.text(ir.left_center() + Vec2::new(10.0, 0.0), Align2::LEFT_CENTER, info, FontId::proportional(12.0), theme::PAPER);
    } else if let (true, Some(e)) = (shown, &sh.error) {
        empty(ui, rect, e);
    }
    if shown {
        if let Some(e) = sh.error.as_ref().filter(|_| sh.tex.is_some()) {
            let er = Rect::from_min_size(rect.left_top() + Vec2::new(12.0, 12.0), Vec2::new(rect.width() - 24.0, 26.0));
            p.rect_filled(er, 8.0, Color32::from_black_alpha(170));
            p.text(er.left_center() + Vec2::new(10.0, 0.0), Align2::LEFT_CENTER, format!("{}  {e}", theme::icon::WARNING), FontId::proportional(12.5), theme::WARN);
        }
    }
    if sh.busy() {
        let br = Rect::from_min_size(rect.right_top() + Vec2::new(-150.0, 12.0), Vec2::new(138.0, 26.0));
        p.rect_filled(br, 8.0, Color32::from_black_alpha(150));
        ui.put(Rect::from_center_size(br.left_center() + Vec2::new(16.0, 0.0), Vec2::splat(14.0)), egui::Spinner::new().size(14.0));
        p.text(br.left_center() + Vec2::new(30.0, 0.0), Align2::LEFT_CENTER, tr("Rysowanie…"), FontId::proportional(12.0), theme::PAPER);
    } else if !shown {
        empty(ui, rect, tr("Rysowanie…"));
    }
    // przyciski przybliżenia
    let zr = Rect::from_min_size(rect.right_bottom() + Vec2::new(-128.0, -40.0), Vec2::new(116.0, 28.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(zr), |ui| {
        ui.horizontal(|ui| {
            let sh = &mut s.sheet;
            if ui.small_button(theme::icon::MAGNIFYING_GLASS_MINUS).clicked() {
                sh.zoom = (sh.zoom / 1.5).max(1.0);
                sh.pan = (sh.pan.0 / 1.5, sh.pan.1 / 1.5);
            }
            if ui.small_button(theme::icon::CORNERS_OUT).on_hover_text(tr("Cały arkusz")).clicked() {
                sh.zoom = 1.0;
                sh.pan = (0.0, 0.0);
            }
            if ui.small_button(theme::icon::MAGNIFYING_GLASS_PLUS).clicked() {
                sh.zoom = (sh.zoom * 1.5).min(12.0);
                sh.pan = (sh.pan.0 * 1.5, sh.pan.1 * 1.5);
            }
        });
    });
}

// ---------------------------------------------------------------- nad mapą 3D

/// Obrys obszaru Studio, punkty do przeciągania i linia przekroju A–B nad widokiem 3D;
/// `at` rzutuje punkt (stopnie) na ekran, leżący na terenie. Punkty obszaru są tylko na
/// zakładce Obszar – na pozostałych obrys jest przygaszony i nie da się go zmienić.
pub fn overlay(s: &Studio, p: &egui::Painter, at: &dyn Fn(f64, f64) -> Option<Pos2>) {
    let poly = |pts: &[(f64, f64)], color: Color32, width: f32| {
        let mut segs: Vec<Vec<Pos2>> = vec![Vec::new()];
        for &(lon, lat) in pts {
            match at(lon, lat) {
                Some(q) => segs.last_mut().unwrap().push(q),
                None => segs.push(Vec::new()),
            }
        }
        for l in segs.into_iter().filter(|l| l.len() > 1) {
            p.add(egui::Shape::line(l.clone(), Stroke::new(width + 2.0, Color32::from_black_alpha(130))));
            p.add(egui::Shape::line(l, Stroke::new(width, color)));
        }
    };
    let editing = s.pick == Pick::Area;
    let accent = if editing { Color32::from_rgb(255, 170, 60) } else { Color32::from_rgb(255, 170, 60).gamma_multiply(0.55) };
    // obrys gotowego kształtu (gęsto próbkowany, żeby leżał na terenie)
    let mut outline = s.o.area.outline();
    if let Some(f) = outline.first().copied() {
        outline.push(f);
    }
    poly(&densify(&outline, 400), accent, if editing { 2.5 } else { 2.0 });
    let handle = |q: Pos2, c: Color32, label: Option<&str>| {
        p.circle_filled(q, 6.5, c);
        p.circle_stroke(q, 6.5, Stroke::new(2.0, theme::INK_0));
        if let Some(t) = label {
            p.text(q + Vec2::new(9.0, -9.0), Align2::LEFT_BOTTOM, t, FontId::proportional(14.0), theme::PAPER);
        }
    };
    if editing {
        // wierzchołki i łamana szkicu
        let pts = &s.o.area.pts;
        if !s.o.area.complete() || matches!(s.o.area.shape, Shape::Ridge | Shape::Polygon) {
            poly(&densify(pts, 200), accent.gamma_multiply(0.6), 1.5);
        }
        for &(lon, lat) in pts {
            if let Some(q) = at(lon, lat) {
                handle(q, accent, None);
            }
        }
    }
    // linia przekroju
    let line = &s.o.section.line;
    if s.pick == Pick::Section && !line.is_empty() {
        let c = Color32::from_rgb(90, 220, 255);
        poly(&densify(line, 300), c, 2.5);
        for (i, &(lon, lat)) in line.iter().enumerate() {
            if let Some(q) = at(lon, lat) {
                handle(q, c, Some(if i == 0 { "A" } else { "B" }));
            }
        }
    }
}

/// Zagęszcza łamaną (stopnie) do ok. `n` punktów, żeby rzut leżał na terenie.
fn densify(pts: &[(f64, f64)], n: usize) -> Vec<(f64, f64)> {
    if pts.len() < 2 {
        return pts.to_vec();
    }
    let per = (n / (pts.len() - 1)).max(2);
    let mut out = Vec::with_capacity(per * pts.len());
    for w in pts.windows(2) {
        for k in 0..per {
            let t = k as f64 / per as f64;
            out.push((w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t));
        }
    }
    out.push(*pts.last().unwrap());
    out
}
