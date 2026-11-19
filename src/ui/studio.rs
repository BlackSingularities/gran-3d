//! Okno Studio (tylko komputer, tylko konta z dostępem): obszar wskazany na mapie, przygotowanie
//! danych, wygląd, model 3D, przekroje i mapy wektorowe – z podglądem na żywo.
//!
//! Silnik jest w [`crate::studio`]; tu są tylko kontrolki, podglądy i rysowanie obszaru na mapie.

use eframe::egui::{self, Color32, Pos2, Sense, Stroke, Vec2};

use crate::i18n::tr;
use crate::studio::color::{Mode, Palette};
use crate::studio::geom::Shape;
use crate::studio::mapdraw::{Paper, Relief};
use crate::studio::section::Plane;
use crate::studio::{Pick, Studio, Tab};
use crate::ui::atlas::{Atlas, Tool};
use crate::ui::theme;

/// Szerokość okna Studio (pt).
const WIDTH: f32 = 470.0;
/// Rozmiar podglądu 3D (pt).
const PREVIEW: Vec2 = Vec2::new(440.0, 300.0);

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
    // klik na mapie: linia przekroju tylko na zakładce przekroju (płaszczyzna inna niż pozioma)
    a.studio.pick = if a.studio.o.tab == Tab::Section && a.studio.o.section.plane != Plane::Horizontal { Pick::Section } else { Pick::Area };
    let cam = (a.cam.lon, a.cam.lat, a.cam.distance);
    let cache = a.heights.store().root().to_path_buf();
    let mut open = true;
    egui::Window::new(format!("{}  {}", theme::icon::CUBE, tr("GRAŃ Studio")))
        .id(egui::Id::new("gran-studio"))
        .open(&mut open)
        .default_width(theme::fit(ctx, WIDTH))
        .min_width(theme::fit(ctx, 380.0))
        .default_pos(Pos2::new(ctx.screen_rect().right() - WIDTH - 24.0, 80.0))
        .resizable(true)
        .collapsible(true)
        .frame(egui::Frame::window(&ctx.style()).fill(theme::POPUP_FILL))
        .show(ctx, |ui| {
            let s = &mut a.studio;
            tabs(s, ui);
            ui.separator();
            egui::ScrollArea::vertical().max_height((ctx.screen_rect().height() - 220.0).max(240.0)).auto_shrink([false, true]).show(ui, |ui| {
                match s.o.tab {
                    Tab::Area => area_tab(s, ui, cam),
                    Tab::Terrain => terrain_tab(s, ui, cache.clone()),
                    Tab::Look => look_tab(s, ui),
                    Tab::Model => model_tab(s, ui),
                    Tab::Section => section_tab(s, ui),
                    Tab::Map => map_tab(s, ui),
                }
            });
            ui.separator();
            status(s, ui);
        });
    if !open {
        a.close_tool();
    }
    a.studio.persist();
}

fn tabs(s: &mut Studio, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        for t in Tab::ALL {
            let needs_scene = !matches!(t, Tab::Area | Tab::Terrain);
            let enabled = !needs_scene || s.scene.is_some();
            let r = ui.add_enabled(enabled, egui::Button::selectable(s.o.tab == t, format!("{}  {}", t.icon(), t.label())));
            let r = if enabled { r } else { r.on_disabled_hover_text(tr("Najpierw przygotuj dane obszaru (zakładka Dane).")) };
            if r.clicked() {
                s.o.tab = t;
            }
        }
    });
}

