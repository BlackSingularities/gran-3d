//! Baner „GRAŃ na Androidzie” na komputerze: przy kilku pierwszych uruchomieniach (po przewodniku)
//! pod górnym paskiem pojawia się karta z kodem QR strony pobierania – wystarczy zeskanować ją
//! aparatem telefonu. Ponownie otworzysz ją z Ustawień → O programie.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use eframe::egui::{self, Color32, CornerRadius, RichText};

use crate::i18n::tr;
use crate::ui::theme;

/// Strona programu w portfolio – na telefonie z Androidem pobiera APK.
pub const URL: &str = "https://redzikowski.com/portfolio/gran-3d";

/// Ile uruchomień z rzędu baner pokazuje się sam (jeśli go nie wyłączono).
pub const SHOWS: u8 = 3;

static OPEN: AtomicBool = AtomicBool::new(false);
/// Baner już policzony w tym uruchomieniu.
static COUNTED: AtomicBool = AtomicBool::new(false);

/// Otwiera baner (Ustawienia → O programie).
pub fn open() {
    OPEN.store(true, Ordering::Relaxed);
}

/// Przy starcie: baner pokazuje się sam przy kilku pierwszych uruchomieniach na komputerze.
pub fn start(shown: u8, off: bool) {
    if !crate::platform::MOBILE && !off && shown < SHOWS {
        OPEN.store(true, Ordering::Relaxed);
    }
}

/// Moduły kodu QR adresu strony (kwadrat `n × n`, `true` – ciemny).
fn modules() -> &'static (usize, Vec<bool>) {
    static QR: OnceLock<(usize, Vec<bool>)> = OnceLock::new();
    QR.get_or_init(|| match qrcodegen::QrCode::encode_text(URL, qrcodegen::QrCodeEcc::Medium) {
        Ok(q) => {
            let n = q.size();
            let cells = (0..n).flat_map(|y| (0..n).map(move |x| (x, y))).map(|(x, y)| q.get_module(x, y)).collect();
            (n as usize, cells)
        }
        Err(_) => (0, Vec::new()),
    })
}

/// Kod QR na białym tle z marginesem (strefa ciszy) – czytelny dla aparatu także w ciemnym motywie.
fn qr(ui: &mut egui::Ui, side: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::same(10), Color32::WHITE);
    let (n, cells) = modules();
    if *n == 0 {
        return resp;
    }
    // całkowita liczba pikseli na moduł – ostre krawędzie bez szwów
    let ppp = ui.ctx().pixels_per_point();
    let quiet = 3.0;
    let cell = (((side - 2.0 * quiet * 4.0) * ppp / *n as f32).floor().max(1.0)) / ppp;
    let size = cell * *n as f32;
    let o = rect.center() - egui::vec2(size, size) / 2.0;
    let o = egui::pos2((o.x * ppp).round() / ppp, (o.y * ppp).round() / ppp);
    let ink = Color32::from_rgb(16, 18, 22);
    for y in 0..*n {
        // ciągłe odcinki ciemnych modułów w wierszu jako jeden prostokąt
        let mut x = 0;
        while x < *n {
            if !cells[y * n + x] {
                x += 1;
                continue;
            }
            let s = x;
            while x < *n && cells[y * n + x] {
                x += 1;
            }
            let r = egui::Rect::from_min_size(o + egui::vec2(s as f32 * cell, y as f32 * cell), egui::vec2((x - s) as f32 * cell, cell));
            p.rect_filled(r, 0.0, ink);
        }
    }
    resp
}

