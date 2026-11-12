//! Legenda na mapie (lewy dolny róg): barwy stylu analitycznego albo włączonej soczewki.
//! Barwy są te same co w `terrain.wgsl` (wartości sRGB z shadera).

use super::*;

/// Szerokość karty legendy.
const W: f32 = 236.0;

fn rgb(c: [f32; 3]) -> Color32 {
    Color32::from_rgb((c[0] * 255.0).round() as u8, (c[1] * 255.0).round() as u8, (c[2] * 255.0).round() as u8)
}

/// Barwa z listy progów (położenie, r, g, b) w punkcie `x` – jak `mix` po progach w shaderze.
fn ramp(stops: &[[f32; 4]], x: f32) -> Color32 {
    let mut c = [stops[0][1], stops[0][2], stops[0][3]];
    for w in stops.windows(2) {
        let (a, b) = (w[0], w[1]);
        if x >= a[0] {
            let t = ((x - a[0]) / (b[0] - a[0])).clamp(0.0, 1.0);
            c = [a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3] + (b[3] - a[3]) * t];
        }
    }
    rgb(c)
}

/// HSV → RGB tak jak `hsv2rgb` w shaderze.
fn hsv(h: f32, s: f32, v: f32) -> Color32 {
    let k = |o: f32| {
        let p = ((h + o).fract() * 6.0 - 3.0).abs();
        v * (1.0 + ((p - 1.0).clamp(0.0, 1.0) - 1.0) * s)
    };
    rgb([k(0.0), k(2.0 / 3.0), k(1.0 / 3.0)])
}

const HYPSO: [[f32; 4]; 10] = [
    [0.0, 0.40, 0.62, 0.40], [300.0, 0.55, 0.73, 0.45], [600.0, 0.76, 0.82, 0.52],
    [900.0, 0.93, 0.89, 0.60], [1200.0, 0.94, 0.79, 0.52], [1500.0, 0.87, 0.64, 0.41],
    [1800.0, 0.74, 0.49, 0.32], [2100.0, 0.60, 0.40, 0.31], [2400.0, 0.66, 0.60, 0.60],
    [2800.0, 0.97, 0.97, 0.98],
];
const INSOL: [[f32; 4]; 5] = [
    [0.0, 0.16, 0.19, 0.40], [0.25, 0.40, 0.30, 0.60], [0.5, 0.80, 0.38, 0.42],
    [0.75, 0.97, 0.64, 0.25], [1.0, 1.0, 0.94, 0.55],
];
const ISO: [[f32; 4]; 5] = [
    [0.0, 0.20, 0.75, 0.45], [0.25, 0.80, 0.88, 0.30], [0.5, 0.98, 0.62, 0.20],
    [0.75, 0.90, 0.25, 0.22], [1.0, 0.55, 0.20, 0.62],
];
/// Wysokość względna: od −1 (dużo niżej) do +1 (dużo wyżej).
const REL: [[f32; 4]; 5] = [
    [-1.0, 0.04, 0.22, 0.45], [-0.001, 0.82, 0.93, 0.95], [0.0, 0.98, 0.94, 0.80],
    [0.5, 0.97, 0.60, 0.22], [1.0, 0.62, 0.08, 0.10],
];

/// Pasek barw z podpisami pod spodem: (położenie 0..1, tekst).
fn gradient(ui: &mut egui::Ui, color: impl Fn(f32) -> Color32, ticks: &[(f32, String)]) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(W, 12.0), egui::Sense::hover());
    let p = ui.painter();
    let n = 64;
    for i in 0..n {
        let (t0, t1) = (i as f32 / n as f32, (i + 1) as f32 / n as f32);
        let r = egui::Rect::from_x_y_ranges(rect.left() + rect.width() * t0..=rect.left() + rect.width() * t1 + 0.5, rect.y_range());
        p.rect_filled(r, 0.0, color((t0 + t1) / 2.0));
    }
    p.rect_stroke(rect, 3.0, egui::Stroke::new(1.0, Color32::from_black_alpha(90)), egui::StrokeKind::Outside);
    let (lr, _) = ui.allocate_exact_size(egui::vec2(W, 14.0), egui::Sense::hover());
    for (t, s) in ticks {
        let x = lr.left() + lr.width() * t;
        let align = if *t <= 0.01 { egui::Align2::LEFT_TOP } else if *t >= 0.99 { egui::Align2::RIGHT_TOP } else { egui::Align2::CENTER_TOP };
        ui.painter().text(egui::pos2(x, lr.top()), align, s, egui::FontId::proportional(11.5), theme::paper_2());
        ui.painter().line_segment([egui::pos2(x, rect.bottom()), egui::pos2(x, rect.bottom() + 3.0)], egui::Stroke::new(1.0, theme::paper_3()));
    }
}

