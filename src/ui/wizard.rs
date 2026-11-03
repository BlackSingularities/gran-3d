//! Kreator pierwszego uruchomienia: folder na dane, maksymalny rozmiar, obszary do pobrania na start
//! i ich poziom szczegółów, potem pobieranie z postępem.

use std::path::PathBuf;

use eframe::egui::{self, RichText};

use crate::areas::{self, Area, Level, PRESETS};
use crate::download::{fmt_duration, Progress};
use crate::geo::BBox;
use crate::settings::{data_dir_in, default_data_dir, Settings, LIMIT_STEPS_GB};
use crate::store::{detect, fmt_bytes, free_space, Found};
use crate::ui::map::{MapView, Overlay};
use crate::ui::theme;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Page {
    Folder,
    Limit,
    Areas,
    Download,
}

pub enum Action {
    /// Zapisz ustawienia i zacznij pobierać (model globalny + wybrane obszary).
    Start(Settings, Vec<Area>),
    /// Przejdź do programu (pobieranie trwa dalej w tle).
    Finish,
}

pub struct Wizard {
    page: Page,
    settings: Settings,
    limit_idx: usize,
    free: Option<u64>,
    found: Found,
    picked: Vec<bool>,
    custom: Vec<(String, BBox)>,
    level: Level,
    selecting: bool,
    custom_name: String,
    fitted: bool,
}

/// Stan pobierania pokazywany na ostatniej stronie.
pub struct DownloadView<'a> {
    pub base: Option<Progress>,
    pub areas: Vec<(&'a Area, Option<Progress>)>,
}

impl Wizard {
    pub fn new(settings: Settings) -> Self {
        let limit_idx = LIMIT_STEPS_GB
            .iter()
            .position(|&g| g >= settings.limit_gb)
            .unwrap_or(5);
        let free = free_space(&settings.data_dir);
        let mut picked = vec![false; PRESETS.len()];
        picked[0] = true; // Tatry na dobry początek
        let found = detect(&settings.data_dir);
        Self {
            page: Page::Folder,
            settings,
            limit_idx,
            free,
            found,
            picked,
            custom: Vec::new(),
            level: Level::Standard,
            selecting: false,
            custom_name: String::new(),
            fitted: false,
        }
    }

    /// Przejście wprost do strony (zrzuty i testy: GRAN_WIZARD_PAGE).
    pub fn jump(&mut self, page: &str) {
        self.page = match page {
            "limit" => Page::Limit,
            "areas" => Page::Areas,
            _ => Page::Folder,
        };
    }

    fn chosen(&self) -> Vec<(String, BBox)> {
        PRESETS
            .iter()
            .zip(&self.picked)
            .filter(|(_, on)| **on)
            .map(|(p, _)| (p.name.to_string(), p.bbox))
            .chain(self.custom.iter().cloned())
            .collect()
    }