/// Pasek stanu: postęp zadań, błąd, komunikat.
fn status(s: &mut Studio, ui: &mut egui::Ui) {
    if let Some((f, msg)) = s.busy() {
        ui.horizontal(|ui| {
            ui.add(egui::ProgressBar::new(f).text(msg).desired_width(ui.available_width() - 90.0));
            if ui.button(tr("Przerwij")).clicked() {
                s.cancel();
            }
        });
    }
    if let Some((f, msg)) = s.exporting() {
        ui.add(egui::ProgressBar::new(f).text(msg));
    }
    if let Some(e) = s.error.clone() {
        ui.horizontal(|ui| {
            ui.colored_label(theme::SIGNAL, format!("{}  {e}", theme::icon::WARNING));
            if ui.small_button(theme::icon::X).clicked() {
                s.error = None;
            }
        });
    } else if let Some(n) = s.notice.clone() {
        ui.horizontal(|ui| {
            ui.colored_label(theme::OK, format!("{}  {n}", theme::icon::CHECK_CIRCLE));
            if ui.small_button(theme::icon::X).clicked() {
                s.notice = None;
            }
        });
    } else if s.busy().is_none() && s.exporting().is_none() {
        ui.label(egui::RichText::new(tr("Wyniki zawierają podpisy źródeł danych (GUGiK, ČÚZK, Copernicus, OpenStreetMap).")).small().color(theme::paper_3()));
    }
}

fn para(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(egui::RichText::new(text).color(theme::paper_2())).wrap());
}

fn km(m: f64) -> String {
    if m >= 1000.0 { format!("{:.2} km", m / 1000.0) } else { format!("{m:.0} m") }
}

fn need_scene(s: &Studio, ui: &mut egui::Ui) -> bool {
    if s.scene.is_none() {
        para(ui, tr("Najpierw przygotuj dane obszaru (zakładka Dane)."));
        return false;
    }
    if s.stale() {
        ui.colored_label(theme::WARN, format!("{}  {}", theme::icon::WARNING, tr("Obszar zmienił się od przygotowania danych – przygotuj je ponownie.")));
    }
    true
}

// ---------------------------------------------------------------- obszar

fn area_tab(s: &mut Studio, ui: &mut egui::Ui, (lon, lat, dist): (f64, f64, f64)) {
    ui.label(theme::kicker("KSZTAŁT WYCINKA"));
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
    para(ui, s.o.area.shape.hint());
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
        let dlat = half / 110_574.0;
        let dlon = half / (111_320.0 * lat.to_radians().cos().max(0.05));
        s.o.area.shape = Shape::Rect;
        s.o.area.pts = vec![(lon - dlon, lat - dlat), (lon + dlon, lat + dlat)];
    }
    if s.o.area.shape == Shape::Ridge {
        ui.add_space(4.0);
        ui.add(egui::Slider::new(&mut s.o.area.width, 100.0..=10_000.0).logarithmic(true).suffix(" m").text(tr("Szerokość pasa")));
    }
    ui.add_space(6.0);
    ui.label(theme::kicker("ODCIĘCIE WYSOKOŚCIĄ"));
    let mut on = s.o.area.above.is_some();
    if ui.checkbox(&mut on, tr("Tylko teren powyżej wysokości (np. sam masyw nad doliną)")).changed() {
        s.o.area.above = on.then(|| s.scene.as_ref().map(|sc| sc.grid.range()).map(|(lo, hi)| lo + (hi - lo) * 0.4).unwrap_or(1200.0));
    }
    if let Some(v) = s.o.area.above.as_mut() {
        ui.add(egui::DragValue::new(v).range(-400.0..=8800.0).speed(5.0).suffix(tr(" m n.p.m.")));
    }
    ui.add_space(6.0);
    if let Some((_, r)) = s.region() {
        let b = r.bounds();
        ui.label(theme::kicker("WIELKOŚĆ"));
        ui.label(tf!("{} × {} · {:.2} km²", km(b[2] - b[0]), km(b[3] - b[1]), r.area_m2() / 1e6));
        if ui.add(theme::primary("Dalej: dane terenu")).clicked() {
            s.o.tab = Tab::Terrain;
        }
    } else {
        para(ui, tr("Wskaż obszar na mapie – punkty możesz potem przeciągać, Backspace usuwa ostatni."));
    }
}

// ---------------------------------------------------------------- dane

