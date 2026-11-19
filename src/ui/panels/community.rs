//! Panele punktów społeczności (komentarze i zdjęcia dla innych turystów, konto) i map offline.

use std::path::PathBuf;

use super::*;
use crate::account::Stage;
use crate::offline::{Area, Detail};
use crate::points::{Category, Photo, Status};
use crate::ui::atlas::{AccountForm, AccountMode, AreaDraft};

/// Klucz egui: zdjęcie otwarte w podglądzie (źródło dużego zdjęcia, podpis).
const VIEWER: &str = "podglad-zdjecia";

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
    account_card(a, ui);
    ui.add_space(8.0);
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
    let g = ui.painter().layout_job(job);
    painter.galley(egui::pos2(rect.left() + 40.0, rect.top() + 4.0), g, theme::PAPER);
    let score = if p.up + p.down > 0 { format!(" · {:+}", p.score()) } else { String::new() };
    let photos = if p.photos.is_empty() { String::new() } else { format!(" · {} {}", theme::icon::IMAGE, p.photos.len()) };
    let sub = format!("{} · {}{}{}", p.category.label(), tf!("komentarze: {}", p.comments.len()), photos, score);
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
    let cols = if ui.available_width() > 420.0 { 4 } else { 3 };
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
    let (limit, _) = a.points.account().photo_limit();
    if limit > 0 {
        ui.label(theme::kicker(&tf!("Zdjęcia (do {})", limit)));
        photo_picker(a, ui, &mut d.photos, limit as usize);
    }
    ui.add_space(4.0);
    theme::para(ui, tr("Wpis trafi na redzikowski.com i po zatwierdzeniu zobaczą go inni użytkownicy GRANI razem z Twoją nazwą. Nie wpisuj danych osobowych ani numerów telefonów, nie dodawaj zdjęć z rozpoznawalnymi osobami."));
    // podobny punkt w pobliżu: propozycja dołączenia zamiast nowego punktu
    let radius = a.points.account().merge_radius();
    let near = a.points.nearby(d.lon, d.lat, radius);
    let mut join: Option<crate::points::Point> = None;
    if let Some((dist, p)) = near.first() {
        ui.add_space(4.0);
        theme::card(true).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(theme::ic(theme::icon::ARROWS_MERGE, "W pobliżu jest już podobny punkt")).strong());
            theme::para(ui, tf!("„{}” ({}, {}) – zamiast tworzyć nowy punkt możesz dodać swój komentarz i zdjęcia do niego. Dzięki temu informacje o jednym miejscu są razem.", p.title, p.category.label(), fmt::dist(*dist)));
            if near.len() > 1 {
                ui.label(RichText::new(tf!("Punktów w promieniu {} m: {}", radius.round(), near.len())).small().color(theme::paper_3()));
            }
            ui.horizontal_wrapped(|ui| {
                if ui.add(theme::primary(&theme::ic(theme::icon::ARROWS_MERGE, "Dołącz do tego punktu"))).clicked() {
                    join = Some(p.clone());
                }
                if ui.button(theme::ic(theme::icon::CROSSHAIR, "Pokaż")).clicked() {
                    a.fly_to(p.lon, p.lat, Some(a.cam.distance.clamp(800.0, 4000.0)));
                }
            });
        });
    }
    let ok = d.title.trim().chars().count() >= 3 && d.text.trim().chars().count() >= 3;
    let logged = a.points.account().logged_in();
    ui.add_space(6.0);
    let mut send = false;
    let mut cancel = false;
    ui.horizontal_wrapped(|ui| {
        if logged {
            send = ui.add_enabled(ok, theme::primary(&theme::ic(theme::icon::PAPER_PLANE_RIGHT, if near.is_empty() { "Wyślij" } else { "Utwórz nowy punkt" }))).on_disabled_hover_text(tr("Wpisz tytuł i komentarz (co najmniej 3 znaki).")).clicked();
        } else if ui.add(theme::primary(&theme::ic(theme::icon::SIGN_IN, "Zaloguj się, żeby wysłać"))).clicked() {
            open_account(a, AccountMode::Login);
        }
        cancel = ui.button(tr("Anuluj")).clicked();
    });
    if !logged {
        ui.label(RichText::new(tr("Szkic zostanie tutaj – po zalogowaniu wyślesz go jednym przyciskiem.")).small().color(theme::paper_3()));
    }
    if let Some(p) = join {
        // tekst i zdjęcia szkicu przechodzą do formularza komentarza wybranego punktu
        let text = if d.text.trim().is_empty() { d.title.clone() } else { d.text.clone() };
        let key = egui::Id::new(("komentarz", p.id.clone()));
        ui.data_mut(|m| m.insert_temp(key, (text, d.photos.clone())));
        a.point_draft = None;
        a.selection = Selection::UserPoint(p.id.clone());
        a.fly_to(p.lon, p.lat, Some(a.cam.distance.clamp(800.0, 4000.0)));
        a.notify(tr("Szkic przeniesiony do komentarza – sprawdź go i wyślij."));
    } else if send {
        a.points.add_point(d.lon, d.lat, Some(d.ele), d.category, &d.title, &d.text, &d.photos);
        a.point_draft = None;
        a.notify(if crate::offline::is_offline() {
            tr("Zapisano – wpis wyśle się po połączeniu z siecią.")
        } else {
            tr("Dziękujemy! Wpis pojawi się dla innych po zatwierdzeniu.")
        });
    } else if cancel {
        for f in &d.photos {
            let _ = std::fs::remove_file(f);
        }
        a.point_draft = None;
    } else {
        a.point_draft = Some(d);
    }
    points_status(a, ui);
}

