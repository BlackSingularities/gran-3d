//! Panele punktów społeczności (komentarze dla innych turystów) i map offline.

use super::*;
use crate::offline::{Area, Detail};
use crate::points::{Category, Status};
use crate::ui::atlas::AreaDraft;

fn text_field(ui: &mut egui::Ui, value: &mut String, hint: &str, multiline: bool) -> egui::Response {
    let w = ui.available_width();
    if multiline {
        ui.add(egui::TextEdit::multiline(value).hint_text(crate::i18n::tr(hint)).desired_rows(4).desired_width(w).char_limit(2000))
    } else {
        ui.add(egui::TextEdit::singleline(value).hint_text(crate::i18n::tr(hint)).desired_width(w).char_limit(120))
    }
}

/// Stan sieci i kolejki wpisów.
fn points_status(a: &Atlas, ui: &mut egui::Ui) {
    let queued = a.points.queued();
    if crate::offline::is_offline() {
        ui.label(RichText::new(theme::ic(theme::icon::CLOUD_SLASH, "Tryb offline – nowe wpisy wyślą się po połączeniu z siecią.")).small().color(theme::WARN));
    } else if let Some(e) = a.points.error() {
        ui.label(RichText::new(tf!("Brak połączenia z redzikowski.com: {}", e)).small().color(theme::WARN));
    }
    if queued > 0 {
        ui.label(RichText::new(tf!("W kolejce do wysłania: {}", queued)).small().color(theme::paper_3()));
    }
}

/// Zakładka narzędzia „Punkty i komentarze”: formularz nowego punktu albo lista punktów w okolicy.
pub fn point_tool_panel(a: &mut Atlas, ui: &mut egui::Ui) {
    use crate::i18n::tr;
    if let Selection::UserPoint(id) = a.selection.clone() {
        if ui.button(theme::ic(theme::icon::ARROW_LEFT, if a.point_draft.is_some() { "Wróć do nowego punktu" } else { "Wszystkie punkty" })).clicked() {
            a.selection = Selection::None;
            return;
        }
        ui.add_space(6.0);
        user_point_info(a, ui, &id);
        return;
    }
    if a.point_draft.is_some() {
        return draft_form(a, ui);
    }
    theme::para(ui, if crate::platform::MOBILE {
        tr("Dotknij mapy w miejscu, które chcesz opisać: źródło, utrudnienie na szlaku, punkt widokowy, parking. Wpis zobaczą inni turyści korzystający z GRANI.")
    } else {
        tr("Kliknij na mapie miejsce, które chcesz opisać: źródło, utrudnienie na szlaku, punkt widokowy, parking. Wpis zobaczą inni turyści korzystający z GRANI.")
    });
    if ui.button(theme::ic(theme::icon::CROSSHAIR, "Dodaj punkt na środku widoku")).clicked() {
        let (lon, lat) = (a.cam.lon, a.cam.lat);
        a.start_point_draft(lon, lat);
    }
    points_status(a, ui);
    if !a.prefs.user_points {
        ui.add_space(6.0);
        theme::switch(ui, &mut a.prefs.user_points, tr("Pokazuj punkty społeczności na mapie"));
    }
    ui.add_space(10.0);
    let all = a.points.all();
    let mine: Vec<_> = all.iter().filter(|p| p.mine).cloned().collect();
    let (cl, ct) = (a.cam.lon, a.cam.lat);
    let mut near: Vec<(f64, crate::points::Point)> = all.into_iter().map(|p| (haversine(cl, ct, p.lon, p.lat), p)).filter(|(d, _)| *d < 30_000.0).collect();
    near.sort_by(|x, y| x.0.total_cmp(&y.0));
    ui.label(theme::kicker(tr("W okolicy")));
    if near.is_empty() {
        theme::para(ui, if a.points.busy() { tr("Wczytywanie punktów…") } else { tr("Brak punktów w promieniu 30 km. Bądź pierwszy!") });
    }
    let mut open = None;
    for (d, p) in near.iter().take(40) {
        if point_row(ui, p, Some(*d)).clicked() {
            open = Some(p.clone());
        }
    }
    if !mine.is_empty() {
        ui.add_space(10.0);
        ui.label(theme::kicker(tr("Moje wpisy")));
        for p in &mine {
            if point_row(ui, p, None).clicked() {
                open = Some(p.clone());
            }
        }
    }
    if let Some(p) = open {
        a.selection = Selection::UserPoint(p.id.clone());
        a.fly_to(p.lon, p.lat, Some(a.cam.distance.clamp(1500.0, 6000.0)));
    }
}