fn terrain_tab(s: &mut Studio, ui: &mut egui::Ui, cache: std::path::PathBuf) {
    use crate::studio::terrain::Source;
    ui.label(theme::kicker("ŹRÓDŁO TERENU"));
    for src in [Source::Best, Source::Copernicus] {
        ui.radio_value(&mut s.o.source, src, src.label());
    }
    ui.add_space(4.0);
    ui.label(theme::kicker("SZCZEGÓŁOWOŚĆ"));
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
        ui.label(egui::RichText::new(tf!("Oczko {:.1} m · siatka {} × {} węzłów", cell, nx.round(), ny.round())).color(theme::paper_3()));
        if s.o.source == Source::Best && cell < 30.0 {
            ui.label(egui::RichText::new(tr("Poza Polską i Czechami dane mają ok. 30 m – drobniejsze oczko tylko wygładza teren.")).small().color(theme::paper_3()));
        }
    }
    ui.add(egui::Slider::new(&mut s.o.smooth, 0..=6).text(tr("Wygładzanie")));
    ui.add_space(4.0);
    ui.label(theme::kicker("OBIEKTY"));
    ui.checkbox(&mut s.o.osm, tr("Lasy, wody, skały, szlaki, drogi i nazwy z OpenStreetMap"));
    ui.add_enabled(s.o.osm, egui::Checkbox::new(&mut s.o.buildings, tr("Budynki (do 150 km²)")));
    ui.add_space(8.0);
    let can = s.region().is_some() && s.busy().is_none();
    let label = if s.scene.is_some() && !s.stale() { "Przygotuj dane ponownie" } else { "Przygotuj dane obszaru" };
    if ui.add_enabled(can, theme::primary(label)).clicked() {
        s.prepare(cache, ui.ctx());
    }
    if s.region().is_none() {
        para(ui, tr("Najpierw wskaż obszar (zakładka Obszar)."));
    }
    if let Some(sc) = s.scene.clone() {
        ui.add_space(8.0);
        ui.label(theme::kicker("PRZYGOTOWANE DANE"));
        let (lo, hi) = sc.grid.range();
        ui.label(tf!("Wysokości {:.0}–{:.0} m n.p.m. · siatka {} × {} ({:.1} m)", lo, hi, sc.grid.nx, sc.grid.ny, sc.grid.cell));
        if let Some(f) = &sc.feats {
            ui.label(tf!("Obiekty: {} linii, {} obszarów, {} nazw", f.lines.len(), f.polys.len(), f.labels.len()));
        }
        for c in sc.credits() {
            ui.label(egui::RichText::new(c).small().color(theme::paper_3()));
        }
    }
}

// ---------------------------------------------------------------- wygląd