/// Konto: kto jest zalogowany albo zachęta do logowania (dodawanie wymaga konta).
fn account_card(a: &mut Atlas, ui: &mut egui::Ui) {
    use crate::i18n::tr;
    let acc = a.points.account().clone();
    ui.label(theme::kicker(tr("Konto")));
    theme::card(false).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if acc.logged_in() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(theme::icon::USER_CIRCLE).size(22.0).color(theme::BRASS));
                ui.vertical(|ui| {
                    ui.label(RichText::new(acc.name()).strong());
                    ui.label(RichText::new(acc.email()).small().color(theme::paper_3()));
                });
            });
            ui.horizontal_wrapped(|ui| {
                if ui.small_button(theme::ic(theme::icon::SIGN_OUT, "Wyloguj")).clicked() {
                    acc.logout();
                    a.notify(tr("Wylogowano. Nowe wpisy poczekają na ponowne zalogowanie."));
                }
                if ui.small_button(theme::ic(theme::icon::TRASH, "Usuń konto…")).clicked() {
                    open_account(a, AccountMode::Delete);
                }
                if ui.small_button(theme::ic(theme::icon::SHIELD_CHECK, "Prywatność")).clicked() {
                    a.privacy_open = true;
                }
            });
        } else {
            theme::para(ui, tr("Przeglądanie punktów nie wymaga konta. Żeby dodawać punkty, komentarze, zdjęcia i oceny, załóż darmowe konto – potwierdzisz je linkiem z wiadomości e-mail."));
            if let Some(e) = acc.error() {
                ui.label(RichText::new(e).small().color(theme::WARN));
            }
            ui.horizontal_wrapped(|ui| {
                if ui.add(theme::primary(&theme::ic(theme::icon::SIGN_IN, "Zaloguj się"))).clicked() {
                    open_account(a, AccountMode::Login);
                }
                if ui.button(theme::ic(theme::icon::USER_PLUS, "Załóż konto")).clicked() {
                    open_account(a, AccountMode::Register);
                }
                if ui.small_button(theme::ic(theme::icon::SHIELD_CHECK, "Prywatność")).clicked() {
                    a.privacy_open = true;
                }
            });
        }
    });
}