fn point_row(ui: &mut egui::Ui, p: &crate::points::Point, dist: Option<f64>) -> egui::Response {
    let h = if crate::platform::MOBILE { 48.0 } else { 40.0 };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::click());
    theme::focus_ring(ui, &resp, rect, 10.0);
    let painter = ui.painter();
    if resp.hovered() {
        painter.rect_filled(rect, 10.0, Color32::from_white_alpha(14));
    }
    let c = rect.left_center() + egui::vec2(18.0, 0.0);
    painter.circle_filled(c, 13.0, p.category.color());
    painter.text(c, egui::Align2::CENTER_CENTER, p.category.icon(), egui::FontId::proportional(15.0), theme::INK_0);
    let right = match dist {
        Some(d) => fmt::dist(d),
        None => p.status.label().to_string(),
    };
    let rg = painter.layout_no_wrap(right, egui::FontId::proportional(12.0), theme::paper_3());
    let rw = rg.size().x;
    painter.galley(egui::pos2(rect.right() - rw - 8.0, rect.center().y - rg.size().y / 2.0), rg, theme::paper_3());
    let tw = rect.width() - 50.0 - rw - 16.0;
    let title = egui::text::LayoutJob::simple_singleline(p.title.clone(), egui::FontId::proportional(14.5), theme::PAPER);
    let mut job = title;
    job.wrap = egui::text::TextWrapping::truncate_at_width(tw.max(40.0));
    let g = ui.fonts(|f| f.layout_job(job));
    painter.galley(egui::pos2(rect.left() + 40.0, rect.top() + 4.0), g, theme::PAPER);
    let sub = format!("{} · {}", p.category.label(), tf!("komentarze: {}", p.comments.len()));
    painter.text(egui::pos2(rect.left() + 40.0, rect.bottom() - 5.0), egui::Align2::LEFT_BOTTOM, sub, egui::FontId::proportional(11.5), theme::paper_3());
    resp
}

/// Formularz nowego punktu.
fn draft_form(a: &mut Atlas, ui: &mut egui::Ui) {
    use crate::i18n::tr;
    let Some(mut d) = a.point_draft.clone() else { return };
    ui.label(theme::kicker(tr("Nowy punkt")));
    ui.label(RichText::new(format!("{}  ·  {}", fmt::coords(d.lon, d.lat), fmt::ele(d.ele as f64))).small().monospace().color(theme::paper_3()));
    theme::para(ui, if crate::platform::MOBILE { tr("Dotknij mapy albo przeciągnij szpilkę, żeby przesunąć punkt.") } else { tr("Kliknij na mapie albo przeciągnij szpilkę, żeby przesunąć punkt.") });
    ui.add_space(6.0);
    ui.label(theme::kicker(tr("Rodzaj")));
    let cols = if ui.available_width() > 360.0 { 4 } else { 2 };
    theme::tile_grid(ui, cols, |ui, w| {
        for c in Category::ALL {
            if theme::tile(ui, c.icon(), c.label(), d.category == c, w).clicked() {
                d.category = c;
            }
        }
    });
    ui.add_space(8.0);
    ui.label(theme::kicker(tr("Tytuł")));
    text_field(ui, &mut d.title, "np. Źródło przy szlaku, zwalone drzewa", false);
    ui.label(theme::kicker(tr("Komentarz")));
    text_field(ui, &mut d.text, "Co warto wiedzieć o tym miejscu? Kiedy to sprawdzałeś?", true);
    ui.label(theme::kicker(tr("Podpis (opcjonalnie)")));
    text_field(ui, &mut d.author, "Imię lub pseudonim", false);
    ui.add_space(4.0);
    theme::para(ui, tr("Wpis trafi na redzikowski.com i po zatwierdzeniu zobaczą go inni użytkownicy GRANI. Nie wpisuj danych osobowych ani numerów telefonów."));
    let ok = d.title.trim().chars().count() >= 3 && d.text.trim().chars().count() >= 3;
    ui.add_space(6.0);
    let mut send = false;
    let mut cancel = false;
    ui.horizontal(|ui| {
        send = ui.add_enabled(ok, theme::primary(&theme::ic(theme::icon::PAPER_PLANE_RIGHT, "Wyślij"))).on_disabled_hover_text(tr("Wpisz tytuł i komentarz (co najmniej 3 znaki).")).clicked();
        cancel = ui.button(tr("Anuluj")).clicked();
    });
    if send {
        a.points.add_point(d.lon, d.lat, Some(d.ele), d.category, &d.title, &d.text, &d.author);
        a.point_draft = None;
        a.notify(if crate::offline::is_offline() {
            tr("Zapisano – wpis wyśle się po połączeniu z siecią.")
        } else {
            tr("Dziękujemy! Wpis pojawi się dla innych po zatwierdzeniu.")
        });
    } else if cancel {
        a.point_draft = None;
    } else {
        a.point_draft = Some(d);
    }
    points_status(a, ui);
}