/// Próbka barwy z podpisem (klasy nachylenia, „poza zasięgiem”).
fn swatch(ui: &mut egui::Ui, c: Color32, text: &str) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 12.0), egui::Sense::hover());
        ui.painter().rect_filled(r, 3.0, c);
        ui.label(RichText::new(text).size(12.0).color(theme::paper_2()));
    });
}

fn title(ui: &mut egui::Ui, glyph: &str, text: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(glyph).size(14.0).color(theme::BRASS));
        ui.label(RichText::new(text).size(13.0).strong());
    });
}

/// Treść legendy; `false` – nic do pokazania.
fn body(a: &Atlas, ui: &mut egui::Ui) -> bool {
    use crate::i18n::tr;
    // soczewka ma pierwszeństwo – to ona zmienia barwy mapy
    match a.lens {
        Lens::Time if a.lens_pin.is_some() => {
            title(ui, theme::icon::TIMER, tr("Czas dojścia"));
            match &a.iso {
                Some(iso) => {
                    let h = iso.hours as f64;
                    gradient(ui, |t| ramp(&ISO, t), &[(0.0, "0".into()), (0.5, fmt::time(h / 2.0)), (1.0, fmt::time(h))]);
                    swatch(ui, Color32::from_rgb(70, 76, 92), tr("poza zasięgiem"));
                    ui.label(RichText::new(tr("linie co 30 min, grubsze co godzinę")).size(11.5).color(theme::paper_3()));
                }
                None => {
                    ui.label(RichText::new(tr("liczę czas marszu…")).size(12.0).color(theme::paper_3()));
                }
            }
            return true;
        }
        Lens::Rel if a.lens_pin.or(a.hover).is_some() => {
            title(ui, theme::icon::PLUS_MINUS, tr("Wysokość względna"));
            let r = a.prefs.rel_range as f64;
            gradient(ui, |t| ramp(&REL, t * 2.0 - 1.0), &[
                (0.0, fmt::signed(-r, "m")),
                (0.5, "0".into()),
                (1.0, fmt::signed(r, "m")),
            ]);
            ui.label(RichText::new(tr("niebieskie niżej, czerwone wyżej; białe pierścienie co 1 km")).size(11.5).color(theme::paper_3()));
            return true;
        }
        _ => {}
    }
    match a.prefs.style {
        Style::Hypso => {
            title(ui, theme::icon::CHART_BAR, tr("Hipsometria"));
            gradient(ui, |t| ramp(&HYPSO, t * 2800.0), &[(0.0, "0".into()), (1000.0 / 2800.0, "1000".into()), (2000.0 / 2800.0, "2000".into()), (1.0, "2800 m".into())]);
        }
        Style::Slope => {
            title(ui, theme::icon::ANGLE, tr("Nachylenie"));
            let classes: [([f32; 3], &str); 7] = [
                ([0.94, 0.93, 0.89], "< 15°"),
                ([0.84, 0.90, 0.72], "15–25°"),
                ([0.99, 0.87, 0.30], "25–30°"),
                ([0.98, 0.56, 0.12], "30–35°"),
                ([0.89, 0.18, 0.15], "35–40°"),
                ([0.63, 0.20, 0.55], "40–45°"),
                ([0.33, 0.18, 0.46], "> 45°"),
            ];
            egui::Grid::new("legenda-nachylenie").num_columns(2).spacing([14.0, 2.0]).show(ui, |ui| {
                for (i, (c, t)) in classes.iter().enumerate() {
                    swatch(ui, rgb(*c), t);
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
            ui.label(RichText::new(tr("lawiny schodzą najczęściej ze stoków 30–45°")).size(11.5).color(theme::paper_3()));
        }
        Style::Aspect => {
            title(ui, theme::icon::COMPASS, tr("Ekspozycja"));
            ui.horizontal(|ui| {
                // tarcza: barwa – kierunek spadku stoku, od środka (płasko) ku brzegowi coraz stromiej
                let (rect, _) = ui.allocate_exact_size(egui::vec2(84.0, 84.0), egui::Sense::hover());
                let p = ui.painter();
                let c = rect.center();
                for ring in 0..5 {
                    let r0 = 8.0 + ring as f32 * 6.0;
                    let sat = (ring as f32 + 1.0) / 5.0 * 0.85;
                    for k in 0..48 {
                        let a0 = k as f32 / 48.0 * std::f32::consts::TAU;
                        let a1 = (k + 1) as f32 / 48.0 * std::f32::consts::TAU;
                        // 0 = północ u góry, zgodnie z zegarem
                        let pt = |a: f32, r: f32| c + egui::vec2(a.sin() * r, -a.cos() * r);
                        let mesh = egui::Shape::convex_polygon(vec![pt(a0, r0), pt(a1, r0), pt(a1, r0 + 6.5), pt(a0, r0 + 6.5)], hsv((k as f32 + 0.5) / 48.0, sat, 0.93), egui::Stroke::NONE);
                        p.add(mesh);
                    }
                }
                p.circle_filled(c, 8.5, hsv(0.0, 0.0, 0.93));
                for (t, a) in [("N", 0.0f32), ("E", 90.0), ("S", 180.0), ("W", 270.0)] {
                    let ar = a.to_radians();
                    p.text(c + egui::vec2(ar.sin() * 23.0, -ar.cos() * 23.0), egui::Align2::CENTER_CENTER, t, egui::FontId::proportional(10.5), Color32::from_rgb(25, 25, 30));
                }
                ui.vertical(|ui| {
                    ui.add_space(8.0);
                    ui.label(RichText::new(tr("barwa – kierunek, w który opada stok")).size(11.5).color(theme::paper_2()));
                    ui.label(RichText::new(tr("nasycenie – nachylenie (szare = płasko)")).size(11.5).color(theme::paper_3()));
                });
            });
        }
        Style::Insolation => {
            title(ui, theme::icon::SUN_DIM, tr("Nasłonecznienie"));
            let (day, _) = a.when();
            let t = sun::times(day, a.cam.lat, a.cam.lon);
            let len = if t.rise.is_finite() && t.set.is_finite() { t.set - t.rise } else if t.max_alt > 0.0 { 24.0 } else { 0.0 };
            gradient(ui, |x| ramp(&INSOL, x), &[(0.0, "0 h".into()), (0.5, fmt::time(len / 2.0)), (1.0, fmt::time(len))]);
            ui.label(RichText::new(tf!("godziny słońca {} · dzień trwa {}", day.label(), fmt::time(len))).size(11.5).color(theme::paper_3()));
        }
        _ => return false,
    }
    true
}

/// Czy jest co pokazać: soczewka zmieniająca barwy albo styl analityczny.
fn active(a: &Atlas) -> bool {
    match a.lens {
        Lens::Time if a.lens_pin.is_some() => return true,
        Lens::Rel if a.lens_pin.or(a.hover).is_some() => return true,
        _ => {}
    }
    matches!(a.prefs.style, Style::Hypso | Style::Slope | Style::Aspect | Style::Insolation)
}

/// Legenda w lewym dolnym rogu mapy (nad paskiem stanu i profilem).
pub fn legend(a: &mut Atlas, ctx: &egui::Context) {
    if !active(a) {
        return;
    }
    let id = egui::Id::new("legenda");
    let mut shown = false;
    egui::Area::new(id)
        .anchor(egui::Align2::LEFT_BOTTOM, [16.0, -16.0 - bottom_inset(a)])
        .interactable(false)
        .show(ctx, |ui| {
            theme::glass_float().inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
                ui.set_width(W);
                ui.spacing_mut().item_spacing.y = 4.0;
                shown = body(a, ui);
            });
        });
    if shown {
        glass_float_area(a, ctx, id, 1.0);
    }
}