/// Otwiera okno konta w danym trybie (adres z poprzedniego logowania zostaje).
pub fn open_account(a: &mut Atlas, mode: AccountMode) {
    let acc = a.points.account();
    acc.clear_message();
    let email = a.account_form.as_ref().map(|f| f.email.clone()).filter(|e| !e.is_empty()).unwrap_or_else(|| acc.email());
    a.account_form = Some(AccountForm { mode, email, ..Default::default() });
}

/// Zdjęcia w formularzu: miniatury z usuwaniem i wybór kolejnego (najwyżej `limit`).
fn photo_picker(a: &mut Atlas, ui: &mut egui::Ui, photos: &mut Vec<PathBuf>, limit: usize) {
    use crate::i18n::tr;
    for r in a.points.take_prepared() {
        match r {
            Ok(f) if photos.len() < limit => photos.push(f),
            Ok(f) => {
                let _ = std::fs::remove_file(f);
                a.notify(tf!("Najwyżej {} zdjęcia.", limit));
            }
            Err(e) => a.notify(tf!("Nie udało się dodać zdjęcia: {}", e)),
        }
    }
    let size = if crate::platform::MOBILE { 76.0 } else { 68.0 };
    let mut remove = None;
    ui.horizontal_wrapped(|ui| {
        for (i, f) in photos.iter().enumerate() {
            let src = f.to_string_lossy();
            let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
            paint_photo(a, ui, rect, &src, false);
            let x = egui::Rect::from_center_size(rect.right_top() + egui::vec2(-12.0, 12.0), egui::vec2(22.0, 22.0));
            let r = ui.interact(x, ui.id().with(("usun-zdjecie", i)), egui::Sense::click());
            ui.painter().circle_filled(x.center(), 11.0, Color32::from_black_alpha(190));
            ui.painter().text(x.center(), egui::Align2::CENTER_CENTER, theme::icon::X, egui::FontId::proportional(13.0), theme::PAPER);
            if r.on_hover_text(tr("Usuń zdjęcie")).clicked() {
                remove = Some(i);
            }
        }
        let busy = a.points.preparing();
        for _ in 0..busy {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
            ui.painter().rect_filled(rect, 10.0, theme::INK_2);
            egui::Spinner::new().paint_at(ui, rect.shrink(size * 0.3));
        }
        if photos.len() + busy < limit {
            let b = egui::Button::new(RichText::new(format!("{}\n{}", theme::icon::CAMERA_PLUS, tr("Dodaj"))).size(13.0)).min_size(egui::vec2(size, size));
            if ui.add(b).on_hover_text(tr("Dodaj zdjęcie (zmniejszone do 2048 px, bez danych o położeniu)")).clicked() {
                crate::platform::pick_photo();
            }
        }
    });
    if let Some(i) = remove {
        let f = photos.remove(i);
        let _ = std::fs::remove_file(f);
    }
}

/// Zdjęcie w prostokącie (przycięte do wypełnienia); w trakcie wczytywania – tło z ikoną.
fn paint_photo(a: &Atlas, ui: &egui::Ui, rect: egui::Rect, source: &str, large: bool) {
    let painter = ui.painter();
    match a.points.textures().get(ui.ctx(), source, large) {
        Some(t) => {
            let [w, h] = t.size();
            let (iw, ih) = (w as f32, h as f32);
            let (rw, rh) = (rect.width(), rect.height());
            // „cover”: wycinek obrazu o proporcjach prostokąta
            let (u, v) = if iw / ih > rw / rh { ((rw / rh) * ih / iw, 1.0) } else { (1.0, (rh / rw) * iw / ih) };
            let uv = egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(u, v));
            let mut mesh = egui::Mesh::with_texture(t.id());
            mesh.add_rect_with_uv(rect, uv, Color32::WHITE);
            painter.rect_filled(rect, 10.0, theme::INK_2);
            painter.add(egui::Shape::mesh(mesh));
        }
        None => {
            painter.rect_filled(rect, 10.0, theme::INK_2);
            let failed = a.points.textures().failed(source, large);
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, if failed { theme::icon::IMAGE_BROKEN } else { theme::icon::IMAGE }, egui::FontId::proportional(20.0), theme::paper_3());
        }
    }
}