/// Szczegóły punktu społeczności: opis, komentarze i odpowiedź.
pub fn user_point_info(a: &mut Atlas, ui: &mut egui::Ui, id: &str) {
    use crate::i18n::tr;
    let Some(p) = a.points.get(id) else {
        theme::para(ui, tr("Ten punkt nie jest już dostępny."));
        return;
    };
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(34.0, 34.0), egui::Sense::hover());
        ui.painter().circle_filled(r.center(), 16.0, p.category.color());
        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, p.category.icon(), egui::FontId::proportional(18.0), theme::INK_0);
        ui.vertical(|ui| {
            ui.add(egui::Label::new(RichText::new(&p.title).size(19.0).strong()).wrap());
            let who = if p.author.is_empty() { tr("anonim").to_string() } else { p.author.clone() };
            ui.label(RichText::new(format!("{} · {} · {}", p.category.label(), who, p.date())).small().color(theme::paper_3()));
        });
    });
    if p.mine && p.status != Status::Approved {
        ui.label(RichText::new(theme::ic(theme::icon::CLOCK, p.status.label())).small().color(theme::WARN));
    }
    let ele = p.ele.or_else(|| a.heights.sample_cached(p.lon, p.lat)).unwrap_or(0.0);
    ui.label(RichText::new(format!("{}  ·  {}", fmt::coords(p.lon, p.lat), fmt::ele(ele as f64))).small().monospace().color(theme::paper_3()));
    ui.add_space(6.0);
    let w = ((ui.available_width() - 12.0) / 4.0).floor();
    let mut report = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if theme::action(ui, theme::icon::PATH, "Trasa tutaj", w).clicked() {
            a.tool = Tool::Route;
            a.add_waypoint(p.lon, p.lat, None);
        }
        if theme::action(ui, theme::icon::CROSSHAIR, "Pokaż", w).clicked() {
            a.fly_to(p.lon, p.lat, Some(a.cam.distance.clamp(1200.0, 5000.0)));
        }
        if theme::action(ui, theme::icon::COPY, "Współrzędne", w).clicked() {
            ui.ctx().copy_text(fmt::coords(p.lon, p.lat));
            a.notify(tr("Skopiowano współrzędne."));
        }
        if !p.mine && theme::action(ui, theme::icon::FLAG_PENNANT, "Zgłoś", w).clicked() {
            report = true;
        }
    });
    if report {
        let id = p.id.clone();
        let ask = crate::ui::dialog::Ask::new(tr("Zgłoś punkt"), tr("Zgłosić ten punkt do sprawdzenia? Moderator zobaczy go w panelu redzikowski.com (np. nieprawdziwy, nieaktualny albo obraźliwy wpis)."), &["Zgłoś", "Anuluj"]);
        a.ask(ask, move |a| {
            let pts = a.points.clone();
            std::thread::spawn(move || {
                if let Err(e) = pts.report(&id, "app") {
                    log::warn!("zgłoszenie punktu: {e}");
                }
            });
            a.notify(tr("Dziękujemy – moderator sprawdzi ten wpis."));
        });
    }
    ui.add_space(10.0);
    ui.label(theme::kicker(tf!("Komentarze ({})", p.comments.len())));
    for c in &p.comments {
        theme::card(c.official).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let who = if c.official { "GRAŃ".to_string() } else if c.author.is_empty() { tr("anonim").to_string() } else { c.author.clone() };
                ui.label(RichText::new(who).strong().color(if c.official { theme::BRASS } else { theme::PAPER }));
                ui.label(RichText::new(c.created_at.get(..10).unwrap_or("")).small().color(theme::paper_3()));
                if c.mine && c.status != Status::Approved {
                    ui.label(RichText::new(c.status.label()).small().color(theme::WARN));
                }
            });
            ui.add(egui::Label::new(RichText::new(&c.text).color(theme::paper_2())).wrap());
        });
        ui.add_space(4.0);
    }
    // nowy komentarz
    ui.add_space(6.0);
    let key = egui::Id::new(("komentarz", id));
    let mut draft: (String, String) = ui.data(|d| d.get_temp(key)).unwrap_or_else(|| (String::new(), a.points.author()));
    ui.label(theme::kicker(tr("Dodaj komentarz")));
    text_field(ui, &mut draft.0, "Aktualne informacje, odpowiedź, uwagi", true);
    text_field(ui, &mut draft.1, "Podpis (opcjonalnie)", false);
    let ok = draft.0.trim().chars().count() >= 3;
    if ui.add_enabled(ok, theme::primary(&theme::ic(theme::icon::PAPER_PLANE_RIGHT, "Wyślij komentarz"))).clicked() {
        a.points.add_comment(&p, &draft.0, &draft.1);
        draft.0.clear();
        a.notify(tr("Dziękujemy! Komentarz pojawi się dla innych po zatwierdzeniu."));
    }
    ui.data_mut(|d| d.insert_temp(key, draft));
    points_status(a, ui);
    if p.mine && p.status != Status::Approved {
        ui.add_space(8.0);
        if ui.button(theme::ic(theme::icon::TRASH, "Usuń mój wpis z tego urządzenia")).clicked() {
            a.points.forget(&p.id);
            a.selection = Selection::None;
        }
    }
}

