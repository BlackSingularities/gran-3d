//! Wygląd: ciemne, lekko przezroczyste szkło unoszące się nad mapą, z jednym niebieskim akcentem.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Sense, Stroke, TextStyle, Visuals};

pub use egui_phosphor::regular as icon;

use crate::i18n::tr;

pub const INK_0: Color32 = Color32::from_rgb(16, 18, 22);
pub const INK_1: Color32 = Color32::from_rgb(26, 29, 35);
pub const INK_2: Color32 = Color32::from_rgb(34, 38, 45);
pub const INK_3: Color32 = Color32::from_rgb(46, 51, 60);
pub const INK_4: Color32 = Color32::from_rgb(62, 68, 78);
pub const PAPER: Color32 = Color32::from_rgb(236, 239, 244);
pub const PAPER_2: Color32 = Color32::from_rgb(190, 196, 206);
pub const PAPER_3: Color32 = Color32::from_rgb(140, 147, 158);
/// Akcent programu.
pub const BRASS: Color32 = Color32::from_rgb(122, 176, 255);
pub const BRASS_DIM: Color32 = Color32::from_rgb(37, 60, 96);
pub const SIGNAL: Color32 = Color32::from_rgb(255, 98, 82);
pub const TEAL: Color32 = Color32::from_rgb(63, 214, 198);
pub const OK: Color32 = Color32::from_rgb(110, 214, 140);
pub const WARN: Color32 = Color32::from_rgb(247, 201, 72);

const ON_ACCENT: Color32 = Color32::from_rgb(8, 20, 38);

/// Ikona z podpisem na przycisku.
pub fn ic(glyph: &str, text: &str) -> String {
    format!("{glyph}  {}", tr(text))
}

pub fn apply(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    // obce pisma w nazwach z OSM (dewanagari, tybetańskie, chińskie, japońskie, koreańskie, arabskie…):
    // zapasowe czcionki systemowe Windows, o ile są
    let win = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    for file in ["Nirmala.ttf", "himalaya.ttf", "msyh.ttc", "YuGothR.ttc", "malgun.ttf", "segoeui.ttf", "seguisym.ttf", "ebrima.ttf", "taile.ttf", "mmrtext.ttf"] {
        let Ok(data) = std::fs::read(std::path::Path::new(&win).join("Fonts").join(file)) else { continue };
        fonts.font_data.insert(file.into(), std::sync::Arc::new(egui::FontData::from_owned(data)));
        for fam in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts.families.entry(fam).or_default().push(file.into());
        }
    }
    ctx.set_fonts(fonts);
    let mut v = Visuals::dark();
    v.panel_fill = INK_1;
    v.window_fill = Color32::from_rgba_premultiplied(18, 20, 25, 232);
    v.extreme_bg_color = INK_0;
    v.faint_bg_color = INK_2;
    v.override_text_color = Some(PAPER);
    v.hyperlink_color = BRASS;
    v.selection.bg_fill = BRASS_DIM;
    v.selection.stroke = Stroke::new(1.0_f32, BRASS);
    v.window_corner_radius = CornerRadius::same(16);
    v.menu_corner_radius = CornerRadius::same(12);
    v.window_stroke = Stroke::new(1.0_f32, GLASS_EDGE);
    v.window_shadow = egui::Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(110) };
    v.popup_shadow = egui::Shadow { offset: [0, 6], blur: 22, spread: 0, color: Color32::from_black_alpha(100) };
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, INK_3);
    v.widgets.noninteractive.bg_fill = INK_1;
    v.widgets.inactive.bg_fill = INK_3;
    v.widgets.inactive.weak_bg_fill = Color32::from_white_alpha(10);
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.hovered.bg_fill = INK_4;
    v.widgets.hovered.weak_bg_fill = Color32::from_white_alpha(20);
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.active.bg_fill = BRASS_DIM;
    v.widgets.active.weak_bg_fill = BRASS_DIM;
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(10);
    }
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        // tekst nie zawija się w wiersze interfejsu; akapity zawija jawnie `para`
        s.wrap_mode = Some(egui::TextWrapMode::Truncate);
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(12.0, 7.0);
        s.spacing.slider_width = 170.0;
        s.text_styles.insert(
            TextStyle::Heading,
            FontId::new(21.0, FontFamily::Proportional),
        );
        s.text_styles
            .insert(TextStyle::Body, FontId::new(14.0, FontFamily::Proportional));
        s.text_styles.insert(
            TextStyle::Button,
            FontId::new(14.0, FontFamily::Proportional),
        );
        s.text_styles.insert(
            TextStyle::Small,
            FontId::new(12.0, FontFamily::Proportional),
        );
        s.text_styles.insert(
            TextStyle::Monospace,
            FontId::new(12.0, FontFamily::Monospace),
        );
    });
}