fn look_tab(s: &mut Studio, ui: &mut egui::Ui) {
    if !need_scene(s, ui) {
        return;
    }
    preview_3d(s, ui);
    let l = &mut s.o.look;
    ui.label(theme::kicker("KOLORY"));
    egui::Grid::new("studio-look").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
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
        ui.label(tr("Kolor podstawy"));
        ui.color_edit_button_srgb(&mut l.base);
        ui.end_row();
    });
    if l.mode == Mode::Elevation {
        ui.checkbox(&mut l.relative, tr("Paleta rozciągnięta na wysokości obszaru"));
    }
    ui.add_space(4.0);
    ui.label(theme::kicker("CIENIOWANIE"));
    ui.add(egui::Slider::new(&mut l.shade, 0.0..=1.0).text(tr("Siła")));
    ui.add(egui::Slider::new(&mut l.sun_az, 0.0..=360.0).suffix("°").text(tr("Azymut słońca")));
    ui.add(egui::Slider::new(&mut l.sun_alt, 5.0..=85.0).suffix("°").text(tr("Wysokość słońca")));
    ui.checkbox(&mut l.multi, tr("Światło z kilku kierunków"));
    ui.horizontal(|ui| {
        ui.checkbox(&mut l.contours, tr("Warstwice co"));
        ui.add_enabled(l.contours, egui::DragValue::new(&mut l.contour_step).range(1.0..=500.0).suffix(" m"));
    });
    ui.add_space(4.0);
    ui.label(theme::kicker("OBIEKTY NA TEKSTURZE"));
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut l.forest, tr("Lasy"));
        ui.checkbox(&mut l.water, tr("Wody"));
        ui.checkbox(&mut l.rock, tr("Skały"));
        ui.checkbox(&mut l.trails, tr("Szlaki"));
        ui.checkbox(&mut l.paths, tr("Ścieżki"));
        ui.checkbox(&mut l.roads, tr("Drogi"));
        ui.checkbox(&mut l.buildings, tr("Budynki"));
    });
    ui.add_space(6.0);
    ui.label(theme::kicker("OBRAZ"));
    ui.horizontal(|ui| {
        ui.label(tr("Tekstura"));
        egui::ComboBox::from_id_salt("studio-tex").selected_text(format!("{} px", s.o.tex_px)).show_ui(ui, |ui| {
            for px in [1024u32, 2048, 4096, 8192, 16384] {
                ui.selectable_value(&mut s.o.tex_px, px, format!("{px} px"));
            }
        });
    });
    ui.horizontal_wrapped(|ui| {
        let free = s.exporting().is_none();
        if ui.add_enabled(free, egui::Button::new(theme::ic(theme::icon::IMAGE, "Zapisz obraz terenu (PNG)"))).clicked() {
            s.export_raster(false);
        }
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
    preview_3d(s, ui);
    let m = &mut s.o.model;
    ui.label(theme::kicker("BRYŁA"));
    ui.add(egui::Slider::new(&mut m.width_mm, 30.0..=1000.0).logarithmic(true).suffix(" mm").text(tr("Dłuższy bok")));
    ui.add(egui::Slider::new(&mut m.exag, 0.5..=5.0).suffix("×").text(tr("Przewyższenie")));
    ui.add(egui::Slider::new(&mut m.base_mm, 0.0..=40.0).suffix(" mm").text(tr("Podstawa")));
    ui.add(egui::Slider::new(&mut m.cells, 50..=2000).text(tr("Szczegółowość (oczka)")));
    if let Some(sc) = s.scene.as_ref() {
        let b = sc.region.bounds();
        let longest = (b[2] - b[0]).max(b[3] - b[1]).max(1.0);
        let k = m.width_mm as f64 / longest;
        let (lo, hi) = sc.grid.range();
        let h = (hi - lo) as f64 * k * m.exag as f64 + m.base_mm as f64;
        ui.label(egui::RichText::new(tf!("Około {:.0} × {:.0} × {:.0} mm · skala 1:{:.0}", (b[2] - b[0]) * k, (b[3] - b[1]) * k, h, 1000.0 / k)).color(theme::paper_3()));
        let tris = (m.cells as f64).powi(2) * 2.0;
        if tris > 3e6 {
            ui.label(egui::RichText::new(tr("Bardzo gęsta siatka – plik będzie duży, a zapis potrwa dłużej.")).small().color(theme::WARN));
        }
    }
    ui.add_space(6.0);
    para(ui, tr("STL do druku 3D (szczelna bryła z podstawą), OBJ z teksturą, GLB do przeglądarek i Blendera, PLY z kolorami wierzchołków."));
    if ui.add_enabled(s.exporting().is_none(), theme::primary("Zapisz model 3D…")).clicked() {
        s.export_model();
    }
}

/// Podgląd 3D: przeciąganie obraca, kółko przybliża, dwuklik – widok domyślny.
fn preview_3d(s: &mut Studio, ui: &mut egui::Ui) {
    let size = Vec2::new(PREVIEW.x.min(ui.available_width()), PREVIEW.y);
    let ppp = ui.ctx().pixels_per_point();
    let (w, h) = ((size.x * ppp) as usize, (size.y * ppp) as usize);
    let view_key = {
        let v = s.view;
        (v.yaw.to_bits() as u64) ^ ((v.pitch.to_bits() as u64) << 21) ^ ((v.zoom.to_bits() as u64) << 42) ^ (w as u64 * 31 + h as u64)
    };
    let key = s.preview_key() ^ view_key.rotate_left(7);
    // przeliczanie przy przeciąganiu suwaków – dopiero po puszczeniu (chyba że nie ma żadnego obrazu)
    let dragging_ui = ui.ctx().input(|i| i.pointer.any_down()) && !ui.ctx().is_being_dragged(egui::Id::new("studio-preview"));
    let stale = s.preview.as_ref().map(|(k, _)| *k != key).unwrap_or(true);
    if stale && (!dragging_ui || s.preview.is_none()) {
        if let Some((mesh, tex)) = s.preview_mesh() {
            let l = &s.o.look;
            let (az, alt) = (l.sun_az.to_radians(), l.sun_alt.to_radians());
            let light = [az.sin() * alt.cos(), az.cos() * alt.cos(), alt.sin()];
            let px = crate::studio::preview::render(&mesh, &tex, l.base, light, &s.view, w, h);
            let img = egui::ColorImage::from_rgba_unmultiplied([w, h], px.as_flattened());
            let t = ui.ctx().load_texture("studio-preview", img, egui::TextureOptions::LINEAR);
            s.preview = Some((key, t));
        }
    }
    let Some((_, t)) = &s.preview else { return };
    let r = ui.add(egui::Image::new((t.id(), size)).sense(Sense::click_and_drag())).on_hover_text(tr("Przeciągnij, żeby obrócić; kółko myszy – przybliżenie; dwuklik – widok domyślny."));
    let r = r.interact(Sense::click_and_drag());
    if r.dragged() {
        let d = r.drag_delta();
        s.view.yaw += d.x * 0.5;
        s.view.pitch = (s.view.pitch + d.y * 0.4).clamp(5.0, 89.0);
    }
    if r.hovered() {
        let z = ui.ctx().input(|i| i.smooth_scroll_delta.y);
        if z != 0.0 {
            s.view.zoom = (s.view.zoom * (1.0 + z * 0.002)).clamp(0.3, 6.0);
        }
    }
    if r.double_clicked() {
        s.view = Default::default();
    }
}

// ---------------------------------------------------------------- przekrój

fn section_tab(s: &mut Studio, ui: &mut egui::Ui) {
    if !need_scene(s, ui) {
        return;
    }
    ui.label(theme::kicker("PŁASZCZYZNA"));
    ui.horizontal_wrapped(|ui| {
        for p in Plane::ALL {
            ui.selectable_value(&mut s.o.section.plane, p, p.label());
        }
    });
    let (lo, hi) = s.scene.as_ref().map(|sc| sc.grid.range()).unwrap_or((0.0, 3000.0));
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
    let o = &mut s.o.section;
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
    if o.plane == Plane::Vertical {
        if let (Some(sc), Some((a, b))) = (s.scene.clone(), s.section_line()) {
            profile_chart(ui, &crate::studio::section::profile(&sc.grid, a, b));
        }
    }
    ui.add_space(6.0);
    para(ui, tr("Zapis jako SVG (warstwy), PDF albo DXF; profil pionowy także jako CSV."));
    if ui.add_enabled(s.exporting().is_none(), theme::primary("Zapisz przekrój…")).clicked() {
        s.export_section();
    }
}

fn profile_chart(ui: &mut egui::Ui, prof: &[(f64, f32, f64, f64)]) {
    if prof.len() < 2 {
        return;
    }
    let (resp, p) = ui.allocate_painter(Vec2::new(ui.available_width(), 120.0), Sense::hover());
    let r = resp.rect.shrink2(Vec2::new(4.0, 8.0));
    p.rect_filled(resp.rect, 8.0, theme::INK_1);
    let len = prof.last().map(|v| v.0).unwrap_or(1.0).max(1.0);
    let (lo, hi) = prof.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), v| (a.min(v.1), b.max(v.1)));
    let span = (hi - lo).max(1.0);
    let at = |d: f64, z: f32| Pos2::new(r.left() + (d / len) as f32 * r.width(), r.bottom() - (z - lo) / span * r.height());
    let mut fill: Vec<Pos2> = prof.iter().map(|v| at(v.0, v.1)).collect();
    let line = fill.clone();
    fill.push(Pos2::new(r.right(), r.bottom()));
    fill.push(Pos2::new(r.left(), r.bottom()));
    // wielokąt profilu może być wklęsły – wypełnienie pionowymi paskami
    for w in line.windows(2) {
        let q = [w[0], w[1], Pos2::new(w[1].x, r.bottom()), Pos2::new(w[0].x, r.bottom())];
        p.add(egui::Shape::convex_polygon(q.to_vec(), theme::BRASS_DIM, Stroke::NONE));
    }
    drop(fill);
    p.add(egui::Shape::line(line, Stroke::new(1.8, theme::BRASS)));
    let small = egui::FontId::proportional(11.0);
    p.text(r.left_top() + Vec2::new(2.0, 0.0), egui::Align2::LEFT_TOP, format!("{hi:.0} m"), small.clone(), theme::paper_2());
    p.text(r.left_bottom() + Vec2::new(2.0, 0.0), egui::Align2::LEFT_BOTTOM, format!("{lo:.0} m"), small.clone(), theme::paper_2());
    p.text(r.right_bottom(), egui::Align2::RIGHT_BOTTOM, km(len), small.clone(), theme::paper_2());
    p.text(Pos2::new(r.left() + 40.0, r.top()), egui::Align2::LEFT_TOP, "A", small.clone(), theme::PAPER);
    p.text(Pos2::new(r.right() - 2.0, r.top()), egui::Align2::RIGHT_TOP, "B", small, theme::PAPER);
}

