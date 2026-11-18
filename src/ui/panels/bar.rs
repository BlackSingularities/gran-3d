//! Górny pasek: wyszukiwarka, narzędzia, soczewki, pora dnia, styl i warstwy.

use super::*;

/// Podpowiedź przycisku: nazwa polecenia i jego skrót.
pub fn hint(act: Action) -> String {
    let (_, label, _) = act.info();
    match act.keys() {
        Some(k) => format!("{label} ({})", k.text()),
        None => label.to_string(),
    }
}

/// Szkło pod obszarem (okno, panel, menu) z poprzedniej klatki: obraz pod nim jest rozmyty.
pub fn glass_area(a: &mut Atlas, ctx: &egui::Context, id: egui::Id, alpha: f32) {
    if let Some(r) = ctx.memory(|m| m.area_rect(id)) {
        a.glass.push((r, alpha));
    }
}

/// Rozmycie pod małą zaokrągloną kontrolką (`theme::glass_float`): prostokąt lekko zmniejszony,
/// żeby rozmyty obraz nie wystawał poza zaokrąglone rogi.
pub fn glass_float_area(a: &mut Atlas, ctx: &egui::Context, id: egui::Id, alpha: f32) {
    if let Some(r) = ctx.memory(|m| m.area_rect(id)) {
        a.glass.push((r.shrink(3.0), alpha));
    }
}

/// Widoczność kontrolek na mapie (autoukrywanie): 1 – widoczne, 0 – schowane.
pub fn controls_alpha(a: &Atlas, ctx: &egui::Context) -> f32 {
    if !a.prefs.autohide || crate::platform::MOBILE || a.search.is_some() || egui::Popup::is_any_open(ctx) {
        return 1.0;
    }
    let screen = ctx.screen_rect();
    let (idle, top) = ctx.input(|i| {
        let over = i.pointer.hover_pos().map_or(false, |p| {
            // nad paskiem u góry, nad panelem bocznym albo nad profilem – nic się nie chowa
            p.y < 80.0 || p.x > screen.right() - right_inset(a) || p.y > screen.bottom() - bottom_inset(a)
        });
        (i.pointer.time_since_last_movement(), over)
    });
    if top {
        return 1.0;
    }
    const HOLD: f32 = 2.2;
    const FADE: f32 = 0.35;
    if idle < HOLD {
        ctx.request_repaint_after(std::time::Duration::from_secs_f32(HOLD - idle + 0.01));
        return 1.0;
    }
    let t = ((idle - HOLD) / FADE).min(1.0);
    if t < 1.0 {
        ctx.request_repaint();
    }
    1.0 - t
}