/// Mały nagłówek sekcji.
pub fn kicker(text: &str) -> egui::RichText {
    egui::RichText::new(tr(text).to_string()).size(12.0).color(PAPER_3).strong()
}

/// Główny przycisk akcji.
pub fn primary(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(tr(text).to_string()).color(ON_ACCENT).strong())
        .fill(BRASS)
        .min_size(egui::vec2(110.0, 34.0))
}

/// Karta na liście.
pub fn card(selected: bool) -> egui::Frame {
    egui::Frame::new()
        .fill(if selected { BRASS_DIM } else { INK_2 })
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(12))
}

/// Szkło: lekko przezroczyste tło powierzchni nad mapą.
pub const GLASS_FILL: Color32 = Color32::from_rgba_premultiplied(17, 19, 24, 170);
/// Delikatna jasna obwódka szkła.
pub const GLASS_EDGE: Color32 = Color32::from_rgba_premultiplied(30, 30, 30, 30);

/// Powierzchnia unosząca się nad mapą: półprzezroczyste szkło z jasną krawędzią i miękkim cieniem.
pub fn glass() -> egui::Frame {
    egui::Frame::new()
        .fill(GLASS_FILL)
        .stroke(Stroke::new(1.0_f32, GLASS_EDGE))
        .corner_radius(CornerRadius::same(18))
        .inner_margin(egui::Margin::same(6))
        .shadow(egui::Shadow { offset: [0, 6], blur: 24, spread: 0, color: Color32::from_black_alpha(90) })
}

/// Kafelek z ikoną i podpisem (warstwy, style, pory dnia).
pub fn tile(ui: &mut egui::Ui, glyph: &str, text: &str, on: bool, w: f32) -> egui::Response {
    tile_ex(ui, glyph, text, on, w, None)
}

/// Kafelek, który może być nieaktywny (`disabled` – powód widoczny po najechaniu): szary, bez kliknięcia.
pub fn tile_ex(ui: &mut egui::Ui, glyph: &str, text: &str, on: bool, w: f32, disabled: Option<&str>) -> egui::Response {
    let text = tr(text);
    let disabled = disabled.map(tr);
    let sense = if disabled.is_some() { Sense::hover() } else { Sense::click() };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 74.0), sense);
    if let Some(why) = disabled {
        let p = ui.painter();
        p.rect_filled(rect, 14.0, Color32::from_white_alpha(3));
        p.rect_stroke(rect, 14.0, Stroke::new(1.0_f32, Color32::from_white_alpha(8)), egui::StrokeKind::Inside);
        let gray = Color32::from_gray(88);
        p.text(rect.center_top() + egui::vec2(0.0, 26.0), egui::Align2::CENTER_CENTER, glyph, FontId::proportional(24.0), gray);
        p.text(egui::pos2(rect.center().x, rect.bottom() - 17.0), egui::Align2::CENTER_CENTER, text, FontId::proportional(12.0), gray);
        return resp.on_hover_text(format!("{text}: {why}"));
    }
    let p = ui.painter();
    let bg = if on {
        Color32::from_rgba_unmultiplied(122, 176, 255, 46)
    } else if resp.hovered() {
        Color32::from_white_alpha(16)
    } else {
        Color32::from_white_alpha(7)
    };
    p.rect_filled(rect, 14.0, bg);
    let edge = if on { Stroke::new(1.5_f32, BRASS) } else { Stroke::new(1.0_f32, Color32::from_white_alpha(14)) };
    p.rect_stroke(rect, 14.0, edge, egui::StrokeKind::Inside);
    let color = if on { BRASS } else { PAPER };
    p.text(rect.center_top() + egui::vec2(0.0, 26.0), egui::Align2::CENTER_CENTER, glyph, FontId::proportional(24.0), color);
    let galley = p.layout_no_wrap(text.to_string(), FontId::proportional(12.0), if on { PAPER } else { PAPER_2 });
    let gw = galley.size().x.min(w - 8.0);
    let clip = rect.shrink2(egui::vec2(4.0, 0.0));
    ui.painter_at(clip).galley(egui::pos2(rect.center().x - gw / 2.0, rect.bottom() - 24.0), galley, PAPER);
    resp.on_hover_text(text)
}