// ---------------------------------------------------------------- mapa

fn map_tab(s: &mut Studio, ui: &mut egui::Ui) {
    if !need_scene(s, ui) {
        return;
    }
    flat_preview(s, ui);
    let o = &mut s.o.map;
    ui.label(theme::kicker("ARKUSZ"));
    egui::Grid::new("studio-map").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        ui.label(tr("Tytuł"));
        ui.add(egui::TextEdit::singleline(&mut o.title).hint_text(tr("np. Masyw Śnieżnika")).desired_width(240.0));
        ui.end_row();
        ui.label(tr("Skala 1:"));
        ui.add(egui::DragValue::new(&mut o.scale).range(1000.0..=1_000_000.0).speed(250.0));
        ui.end_row();
        ui.label(tr("Papier"));
        egui::ComboBox::from_id_salt("studio-paper").selected_text(o.paper.label()).show_ui(ui, |ui| {
            for p in Paper::ALL {
                ui.selectable_value(&mut o.paper, p, p.label());
            }
        });
        ui.end_row();
        ui.label(tr("Podkład"));
        egui::ComboBox::from_id_salt("studio-relief").selected_text(o.relief.label()).show_ui(ui, |ui| {
            for r in Relief::ALL {
                ui.selectable_value(&mut o.relief, r, r.label());
            }
        });
        ui.end_row();
        if o.relief != Relief::None {
            ui.label(tr("Rozdzielczość podkładu"));
            ui.add(egui::DragValue::new(&mut o.dpi).range(72..=600).suffix(" dpi"));
            ui.end_row();
        }
    });
    if o.paper != Paper::Scale {
        ui.label(egui::RichText::new(tr("Skala zostanie dopasowana tak, żeby obszar zmieścił się na arkuszu.")).small().color(theme::paper_3()));
    }
    ui.add_space(4.0);
    ui.label(theme::kicker("WARSTWY"));
    ui.horizontal(|ui| {
        ui.checkbox(&mut o.contours, tr("Warstwice co"));
        ui.add_enabled(o.contours, egui::DragValue::new(&mut o.step).range(1.0..=500.0).suffix(" m"));
        ui.add_enabled(o.contours, egui::Checkbox::new(&mut o.contour_labels, tr("z opisem")));
    });
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut o.forest, tr("Lasy"));
        ui.checkbox(&mut o.water, tr("Wody"));
        ui.checkbox(&mut o.rock, tr("Skały"));
        ui.checkbox(&mut o.buildings, tr("Budynki"));
        ui.checkbox(&mut o.roads, tr("Drogi"));
        ui.checkbox(&mut o.paths, tr("Ścieżki"));
        ui.checkbox(&mut o.trails, tr("Szlaki"));
        ui.checkbox(&mut o.peaks, tr("Szczyty"));
        ui.checkbox(&mut o.places, tr("Miejscowości"));
        ui.checkbox(&mut o.graticule, tr("Siatka współrzędnych"));
        ui.checkbox(&mut o.legend, tr("Legenda i podziałka"));
    });
    ui.add_space(6.0);
    para(ui, tr("SVG z warstwami (Inkscape, Illustrator), PDF do druku albo DXF do programów CAD. Warstwice osobno jako GeoJSON do programów GIS."));
    ui.horizontal_wrapped(|ui| {
        let free = s.exporting().is_none();
        if ui.add_enabled(free, theme::primary("Zapisz mapę…")).clicked() {
            s.export_map();
        }
        if ui.add_enabled(free, egui::Button::new(theme::ic(theme::icon::WAVE_SINE, "Warstwice (GeoJSON)"))).clicked() {
            s.export_contours();
        }
    });
}

