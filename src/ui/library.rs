//! Panel biblioteki: zapisane trasy, ulubione miejsca i zakładki widoku – otwieranie,
//! zmiana nazwy, grupy (foldery), sortowanie, wyszukiwanie i usuwanie.

use eframe::egui::{self, RichText};

use crate::fmt;
use crate::i18n::tr;
use crate::library::Sort;
use crate::ui::atlas::Atlas;
use crate::ui::theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Route,
    Place,
    View,
}

struct Row {
    kind: Kind,
    idx: usize,
    id: u64,
    name: String,
    group: String,
    sub: String,
}

enum Action {
    Open(Kind, usize),
    Edit(u64),
    Delete(Kind, usize),
    Save(Kind, usize, String, String),
    ExportRoute(usize),
}

/// Stan edycji w pamięci egui: identyfikator, nazwa, grupa.
#[derive(Clone, Default)]
struct Editing {
    id: u64,
    name: String,
    group: String,
}

fn rows(a: &Atlas, kind: Kind, filter: &str) -> Vec<Row> {
    let l = &a.library;
    let f = filter.trim().to_lowercase();
    let mut out: Vec<Row> = match kind {
        Kind::Route => l
            .order(&l.routes, |r| (r.name.as_str(), r.created))
            .into_iter()
            .map(|i| {
                let r = &l.routes[i];
                let sub = format!("{} · ↑ {} · {}", fmt::dist(r.dist), fmt::ele(r.up), fmt::time(r.time));
                Row { kind, idx: i, id: r.id, name: r.name.clone(), group: r.group.clone(), sub }
            })
            .collect(),
        Kind::Place => l
            .order(&l.places, |p| (p.name.as_str(), p.created))
            .into_iter()
            .map(|i| {
                let p = &l.places[i];
                Row { kind, idx: i, id: p.id, name: p.name.clone(), group: p.group.clone(), sub: fmt::ele(p.e as f64) }
            })
            .collect(),
        Kind::View => l
            .order(&l.views, |v| (v.name.as_str(), v.created))
            .into_iter()
            .map(|i| {
                let v = &l.views[i];
                let style = crate::ui::atlas::Style::from_code(v.style).map(|s| tr(s.label())).unwrap_or("");
                Row { kind, idx: i, id: v.id, name: v.name.clone(), group: v.group.clone(), sub: style.to_string() }
            })
            .collect(),
    };
    if !f.is_empty() {
        out.retain(|r| r.name.to_lowercase().contains(&f) || r.group.to_lowercase().contains(&f));
    }
    // najpierw bez grupy, potem grupy alfabetycznie (kolejność w grupie – według sortowania)
    out.sort_by_key(|r| (!r.group.is_empty(), r.group.to_lowercase()));
    out
}