    fn total_estimate(&self) -> areas::Estimate {
        self.chosen()
            .iter()
            .fold(areas::Estimate::default(), |acc, (_, b)| {
                acc + areas::estimate(b, self.level)
            })
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        map: &mut MapView,
        dl: Option<DownloadView>,
    ) -> Option<Action> {
        let mut action = None;
        egui::TopBottomPanel::top("wiz-head")
            .frame(
                egui::Frame::new()
                    .fill(theme::INK_0)
                    .inner_margin(egui::Margin::symmetric(28, 18)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Grań").size(26.0).strong());
                    ui.add_space(6.0);
                    ui.label(theme::kicker(&format!(
                        "atlas szlaków 3D · wersja {}",
                        env!("CARGO_PKG_VERSION")
                    )));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        for (i, name) in ["Pobieranie", "Obszary", "Rozmiar", "Folder"]
                            .iter()
                            .enumerate()
                        {
                            let p = [Page::Download, Page::Areas, Page::Limit, Page::Folder][i];
                            let c = if p == self.page {
                                theme::BRASS
                            } else if p < self.page {
                                theme::PAPER_2
                            } else {
                                theme::PAPER_3
                            };
                            ui.label(RichText::new(format!("{}. {name}", 4 - i)).color(c));
                        }
                    });
                });
            });
        egui::TopBottomPanel::bottom("wiz-foot")
            .frame(
                egui::Frame::new()
                    .fill(theme::INK_0)
                    .inner_margin(egui::Margin::symmetric(28, 14)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::kicker(
                        "© OpenStreetMap · GUGiK · ČÚZK · EOX · AWS Open Data",
                    ));
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| match self.page {
                            Page::Folder => {
                                if ui.add(theme::primary("Dalej")).clicked() {
                                    self.page = Page::Limit;
                                }
                            }
                            Page::Limit => {
                                if ui.add(theme::primary("Dalej")).clicked() {
                                    self.page = Page::Areas;
                                }
                                if ui.button("Wstecz").clicked() {
                                    self.page = Page::Folder;
                                }
                            }
                            Page::Areas => {
                                let label = if self.chosen().is_empty() {
                                    "Pomiń i rozpocznij"
                                } else {
                                    "Pobierz i rozpocznij"
                                };
                                if ui.add(theme::primary(label)).clicked() {
                                    self.settings.limit_gb = LIMIT_STEPS_GB[self.limit_idx];
                                    self.settings.setup_done = true;
                                    let list = self
                                        .chosen()
                                        .into_iter()
                                        .map(|(n, b)| Area::new(&n, b, self.level))
                                        .collect();
                                    action = Some(Action::Start(self.settings.clone(), list));
                                    self.page = Page::Download;
                                }
                                if ui.button("Wstecz").clicked() {
                                    self.page = Page::Limit;
                                }
                            }
                            Page::Download => {
                                let base_done = dl
                                    .as_ref()
                                    .and_then(|d| d.base.as_ref())
                                    .is_none_or(|p| p.fraction() >= 1.0);
                                let label = if dl
                                    .as_ref()
                                    .is_some_and(|d| d.areas.iter().all(|(a, _)| a.complete))
                                    && base_done
                                {
                                    "Przejdź do mapy"
                                } else {
                                    "Przejdź do mapy – pobieraj w tle"
                                };
                                if ui.add_enabled(base_done, theme::primary(label)).clicked() {
                                    action = Some(Action::Finish);
                                }
                                if !base_done {
                                    ui.label(
                                        RichText::new("najpierw model globalny (kilkadziesiąt MB)")
                                            .color(theme::PAPER_3),
                                    );
                                }
                            }
                        },
                    );
                });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::INK_0)
                    .inner_margin(egui::Margin::symmetric(28, 10)),
            )
            .show(ctx, |ui| match self.page {
                Page::Folder => self.folder_page(ui),
                Page::Limit => self.limit_page(ui),
                Page::Areas => self.areas_page(ui, map),
                Page::Download => Self::download_page(ui, dl),
            });
        action
    }

    fn folder_page(&mut self, ui: &mut egui::Ui) {
        ui.label(theme::kicker("Pierwsze uruchomienie"));
        ui.heading("Gdzie zapisywać mapy?");
        ui.add_space(4.0);
        ui.label(RichText::new("Grań pobiera teren, zdjęcia satelitarne, szlaki i dane okolic. Wszystko trafia na stałe do jednego folderu – wybierz dysk, na którym jest dużo miejsca. W wybranym miejscu powstaje podfolder „GRAN-dane”; inne pliki nie są ruszane.").color(theme::PAPER_2));
        ui.add_space(16.0);
        theme::card(true).show(ui, |ui| {
            ui.set_width(ui.available_width().min(760.0));
            ui.label(theme::kicker("Folder na dane map"));
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(self.settings.data_dir.display().to_string())
                        .monospace()
                        .color(theme::PAPER),
                );
            });
            ui.horizontal(|ui| {
                if ui.button("Wybierz folder…").clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .set_title("Folder na dane map GRANIA")
                        .pick_folder()
                    {
                        self.set_dir(data_dir_in(&p));
                    }
                }
                if self.settings.data_dir != default_data_dir() && ui.button("Domyślny").clicked()
                {
                    self.set_dir(default_data_dir());
                }
            });
            if let Some(f) = self.free {
                ui.label(
                    RichText::new(format!("Wolne miejsce na tym dysku: {}", fmt_bytes(f)))
                        .color(theme::PAPER_3),
                );
            }
            if let Some(msg) = self.found.describe() {
                ui.label(RichText::new(msg).color(theme::OK));
            }
        });
        ui.add_space(8.0);
        ui.label(RichText::new("Masz już pobrane mapy (np. z innego komputera albo z wersji 1.x)? Wskaż ich folder – program je rozpozna i od razu z nich skorzysta.").small().color(theme::PAPER_3));
    }

    fn set_dir(&mut self, dir: PathBuf) {
        match probe_writable(&dir) {
            Ok(()) => {
                self.free = free_space(&dir);
                self.found = detect(&dir);
                self.settings.data_dir = dir;
            }
            Err(e) => {
                rfd::MessageDialog::new()
                    .set_title("GRAŃ")
                    .set_description(format!(
                        "Brak uprawnień do zapisu w tym folderze:\n{}\n\n{e}",
                        dir.display()
                    ))
                    .set_level(rfd::MessageLevel::Warning)
                    .show();
            }
        }
    }

    fn limit_page(&mut self, ui: &mut egui::Ui) {
        ui.label(theme::kicker("Pierwsze uruchomienie"));
        ui.heading("Ile miejsca mogą zająć mapy?");
        ui.add_space(4.0);
        ui.label(RichText::new("Po przekroczeniu limitu najdawniej oglądane kafle są usuwane w tle. Obszary, które zapiszesz do pracy bez internetu, nigdy nie są usuwane automatycznie.").color(theme::PAPER_2));
        ui.add_space(16.0);
        theme::card(true).show(ui, |ui| {
            ui.set_width(ui.available_width().min(760.0));
            ui.horizontal(|ui| {
                ui.label(theme::kicker("Maksymalny rozmiar danych"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{} GB", LIMIT_STEPS_GB[self.limit_idx]))
                            .size(24.0)
                            .strong(),
                    );
                });
            });
            ui.spacing_mut().slider_width = ui.available_width();
            ui.add(
                egui::Slider::new(&mut self.limit_idx, 0..=LIMIT_STEPS_GB.len() - 1)
                    .show_value(false),
            );
            ui.horizontal(|ui| {
                for g in [1, 10, 50, 500] {
                    ui.label(
                        RichText::new(format!("{g} GB"))
                            .small()
                            .color(theme::PAPER_3),
                    );
                    ui.add_space(ui.available_width() / 4.0 - 30.0);
                }
            });
            if let Some(f) = self.free {
                let limit = LIMIT_STEPS_GB[self.limit_idx] as u64 * (1 << 30);
                let msg = format!("Wolne miejsce: {}", fmt_bytes(f));
                if limit > f {
                    ui.label(
                        RichText::new(format!("{msg} – limit jest większy niż wolne miejsce."))
                            .color(theme::WARN),
                    );
                } else {
                    ui.label(RichText::new(msg).color(theme::PAPER_3));
                }
            }
        });
    }

    fn areas_page(&mut self, ui: &mut egui::Ui, map: &mut MapView) {
        let est = self.total_estimate();
        let limit = LIMIT_STEPS_GB[self.limit_idx] as u64 * (1 << 30);
        egui::SidePanel::left("wiz-areas").resizable(false).exact_width(360.0).frame(egui::Frame::new().fill(theme::INK_0).inner_margin(egui::Margin { left: 0, right: 16, top: 0, bottom: 0 })).show_inside(ui, |ui| {
            ui.label(theme::kicker("Pierwsze uruchomienie"));
            ui.heading("Co pobrać na start?");
            ui.label(RichText::new("Zaznaczone obszary będą działać bez internetu. Resztę Europy program dociąga w miarę oglądania.").color(theme::PAPER_2));
            ui.add_space(8.0);
            ui.label(theme::kicker("Poziom szczegółów"));
            for l in Level::ALL {
                theme::card(self.level == l).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if ui.radio(self.level == l, RichText::new(l.name()).strong()).clicked() {
                        self.level = l;
                    }
                    ui.label(RichText::new(l.hint()).small().color(theme::PAPER_3));
                });
            }
            ui.add_space(8.0);
            ui.label(theme::kicker("Obszary"));
            egui::ScrollArea::vertical().max_height(ui.available_height() - 120.0).show(ui, |ui| {
                for (i, p) in PRESETS.iter().enumerate() {
                    let e = areas::estimate(&p.bbox, self.level);
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.picked[i], RichText::new(p.name).strong());
                        ui.label(RichText::new(p.country).small().color(theme::PAPER_3));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(format!("~{}", fmt_bytes(e.bytes))).small().monospace().color(theme::PAPER_3));
                        });
                    });
                }
                let mut remove = None;
                for (i, (name, b)) in self.custom.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.small_button("Usuń").on_hover_text("Usuń").clicked() {
                            remove = Some(i);
                        }
                        ui.label(RichText::new(name).strong().color(theme::TEAL));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(format!("~{}", fmt_bytes(areas::estimate(b, self.level).bytes))).small().monospace().color(theme::PAPER_3));
                        });
                    });
                }
                if let Some(i) = remove {
                    self.custom.remove(i);
                }
            });
            ui.add_space(6.0);
            let btn = if self.selecting { "Zaznacz prostokąt na mapie…" } else { "+ Własny obszar z mapy" };
            if ui.add(egui::Button::new(btn).selected(self.selecting)).clicked() {
                self.selecting = !self.selecting;
            }
            ui.add_space(6.0);
            ui.label(RichText::new(format!("Razem: ~{} · {} kafli", fmt_bytes(est.bytes), est.tiles)).strong());
            if est.bytes > limit * 9 / 10 {
                ui.label(RichText::new("Wybrane obszary nie zmieszczą się w limicie – zwiększ limit albo wybierz niższy poziom.").color(theme::WARN));
            }
        });
        egui::CentralPanel::default().frame(egui::Frame::new().corner_radius(12)).show_inside(ui, |ui| {
            if !self.fitted {
                map.fit(&BBox::new(14.5, 48.6, 24.2, 51.2), ui.available_size());
                self.fitted = true;
            }
            let mut overlays: Vec<Overlay> = PRESETS
                .iter()
                .zip(&self.picked)
                .map(|(p, on)| Overlay { bbox: p.bbox, color: if *on { theme::BRASS } else { theme::PAPER_3 }, label: p.name.into(), strong: *on })
                .collect();
            overlays.extend(self.custom.iter().map(|(n, b)| Overlay { bbox: *b, color: theme::TEAL, label: n.clone(), strong: true }));
            let r = map.show(ui, &overlays, self.selecting);
            if let Some(b) = r.selected {
                if b.km2() > areas::MAX_AREA_KM2 {
                    rfd::MessageDialog::new().set_title("GRAŃ").set_description(format!("Obszar za duży: {:.0} km² (limit {:.0} km²). Zaznacz mniejszy prostokąt.", b.km2(), areas::MAX_AREA_KM2)).show();
                } else {
                    let n = self.custom.len() + 1;
                    let name = if self.custom_name.trim().is_empty() { format!("Mój obszar {n}") } else { std::mem::take(&mut self.custom_name) };
                    self.custom.push((name, b));
                    self.selecting = false;
                }
            } else if let Some([lon, lat]) = r.clicked {
                // kliknięcie w propozycję przełącza jej zaznaczenie
                for (i, p) in PRESETS.iter().enumerate() {
                    if lon > p.bbox.w && lon < p.bbox.e && lat > p.bbox.s && lat < p.bbox.n {
                        self.picked[i] = !self.picked[i];
                    }
                }
            }
        });
    }

    fn download_page(ui: &mut egui::Ui, dl: Option<DownloadView>) {
        ui.label(theme::kicker("Pierwsze uruchomienie"));
        ui.heading("Pobieranie danych startowych");
        ui.label(RichText::new("Możesz przejść do mapy, gdy tylko model globalny będzie gotowy – obszary pobierają się dalej w tle, a postęp widać w panelu „Dane map”.").color(theme::PAPER_2));
        ui.add_space(12.0);
        let Some(dl) = dl else { return };
        ui.set_max_width(760.0);
        let row = |ui: &mut egui::Ui, name: &str, p: Option<&Progress>, complete: bool| {
            theme::card(false).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(name).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let txt = match (p, complete) {
                            (_, true) => "gotowe".to_string(),
                            (Some(p), _) => format!(
                                "{:.0}% · {} · {}",
                                p.fraction() * 100.0,
                                fmt_bytes(p.bytes),
                                p.eta
                                    .map(|e| format!("zostało {}", fmt_duration(e)))
                                    .unwrap_or_else(|| "szacowanie…".into())
                            ),
                            (None, _) => "w kolejce".into(),
                        };
                        ui.label(RichText::new(txt).monospace().color(if complete {
                            theme::OK
                        } else {
                            theme::PAPER_3
                        }));
                    });
                });
                theme::bar(
                    ui,
                    if complete {
                        1.0
                    } else {
                        p.map_or(0.0, |p| p.fraction())
                    },
                    if complete { theme::OK } else { theme::BRASS },
                );
                if let Some(e) = p.and_then(|p| p.last_error.as_ref()) {
                    ui.label(
                        RichText::new(format!(
                            "Ostatni błąd: {e} (kafle zostaną pobrane ponownie później)"
                        ))
                        .small()
                        .color(theme::WARN),
                    );
                }
            });
        };
        row(
            ui,
            "Model globalny Europy",
            dl.base.as_ref(),
            dl.base.as_ref().is_some_and(|p| p.fraction() >= 1.0),
        );
        for (a, p) in &dl.areas {
            row(
                ui,
                &format!("{} · {}", a.name, a.level.name()),
                p.as_ref(),
                a.complete,
            );
        }
    }
}

fn probe_writable(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let probe = dir.join(".gran-test");
    std::fs::write(&probe, b"")?;
    std::fs::remove_file(probe)
}