/// Górny pasek na całą szerokość okna, ze szkła nad mapą: od lewej narzędzia (zaczyna je przycisk
/// zaznaczania), wyszukiwarka na środku, po prawej pora dnia, styl mapy, warstwy i `trailing`.
pub fn top_bar(a: &mut Atlas, ctx: &egui::Context, trailing: impl FnOnce(&mut Atlas, &mut egui::Ui)) {
    // zegar przy porze „teraz” ma iść także przy nieruchomym widoku
    ctx.request_repaint_after(std::time::Duration::from_secs(20));
    let alpha = controls_alpha(a, ctx);
    let screen = ctx.screen_rect();
    // pod paskiem stanu systemu (Android), który dostaje własne ciemne tło
    let top = screen.top() + a.safe_top;
    if a.safe_top > 0.0 {
        let strip = egui::Rect::from_min_max(screen.min, egui::pos2(screen.right(), top));
        ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::new("pasek-systemu"))).rect_filled(strip, 0.0, theme::INK_0);
    }
    egui::Area::new(egui::Id::new("gorny-pasek"))
        .fixed_pos(egui::pos2(screen.left(), top))
        .order(egui::Order::Middle)
        .interactable(alpha > 0.05)
        .show(ctx, |ui| {
            ui.multiply_opacity(alpha);
            let bar = egui::Rect::from_min_size(egui::pos2(screen.left(), top), egui::vec2(screen.width(), TOP_BAR_H));
            let p = ui.painter();
            p.rect_filled(bar, 0.0, theme::glass_fill());
            p.line_segment([bar.left_bottom(), bar.right_bottom()], egui::Stroke::new(1.0, theme::glass_edge()));
            ui.allocate_rect(bar, egui::Sense::hover());
            a.glass.push((bar, alpha));
            let inner = bar.shrink2(egui::vec2(12.0, 7.0));
            let row = |ui: &mut egui::Ui, rect: egui::Rect, layout: egui::Layout, body: &mut dyn FnMut(&mut egui::Ui)| {
                ui.scope_builder(egui::UiBuilder::new().max_rect(rect).layout(layout), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    body(ui);
                });
            };
            if a.compact {
                let mut trailing = Some(trailing);
                row(ui, inner, egui::Layout::right_to_left(egui::Align::Center), &mut |ui| {
                    if let Some(t) = trailing.take() {
                        t(a, ui);
                    }
                    layers_button(a, ui);
                    style_button(a, ui);
                    sun_button(a, ui);
                    if a.search.is_none() {
                        let w = ui.available_width() - 4.0;
                        search_field_w(a, ui, w);
                    }
                });
                return;
            }
            row(ui, inner, egui::Layout::left_to_right(egui::Align::Center), &mut |ui| tools(a, ui));
            if a.search.is_none() {
                let w = (screen.width() * 0.28).clamp(220.0, 448.0);
                let r = egui::Rect::from_center_size(inner.center(), egui::vec2(w, inner.height()));
                row(ui, r, egui::Layout::left_to_right(egui::Align::Center), &mut |ui| search_field(a, ui));
            }
            let mut trailing = Some(trailing);
            row(ui, inner, egui::Layout::right_to_left(egui::Align::Center), &mut |ui| {
                if let Some(t) = trailing.take() {
                    t(a, ui);
                }
                ui.separator();
                layers_button(a, ui);
                style_button(a, ui);
                sun_button(a, ui);
            });
        });
}

/// Wysokość górnego paska.
pub const TOP_BAR_H: f32 = 56.0;

/// Wysokość paska stanu na dole mapy.
pub const STATUS_H: f32 = 24.0;

/// Wysokość dolnego paska narzędzi na wąskim ekranie.
pub const TOOLBAR_H: f32 = 60.0;

/// Dolny pasek narzędzi (tylko wąski ekran).
pub fn toolbar_h(a: &Atlas) -> f32 {
    if a.compact { TOOLBAR_H } else { 0.0 }
}

/// Arkusz szczegółów otwarty od dołu (wąski ekran).
pub fn sheet_open(a: &Atlas) -> bool {
    a.compact && a.prefs.panel_open
}

/// Profil trasy rozwinięty (na telefonie zwinięty, gdy otwarty jest arkusz).
pub fn profile_expanded(a: &Atlas) -> bool {
    a.prefs.profile_open && !sheet_open(a)
}

/// Odsunięcie od dołu okna: pasek stanu, pasek narzędzi i arkusz (telefon) oraz profil trasy.
pub fn bottom_inset(a: &Atlas) -> f32 {
    let sheet = if sheet_open(a) { a.sheet_h } else { 0.0 };
    a.safe_bottom + STATUS_H + toolbar_h(a) + sheet + match (a.profile_route().is_some(), profile_expanded(a)) {
        (false, _) => 0.0,
        (true, true) => PROFILE_H + 20.0,
        (true, false) => PROFILE_BAR_H + 20.0,
    }
}

