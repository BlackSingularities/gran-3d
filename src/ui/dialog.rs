//! Okna pytań i komunikatów w stylu programu (zamiast okien systemu – te same na Windows i Androidzie).

use eframe::egui::{self, RichText};

use crate::ui::theme;

/// Pytanie z przyciskami; pierwszy przycisk jest główny (wyróżniony).
pub struct Ask {
    pub title: String,
    pub text: String,
    pub buttons: Vec<String>,
    /// Główny przycisk usuwa coś na dobre – czerwony.
    pub danger: bool,
}

impl Ask {
    pub fn new(title: impl Into<String>, text: impl Into<String>, buttons: &[&str]) -> Self {
        Self { title: title.into(), text: text.into(), buttons: buttons.iter().map(|b| crate::i18n::tr(b).to_string()).collect(), danger: false }
    }

    /// Sam komunikat z przyciskiem „OK”.
    pub fn info(title: impl Into<String>, text: impl Into<String>) -> Self {
        Self::new(title, text, &["OK"])
    }

    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
}

/// Wynik klatki: `None` – okno nadal otwarte, `Some(i)` – kliknięty przycisk `i`,
/// `Some(usize::MAX)` – zamknięte bez wyboru (Esc, „wstecz”, klik obok).
pub fn show(ctx: &egui::Context, id: &str, ask: &Ask) -> Option<usize> {
    let mut pick = None;
    let w = theme::fit(ctx, 440.0);
    let modal = egui::Modal::new(egui::Id::new(("pytanie", id))).frame(theme::glass().inner_margin(egui::Margin::same(20))).show(ctx, |ui| {
        ui.set_width(w);
        ui.label(RichText::new(&ask.title).size(19.0).strong());
        ui.add_space(8.0);
        ui.add(egui::Label::new(RichText::new(&ask.text).size(15.0).color(theme::paper_2())).wrap());
        ui.add_space(16.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for (i, b) in ask.buttons.iter().enumerate() {
                let resp = if i == 0 {
                    let btn = theme::primary(b);
                    ui.add(if ask.danger { btn.fill(theme::SIGNAL) } else { btn })
                } else {
                    ui.add(egui::Button::new(b.as_str()).min_size(egui::vec2(90.0, 34.0)))
                };
                if resp.clicked() {
                    pick = Some(i);
                }
            }
        });
    });
    if pick.is_none() && (modal.should_close() || ctx.input(|i| i.key_pressed(egui::Key::BrowserBack))) {
        pick = Some(usize::MAX);
    }
    pick
}