/// Rysuje baner, gdy jest otwarty. `blocked` – inne okno na wierzchu (przewodnik, ustawienia,
/// pytanie): baner czeka. Zmienia `shown` (licznik pokazań) i `off` (nie pokazuj ponownie);
/// zwraca `true`, gdy ustawienia trzeba zapisać.
pub fn show(ctx: &egui::Context, shown: &mut u8, off: &mut bool, blocked: bool) -> bool {
    if crate::platform::MOBILE || !OPEN.load(Ordering::Relaxed) || blocked {
        return false;
    }
    let mut save = false;
    if !COUNTED.swap(true, Ordering::Relaxed) {
        *shown = shown.saturating_add(1);
        save = true;
    }
    let mut close = false;
    let mut never = false;
    let w = theme::fit(ctx, 600.0);
    let narrow = w < 470.0;
    let top = crate::ui::panels::TOP_BAR_H + 10.0;
    egui::Area::new(egui::Id::new("baner-android"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_TOP, [0.0, top])
        .show(ctx, |ui| {
            theme::glass().inner_margin(egui::Margin::same(16)).show(ui, |ui| {
                ui.set_width(w - 32.0);
                let text = |ui: &mut egui::Ui, close: &mut bool, never: &mut bool| {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::hover());
                        ui.painter().circle_filled(r.center(), 15.0, theme::BRASS_DIM);
                        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, theme::icon::ANDROID_LOGO, egui::FontId::proportional(18.0), theme::BRASS);
                        ui.label(theme::kicker("Nowość"));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if theme::icon_button(ui, theme::icon::X, false).on_hover_text(tr("Zamknij")).clicked() {
                                *close = true;
                            }
                        });
                    });
                    ui.add_space(4.0);
                    ui.add(egui::Label::new(RichText::new(tr("GRAŃ jest już na Androidzie")).size(20.0).strong()).wrap());
                    ui.add_space(4.0);
                    ui.add(egui::Label::new(RichText::new(tr("Te same mapy 3D, szlaki i trasy w telefonie – a do tego położenie GPS, nagrywanie przejść i mapy offline na drogę. Zeskanuj kod aparatem telefonu: otworzy się strona, z której pobierzesz aplikację.")).color(theme::paper_2())).wrap());
                    ui.add_space(10.0);
                    ui.horizontal_wrapped(|ui| {
                        if ui.add(theme::primary(&theme::ic(theme::icon::DOWNLOAD_SIMPLE, "Strona pobierania"))).on_hover_text(URL).clicked() {
                            crate::platform::open_url(URL);
                        }
                        if ui.add(egui::Button::new(tr("Nie pokazuj ponownie")).min_size(egui::vec2(0.0, 34.0))).clicked() {
                            *never = true;
                        }
                    });
                    ui.add_space(2.0);
                    ui.label(RichText::new("redzikowski.com/portfolio/gran-3d").small().color(theme::paper_3()));
                };
                let code = |ui: &mut egui::Ui| {
                    ui.vertical_centered(|ui| {
                        if qr(ui, 148.0).on_hover_text(URL).clicked() {
                            crate::platform::open_url(URL);
                        }
                        ui.add_space(4.0);
                        ui.label(RichText::new(theme::ic(theme::icon::DEVICE_MOBILE, "Zeskanuj telefonem")).small().color(theme::paper_3()));
                    });
                };
                if narrow {
                    text(ui, &mut close, &mut never);
                    ui.add_space(12.0);
                    code(ui);
                } else {
                    ui.horizontal_top(|ui| {
                        ui.allocate_ui(egui::vec2(160.0, 180.0), |ui| code(ui));
                        ui.add_space(14.0);
                        ui.vertical(|ui| text(ui, &mut close, &mut never));
                    });
                }
            });
        });
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        close = true;
    }
    if never {
        *off = true;
        save = true;
        close = true;
    }
    if close {
        OPEN.store(false, Ordering::Relaxed);
    }
    save
}

#[cfg(test)]
mod tests {
    #[test]
    fn kod_qr_strony() {
        let (n, cells) = super::modules();
        // wersja 3 (29×29) dla adresu strony przy poziomie korekcji M
        assert!(*n >= 21 && (*n - 17) % 4 == 0, "rozmiar {n}");
        assert_eq!(cells.len(), n * n);
        // znaczniki pozycji w rogach: ciemne obramowanie 7×7
        assert!(cells[0] && cells[6] && cells[6 * n] && cells[n - 1]);
    }
}