/// Narzędzie „Mapy offline”: wybór obszaru, szczegółowości i lista pobranych obszarów.
pub fn area_tool_panel(a: &mut Atlas, ui: &mut egui::Ui) {
    use crate::i18n::tr;
    let mut d = a.area_draft.clone().unwrap_or_default();
    ui.label(theme::kicker(tr("Nowy obszar")));
    let corners = (d.a, d.b);
    if !matches!(corners, (Some(_), Some(_))) {
        theme::para(ui, if crate::platform::MOBILE {
            tr("Dotknij mapy w dwóch przeciwległych narożnikach obszaru albo weź obszar z bieżącego widoku.")
        } else {
            tr("Kliknij na mapie dwa przeciwległe narożniki obszaru albo weź obszar z bieżącego widoku.")
        });
    }
    if ui.button(theme::ic(theme::icon::FRAME_CORNERS, "Obszar z bieżącego widoku")).clicked() {
        let (w, s, e, n) = a.view_bbox();
        // środkowa część kadru – to, co faktycznie widać
        let (cx, cy, hw, hh) = ((w + e) / 2.0, (s + n) / 2.0, (e - w) / 5.0, (n - s) / 5.0);
        d.a = Some((cx - hw, cy - hh));
        d.b = Some((cx + hw, cy + hh));
    }
    if let (Some(p), Some(q)) = (d.a, d.b) {
        let area = Area {
            id: 0,
            name: String::new(),
            bbox: [p.0.min(q.0), p.1.min(q.1), p.0.max(q.0), p.1.max(q.1)],
            detail: d.detail,
            ortho: d.ortho,
            created: 0,
            total: 0,
            done: 0,
            failed: 0,
            complete: false,
        };
        let bb = area.bbox();
        ui.label(RichText::new(format!("{} · {:.0} km²", bb.label(), bb.km2())).small().color(theme::paper_3()));
        ui.add_space(6.0);
        ui.label(theme::kicker(tr("Szczegółowość")));
        for det in Detail::ALL {
            let mut probe = area.clone();
            probe.detail = det;
            let (_, size) = probe.estimate();
            if theme::option_card(ui, &format!("{} · ~{}", det.label(), crate::store::fmt_bytes(size)), det.desc(), d.detail == det).clicked() {
                d.detail = det;
            }
        }
        theme::switch(ui, &mut d.ortho, tr("Także ortofoto krajowe (więcej miejsca)"));
        let (tiles, size) = area.estimate();
        let free = crate::store::free_space(a.heights.store().root());
        ui.add_space(4.0);
        ui.label(RichText::new(tf!("{} kafli · ok. {}", tiles, crate::store::fmt_bytes(size))).strong());
        if let Some(f) = free {
            ui.label(RichText::new(tf!("Wolne miejsce: {}", crate::store::fmt_bytes(f))).small().color(theme::paper_3()));
        }
        let too_big = tiles > crate::offline::MAX_TILES;
        let no_space = free.is_some_and(|f| f < size + (512 << 20));
        if too_big {
            ui.label(RichText::new(tr("Obszar jest za duży – zmniejsz go albo wybierz mniejszą szczegółowość.")).small().color(theme::SIGNAL));
        } else if no_space {
            ui.label(RichText::new(tr("Za mało wolnego miejsca na dysku.")).small().color(theme::SIGNAL));
        }
        theme::para(ui, tr("Pobrane obszary nie są usuwane przy sprzątaniu do limitu danych. Wliczają się jednak do zajętego miejsca."));
        ui.label(theme::kicker(tr("Nazwa")));
        if d.name.is_empty() {
            let c = bb.center();
            d.name = a.name_near(c[0], c[1], 8000.0).unwrap_or_else(|| tr("Obszar offline").to_string());
        }
        text_field(ui, &mut d.name, "Nazwa obszaru", false);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.add_enabled(!too_big && !no_space, theme::primary(&theme::ic(theme::icon::DOWNLOAD_SIMPLE, "Pobierz"))).clicked() {
                let mut ar = area.clone();
                ar.name = d.name.trim().to_string();
                if crate::offline::is_offline() {
                    a.set_offline(false);
                }
                a.offline.add(ar);
                d = AreaDraft::default();
                a.notify(tr("Pobieranie obszaru rozpoczęte – postęp w liście poniżej."));
            }
            if ui.button(tr("Wyczyść")).clicked() {
                d = AreaDraft::default();
            }
        });
    }
    a.area_draft = Some(d);
    ui.add_space(14.0);
    offline_areas(a, ui);
}