/// Dolny pasek narzędzi na wąskim ekranie: te same przyciski co na górnym pasku komputera,
/// przewijane palcem w poziomie.
pub fn bottom_toolbar(a: &mut Atlas, ctx: &egui::Context) {
    if !a.compact {
        return;
    }
    let screen = ctx.screen_rect();
    // na samym dole, nad paskiem nawigacji systemu (pasek stanu programu leży nad narzędziami)
    let bar = egui::Rect::from_min_size(egui::pos2(screen.left(), screen.bottom() - a.safe_bottom - TOOLBAR_H), egui::vec2(screen.width(), TOOLBAR_H));
    a.glass.push((bar, 1.0));
    egui::Area::new(egui::Id::new("dolny-pasek"))
        .fixed_pos(bar.min)
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            let p = ui.painter();
            p.rect_filled(bar, 0.0, theme::glass_fill());
            if a.safe_bottom > 0.0 {
                p.rect_filled(egui::Rect::from_min_max(bar.left_bottom(), screen.right_bottom()), 0.0, theme::INK_0);
            }
            p.line_segment([bar.left_top(), bar.right_top()], egui::Stroke::new(1.0, theme::glass_edge()));
            ui.scope_builder(egui::UiBuilder::new().max_rect(bar.shrink2(egui::vec2(4.0, 6.0))), |ui| {
                egui::ScrollArea::horizontal().id_salt("narzedzia-dol").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        compact_tools(a, ui);
                    });
                });
            });
        });
}

/// Pole wyszukiwarki; po kliknięciu okno wyników rozwija się w tym samym miejscu.
pub(super) fn search_field(a: &mut Atlas, ui: &mut egui::Ui) {
    let w = (ui.ctx().screen_rect().width() * 0.28).clamp(220.0, 448.0);
    search_field_w(a, ui, w);
}

fn search_field_w(a: &mut Atlas, ui: &mut egui::Ui, w: f32) {
    let w = w.max(44.0);
    ui.allocate_ui(egui::vec2(w, 42.0), |ui| {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 42.0), egui::Sense::click());
        theme::focus_ring(ui, &resp, rect, 12.0);
        let p = ui.painter();
        p.rect_filled(rect, 12.0, if resp.hovered() { Color32::from_white_alpha(18) } else { Color32::from_white_alpha(8) });
        p.text(rect.left_center() + egui::vec2(16.0, 0.0), egui::Align2::LEFT_CENTER, theme::icon::MAGNIFYING_GLASS, egui::FontId::proportional(18.0), theme::paper_2());
        let hint = if a.compact { crate::i18n::tr("Szukaj") } else { crate::i18n::tr("Szukaj szczytu, szlaku, miejscowości") };
        if w > 90.0 {
            p.with_clip_rect(rect.shrink(4.0)).text(rect.left_center() + egui::vec2(44.0, 0.0), egui::Align2::LEFT_CENTER, hint, egui::FontId::proportional(14.0), theme::paper_3());
        }
        if !crate::platform::MOBILE {
            p.text(rect.right_center() - egui::vec2(14.0, 0.0), egui::Align2::RIGHT_CENTER, "/", egui::FontId::monospace(12.0), theme::paper_3());
        }
        if resp.clicked() {
            a.search = Some(Default::default());
        }
    });
}

/// Zwykły tryb: klik zaznacza, bez narzędzia, soczewki i panoramy.
pub(super) fn select_mode(a: &mut Atlas, ctx: &egui::Context) {
    a.run(Action::Tool(Tool::Explore), ctx);
}