/// Płaski podgląd tekstury terenu (z ustawień wyglądu) z obrysem wycinka.
fn flat_preview(s: &mut Studio, ui: &mut egui::Ui) {
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
    let w = PREVIEW.x.min(ui.available_width());
    let h = (w * th as f32 / tw.max(1) as f32).min(PREVIEW.y);
    let w = h * tw as f32 / th.max(1) as f32;
    let r = ui.add(egui::Image::new((t.id(), Vec2::new(w, h))));
    // obrys wycinka na podglądzie
    let g = &sc.grid;
    let to = |x: f64, y: f64| Pos2::new(r.rect.left() + ((x - g.x0) / (g.x1() - g.x0)) as f32 * r.rect.width(), r.rect.top() + ((g.y1 - y) / (g.y1 - g.y0())) as f32 * r.rect.height());
    let mut pts: Vec<Pos2> = sc.region.outline(120).into_iter().map(|(x, y)| to(x, y)).collect();
    if let Some(f) = pts.first().copied() {
        pts.push(f);
    }
    ui.painter_at(r.rect).add(egui::Shape::line(pts, Stroke::new(1.5, Color32::from_white_alpha(200))));
}

// ---------------------------------------------------------------- nad mapą 3D

/// Obrys obszaru Studio, punkty do przeciągania i linia przekroju A–B nad widokiem 3D;
/// `at` rzutuje punkt (stopnie) na ekran, leżący na terenie.
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
    let accent = Color32::from_rgb(255, 170, 60);
    // obrys gotowego kształtu (gęsto próbkowany, żeby leżał na terenie)
    let mut outline = s.o.area.outline();
    if let Some(f) = outline.first().copied() {
        outline.push(f);
    }
    poly(&densify(&outline, 400), accent, 2.5);
    // wierzchołki i łamana szkicu
    let pts = &s.o.area.pts;
    if !s.o.area.complete() || matches!(s.o.area.shape, Shape::Ridge | Shape::Polygon) {
        poly(&densify(pts, 200), accent.gamma_multiply(0.6), 1.5);
    }
    let handle = |q: Pos2, c: Color32, label: Option<&str>| {
        p.circle_filled(q, 6.5, c);
        p.circle_stroke(q, 6.5, Stroke::new(2.0, theme::INK_0));
        if let Some(t) = label {
            p.text(q + Vec2::new(9.0, -9.0), egui::Align2::LEFT_BOTTOM, t, egui::FontId::proportional(14.0), theme::PAPER);
        }
    };
    for &(lon, lat) in pts {
        if let Some(q) = at(lon, lat) {
            handle(q, accent, None);
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