/// Siatka kafelków o równej szerokości.
pub fn tile_grid(ui: &mut egui::Ui, cols: usize, body: impl FnOnce(&mut egui::Ui, f32)) {
    let gap = 8.0;
    let w = ((ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32).floor();
    ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
    ui.horizontal_wrapped(|ui| {
        ui.set_max_width(w * cols as f32 + gap * (cols as f32 - 1.0) + 1.0);
        body(ui, w);
    });
}

/// Karta wyboru: tytuł i opis, zaznaczona obwódką dookoła.
pub fn option_card(ui: &mut egui::Ui, title: &str, desc: &str, on: bool) -> egui::Response {
    let (title, desc) = (tr(title), tr(desc));
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 52.0), Sense::click());
    let p = ui.painter();
    let bg = if on {
        Color32::from_rgba_unmultiplied(122, 176, 255, 40)
    } else if resp.hovered() {
        Color32::from_white_alpha(14)
    } else {
        Color32::from_white_alpha(6)
    };
    p.rect_filled(rect, 12.0, bg);
    if on {
        p.rect_stroke(rect, 12.0, Stroke::new(1.5_f32, BRASS), egui::StrokeKind::Inside);
    }
    let dot = rect.left_center() + egui::vec2(18.0, 0.0);
    p.circle_stroke(dot, 7.0, Stroke::new(1.5_f32, if on { BRASS } else { PAPER_3 }));
    if on {
        p.circle_filled(dot, 4.0, BRASS);
    }
    p.text(rect.left_top() + egui::vec2(36.0, 9.0), egui::Align2::LEFT_TOP, title, FontId::proportional(14.0), PAPER);
    p.text(rect.left_bottom() + egui::vec2(36.0, -9.0), egui::Align2::LEFT_BOTTOM, desc, FontId::proportional(11.5), PAPER_3);
    resp
}

/// Pasek postępu.
pub fn bar(ui: &mut egui::Ui, frac: f32, color: Color32) {
    let (r, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 5.0), egui::Sense::hover());
    ui.painter().rect_filled(r, 3.0, INK_4);
    let mut f = r;
    f.set_width(r.width() * frac.clamp(0.0, 1.0));
    ui.painter().rect_filled(f, 3.0, color);
}

/// Akapit objaśnienia: jedyne miejsce, gdzie tekst się zawija.
pub fn para(ui: &mut egui::Ui, text: impl Into<String>) {
    let text: String = text.into();
    ui.add(egui::Label::new(egui::RichText::new(tr(&text).to_string()).small().color(PAPER_3)).wrap());
}

/// Okrągły przycisk z samą ikoną.
pub fn icon_button(ui: &mut egui::Ui, glyph: &str, on: bool) -> egui::Response {
    let text = egui::RichText::new(glyph).size(20.0).color(if on { BRASS } else { PAPER });
    ui.add(
        egui::Button::new(text)
            .min_size(egui::vec2(42.0, 42.0))
            .fill(if on { BRASS_DIM } else { Color32::TRANSPARENT })
            .stroke(Stroke::NONE),
    )
}

/// Wiersz listy na całą szerokość: ikona i podpis.
pub fn row_button(ui: &mut egui::Ui, glyph: &str, text: &str, on: bool) -> egui::Response {
    let text = tr(text);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), Sense::click());
    let bg = if on {
        BRASS_DIM
    } else if resp.hovered() {
        Color32::from_white_alpha(16)
    } else {
        Color32::TRANSPARENT
    };
    let p = ui.painter();
    p.rect_filled(rect, 10.0, bg);
    let color = if on { BRASS } else { PAPER };
    p.text(
        rect.left_center() + egui::vec2(12.0, 0.0),
        egui::Align2::LEFT_CENTER,
        glyph,
        FontId::proportional(18.0),
        color,
    );
    p.text(
        rect.left_center() + egui::vec2(42.0, 0.0),
        egui::Align2::LEFT_CENTER,
        text,
        FontId::proportional(14.0),
        color,
    );
    resp
}

/// Przełącznik w wierszu: podpis po lewej, suwak po prawej.
pub fn switch(ui: &mut egui::Ui, value: &mut bool, text: &str) -> egui::Response {
    let text = tr(text);
    let (rect, mut resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), Sense::click());
    if resp.clicked() {
        *value = !*value;
        resp.mark_changed();
    }
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, 8.0, Color32::from_white_alpha(14));
    }
    p.text(
        rect.left_center() + egui::vec2(8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        text,
        FontId::proportional(14.0),
        PAPER,
    );
    let track = egui::Rect::from_center_size(rect.right_center() - egui::vec2(26.0, 0.0), egui::vec2(34.0, 18.0));
    p.rect_filled(track, 9.0, if *value { BRASS } else { INK_4 });
    let x = if *value { track.right() - 9.0 } else { track.left() + 9.0 };
    p.circle_filled(egui::pos2(x, track.center().y), 7.0, if *value { ON_ACCENT } else { PAPER_2 });
    resp
}