pub(super) fn tools(a: &mut Atlas, ui: &mut egui::Ui) {
    let plain = a.tool == Tool::Explore && a.lens == Lens::None && a.panorama.is_none();
    let ctx = ui.ctx().clone();
    if theme::icon_button(ui, theme::icon::CURSOR, plain).on_hover_text(hint(Action::Tool(Tool::Explore))).clicked() {
        select_mode(a, &ctx);
    }
    if theme::icon_button(ui, theme::icon::COMPASS, a.prefs.panel_open && a.tab == Tab::Info).on_hover_text(crate::i18n::tr("Szczegóły okolicy (P)")).clicked() {
        a.prefs.panel_open = !(a.prefs.panel_open && a.tab == Tab::Info);
        a.tab = Tab::Info;
    }
    if theme::icon_button(ui, theme::icon::BOOKMARKS, a.prefs.panel_open && a.tab == Tab::Library).on_hover_text(crate::i18n::tr("Biblioteka: zapisane trasy, miejsca i widoki (B)")).clicked() {
        a.prefs.panel_open = !(a.prefs.panel_open && a.tab == Tab::Library);
        a.tab = Tab::Library;
    }
    if theme::icon_button(ui, theme::icon::CLOUD_SUN, a.prefs.panel_open && a.tab == Tab::Conditions).on_hover_text(crate::i18n::tr("Warunki: pogoda i zagrożenie lawinowe (K)")).clicked() {
        a.prefs.panel_open = !(a.prefs.panel_open && a.tab == Tab::Conditions);
        a.tab = Tab::Conditions;
    }
    ui.separator();
    // narzędzia inne niż zwykły tryb – z interfejsu MapTool
    for t in Tool::ALL.into_iter().filter(|t| *t != Tool::Explore) {
        if theme::icon_button(ui, t.behavior().icon(), a.tool == t).on_hover_text(hint(Action::Tool(t))).clicked() {
            a.run(Action::Tool(t), &ctx);
        }
    }
    ui.separator();
    for l in Lens::ALL {
        if theme::icon_button(ui, l.icon(), a.lens == l).on_hover_text(hint(Action::Lens(l))).clicked() {
            a.set_lens(l);
        }
    }
    ui.separator();
    if theme::icon_button(ui, theme::icon::PERSON_SIMPLE_WALK, a.panorama.is_some()).on_hover_text(crate::i18n::tr("Spacer ze środka widoku: W A S D, spacja – skok, prawy przycisk – rozglądanie, Esc – wyjście")).clicked() {
        if a.panorama.is_some() {
            a.exit_panorama();
        } else {
            let (lon, lat) = (a.cam.lon, a.cam.lat);
            a.enter_panorama(lon, lat);
        }
    }
}

/// Narzędzia dolnego paska na telefonie: te same co na komputerze, z krótkimi podpisami.
fn compact_tools(a: &mut Atlas, ui: &mut egui::Ui) {
    const W: f32 = 58.0;
    let plain = a.tool == Tool::Explore && a.lens == Lens::None && a.panorama.is_none();
    let ctx = ui.ctx().clone();
    if theme::tool_tile(ui, theme::icon::CURSOR, "Wybór", plain, W).clicked() {
        select_mode(a, &ctx);
    }
    for (tab, glyph, label) in [(Tab::Info, theme::icon::COMPASS, "Szczegóły"), (Tab::Library, theme::icon::BOOKMARKS, "Biblioteka"), (Tab::Conditions, theme::icon::CLOUD_SUN, "Warunki")] {
        let on = a.prefs.panel_open && a.tab == tab;
        if theme::tool_tile(ui, glyph, label, on, W).clicked() {
            a.prefs.panel_open = !on;
            a.tab = tab;
        }
    }
    ui.separator();
    for t in Tool::ALL.into_iter().filter(|t| *t != Tool::Explore) {
        let label = match t {
            Tool::Route => "Trasa",
            Tool::Measure => "Pomiar",
            Tool::Point => "Punkty",
            Tool::Area => "Offline",
            Tool::Explore => "",
        };
        if theme::tool_tile(ui, t.behavior().icon(), label, a.tool == t, W).clicked() {
            a.run(Action::Tool(t), &ctx);
        }
    }
    ui.separator();
    for l in Lens::ALL {
        let label = match l {
            Lens::Band => "Poziom",
            Lens::Rel => "Względna",
            Lens::Vis => "Widok stąd",
            Lens::Time => "Dojście",
            Lens::None => "",
        };
        if theme::tool_tile(ui, l.icon(), label, a.lens == l, W).clicked() {
            a.set_lens(l);
        }
    }
    ui.separator();
    if theme::tool_tile(ui, theme::icon::PERSON_SIMPLE_WALK, "Spacer", a.panorama.is_some(), W).clicked() {
        if a.panorama.is_some() {
            a.exit_panorama();
        } else {
            let (lon, lat) = (a.cam.lon, a.cam.lat);
            a.enter_panorama(lon, lat);
        }
    }
}

