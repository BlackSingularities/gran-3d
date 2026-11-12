//! Ustawienia mapy: styl, warstwy, nazwy, światło i grafika.

use super::*;

/// Styl mapy jako kafelki z ikonami.
pub(super) fn style_tab(a: &mut Atlas, ui: &mut egui::Ui) {
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
        Style::Swiss => Some(crate::i18n::tr("Jak na mapach szwajcarskich: cieniowanie od północnego zachodu, skały kreskami, lodowce i wody na niebiesko.")),
        Style::Sketch => Some(crate::i18n::tr("Jak na dawnych rycinach: biały papier, gęste poziomice i kreski spadku – im stromiej, tym grubsze.")),
        Style::Insolation => Some(crate::i18n::tr("Godziny bezpośredniego słońca w wybranym dniu, z cieniami gór: granat – cały dzień w cieniu, żółć – słońce od wschodu do zachodu. Linie co godzinę. Dzień ustawisz w panelu pory dnia.")),
        _ => None,
    };
    if let Some(l) = legend {
        theme::para(ui, l);
    }
    ui.add_space(10.0);
    ui.add(egui::Slider::new(&mut a.prefs.exag, 1.0..=3.0).step_by(0.1).text(crate::i18n::tr("przewyższenie ×")));
}

/// Nakładki na mapie jako kafelki z ikonami.
pub(super) fn layers_tab(a: &mut Atlas, ui: &mut egui::Ui) {
    // co naprawdę zmienia obraz w danym stylu: śnieg tylko w barwach terenu (na zdjęciu jest
    // zdjęcie), pokrycie (woda, lasy, drzewa) w stylach Teren, Satelita i Mapa – nie w analitycznych
    let snow_off = (a.prefs.style != Style::Terrain).then_some(crate::i18n::tr("widać tylko w stylu Teren (zdjęcie satelitarne ma własny śnieg)"));
    let land_off = a.prefs.style.analytic().then_some(crate::i18n::tr("w stylach analitycznych nie ma wody, lasów ani drzew"));
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
        (theme::icon::ROAD_HORIZON, crate::i18n::tr("Drogi i ulice"), &mut p.roads, None),
        (theme::icon::WAVE_SINE, crate::i18n::tr("Poziomice (C)"), &mut p.contours, None),
        (theme::icon::GRID_FOUR, crate::i18n::tr("Siatka UTM"), &mut p.km_grid, None),
        (theme::icon::LINK, crate::i18n::tr("Łańcuchy i klamry"), &mut p.chains, None),
        (theme::icon::FLAG, crate::i18n::tr("Granice parków"), &mut p.boundaries, None),
        (theme::icon::PROHIBIT, crate::i18n::tr("Zamknięte szlaki (Z)"), &mut p.closures, None),
        (theme::icon::PERSON_SIMPLE_SKI, crate::i18n::tr("Trasy narciarskie (J)"), &mut p.pistes, None),
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
    theme::para(ui, "Nazwy pokazują się tylko dla miejsc, które widać – te za graniami są ukryte. Przystanki, stacje, parkingi, granie, doliny i źródła doczytują się razem ze szlakami okolicy; przystanki widać z bliska (do kilku kilometrów).");
}

/// Strona „Mapa” w ustawieniach.
pub fn map_settings(a: &mut Atlas, ui: &mut egui::Ui) {
    ui.label(theme::kicker(crate::i18n::tr("Zapis współrzędnych")));
    for f in crate::coords::CoordFormat::ALL {
        if theme::option_card(ui, f.name(), &f.example(), a.prefs.coord_format == f).clicked() {
            a.prefs.coord_format = f;
            crate::coords::set_format(f);
        }
    }
    theme::para(ui, crate::i18n::tr("Tak współrzędne pokazują się przy kursorze, w szczegółach punktu i przy kopiowaniu. UTM i MGRS to siatka w metrach z map topograficznych; MGRS podają ratownicy (TOPR, GOPR) i służby. W wyszukiwarkę można wpisać współrzędne w każdym z tych zapisów."));
    ui.add_space(12.0);
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

pub(super) fn light_tab(a: &mut Atlas, ui: &mut egui::Ui) {
    let (day, hour) = a.when();
    let mut h = hour;
    let mut d = day;
    let t = sun::times(d, a.cam.lat, a.cam.lon);
    let (az, alt) = a.sun();
    ui.horizontal(|ui| {
        ui.label(RichText::new(fmt::hour(hour)).size(34.0).strong());
        ui.vertical(|ui| {
            ui.label(RichText::new(if a.prefs.custom_time.is_some() { crate::i18n::tr("pora ustawiona") } else { crate::i18n::tr("teraz") }).small().color(theme::paper_3()));
            ui.label(RichText::new(tf!("Słońce {}° · {} {}", fmt::dec(alt, 0), az.round(), fmt::compass(az))).small().color(theme::paper_2()));
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
        ui.label(RichText::new(tf!("Rysowane kafle: {} · w pamięci: teren {}, zdjęcia {}", r.drawn, d, i)).small().monospace().color(theme::paper_3()));
    }
}
