//! Układ telefonu (wąski ekran): pływające pole wyszukiwania u góry, przyciski stylu i warstw pod
//! nim, dolny pasek nawigacji z pięcioma stałymi pozycjami i arkusz „Więcej” z pozostałymi
//! narzędziami. Zamiast rzędu kilkunastu przewijanych kafelków – wszystko widać od razu.

use super::*;

/// Promień zaokrąglenia pola wyszukiwania (pigułka).
const PILL_R: f32 = 23.0;

/// Okrągły przycisk z ikoną (pole wyszukiwania, pływające kontrolki).
pub fn round_icon(ui: &mut egui::Ui, glyph: &str, color: Color32, hint: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::click());
    theme::focus_ring(ui, &resp, rect, 20.0);
    let p = ui.painter();
    if resp.is_pointer_button_down_on() {
        p.circle_filled(rect.center(), 19.0, Color32::from_white_alpha(28));
    } else if resp.hovered() {
        p.circle_filled(rect.center(), 19.0, Color32::from_white_alpha(14));
    }
    p.text(rect.center(), egui::Align2::CENTER_CENTER, glyph, egui::FontId::proportional(20.0), color);
    resp.on_hover_text(hint)
}

/// Górna część ekranu telefonu: pole wyszukiwania (pigułka) z porą dnia i przyciskami z `trailing`
/// (ustawienia, aktualizacja).
pub(super) fn top(a: &mut Atlas, ui: &mut egui::Ui, top: f32, trailing: impl FnOnce(&mut Atlas, &mut egui::Ui)) {
    let ctx = ui.ctx().clone();
    let screen = ctx.screen_rect();
    let pill = egui::Rect::from_min_max(egui::pos2(screen.left() + 10.0, top + 6.0), egui::pos2(screen.right() - 10.0, top + TOP_BAR_H - 4.0));
    a.glass.push((pill.shrink(4.0), 1.0));
    // cała pigułka należy do interfejsu (dotknięcie nie przechodzi na mapę)
    ui.allocate_rect(pill, egui::Sense::hover());
    {
        let p = ui.painter();
        // miękki cień pod pigułką
        for (i, a8) in [(6.0, 26u8), (3.0, 40)] {
            p.rect_filled(pill.translate(egui::vec2(0.0, i)), PILL_R, Color32::from_black_alpha(a8));
        }
        p.rect_filled(pill, PILL_R, theme::glass_fill());
        p.rect_stroke(pill, PILL_R, egui::Stroke::new(1.0, theme::glass_edge()), egui::StrokeKind::Inside);
    }
    // prawa część: przyciski programu i pora dnia
    let mut right_w = 0.0;
    ui.scope_builder(egui::UiBuilder::new().max_rect(pill.shrink2(egui::vec2(6.0, 2.0))).layout(egui::Layout::right_to_left(egui::Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        trailing(a, ui);
        sun_button(a, ui);
        right_w = ui.min_rect().width();
    });
    // lewa część: pole wyszukiwania
    let search = egui::Rect::from_min_max(pill.min, egui::pos2(pill.right() - right_w - 12.0, pill.bottom()));
    let resp = ui.interact(search, egui::Id::new("szukaj-pigulka"), egui::Sense::click());
    let p = ui.painter();
    if resp.is_pointer_button_down_on() {
        p.rect_filled(search.shrink(3.0), PILL_R - 3.0, Color32::from_white_alpha(14));
    }
    p.text(search.left_center() + egui::vec2(22.0, 0.0), egui::Align2::CENTER_CENTER, theme::icon::MAGNIFYING_GLASS, egui::FontId::proportional(19.0), theme::paper_2());
    p.with_clip_rect(search.shrink2(egui::vec2(4.0, 0.0))).text(
        search.left_center() + egui::vec2(42.0, 0.0),
        egui::Align2::LEFT_CENTER,
        crate::i18n::tr("Szukaj szczytu, schroniska, miejsca"),
        egui::FontId::proportional(14.5),
        theme::paper_3(),
    );
    // pionowa kreska między wyszukiwaniem a przyciskami
    let x = search.right() + 4.0;
    p.line_segment([egui::pos2(x, pill.top() + 12.0), egui::pos2(x, pill.bottom() - 12.0)], egui::Stroke::new(1.0, Color32::from_white_alpha(22)));
    // wyszukiwarka rozwija się z całej pigułki
    ctx.data_mut(|d| {
        d.insert_temp(egui::Id::new("szukaj-x"), pill.left());
        d.insert_temp(egui::Id::new("szukaj-w"), pill.width());
    });
    if resp.on_hover_text(crate::i18n::tr("Szukaj na mapie")).clicked() {
        a.search = Some(Default::default());
    }
    // zamykanie wyszukiwarki: pole zwija się z powrotem do pigułki
    let t = ctx.animate_bool_with_time(egui::Id::new("szukaj-rozwiniecie"), a.search.is_some(), SEARCH_ANIM);
    if a.search.is_none() && t > 0.01 {
        let r = search_rect(&ctx, a.safe_top, t);
        ui.painter().rect_filled(r, 14.0, theme::POPUP_FILL);
        ctx.request_repaint();
    }
}

/// Wysokość pływającej kolumny stylu i warstw pod polem wyszukiwania (kompas leży pod nią).
pub const MAP_BUTTONS_H: f32 = 112.0;

/// Styl mapy i warstwy: pionowa szklana kolumna przy prawej krawędzi, pod polem wyszukiwania.
pub fn map_buttons(a: &mut Atlas, ctx: &egui::Context) {
    if !a.compact || sheet_covers(a) {
        return;
    }
    let alpha = controls_alpha(a, ctx);
    let id = egui::Id::new("styl-warstwy");
    egui::Area::new(id)
        .anchor(egui::Align2::RIGHT_TOP, [-10.0, a.safe_top + TOP_BAR_H + 8.0])
        .order(egui::Order::Middle)
        .interactable(alpha > 0.05)
        .show(ctx, |ui| {
            ui.multiply_opacity(alpha);
            theme::glass_float().corner_radius(egui::CornerRadius::same(16)).inner_margin(egui::Margin::same(3)).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.vertical(|ui| {
                    style_button(a, ui);
                    layers_button(a, ui);
                });
            });
        });
    glass_float_area(a, ctx, id, alpha);
}