pub fn show(a: &mut Atlas, ui: &mut egui::Ui) {
    let filter_id = egui::Id::new("biblioteka-szukaj");
    let edit_id = egui::Id::new("biblioteka-edycja");
    let mut filter = ui.data_mut(|d| d.get_temp::<String>(filter_id)).unwrap_or_default();
    let mut editing = ui.data_mut(|d| d.get_temp::<Editing>(edit_id));

    theme::para(ui, crate::i18n::tr("Zapisane trasy, ulubione miejsca i widoki. Kliknij nazwę, żeby otworzyć."));
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut filter).hint_text(tr("Szukaj w bibliotece")).desired_width(200.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut sort = a.library.sort;
            ui.selectable_value(&mut sort, Sort::Name, tr("A–Z"));
            ui.selectable_value(&mut sort, Sort::Newest, tr("Najnowsze"));
            if sort != a.library.sort {
                a.library.sort = sort;
                a.save_library();
            }
        });
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if a.current_route().is_some() && ui.button(theme::ic(theme::icon::PATH, tr("Zapisz bieżącą trasę"))).clicked() {
            a.save_route_to_library();
        }
        if ui.button(theme::ic(theme::icon::BOOKMARK_SIMPLE, tr("Zapisz widok"))).clicked() {
            a.save_view();
        }
    });

    let groups = a.library.groups();
    let mut actions = Vec::new();
    let sections = [
        (Kind::Route, theme::icon::PATH, crate::i18n::tr("Trasy"), crate::i18n::tr("Brak zapisanych tras. Zaplanuj trasę albo wczytaj ślad i wybierz „Zapisz w bibliotece” (Ctrl+S).")),
        (Kind::Place, theme::icon::STAR, crate::i18n::tr("Ulubione miejsca"), crate::i18n::tr("Brak ulubionych miejsc. Zaznacz szczyt lub punkt na mapie i wybierz „Dodaj do ulubionych”.")),
        (Kind::View, theme::icon::BOOKMARK_SIMPLE, crate::i18n::tr("Widoki"), crate::i18n::tr("Brak zapisanych widoków. „Zapisz widok” (Ctrl+D) zapamiętuje kadr kamery i styl mapy.")),
    ];
    for (kind, glyph, title, empty) in sections {
        let list = rows(a, kind, &filter);
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(glyph).size(14.0).color(theme::BRASS));
            ui.label(theme::kicker(&format!("{} · {}", tr(title), list.len())));
        });
        if list.is_empty() {
            theme::para(ui, if filter.trim().is_empty() { tr(empty) } else { tr("Nic nie pasuje do wyszukiwania.") });
            continue;
        }
        let mut last_group: Option<String> = None;
        for r in list {
            if !r.group.is_empty() && last_group.as_deref() != Some(r.group.as_str()) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(theme::icon::FOLDER_SIMPLE).size(13.0).color(theme::PAPER_3));
                    ui.label(RichText::new(&r.group).small().strong().color(theme::PAPER_2));
                });
            }
            last_group = Some(r.group.clone());
            if let Some(e) = editing.as_mut().filter(|e| e.id == r.id) {
                edit_row(ui, e, &groups, &r, &mut actions);
            } else {
                item_row(ui, &r, &mut actions);
            }
        }
    }
    ui.add_space(12.0);
    theme::para(ui, tr("Biblioteka jest zapisana w pliku biblioteka.json w folderze ustawień programu."));

    for act in actions {
        match act {
            Action::Open(Kind::Route, i) => a.open_saved_route(i),
            Action::Open(Kind::Place, i) => {
                if let Some(p) = a.library.places.get(i).cloned() {
                    a.selection = crate::ui::atlas::Selection::Point { lon: p.lon, lat: p.lat };
                    a.fly_to(p.lon, p.lat, Some(a.cam.distance.clamp(2500.0, 9000.0)));
                }
            }
            Action::Open(Kind::View, i) => a.open_view(i),
            Action::Edit(id) => {
                let found = a
                    .library
                    .routes
                    .iter()
                    .map(|r| (r.id, &r.name, &r.group))
                    .chain(a.library.places.iter().map(|p| (p.id, &p.name, &p.group)))
                    .chain(a.library.views.iter().map(|v| (v.id, &v.name, &v.group)))
                    .find(|x| x.0 == id)
                    .map(|(id, n, g)| Editing { id, name: n.clone(), group: g.clone() });
                editing = found;
            }
            Action::Save(kind, i, name, group) => {
                let name = name.trim().to_string();
                let group = group.trim().to_string();
                let l = &mut a.library;
                let slot = match kind {
                    Kind::Route => l.routes.get_mut(i).map(|r| (&mut r.name, &mut r.group)),
                    Kind::Place => l.places.get_mut(i).map(|p| (&mut p.name, &mut p.group)),
                    Kind::View => l.views.get_mut(i).map(|v| (&mut v.name, &mut v.group)),
                };
                if let Some((n, g)) = slot {
                    if !name.is_empty() {
                        *n = name;
                    }
                    *g = group;
                }
                editing = None;
                a.save_library();
            }
            Action::Delete(kind, i) => {
                let name = match kind {
                    Kind::Route => a.library.routes.get(i).map(|r| r.name.clone()),
                    Kind::Place => a.library.places.get(i).map(|p| p.name.clone()),
                    Kind::View => a.library.views.get(i).map(|v| v.name.clone()),
                }
                .unwrap_or_default();
                let ok = rfd::MessageDialog::new()
                    .set_title(crate::i18n::tr("GRAŃ"))
                    .set_description(tf!("Usunąć „{}” z biblioteki?", name))
                    .set_buttons(rfd::MessageButtons::YesNo)
                    .show();
                if ok == rfd::MessageDialogResult::Yes {
                    match kind {
                        Kind::Route if i < a.library.routes.len() => {
                            a.library.routes.remove(i);
                        }
                        Kind::Place if i < a.library.places.len() => {
                            a.library.places.remove(i);
                        }
                        Kind::View if i < a.library.views.len() => {
                            a.library.views.remove(i);
                        }
                        _ => {}
                    }
                    editing = None;
                    a.save_library();
                }
            }
            Action::ExportRoute(i) => {
                a.open_saved_route(i);
                a.export_route_after_routing();
            }
        }
    }
    ui.data_mut(|d| {
        d.insert_temp(filter_id, filter);
        match editing {
            Some(e) => d.insert_temp(edit_id, e),
            None => d.remove::<Editing>(edit_id),
        }
    });
}