/// Pora dnia: ikona słońca lub księżyca, godzina i data; rozwija panel światła.
pub(super) fn sun_button(a: &mut Atlas, ui: &mut egui::Ui) {
    let (day, hour) = a.when();
    let custom = a.prefs.custom_time.is_some();
    let (_, alt) = a.sun();
    let glyph = if alt > 6.0 {
        theme::icon::SUN
    } else if alt > -6.0 {
        theme::icon::SUN_HORIZON
    } else {
        theme::icon::MOON_STARS
    };
    let accent = if custom { theme::BRASS } else { theme::PAPER };
    let narrow = a.compact;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(if narrow { 46.0 } else { 118.0 }, if narrow { 48.0 } else { 42.0 }), egui::Sense::click());
    theme::focus_ring(ui, &resp, rect, 12.0);
    let p = ui.painter();
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    p.rect_filled(rect, 12.0, if open { Color32::from_rgba_unmultiplied(122, 176, 255, 40) } else if resp.hovered() { Color32::from_white_alpha(16) } else { Color32::TRANSPARENT });
    let sun_col = if alt > 6.0 { theme::WARN } else if alt > -6.0 { Color32::from_rgb(255, 150, 90) } else { Color32::from_rgb(160, 180, 255) };
    if narrow {
        p.text(rect.center() - egui::vec2(0.0, 7.0), egui::Align2::CENTER_CENTER, glyph, egui::FontId::proportional(20.0), if custom { theme::BRASS } else { sun_col });
        p.text(rect.center_bottom() - egui::vec2(0.0, 9.0), egui::Align2::CENTER_CENTER, fmt::hour(hour), egui::FontId::proportional(9.5), if custom { theme::BRASS } else { theme::paper_2() });
    } else {
        p.text(rect.left_center() + egui::vec2(18.0, 0.0), egui::Align2::CENTER_CENTER, glyph, egui::FontId::proportional(22.0), sun_col);
        p.text(rect.left_top() + egui::vec2(38.0, 5.0), egui::Align2::LEFT_TOP, fmt::hour(hour), egui::FontId::proportional(16.0), accent);
        p.text(rect.left_bottom() + egui::vec2(38.0, -5.0), egui::Align2::LEFT_BOTTOM, if custom { day.label() } else { crate::i18n::tr("teraz").into() }, egui::FontId::proportional(10.5), theme::paper_3());
    }
    let zone = crate::tz::label(a.cam.lon, a.cam.lat, a.when_unix());
    let resp = resp.on_hover_text(format!(
        "{}\n{}",
        if custom { crate::i18n::tr("Pora ustawiona ręcznie") } else { crate::i18n::tr("Oświetlenie jak teraz") },
        tf!("czas lokalny środka widoku: {}", zone)
    ));
    let pw = theme::fit(ui.ctx(), 360.0);
    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).frame(theme::glass().inner_margin(egui::Margin::same(14))).show(|ui| {
        ui.set_width(pw);
        light_tab(a, ui);
    });
    let pid = egui::Popup::default_response_id(&resp);
    if egui::Popup::is_id_open(ui.ctx(), pid) {
        let ctx = ui.ctx().clone();
        glass_area(a, &ctx, pid, 1.0);
    }
}