/// Pozycje dolnego paska.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Nav {
    Map,
    Route,
    Record,
    Library,
    More,
}

fn more_id() -> egui::Id {
    egui::Id::new("arkusz-wiecej")
}

fn more_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(more_id())).unwrap_or(false)
}

fn set_more(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(more_id(), on));
}

/// Zamyka arkusz „Więcej” (przycisk „wstecz”); zwraca, czy był otwarty.
pub fn close_more(ctx: &egui::Context) -> bool {
    let was = more_open(ctx);
    set_more(ctx, false);
    was
}

/// Dolny pasek nawigacji: Mapa, Trasa, Nagraj (albo Szczegóły bez GPS), Biblioteka, Więcej.
pub(super) fn bottom_nav(a: &mut Atlas, ctx: &egui::Context) {
    let screen = ctx.screen_rect();
    let bar = egui::Rect::from_min_size(egui::pos2(screen.left(), screen.bottom() - a.safe_bottom - TOOLBAR_H), egui::vec2(screen.width(), TOOLBAR_H));
    a.glass.push((bar, 1.0));
    let more = more_open(ctx);
    let lib = a.prefs.panel_open && a.tab == Tab::Library;
    let rec_tab = a.prefs.panel_open && a.tab == Tab::Record;
    let other_tool = matches!(a.tool, Tool::Measure | Tool::Point | Tool::Area) || a.lens != Lens::None || a.panorama.is_some();
    let active = if more {
        Nav::More
    } else if a.tool == Tool::Route {
        Nav::Route
    } else if lib {
        Nav::Library
    } else if rec_tab {
        Nav::Record
    } else if other_tool || (a.prefs.panel_open && a.tab == Tab::Conditions) {
        Nav::More
    } else {
        Nav::Map
    };
    let gps = crate::platform::LOCATION;
    let recording = a.rec.is_some();
    let items: [(Nav, &str, &str); 5] = [
        (Nav::Map, theme::icon::MAP_TRIFOLD, "Mapa"),
        (Nav::Route, theme::icon::PATH, "Trasa"),
        if gps { (Nav::Record, theme::icon::RECORD, if recording { "Nagrywam" } else { "Nagraj" }) } else { (Nav::Record, theme::icon::COMPASS, "Szczegóły") },
        (Nav::Library, theme::icon::BOOKMARKS, "Biblioteka"),
        (Nav::More, theme::icon::SQUARES_FOUR, "Więcej"),
    ];
    let mut clicked = None;
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
            let w = bar.width() / items.len() as f32;
            for (k, (nav, glyph, label)) in items.iter().enumerate() {
                let cell = egui::Rect::from_min_size(egui::pos2(bar.left() + k as f32 * w, bar.top()), egui::vec2(w, TOOLBAR_H));
                let resp = ui.interact(cell, egui::Id::new(("nawigacja", k)), egui::Sense::click());
                theme::focus_ring(ui, &resp, cell.shrink(4.0), 12.0);
                let on = *nav == active || (*nav == Nav::Record && recording);
                let p = ui.painter();
                // wskaźnik aktywnej pozycji: pigułka za ikoną
                let pill = egui::Rect::from_center_size(egui::pos2(cell.center().x, cell.top() + 21.0), egui::vec2(56.0, 30.0));
                if on {
                    p.rect_filled(pill, 15.0, theme::BRASS_DIM);
                } else if resp.is_pointer_button_down_on() {
                    p.rect_filled(pill, 15.0, Color32::from_white_alpha(18));
                }
                let rec_red = *nav == Nav::Record && recording;
                let color = if rec_red { theme::SIGNAL } else if on { theme::BRASS } else { theme::paper_2() };
                p.text(pill.center(), egui::Align2::CENTER_CENTER, *glyph, egui::FontId::proportional(21.0), color);
                let text = crate::i18n::tr(label);
                let g = p.layout_no_wrap(text.to_string(), egui::FontId::proportional(11.5), if on { theme::PAPER } else { theme::paper_3() });
                let gx = cell.center().x - (g.size().x / 2.0).min(w / 2.0 - 2.0);
                p.with_clip_rect(cell).galley(egui::pos2(gx, cell.bottom() - 19.0), g, theme::PAPER);
                if resp.on_hover_text(text).clicked() {
                    clicked = Some(*nav);
                }
            }
        });
    if let Some(nav) = clicked {
        if nav != Nav::More {
            set_more(ctx, false);
        }
        match nav {
            Nav::Map => {
                select_mode(a, ctx);
                a.prefs.panel_open = false;
            }
            Nav::Route => a.run(Action::Tool(Tool::Route), ctx),
            Nav::Record if gps => a.run(Action::Record, ctx),
            Nav::Record => a.run(Action::Details, ctx),
            Nav::Library => a.run(Action::Library, ctx),
            Nav::More => set_more(ctx, !more),
        }
    }
    if more_open(ctx) {
        more_sheet(a, ctx, bar);
    }
}