fn item_row(ui: &mut egui::Ui, r: &Row, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        ui.set_width(ui.available_width());
        let name = ui
            .add(egui::Label::new(RichText::new(&r.name).strong()).sense(egui::Sense::click()).truncate())
            .on_hover_text(tr("Otwórz"));
        if name.clicked() {
            actions.push(Action::Open(r.kind, r.idx));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button(theme::icon::TRASH).on_hover_text(tr("Usuń")).clicked() {
                actions.push(Action::Delete(r.kind, r.idx));
            }
            if ui.small_button(theme::icon::PENCIL_SIMPLE).on_hover_text(tr("Zmień nazwę lub grupę")).clicked() {
                actions.push(Action::Edit(r.id));
            }
            if r.kind == Kind::Route && ui.small_button(theme::icon::EXPORT).on_hover_text(tr("Eksportuj do pliku")).clicked() {
                actions.push(Action::ExportRoute(r.idx));
            }
            ui.label(RichText::new(&r.sub).small().color(theme::PAPER_3));
        });
    });
}

fn edit_row(ui: &mut egui::Ui, e: &mut Editing, groups: &[String], r: &Row, actions: &mut Vec<Action>) {
    theme::card(true).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(tr("Nazwa")).small().color(theme::PAPER_3));
        let resp = ui.add(egui::TextEdit::singleline(&mut e.name).desired_width(f32::INFINITY));
        ui.label(RichText::new(tr("Grupa (folder)")).small().color(theme::PAPER_3));
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut e.group).hint_text(tr("bez grupy")).desired_width(180.0));
            if !groups.is_empty() {
                egui::ComboBox::from_id_salt(("grupa", e.id)).selected_text(tr("wybierz…")).show_ui(ui, |ui| {
                    if ui.selectable_label(e.group.is_empty(), tr("bez grupy")).clicked() {
                        e.group.clear();
                    }
                    for g in groups {
                        if ui.selectable_label(&e.group == g, g).clicked() {
                            e.group = g.clone();
                        }
                    }
                });
            }
        });
        ui.horizontal(|ui| {
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.add(theme::primary(tr("Zapisz"))).clicked() || enter {
                actions.push(Action::Save(r.kind, r.idx, e.name.clone(), e.group.clone()));
            }
            if ui.button(tr("Anuluj")).clicked() {
                actions.push(Action::Edit(u64::MAX));
            }
        });
    });
}
