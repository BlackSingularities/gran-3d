//! Panel boczny z zakładkami i kontrolki nawigacji.

use super::*;

/// Szerokość prawego panelu szczegółów.
pub(super) const SIDEBAR_W: f32 = 440.0;

pub(super) fn sidebar_open(a: &Atlas) -> bool {
    a.prefs.panel_open
}

/// Odsunięcie elementów przy prawej krawędzi, gdy panel szczegółów jest otwarty.
pub fn right_inset(a: &Atlas) -> f32 {
    if sidebar_open(a) { SIDEBAR_W } else { 0.0 }
}

/// Szczegóły w prawym panelu bocznym (zwęża widok mapy); wywoływać przed panelem środkowym.
pub fn sidebar(a: &mut Atlas, ctx: &egui::Context) {
    if !sidebar_open(a) {
        // schowany panel: uchwyt przy prawej krawędzi
        let alpha = controls_alpha(a, ctx);
        egui::Area::new(egui::Id::new("szczegoly-uchwyt"))
            .anchor(egui::Align2::RIGHT_CENTER, [-6.0, 0.0])
            .interactable(alpha > 0.05)
            .show(ctx, |ui| {
                ui.multiply_opacity(alpha);
                theme::glass().inner_margin(egui::Margin::symmetric(4, 10)).show(ui, |ui| {
                    if ui.small_button(theme::icon::CARET_LEFT).on_hover_text(crate::i18n::tr("Pokaż szczegóły (P)")).clicked() {
                        a.prefs.panel_open = true;
                        a.tab = Tab::Info;
                    }
                });
            });
        glass_area(a, ctx, egui::Id::new("szczegoly-uchwyt"), alpha);
        return;
    }
    // panel boczny przy prawej krawędzi, ze szkła: od górnego paska do dolnego (profil, pasek stanu)
    let screen = ctx.screen_rect();
    let height = (screen.height() - TOP_BAR_H - bottom_inset(a)).max(200.0);
    let alpha = controls_alpha(a, ctx);
    a.glass.push((egui::Rect::from_min_size(egui::pos2(screen.right() - SIDEBAR_W, screen.top() + TOP_BAR_H), egui::vec2(SIDEBAR_W, height)), alpha));
    egui::Area::new(egui::Id::new("szczegoly"))
        .fixed_pos([screen.right() - SIDEBAR_W, screen.top() + TOP_BAR_H])
        .order(egui::Order::Middle)
        .interactable(alpha > 0.05)
        .show(ctx, |ui| {
          ui.multiply_opacity(alpha);
          egui::Frame::new().fill(theme::GLASS_FILL).inner_margin(egui::Margin::symmetric(18, 16)).show(ui, |ui| {
            ui.set_width(SIDEBAR_W - 36.0);
            ui.set_height(height - 32.0);
            // większe pismo niż w szufladzie
            let st = ui.style_mut();
            st.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
            st.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
            st.text_styles.insert(egui::TextStyle::Heading, egui::FontId::proportional(27.0));
            st.text_styles.insert(egui::TextStyle::Monospace, egui::FontId::monospace(14.0));
            st.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(12.5));
            st.spacing.item_spacing.y = 7.0;
            ui.horizontal(|ui| {
                let (glyph, title) = match (a.tab, a.tool) {
                    (Tab::Library, _) => (theme::icon::BOOKMARKS, crate::i18n::tr("Biblioteka")),
                    (Tab::Conditions, _) => (theme::icon::CLOUD_SUN, crate::i18n::tr("Warunki")),
                    (_, Tool::Route) => (theme::icon::PATH, crate::i18n::tr("Trasa")),
                    (_, Tool::Measure) => (theme::icon::RULER, crate::i18n::tr("Pomiar")),
                    (_, Tool::Explore) => (theme::icon::COMPASS, crate::i18n::tr("Szczegóły")),
                };
                ui.label(RichText::new(glyph).size(19.0).color(theme::BRASS));
                ui.label(RichText::new(title).size(17.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(theme::icon::CARET_RIGHT).on_hover_text(crate::i18n::tr("Ukryj panel (P)")).clicked() {
                        a.prefs.panel_open = false;
                    }
                    if a.tab == Tab::Info && a.tool == Tool::Route && ui.button(theme::ic(theme::icon::X, "Zakończ planowanie")).on_hover_text(crate::i18n::tr("Usuwa trasę z mapy i wraca do zwykłego trybu")).clicked() {
                        a.end_planning();
                    }
                });
            });
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                // nic nie rozpycha panelu poza jego szerokość
                ui.set_max_width(ui.available_width());
                if a.tab == Tab::Library {
                    crate::ui::library::show(a, ui);
                } else if a.tab == Tab::Conditions {
                    crate::ui::conditions::show(a, ui);
                } else {
                    info(a, ui);
                }
            });
          });
        });
}

/// Sterowanie kamerą w prawym dolnym rogu.
pub fn nav(a: &mut Atlas, ctx: &egui::Context) {
    let alpha = controls_alpha(a, ctx);
    // nad profilem trasy, gdy jest otwarty
    egui::Area::new(egui::Id::new("kamera"))
        .anchor(egui::Align2::RIGHT_BOTTOM, [-16.0 - right_inset(a), -16.0 - bottom_inset(a)])
        .interactable(alpha > 0.05)
        .show(ctx, |ui| {
            ui.multiply_opacity(alpha);
            theme::glass().show(ui, |ui| {
                ui.vertical(|ui| {
                    let zoom = |a: &mut Atlas, k: f64| {
                        let mut to = a.cam;
                        to.distance = (a.cam.distance * k).clamp(a.min_dist(), 900_000.0);
                        a.start_fly(to, 0.35);
                    };
                    if theme::icon_button(ui, theme::icon::PLUS, false).on_hover_text(crate::i18n::tr("Przybliż")).clicked() {
                        zoom(a, 0.55);
                    }
                    if theme::icon_button(ui, theme::icon::MINUS, false).on_hover_text(crate::i18n::tr("Oddal")).clicked() {
                        zoom(a, 1.8);
                    }
                    ui.separator();
                    let full = ui.ctx().input(|i| i.viewport().fullscreen.unwrap_or(false));
                    if theme::icon_button(ui, if full { theme::icon::CORNERS_IN } else { theme::icon::CORNERS_OUT }, full).on_hover_text(crate::i18n::tr("Pełny ekran (F11)")).clicked() {
                        let ctx = ui.ctx().clone();
                        a.toggle_fullscreen(&ctx);
                    }
                    if theme::icon_button(ui, theme::icon::PERSPECTIVE, a.cam.pitch < 75.0).on_hover_text(crate::i18n::tr("Widok ukośny / z góry (V)")).clicked() {
                        let mut to = a.cam;
                        to.pitch = if a.cam.pitch > 75.0 { 40.0 } else { 89.0 };
                        a.start_fly(to, 0.7);
                    }
                });
            });
        });
    glass_area(a, ctx, egui::Id::new("kamera"), alpha);
}

pub(super) fn info(a: &mut Atlas, ui: &mut egui::Ui) {
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