/// Zdjęcia przy punkcie: miniatury, dotknięcie otwiera podgląd.
fn photo_gallery(a: &Atlas, ui: &mut egui::Ui, photos: &[Photo]) {
    use crate::i18n::tr;
    let size = egui::vec2(if crate::platform::MOBILE { 104.0 } else { 96.0 }, 78.0);
    ui.horizontal_wrapped(|ui| {
        for (i, f) in photos.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
            paint_photo(a, ui, rect, &f.source(false), false);
            if f.mine && f.status != Status::Approved {
                let tag = egui::Rect::from_min_size(rect.left_bottom() - egui::vec2(0.0, 18.0), egui::vec2(rect.width(), 18.0));
                ui.painter().rect_filled(tag, 0.0, Color32::from_black_alpha(170));
                ui.painter().text(tag.center(), egui::Align2::CENTER_CENTER, f.status.label(), egui::FontId::proportional(10.5), theme::WARN);
            }
            let who = if f.author.is_empty() { tr("anonim").to_string() } else { f.author.clone() };
            if resp.on_hover_text(&who).clicked() {
                ui.data_mut(|d| d.insert_temp(egui::Id::new(VIEWER), Some((f.source(true), who, i))));
            }
        }
    });
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
    // ocena przez innych: +1 / −1 (ponowne kliknięcie cofa ocenę)
    if !p.mine || p.status == Status::Approved {
        let mine = a.points.my_vote(&p.id);
        let mut vote = None;
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let up = egui::Button::new(RichText::new(format!("{}  +1  ·  {}", theme::icon::THUMBS_UP, p.up)).color(if mine == 1 { theme::OK } else { theme::PAPER })).selected(mine == 1).min_size(egui::vec2(0.0, 36.0));
            if ui.add(up).on_hover_text(tr("Wpis jest prawdziwy i przydatny")).clicked() {
                vote = Some(if mine == 1 { 0 } else { 1 });
            }
            let down = egui::Button::new(RichText::new(format!("{}  −1  ·  {}", theme::icon::THUMBS_DOWN, p.down)).color(if mine == -1 { theme::SIGNAL } else { theme::PAPER })).selected(mine == -1).min_size(egui::vec2(0.0, 36.0));
            if ui.add(down).on_hover_text(tr("Wpis jest nieaktualny albo nieprzydatny")).clicked() {
                vote = Some(if mine == -1 { 0 } else { -1 });
            }
            ui.label(RichText::new(tf!("ocena: {}", format!("{:+}", p.score()))).small().color(theme::paper_3()));
        });
        if let Some(v) = vote {
            if !a.points.account().logged_in() {
                open_account(a, AccountMode::Login);
            } else if let Err(e) = a.points.vote(&p.id, v) {
                a.notify(e);
            }
        }
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
    if !p.photos.is_empty() {
        ui.add_space(10.0);
        ui.label(theme::kicker(&tf!("Zdjęcia ({})", p.photos.len())));
        photo_gallery(a, ui, &p.photos);
    }
    ui.add_space(10.0);
    ui.label(theme::kicker(&tf!("Komentarze ({})", p.comments.len())));
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
    let key = egui::Id::new(("komentarz", id.to_string()));
    let mut draft: (String, Vec<PathBuf>) = ui.data(|d| d.get_temp(key)).unwrap_or_default();
    ui.label(theme::kicker(tr("Dodaj komentarz lub zdjęcia")));
    text_field(ui, &mut draft.0, "Aktualne informacje, odpowiedź, uwagi", true);
    let (limit, per_point) = a.points.account().photo_limit();
    let left = if per_point { (limit as usize).saturating_sub(p.my_photos()) } else { limit as usize };
    if left > 0 && !p.id.starts_with("local-") {
        photo_picker(a, ui, &mut draft.1, left);
    } else if limit > 0 && per_point && !p.id.starts_with("local-") {
        ui.label(RichText::new(tf!("Dodałeś już {} zdjęcia do tego punktu (najwięcej).", limit)).small().color(theme::paper_3()));
    }
    let text_ok = draft.0.trim().chars().count() >= 3;
    let ok = text_ok || (draft.0.trim().is_empty() && !draft.1.is_empty());
    if a.points.account().logged_in() {
        let label = if draft.1.is_empty() { "Wyślij komentarz" } else { "Wyślij" };
        if ui.add_enabled(ok, theme::primary(&theme::ic(theme::icon::PAPER_PLANE_RIGHT, label))).on_disabled_hover_text(tr("Wpisz komentarz (co najmniej 3 znaki) albo dodaj zdjęcie.")).clicked() {
            a.points.add_comment(&p, &draft.0, &draft.1);
            draft = Default::default();
            a.notify(tr("Dziękujemy! Wpis pojawi się dla innych po zatwierdzeniu."));
        }
    } else if ui.add(theme::primary(&theme::ic(theme::icon::SIGN_IN, "Zaloguj się, żeby dodać wpis"))).clicked() {
        open_account(a, AccountMode::Login);
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

/// Okna punktów społeczności (wywoływać co klatkę): konto, polityka prywatności, podgląd zdjęcia.
pub fn community_windows(a: &mut Atlas, ctx: &egui::Context) {
    account_window(a, ctx);
    let support = a.points.account().support();
    crate::ui::privacy::window(ctx, &mut a.privacy_open, &support);
    photo_viewer(a, ctx);
}

fn photo_viewer(a: &mut Atlas, ctx: &egui::Context) {
    use crate::i18n::tr;
    let id = egui::Id::new(VIEWER);
    let Some(Some((source, who, _))) = ctx.data(|d| d.get_temp::<Option<(String, String, usize)>>(id)) else { return };
    let mut open = true;
    let screen = ctx.screen_rect();
    egui::Window::new(tf!("Zdjęcie · {}", who))
        .id(egui::Id::new("okno-zdjecia"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .open(&mut open)
        .show(ctx, |ui| {
            let max = egui::vec2(theme::fit(ctx, 1100.0), (screen.height() - 140.0).max(200.0));
            match a.points.textures().get(ctx, &source, true) {
                Some(t) => {
                    let [w, h] = t.size();
                    let k = (max.x / w as f32).min(max.y / h as f32).min(1.0);
                    let size = egui::vec2(w as f32 * k, h as f32 * k);
                    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    let mut mesh = egui::Mesh::with_texture(t.id());
                    mesh.add_rect_with_uv(rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                    ui.painter().add(egui::Shape::mesh(mesh));
                }
                None if a.points.textures().failed(&source, true) => {
                    ui.label(tr("Nie udało się wczytać zdjęcia."));
                }
                None => {
                    ui.set_min_size(egui::vec2(max.x.min(480.0), 240.0));
                    ui.centered_and_justified(|ui| ui.spinner());
                }
            }
        });
    if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        ctx.data_mut(|d| d.insert_temp::<Option<(String, String, usize)>>(id, None));
    }
}

/// Okno konta: logowanie, rejestracja (z akceptacją polityki prywatności), nowe hasło, usunięcie.
fn account_window(a: &mut Atlas, ctx: &egui::Context) {
    use crate::i18n::tr;
    let Some(mut form) = a.account_form.clone() else { return };
    let acc = a.points.account().clone();
    // wynik ostatniej operacji
    match acc.stage() {
        Stage::LoggedIn => {
            acc.clear_message();
            a.account_form = None;
            a.points.flush_now();
            a.notify(tf!("Zalogowano jako {}.", acc.name()));
            return;
        }
        Stage::Deleted => {
            acc.clear_message();
            a.account_form = None;
            a.notify(tr("Konto zostało usunięte."));
            return;
        }
        _ => {}
    }
    let busy = acc.busy();
    let title = match form.mode {
        AccountMode::Login => tr("Zaloguj się"),
        AccountMode::Register => tr("Załóż konto"),
        AccountMode::Forgot => tr("Nowe hasło"),
        AccountMode::Delete => tr("Usuń konto"),
    };
    let mut open = true;
    let mut privacy = false;
    egui::Window::new(title)
        .id(egui::Id::new("okno-konta"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .open(&mut open)
        .show(ctx, |ui| {
            ui.set_width(theme::fit(ctx, 420.0));
            let field = |ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str, secret: bool| {
                ui.label(RichText::new(tr(label)).small().color(theme::paper_3()));
                ui.add(egui::TextEdit::singleline(value).password(secret).hint_text(tr(hint)).desired_width(f32::INFINITY).char_limit(200))
            };
            match acc.stage() {
                Stage::Pending(email) => {
                    ui.label(RichText::new(theme::ic(theme::icon::ENVELOPE_SIMPLE, "Sprawdź skrzynkę")).strong().size(16.0));
                    theme::para(ui, tf!("Wysłaliśmy wiadomość na {}. Kliknij w niej link aktywacyjny (sprawdź też folder spam), a potem zaloguj się tutaj.", email));
                    ui.horizontal_wrapped(|ui| {
                        if ui.add_enabled(!busy, theme::primary(&theme::ic(theme::icon::SIGN_IN, "Zaloguj się"))).clicked() {
                            acc.clear_message();
                            form.mode = AccountMode::Login;
                            form.email = email.clone();
                        }
                        if ui.add_enabled(!busy, egui::Button::new(tr("Wyślij link ponownie"))).clicked() {
                            acc.resend(ctx, &email);
                        }
                    });
                }
                Stage::ResetSent(email) => {
                    ui.label(RichText::new(theme::ic(theme::icon::ENVELOPE_SIMPLE, "Sprawdź skrzynkę")).strong().size(16.0));
                    theme::para(ui, tf!("Jeśli konto z adresem {} istnieje, wysłaliśmy link do ustawienia nowego hasła (ważny 2 godziny). Po zmianie zaloguj się nowym hasłem.", email));
                    if ui.add(theme::primary(&theme::ic(theme::icon::SIGN_IN, "Zaloguj się"))).clicked() {
                        acc.clear_message();
                        form.mode = AccountMode::Login;
                    }
                }
                _ => match form.mode {
                    AccountMode::Login => {
                        field(ui, "Adres e-mail", &mut form.email, "np. jan@example.com", false);
                        let r = field(ui, "Hasło", &mut form.password, "", true);
                        let ok = crate::account::email_ok(&form.email) && !form.password.is_empty();
                        ui.add_space(6.0);
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if (ui.add_enabled(ok && !busy, theme::primary(&theme::ic(theme::icon::SIGN_IN, "Zaloguj się"))).clicked() || (enter && ok && !busy)) {
                            acc.login(ctx, &form.email, &form.password);
                            form.password.clear();
                        }
                        ui.horizontal_wrapped(|ui| {
                            if ui.link(tr("Nie masz konta? Załóż je")).clicked() {
                                acc.clear_message();
                                form.mode = AccountMode::Register;
                            }
                            if ui.link(tr("Nie pamiętasz hasła?")).clicked() {
                                acc.clear_message();
                                form.mode = AccountMode::Forgot;
                            }
                        });
                    }
                    AccountMode::Register => {
                        field(ui, "Nazwa widoczna przy wpisach", &mut form.name, "imię lub pseudonim", false);
                        field(ui, "Adres e-mail", &mut form.email, "np. jan@example.com", false);
                        field(ui, "Hasło (co najmniej 8 znaków)", &mut form.password, "", true);
                        field(ui, "Powtórz hasło", &mut form.password2, "", true);
                        ui.add_space(4.0);
                        ui.checkbox(&mut form.consent, tr("Mam co najmniej 16 lat i akceptuję politykę prywatności GRANI"));
                        if ui.link(theme::ic(theme::icon::SHIELD_CHECK, "Przeczytaj politykę prywatności")).clicked() {
                            privacy = true;
                        }
                        let problem = if form.name.trim().chars().count() < 2 {
                            Some(tr("Podaj nazwę (co najmniej 2 znaki)."))
                        } else if !crate::account::email_ok(&form.email) {
                            Some(tr("Podaj poprawny adres e-mail."))
                        } else if form.password.chars().count() < 8 {
                            Some(tr("Hasło musi mieć co najmniej 8 znaków."))
                        } else if form.password != form.password2 {
                            Some(tr("Hasła się różnią."))
                        } else if !form.consent {
                            Some(tr("Zaakceptuj politykę prywatności."))
                        } else {
                            None
                        };
                        if let Some(p) = problem {
                            if !form.password.is_empty() || !form.name.is_empty() {
                                ui.label(RichText::new(p).small().color(theme::paper_3()));
                            }
                        }
                        ui.add_space(6.0);
                        if ui.add_enabled(problem.is_none() && !busy, theme::primary(&theme::ic(theme::icon::USER_PLUS, "Załóż konto"))).clicked() {
                            acc.register(ctx, &form.email, &form.password, &form.name);
                            form.password.clear();
                            form.password2.clear();
                        }
                        theme::para(ui, tr("Nazwa jest publiczna – zobaczą ją inni przy Twoich wpisach. Adresu e-mail nikt poza administratorem nie widzi."));
                        if ui.link(tr("Masz już konto? Zaloguj się")).clicked() {
                            acc.clear_message();
                            form.mode = AccountMode::Login;
                        }
                    }
                    AccountMode::Forgot => {
                        theme::para(ui, tr("Podaj adres konta – wyślemy link do ustawienia nowego hasła."));
                        field(ui, "Adres e-mail", &mut form.email, "np. jan@example.com", false);
                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            if ui.add_enabled(crate::account::email_ok(&form.email) && !busy, theme::primary(&theme::ic(theme::icon::ENVELOPE_SIMPLE, "Wyślij link"))).clicked() {
                                acc.forgot(ctx, &form.email);
                            }
                            if ui.link(tr("Wróć do logowania")).clicked() {
                                acc.clear_message();
                                form.mode = AccountMode::Login;
                            }
                        });
                    }
                    AccountMode::Delete => {
                        theme::para(ui, tr("Usunięcie konta jest nieodwracalne. Twoje zdjęcia zostaną usunięte z serwera, a punkty i komentarze zostaną jako anonimowe (bez Twojej nazwy). Wpisy czekające w kolejce na tym urządzeniu nie zostaną wysłane bez ponownego logowania."));
                        field(ui, "Hasło (potwierdzenie)", &mut form.password, "", true);
                        ui.add_space(6.0);
                        let b = egui::Button::new(RichText::new(theme::ic(theme::icon::TRASH, "Usuń konto na zawsze")).color(Color32::WHITE).strong()).fill(theme::SIGNAL).min_size(egui::vec2(110.0, 34.0));
                        if ui.add_enabled(!form.password.is_empty() && !busy && acc.logged_in(), b).clicked() {
                            acc.delete(ctx, &form.password);
                            form.password.clear();
                        }
                    }
                },
            }
            if busy {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(tr("Łączę z redzikowski.com…")).small().color(theme::paper_3()));
                });
            }
            if let Some(e) = acc.error() {
                ui.add_space(4.0);
                ui.label(RichText::new(e).small().color(theme::SIGNAL));
            }
        });
    if privacy {
        a.privacy_open = true;
    }
    a.account_form = if open && !ctx.input(|i| i.key_pressed(egui::Key::Escape)) || privacy { Some(form) } else { None };
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