/// Arkusz „Więcej”: pozostałe narzędzia, soczewki i polecenia w siatce kafelków nad dolnym paskiem.
fn more_sheet(a: &mut Atlas, ctx: &egui::Context, bar: egui::Rect) {
    let screen = ctx.screen_rect();
    let w = screen.width();
    let id = egui::Id::new("wiecej");
    let mut picked: Option<Box<dyn FnOnce(&mut Atlas, &egui::Context)>> = None;
    let mut close = false;
    let resp = egui::Area::new(id)
        .anchor(egui::Align2::LEFT_BOTTOM, [0.0, -(screen.bottom() - bar.top())])
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(theme::POPUP_FILL)
                .stroke(egui::Stroke::new(1.0, theme::glass_edge()))
                .corner_radius(egui::CornerRadius { nw: 24, ne: 24, sw: 0, se: 0 })
                .inner_margin(egui::Margin { left: 14, right: 14, top: 10, bottom: 14 })
                .shadow(egui::Shadow { offset: [0, -4], blur: 24, spread: 0, color: Color32::from_black_alpha(90) })
                .show(ui, |ui| {
                    ui.set_width(w - 28.0);
                    // uchwyt arkusza
                    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 10.0), egui::Sense::hover());
                    ui.painter().rect_filled(egui::Rect::from_center_size(r.center(), egui::vec2(36.0, 4.0)), 2.0, theme::INK_4);
                    let max_h = (bar.top() - screen.top() - a.safe_top - TOP_BAR_H - 40.0).max(200.0);
                    egui::ScrollArea::vertical().id_salt("wiecej-przewijanie").max_height(max_h).show(ui, |ui| {
                        let mut group = |ui: &mut egui::Ui, title: &str, tiles: Vec<(&'static str, &'static str, bool, Box<dyn FnOnce(&mut Atlas, &egui::Context)>)>| {
                            ui.add_space(4.0);
                            ui.label(theme::kicker(title));
                            ui.add_space(2.0);
                            theme::tile_grid(ui, 4, |ui, tw| {
                                for (glyph, label, on, run) in tiles {
                                    if theme::tile(ui, glyph, label, on, tw).clicked() {
                                        picked = Some(run);
                                    }
                                }
                            });
                            ui.add_space(6.0);
                        };
                        let t = |tool: Tool| -> Box<dyn FnOnce(&mut Atlas, &egui::Context)> { Box::new(move |a: &mut Atlas, ctx: &egui::Context| a.run(Action::Tool(tool), ctx)) };
                        let l = |lens: Lens| -> Box<dyn FnOnce(&mut Atlas, &egui::Context)> { Box::new(move |a: &mut Atlas, _: &egui::Context| a.set_lens(if a.lens == lens { Lens::None } else { lens })) };
                        let mut tools: Vec<(&'static str, &'static str, bool, Box<dyn FnOnce(&mut Atlas, &egui::Context)>)> = vec![
                            (theme::icon::COMPASS, "Szczegóły", a.prefs.panel_open && a.tab == Tab::Info && a.tool == Tool::Explore, Box::new(|a: &mut Atlas, ctx: &egui::Context| a.run(Action::Details, ctx))),
                            (theme::icon::CLOUD_SUN, "Warunki", a.prefs.panel_open && a.tab == Tab::Conditions, Box::new(|a: &mut Atlas, ctx: &egui::Context| a.run(Action::Conditions, ctx))),
                            (Tool::Measure.behavior().icon(), "Pomiar", a.tool == Tool::Measure, t(Tool::Measure)),
                            (Tool::Point.behavior().icon(), "Punkty", a.tool == Tool::Point, t(Tool::Point)),
                            (Tool::Area.behavior().icon(), "Mapy offline", a.tool == Tool::Area, t(Tool::Area)),
                            (theme::icon::PERSON_SIMPLE_WALK, "Spacer", a.panorama.is_some(), Box::new(|a: &mut Atlas, ctx: &egui::Context| a.run(Action::Walk, ctx))),
                        ];
                        if crate::platform::LOCATION {
                            tools.insert(0, (theme::icon::CROSSHAIR, "Położenie", a.gps.on, Box::new(|a: &mut Atlas, _: &egui::Context| a.toggle_locate())));
                        }
                        if a.current_route().is_some() {
                            tools.push((theme::icon::AIRPLANE_TILT, "Przelot", a.flight.is_some(), Box::new(|a: &mut Atlas, ctx: &egui::Context| a.run(Action::Flight, ctx))));
                        }
                        group(ui, "Narzędzia", tools);
                        let lenses = Lens::ALL
                            .iter()
                            .map(|&lens| {
                                let label = match lens {
                                    Lens::Band => "Poziom",
                                    Lens::Rel => "Względna",
                                    Lens::Vis => "Widok stąd",
                                    Lens::Time => "Dojście",
                                    Lens::None => "",
                                };
                                (lens.icon(), label, a.lens == lens, l(lens))
                            })
                            .filter(|x| !x.1.is_empty())
                            .collect();
                        group(ui, "Soczewki terenu", lenses);
                        group(
                            ui,
                            "Program",
                            vec![
                                (theme::icon::GEAR, "Ustawienia", false, Box::new(|a: &mut Atlas, ctx: &egui::Context| a.run(Action::Settings, ctx))),
                                (theme::icon::QUESTION, "Pomoc", a.help, Box::new(|a: &mut Atlas, ctx: &egui::Context| a.run(Action::Help, ctx))),
                                (theme::icon::CAMERA, "Zrzut ekranu", false, Box::new(|a: &mut Atlas, ctx: &egui::Context| a.run(Action::Screenshot, ctx))),
                                (theme::icon::CLOUD_SLASH, "Tryb offline", a.prefs.offline_mode, Box::new(|a: &mut Atlas, ctx: &egui::Context| a.run(Action::Offline, ctx))),
                            ],
                        );
                    });
                });
        })
        .response;
    a.glass.push((resp.rect, 1.0));
    // stuknięcie poza arkuszem (oprócz dolnego paska) albo Esc zamyka
    let outside = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !resp.rect.contains(p) && !bar.contains(p)));
    if outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close = true;
    }
    if let Some(run) = picked {
        run(a, ctx);
        close = true;
    }
    if close {
        set_more(ctx, false);
    }
}