/// Przycisk paska z ikoną, podpisem i podtytułem; rozwija panel z kafelkami.
#[allow(clippy::too_many_arguments)]
pub(super) fn bar_menu(ui: &mut egui::Ui, glyph: &str, glyph_color: Color32, title: &str, sub: &str, w: f32, hint: &str, width: f32, body: impl FnOnce(&mut egui::Ui)) -> Option<egui::Rect> {
    let narrow = theme::compact(ui.ctx());
    let w = if narrow { 50.0 } else { w };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, if narrow { 48.0 } else { 42.0 }), egui::Sense::click());
    theme::focus_ring(ui, &resp, rect, 12.0);
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    let p = ui.painter();
    p.rect_filled(rect, 12.0, if open { Color32::from_rgba_unmultiplied(122, 176, 255, 40) } else if resp.hovered() { Color32::from_white_alpha(16) } else { Color32::TRANSPARENT });
    if narrow {
        p.text(rect.center() - egui::vec2(0.0, 7.0), egui::Align2::CENTER_CENTER, glyph, egui::FontId::proportional(20.0), glyph_color);
        let short = title.split_whitespace().next().unwrap_or(title);
        p.text(rect.center_bottom() - egui::vec2(0.0, 9.0), egui::Align2::CENTER_CENTER, short, egui::FontId::proportional(9.5), theme::paper_2());
    } else {
        p.text(rect.left_center() + egui::vec2(18.0, 0.0), egui::Align2::CENTER_CENTER, glyph, egui::FontId::proportional(20.0), glyph_color);
        p.text(rect.left_top() + egui::vec2(38.0, 5.0), egui::Align2::LEFT_TOP, title, egui::FontId::proportional(14.0), theme::PAPER);
        p.text(rect.left_bottom() + egui::vec2(38.0, -5.0), egui::Align2::LEFT_BOTTOM, sub, egui::FontId::proportional(10.5), theme::paper_3());
        p.text(rect.right_center() - egui::vec2(10.0, 0.0), egui::Align2::RIGHT_CENTER, theme::icon::CARET_DOWN, egui::FontId::proportional(12.0), theme::paper_3());
    }
    let resp = resp.on_hover_text(hint);
    let width = theme::fit(ui.ctx(), width);
    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).frame(theme::glass().inner_margin(egui::Margin::same(14))).show(|ui| {
        ui.set_width(width);
        body(ui);
    });
    let pid = egui::Popup::default_response_id(&resp);
    if egui::Popup::is_id_open(ui.ctx(), pid) {
        ui.ctx().memory(|m| m.area_rect(pid))
    } else {
        None
    }
}

/// Styl mapy: osobny przycisk z bieżącym stylem.
pub(super) fn style_button(a: &mut Atlas, ui: &mut egui::Ui) {
    let label = a.prefs.style.label();
    let shown = bar_menu(ui, style_glyph(a.prefs.style), theme::BRASS, crate::i18n::tr("Styl mapy"), label, 128.0, crate::i18n::tr("Styl mapy (M)"), 340.0, |ui| style_tab(a, ui));
    if let Some(r) = shown {
        a.glass.push((r, 1.0));
    }
}

/// Warstwy: osobny przycisk z liczbą włączonych.
pub(super) fn layers_button(a: &mut Atlas, ui: &mut egui::Ui) {
    let p = &a.prefs;
    let n = [p.labels, p.trails, p.roads, p.chains, p.boundaries, p.contours, p.km_grid, p.shadows, p.detail, p.snow, p.landcover].iter().filter(|v| **v).count();
    let shown = bar_menu(ui, theme::icon::STACK, theme::BRASS, crate::i18n::tr("Warstwy"), &tf!("włączone: {}", n), 118.0, crate::i18n::tr("Warstwy na mapie"), 380.0, |ui| layers_tab(a, ui));
    if let Some(r) = shown {
        a.glass.push((r, 1.0));
    }
}

pub(super) fn style_glyph(st: Style) -> &'static str {
    match st {
        Style::Satellite => theme::icon::GLOBE_HEMISPHERE_EAST,
        Style::Terrain => theme::icon::MOUNTAINS,
        Style::Map => theme::icon::MAP_TRIFOLD,
        Style::Hypso => theme::icon::CHART_BAR,
        Style::Slope => theme::icon::ANGLE,
        Style::Aspect => theme::icon::COMPASS,
        Style::Swiss => theme::icon::FLAG_BANNER,
        Style::Sketch => theme::icon::FEATHER,
        Style::Insolation => theme::icon::SUN_DIM,
    }
}
