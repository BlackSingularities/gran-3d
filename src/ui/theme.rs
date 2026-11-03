//! Wygląd: te same barwy co w wersji 1.x (atrament, papier, mosiądz, sygnał).

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals};

pub const INK_0: Color32 = Color32::from_rgb(7, 10, 12);
pub const INK_1: Color32 = Color32::from_rgb(12, 17, 20);
pub const INK_2: Color32 = Color32::from_rgb(18, 26, 31);
pub const INK_3: Color32 = Color32::from_rgb(27, 37, 43);
pub const INK_4: Color32 = Color32::from_rgb(38, 50, 58);
pub const PAPER: Color32 = Color32::from_rgb(239, 231, 214);
pub const PAPER_2: Color32 = Color32::from_rgb(207, 198, 178);
pub const PAPER_3: Color32 = Color32::from_rgb(151, 143, 126);
pub const BRASS: Color32 = Color32::from_rgb(217, 181, 111);
pub const BRASS_DIM: Color32 = Color32::from_rgb(86, 72, 46);
pub const SIGNAL: Color32 = Color32::from_rgb(255, 90, 54);
pub const TEAL: Color32 = Color32::from_rgb(63, 214, 198);
pub const OK: Color32 = Color32::from_rgb(134, 224, 127);
pub const WARN: Color32 = Color32::from_rgb(247, 201, 72);

pub use egui_phosphor::regular as icon;

/// Ikona z podpisem na przycisku.
pub fn ic(glyph: &str, text: &str) -> String {
    format!("{glyph}  {text}")
}

pub fn apply(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);
    let mut v = Visuals::dark();
    v.panel_fill = INK_1;
    v.window_fill = INK_1;
    v.extreme_bg_color = INK_0;
    v.faint_bg_color = INK_2;
    v.override_text_color = Some(PAPER);
    v.hyperlink_color = BRASS;
    v.selection.bg_fill = BRASS_DIM;
    v.selection.stroke = Stroke::new(1.0_f32, BRASS);
    v.window_corner_radius = CornerRadius::same(14);
    v.menu_corner_radius = CornerRadius::same(10);
    v.window_stroke = Stroke::new(1.0_f32, INK_4);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, INK_3);
    v.widgets.noninteractive.bg_fill = INK_1;
    v.widgets.inactive.bg_fill = INK_2;
    v.widgets.inactive.weak_bg_fill = INK_2;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, INK_4);
    v.widgets.hovered.bg_fill = INK_3;
    v.widgets.hovered.weak_bg_fill = INK_3;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, PAPER_3);
    v.widgets.active.bg_fill = BRASS_DIM;
    v.widgets.active.weak_bg_fill = BRASS_DIM;
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(8);
    }
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 7.0);
        s.spacing.button_padding = egui::vec2(12.0, 6.0);
        s.spacing.slider_width = 260.0;
        s.text_styles.insert(
            TextStyle::Heading,
            FontId::new(26.0, FontFamily::Proportional),
        );
        s.text_styles
            .insert(TextStyle::Body, FontId::new(14.0, FontFamily::Proportional));
        s.text_styles.insert(
            TextStyle::Button,
            FontId::new(14.0, FontFamily::Proportional),
        );
        s.text_styles.insert(
            TextStyle::Small,
            FontId::new(11.5, FontFamily::Proportional),
        );
        s.text_styles.insert(
            TextStyle::Monospace,
            FontId::new(12.0, FontFamily::Monospace),
        );
    });
}

/// Mały nagłówek sekcji wersalikami (jak „kicker” w wersji 1.x).
pub fn kicker(text: &str) -> egui::RichText {
    egui::RichText::new(text.to_uppercase())
        .size(10.5)
        .color(PAPER_3)
        .extra_letter_spacing(1.4)
}

/// Główny przycisk akcji (mosiężny).
pub fn primary(text: &str) -> egui::Button<'static> {
    egui::Button::new(
        egui::RichText::new(text.to_string())
            .color(Color32::from_rgb(27, 20, 8))
            .strong(),
    )
    .fill(BRASS)
    .min_size(egui::vec2(120.0, 34.0))
}

/// Karta z ramką dookoła.
pub fn card(selected: bool) -> egui::Frame {
    egui::Frame::new()
        .fill(if selected {
            Color32::from_rgb(30, 31, 27)
        } else {
            INK_2
        })
        .stroke(Stroke::new(1.0_f32, if selected { BRASS } else { INK_3 }))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(12))
}

/// Pasek postępu w barwach programu.
pub fn bar(ui: &mut egui::Ui, frac: f32, color: Color32) {
    let (r, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 5.0), egui::Sense::hover());
    ui.painter().rect_filled(r, 3.0, INK_4);
    let mut f = r;
    f.set_width(r.width() * frac.clamp(0.0, 1.0));
    ui.painter().rect_filled(f, 3.0, color);
}