/// Pobrane obszary, postęp pobierania i przełącznik trybu offline (narzędzie i ustawienia).
pub fn offline_areas(a: &mut Atlas, ui: &mut egui::Ui) {
    use crate::i18n::tr;
    let mut off = a.prefs.offline_mode;
    if theme::switch(ui, &mut off, tr("Tryb offline – tylko zapisane dane")).changed() {
        a.set_offline(off);
    }
    theme::para(ui, tr("W trybie offline GRAŃ nie łączy się z internetem: mapa, szlaki i nazwy pochodzą z dysku, a komentarze czekają w kolejce."));
    ui.add_space(8.0);
    ui.label(theme::kicker(tr("Pobrane obszary")));
    let job = a.offline.job();
    let areas = a.offline.areas();
    if areas.is_empty() {
        theme::para(ui, tr("Nie masz jeszcze obszarów offline. Wybierz narzędzie „Mapy offline” i zaznacz prostokąt na mapie."));
    }
    let mut act: Option<(u8, Area)> = None;
    for ar in &areas {
        theme::card(false).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (glyph, color) = if ar.complete { (theme::icon::CHECK_CIRCLE, theme::OK) } else { (theme::icon::CLOUD_ARROW_DOWN, theme::WARN) };
                ui.label(RichText::new(glyph).size(18.0).color(color));
                ui.label(RichText::new(&ar.name).strong());
            });
            ui.label(RichText::new(format!("{} · {}", ar.detail.label(), ar.bbox().label())).small().color(theme::paper_3()));
            match &job {
                Some(j) if j.area == ar.id => {
                    let (done, total) = (j.done.load(std::sync::atomic::Ordering::Relaxed), j.total.load(std::sync::atomic::Ordering::Relaxed).max(1));
                    let phase = j.phase.load(std::sync::atomic::Ordering::Relaxed);
                    ui.label(RichText::new(if phase == 0 { tr("Szlaki, nazwy, drogi i lasy…").to_string() } else { tf!("{} z {} kafli", done, total) }).small().color(theme::BRASS));
                    theme::bar(ui, if phase == 0 { 0.02 } else { done as f32 / total as f32 }, theme::BRASS);
                    let failed = j.failed.load(std::sync::atomic::Ordering::Relaxed);
                    if failed > 0 {
                        ui.label(RichText::new(tf!("nieudane: {} (uzupełnisz później)", failed)).small().color(theme::WARN));
                    }
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(400));
                }
                _ if a.offline.queued(ar.id) => {
                    ui.label(RichText::new(tr("w kolejce")).small().color(theme::paper_3()));
                }
                _ if !ar.complete => {
                    ui.label(RichText::new(tf!("niepełny: {} z {} kafli", ar.done, ar.total)).small().color(theme::WARN));
                }
                _ => {}
            }
            ui.horizontal(|ui| {
                if ui.small_button(theme::ic(theme::icon::CROSSHAIR, "Pokaż")).clicked() {
                    act = Some((0, ar.clone()));
                }
                let busy = job.as_ref().is_some_and(|j| j.area == ar.id);
                if busy {
                    if ui.small_button(theme::ic(theme::icon::STOP, "Przerwij")).clicked() {
                        act = Some((3, ar.clone()));
                    }
                } else if !ar.complete && ui.small_button(theme::ic(theme::icon::ARROWS_CLOCKWISE, "Wznów")).clicked() {
                    act = Some((1, ar.clone()));
                } else if ar.complete && ui.small_button(theme::ic(theme::icon::ARROWS_CLOCKWISE, "Odśwież")).on_hover_text(tr("Uzupełnia brakujące dane (np. po zmianie źródeł)")).clicked() {
                    act = Some((1, ar.clone()));
                }
                if ui.small_button(theme::ic(theme::icon::TRASH, "Usuń")).clicked() {
                    act = Some((2, ar.clone()));
                }
            });
        });
        ui.add_space(4.0);
    }
    match act {
        Some((0, ar)) => {
            let bb = ar.bbox();
            let c = bb.center();
            let span = ((bb.e - bb.w) * 111_320.0 * c[1].to_radians().cos()).max((bb.n - bb.s) * 110_574.0);
            a.fly_to(c[0], c[1], Some((span * 1.3).clamp(1500.0, 400_000.0)));
            a.tool = Tool::Area;
        }
        Some((1, ar)) => {
            if crate::offline::is_offline() {
                a.set_offline(false);
            }
            a.offline.download(ar.id);
        }
        Some((2, ar)) => {
            let ask = crate::ui::dialog::Ask::new(tr("Mapy offline"), tf!("Usunąć obszar „{}”? Jego dane przestaną być chronione i zostaną usunięte przy sprzątaniu do limitu.", ar.name), &["Usuń", "Anuluj"]).danger();
            a.ask(ask, move |a| a.offline.remove(ar.id));
        }
        Some((3, _)) => a.offline.cancel(),
        _ => {}
    }
}
