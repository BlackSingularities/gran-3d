//! Paleta poleceń (Ctrl+K): wszystkie polecenia programu z rejestru [`Action`] z wyszukiwaniem,
//! obsługą strzałkami i Enterem. Gdy nic nie pasuje, wpisany tekst można wyszukać na mapie.

use eframe::egui::{self, RichText};

use crate::ui::atlas::{Action, Atlas};
use crate::ui::search::fold;
use crate::ui::theme;

#[derive(Default)]
pub struct Palette {
    pub q: String,
    sel: usize,
    focused: bool,
}

/// Wiersz palety: polecenie albo wyszukanie tekstu na mapie.
#[derive(Clone, Copy, PartialEq)]
enum Item {
    Act(Action),
    SearchMap,
}

/// Polecenia pasujące do zapytania: każde słowo zapytania musi wystąpić w nazwie, grupie albo skrócie.
fn matches(q: &str) -> Vec<Action> {
    let words: Vec<String> = fold(q).split_whitespace().map(str::to_string).collect();
    Action::all()
        .into_iter()
        .filter(|a| {
            let (_, label, group) = a.info();
            let keys = a.keys().map(|k| k.text()).unwrap_or_default();
            let hay = fold(&format!("{label} {} {keys}", group.label()));
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .collect()
}

pub fn show(a: &mut Atlas, ctx: &egui::Context) {
    let Some(mut p) = a.palette.take() else { return };
    let mut items: Vec<Item> = matches(&p.q).into_iter().map(Item::Act).collect();
    if !p.q.trim().is_empty() {
        items.push(Item::SearchMap);
    }
    p.sel = p.sel.min(items.len().saturating_sub(1));
    let (up, down, enter, esc) = ctx.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
        )
    });
    if up {
        p.sel = p.sel.checked_sub(1).unwrap_or(items.len().saturating_sub(1));
    }
    if down && !items.is_empty() {
        p.sel = (p.sel + 1) % items.len();
    }
    let mut run: Option<Item> = enter.then(|| items.get(p.sel).copied()).flatten();
    let mut close = esc;

    let w = (ctx.screen_rect().width() * 0.5).clamp(360.0, 620.0);
    let resp = egui::Area::new(egui::Id::new("paleta-polecen"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 90.0))
        .show(ctx, |ui| {
            theme::glass().inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                ui.set_width(w);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(theme::icon::COMMAND).size(18.0).color(theme::BRASS));
                    let te = egui::TextEdit::singleline(&mut p.q)
                        .hint_text(crate::i18n::tr("Wpisz polecenie, np. „siatka”, „kontrast”, „gpx”…"))
                        .desired_width(f32::INFINITY)
                        .frame(false)
                        .font(egui::FontId::proportional(16.0));
                    let r = ui.add(te);
                    if !p.focused {
                        r.request_focus();
                        p.focused = true;
                    }
                    if r.changed() {
                        p.sel = 0;
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().max_height(420.0).auto_shrink([false, true]).show(ui, |ui| {
                    if items.is_empty() {
                        ui.label(RichText::new(crate::i18n::tr("Brak pasujących poleceń.")).color(theme::paper_3()));
                    }
                    let mut last_group = None;
                    for (n, item) in items.iter().enumerate() {
                        let selected = n == p.sel;
                        let (glyph, label, group, keys, state, enabled) = match item {
                            Item::Act(act) => {
                                let (g, l, gr) = act.info();
                                (g, l.to_string(), Some(gr), act.keys().map(|k| k.text()), act.checked(a), act.enabled(a))
                            }
                            Item::SearchMap => (
                                theme::icon::MAGNIFYING_GLASS,
                                tf!("Szukaj „{}” na mapie", p.q.trim()),
                                None,
                                Some("/".to_string()),
                                None,
                                true,
                            ),
                        };
                        if p.q.trim().is_empty() && group != last_group {
                            if let Some(g) = group {
                                ui.add_space(4.0);
                                ui.label(theme::kicker(g.label()));
                            }
                            last_group = group;
                        }
                        let (rect, r) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::click());
                        if selected {
                            r.scroll_to_me(None);
                        }
                        let painter = ui.painter();
                        if selected || r.hovered() {
                            painter.rect_filled(rect, 8.0, if selected { theme::BRASS_DIM } else { egui::Color32::from_white_alpha(14) });
                        }
                        let color = if !enabled {
                            theme::INK_4
                        } else if selected {
                            theme::BRASS
                        } else {
                            theme::PAPER
                        };
                        painter.text(rect.left_center() + egui::vec2(10.0, 0.0), egui::Align2::LEFT_CENTER, glyph, egui::FontId::proportional(17.0), color);
                        painter.text(rect.left_center() + egui::vec2(38.0, 0.0), egui::Align2::LEFT_CENTER, &label, egui::FontId::proportional(14.0), if enabled { theme::PAPER } else { theme::paper_3() });
                        let mut x = rect.right() - 10.0;
                        if let Some(k) = keys {
                            let g = painter.layout_no_wrap(k, egui::FontId::monospace(11.5), theme::paper_2());
                            let kr = egui::Rect::from_min_size(egui::pos2(x - g.size().x - 10.0, rect.center().y - g.size().y / 2.0 - 3.0), g.size() + egui::vec2(10.0, 6.0));
                            painter.rect_stroke(kr, 5.0, egui::Stroke::new(1.0, theme::INK_4), egui::StrokeKind::Inside);
                            painter.galley(kr.min + egui::vec2(5.0, 3.0), g, theme::paper_2());
                            x = kr.left() - 10.0;
                        }
                        if let Some(on) = state {
                            let t = if on { crate::i18n::tr("wł.") } else { crate::i18n::tr("wył.") };
                            painter.text(egui::pos2(x, rect.center().y), egui::Align2::RIGHT_CENTER, t, egui::FontId::proportional(12.0), if on { theme::OK } else { theme::paper_3() });
                        }
                        if r.clicked() {
                            run = Some(*item);
                        }
                        if r.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                            p.sel = n;
                        }
                    }
                });
                ui.add_space(4.0);
                ui.label(RichText::new(crate::i18n::tr("↑ ↓ wybór · Enter wykonuje · Esc zamyka")).small().color(theme::paper_3()));
            });
        });
    // klik poza paletą zamyka ją
    if resp.response.clicked_elsewhere() {
        close = true;
    }
    match run {
        Some(Item::Act(act)) if act.enabled(a) => {
            a.palette = None;
            a.run(act, ctx);
            // polecenie mogło otworzyć paletę na nowo (Ctrl+K w palecie)
            return;
        }
        Some(Item::SearchMap) => {
            close = true;
            a.search = Some(crate::ui::search::Search::with_query(p.q.trim()));
        }
        _ => {}
    }
    if !close {
        a.palette = Some(p);
    }
}
