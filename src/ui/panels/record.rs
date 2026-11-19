//! Położenie i nagrywanie przejścia: pasek nagrywania nad mapą i zakładka panelu (statystyki na
//! żywo, przerwa, koniec, ustawienia, ostatnie przejścia).

use super::*;
use crate::recording::{clock, kmh, pace};
use crate::ui::atlas::Follow;

/// Pasek nagrywania pod górnym paskiem: kropka, czas, dystans, podejścia. Klik – zakładka nagrywania.
pub fn record_hud(a: &mut Atlas, ctx: &egui::Context) {
    let Some(r) = &a.rec else { return };
    if sheet_covers(a) {
        return;
    }
    let paused = r.paused();
    let (time, dist, up) = (clock(r.elapsed()), fmt::dist(r.stats.dist), fmt::ele(r.stats.up));
    let waiting = r.points.is_empty() && !a.gps.fresh();
    let id = egui::Id::new("nagrywanie-pasek");
    let pos = egui::pos2(ctx.screen_rect().left() + 12.0, ctx.screen_rect().top() + a.safe_top + TOP_BAR_H + 10.0);
    let mut open = false;
    egui::Area::new(id).fixed_pos(pos).order(egui::Order::Middle).show(ctx, |ui| {
        let inner = theme::glass_float().corner_radius(egui::CornerRadius::same(14)).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                // pulsująca kropka nagrywania, bursztynowa w przerwie
                let t = ui.input(|i| i.time);
                let pulse = if paused { 1.0 } else { 0.55 + 0.45 * ((t * 3.0).sin() as f32 * 0.5 + 0.5) };
                let (dot, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                let color = if paused { theme::WARN } else { theme::SIGNAL };
                ui.painter().circle_filled(dot.center(), 5.5, color.gamma_multiply(pulse));
                ui.label(RichText::new(time).size(16.0).strong().color(theme::PAPER));
                if waiting {
                    ui.label(RichText::new(crate::i18n::tr("szukam GPS…")).size(13.0).color(theme::paper_3()));
                } else {
                    ui.label(RichText::new(dist).size(14.0).color(theme::paper_2()));
                    ui.label(RichText::new(format!("↑ {up}")).size(14.0).color(theme::paper_2()));
                }
                if paused {
                    ui.label(RichText::new(crate::i18n::tr("przerwa")).size(13.0).color(theme::WARN));
                }
            });
        });
        let resp = inner.response.interact(egui::Sense::click());
        if resp.on_hover_text(crate::i18n::tr("Szczegóły nagrywania")).clicked() {
            open = true;
        }
    });
    glass_float_area(a, ctx, id, 1.0);
    if open {
        a.open_record_panel();
    }
    if !paused {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

/// Zakładka panelu: położenie, nagrywanie i ustawienia.
pub fn record_panel(a: &mut Atlas, ui: &mut egui::Ui) {
    use crate::i18n::tr;
    if !crate::platform::LOCATION {
        theme::para(ui, tr("Położenie GPS i nagrywanie przejść działają w aplikacji na telefon (Android, iPhone). Nagrane tam przejścia, a także pliki z zegarka (FIT, GPX), obejrzysz i wyeksportujesz w bibliotece."));
        if ui.button(theme::ic(theme::icon::BOOKMARKS, tr("Otwórz bibliotekę"))).clicked() {
            a.tab = Tab::Library;
        }
        return;
    }
    // stan odbiornika
    let gps_line = match (a.gps.on, a.gps.fix, a.gps.fresh()) {
        (false, _, _) => (theme::paper_3(), tr("Położenie wyłączone").to_string()),
        (true, None, _) => (theme::WARN, tr("Szukam sygnału GPS… Na zewnątrz, z widokiem na niebo, zwykle trwa to kilkanaście sekund.").to_string()),
        (true, Some(_), false) => (theme::WARN, tr("Brak świeżego odczytu – sprawdź, czy lokalizacja w telefonie jest włączona.").to_string()),
        (true, Some(f), true) => (theme::OK, tf!("Sygnał GPS: dokładność ± {} m", f.acc.round() as i64)),
    };
    pill(ui, &gps_line.1, gps_line.0);
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        let (label, on) = match (a.gps.on, a.gps.follow) {
            (false, _) => (tr("Pokaż moje położenie"), false),
            (true, Follow::Free) => (tr("Wróć do położenia"), false),
            (true, Follow::Follow) => (tr("Mapa za mną · kompas"), true),
            (true, Follow::Compass) => (tr("Wyłącz podążanie"), true),
        };
        if ui.add(egui::Button::new(theme::ic(theme::icon::CROSSHAIR, label)).selected(on).min_size(egui::vec2(0.0, 36.0))).clicked() {
            a.toggle_locate();
        }
    });

    match &a.rec {
        Some(r) => {
            let paused = r.paused();
            let s = r.stats;
            let elapsed = r.elapsed();
            let ele_now = r.points.last().map(|p| p.ele as f64);
            let speed_now = a.gps.fix.filter(|_| a.gps.fresh()).and_then(|f| f.speed).map(|v| v as f64);
            let acc = a.gps.fix.map(|f| f.acc);
            let profile: Vec<f32> = r.points.iter().map(|p| p.ele).collect();
            let name = r.state.name.clone();
            let progress = a.route_progress();
            section(ui, theme::icon::RECORD, if paused { "Nagrywanie wstrzymane" } else { "Nagrywanie" });
            ui.label(RichText::new(&name).small().color(theme::paper_3()));
            big(ui, clock(elapsed), tr("czas nagrania (bez przerw)"), if paused { theme::WARN } else { theme::PAPER });
            let mut items: Vec<(&str, String)> = vec![
                ("Dystans", fmt::dist(s.dist)),
                ("Czas ruchu", clock(s.moving)),
                ("Podejścia", fmt::ele(s.up)),
                ("Zejścia", fmt::ele(s.down)),
                ("Wysokość", ele_now.map(fmt::ele).unwrap_or_else(|| "—".into())),
                ("Prędkość", speed_now.map(kmh).unwrap_or_else(|| "—".into())),
                ("Śr. prędkość", kmh(s.avg_speed())),
                ("Tempo", pace(s.avg_speed())),
                ("Maks. prędkość", kmh(s.max_speed)),
                ("Dokładność GPS", acc.map(|v| format!("± {} m", v.round() as i64)).unwrap_or_else(|| "—".into())),
            ];
            if let Some((left, off)) = progress {
                items.push(("Do celu", fmt::dist(left)));
                items.push(("Od trasy", fmt::dist(off)));
            }
            grid2(ui, items);
            if let Some((_, off)) = progress {
                if off > 80.0 {
                    pill(ui, &tf!("Jesteś {} od trasy na mapie", fmt::dist(off)), theme::WARN);
                }
            }
            if profile.len() >= 2 {
                ui.add_space(6.0);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 54.0), egui::Sense::hover());
                spark(&ui.painter().with_clip_rect(rect), rect, &profile, theme::SIGNAL);
            }
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                let (glyph, label) = if paused { (theme::icon::PLAY, tr("Wznów")) } else { (theme::icon::PAUSE, tr("Przerwa")) };
                if ui.add(egui::Button::new(theme::ic(glyph, label)).min_size(egui::vec2(0.0, 38.0))).clicked() {
                    a.pause_recording(!paused);
                }
                if ui.add(theme::primary(&theme::ic(theme::icon::STOP, tr("Zakończ"))).min_size(egui::vec2(0.0, 38.0))).clicked() {
                    a.stop_recording();
                }
            });
            ui.add_space(4.0);
            theme::para(ui, tr("Nagrywanie trwa także przy wyłączonym ekranie i w tle. Dystans i podejścia liczę z modelu terenu, więc są dokładniejsze niż z samego GPS; postoje nie wliczają się do czasu ruchu."));
        }
        None => {
            section(ui, theme::icon::RECORD, "Nagrywanie przejścia");
            theme::para(ui, tr("Zapisz swoje przejście: ślad, czas, dystans, podejścia i prędkość. Nagranie działa przy wyłączonym ekranie, a po zakończeniu trafia do biblioteki – stamtąd wyślesz je do zegarka i serwisów (FIT, GPX)."));
            if ui.add(theme::primary(&theme::ic(theme::icon::RECORD, tr("Rozpocznij nagrywanie"))).min_size(egui::vec2(0.0, 40.0))).clicked() {
                a.start_recording();
            }
        }
    }

    // ostatnie przejścia
    if !a.library.recordings.is_empty() {
        section(ui, theme::icon::PATH, "Ostatnie przejścia");
        let mut list: Vec<(u64, String, String)> = a
            .library
            .recordings
            .iter()
            .map(|m| (m.id, m.name.clone(), format!("{} · ↑ {} · {}", fmt::dist(m.stats.dist), fmt::ele(m.stats.up), clock(m.stats.duration))))
            .collect();
        list.sort_by(|x, y| y.0.cmp(&x.0));
        for (id, name, sub) in list.into_iter().take(3) {
            if row(ui, &name, "", &sub) {
                a.open_recording(id);
            }
        }
        if ui.button(theme::ic(theme::icon::BOOKMARKS, tr("Wszystkie w bibliotece"))).clicked() {
            a.tab = Tab::Library;
        }
    }

    // ustawienia
    if fold(ui, "nagrywanie-ustawienia", theme::icon::GEAR, "Ustawienia nagrywania", false) {
        ui.label(RichText::new(tr("Pomijaj odczyty mniej dokładne niż")).small().color(theme::paper_3()));
        let mut acc = a.prefs.gps_max_acc;
        if ui.add(egui::Slider::new(&mut acc, 10.0..=60.0).step_by(5.0).suffix(" m")).changed() {
            a.prefs.gps_max_acc = acc;
        }
        theme::para(ui, tr("Mniejsza wartość – czystszy ślad, ale w lesie i w wąwozach mogą zostać luki. Zmiana działa od następnego nagrania."));
        ui.add_space(4.0);
        if ui.button(theme::ic(theme::icon::GEAR, tr("Uprawnienia GRANI w systemie"))).clicked() {
            crate::platform::location_settings();
        }
        if !crate::platform::IOS {
            theme::para(ui, tr("Telefony Huawei, Xiaomi, Samsung i inne potrafią usypiać aplikacje w tle. Jeśli nagranie ma przerwy przy wyłączonym ekranie, w ustawieniach baterii ustaw GRANI „bez ograniczeń” / „zarządzaj ręcznie – działanie w tle”."));
        }
    }
}
