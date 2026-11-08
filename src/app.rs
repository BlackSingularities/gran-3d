//! Główna aplikacja: mapa 3D na całe okno i unoszący się nad nią interfejs. Dane zapisują się same
//! w miarę oglądania, a sprzątacz pilnuje limitu rozmiaru.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText};

use crate::heights::Heights;
use crate::settings::{data_dir_in, Settings, LIMIT_STEPS_GB};
use crate::store::{fmt_bytes, free_space, Store};
use crate::ui::atlas::Atlas;
use crate::ui::theme;
use crate::updater;
use crate::world::World;

/// Stan aktualizacji programu.
enum Update {
    Idle,
    Checking(Arc<Mutex<Option<Result<updater::Release, String>>>>, bool),
    Available(updater::Release),
    Downloading(updater::Release, Arc<AtomicU64>, Arc<Mutex<Option<Result<PathBuf, String>>>>),
}

/// Zajętość dysku liczona w tle (przejście po tysiącach plików nie może blokować interfejsu).
struct Usage {
    bytes: AtomicU64,
    known: AtomicBool,
    busy: AtomicBool,
}

/// Przeniesienie folderu danych w tle.
struct Moving {
    to: PathBuf,
    result: Arc<Mutex<Option<Result<(), String>>>>,
    settings: Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    General,
    Map,
    Graphics,
    Performance,
    Data,
    About,
}

const PAGES: [(Page, &str, &str); 6] = [
    (Page::General, theme::icon::SLIDERS_HORIZONTAL, "Ogólne"),
    (Page::Map, theme::icon::MAP_TRIFOLD, "Mapa"),
    (Page::Graphics, theme::icon::MONITOR, "Grafika"),
    (Page::Performance, theme::icon::GAUGE, "Zasoby"),
    (Page::Data, theme::icon::HARD_DRIVE, "Dane map"),
    (Page::About, theme::icon::INFO, "O programie"),
];

/// Otwarte ustawienia: kopia robocza i bieżąca strona.
struct SettingsView {
    draft: Settings,
    page: Page,
}

pub struct App {
    settings: Settings,
    store: Store,
    usage: Arc<Usage>,
    last_gc: Instant,
    settings_open: Option<SettingsView>,
    moving: Option<Moving>,
    /// Zrzut okna do pliku (GRAN_CAPTURE=plik.png, GRAN_CAPTURE_AFTER=sekundy) – zrzuty do dokumentacji i testy.
    capture: Option<(PathBuf, Instant, f32, bool)>,
    heights: Heights,
    world: World,
    atlas: Atlas,
    update: Update,
    feedback: crate::feedback::FeedbackUi,
    /// Przewodnik pierwszego uruchomienia: bieżący krok.
    tour: Option<usize>,
}

/// Klucz ustawień widoku; nowy układ zaczyna od nowych domyślnych.
const PREFS_KEY: &str = "atlas2";

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let settings = Settings::load();
        crate::limits::apply(settings.resources);
        let store = Store::new(&settings.data_dir);
        let heights = Heights::new(store.clone());
        let world = World::new(heights.clone(), cc.egui_ctx.clone());
        let prefs = cc.storage.and_then(|s| eframe::get_value::<crate::ui::atlas::Prefs>(s, PREFS_KEY));
        let atlas = Atlas::new(heights.clone(), world.clone(), prefs);
        updater::cleanup();
        let mut app = Self {
            heights,
            world,
            atlas,
            update: Update::Idle,
            feedback: Default::default(),
            tour: None,
            settings,
            store,
            usage: Arc::new(Usage {
                bytes: AtomicU64::new(0),
                known: AtomicBool::new(false),
                busy: AtomicBool::new(false),
            }),
            last_gc: Instant::now(),
            settings_open: None,
            moving: None,
            capture: std::env::var_os("GRAN_CAPTURE").map(|p| {
                (
                    PathBuf::from(p),
                    Instant::now(),
                    std::env::var("GRAN_CAPTURE_AFTER")
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(6.0),
                    false,
                )
            }),
        };
        app.collect_garbage();
        // link gran:// z wiersza poleceń (kliknięty w systemie)
        if let Some(l) = std::env::args().find(|a| a.starts_with("gran://")) {
            app.atlas.apply_link(&l);
        }
        // plik śladu otwarty z Eksploratora („Otwórz za pomocą”, dwuklik na .gpx)
        if let Some(f) = std::env::args().skip(1).map(PathBuf::from).find(|p| crate::formats::is_track_file(p) && p.is_file()) {
            app.atlas.import_track(&f);
        }
        app.tour = if app.settings.tour_done || std::env::var_os("GRAN_CAPTURE").is_some() { None } else { Some(0) };
        if app.settings.auto_update && std::env::var_os("GRAN_CAPTURE").is_none() {
            app.check_updates(false);
        }
        app
    }

    // ------------------------------------------------------------------ dane

    /// Sprzątanie do limitu (najdawniej używane kafle) i przeliczenie zajętości, w tle.
    fn collect_garbage(&mut self) {
        self.last_gc = Instant::now();
        if self.usage.busy.swap(true, Ordering::Relaxed) {
            return;
        }
        let (store, limit, usage) = (self.store.clone(), self.settings.limit_bytes(), self.usage.clone());
        thread::spawn(move || {
            let (n, freed) = store.collect_garbage(limit);
            if n > 0 {
                log::info!("sprzątanie: usunięto {n} paczek ({})", fmt_bytes(freed));
            }
            usage.bytes.store(store.usage(), Ordering::Relaxed);
            usage.known.store(true, Ordering::Relaxed);
            usage.busy.store(false, Ordering::Relaxed);
        });
    }

    // ------------------------------------------------------------------ ustawienia i przenoszenie

    fn open_settings(&mut self, page: Page) {
        self.settings_open = Some(SettingsView { draft: self.settings.clone(), page });
    }

    fn apply_settings(&mut self, next: Settings) {
        let moved = next.data_dir != self.settings.data_dir;
        if moved
            && Path::new(&self.settings.data_dir).exists()
            && self.usage.bytes.load(Ordering::Relaxed) > 0
        {
            let r = rfd::MessageDialog::new()
                .set_title(crate::i18n::tr("GRAŃ"))
                .set_description(tf!("Przenieść zapisane dane do nowego folderu?\n\nZ: {}\nDo: {}\n\nTak – dane trafią do nowego folderu, a stary zostanie usunięty.\nNie – nowy folder zacznie się wypełniać od początku, stare dane zostaną w poprzednim miejscu.", self.settings.data_dir.display(), next.data_dir.display()))
                .set_buttons(rfd::MessageButtons::YesNoCancel)
                .show();
            match r {
                rfd::MessageDialogResult::Yes => return self.start_move(next),
                rfd::MessageDialogResult::No => {}
                _ => return,
            }
        }
        self.commit_settings(next);
    }

    fn commit_settings(&mut self, next: Settings) {
        let moved = next.data_dir != self.settings.data_dir;
        if next.lang != self.settings.lang {
            crate::i18n::set(if next.lang.is_empty() { crate::i18n::system() } else { &next.lang });
        }
        crate::limits::apply(next.resources);
        self.settings = next;
        if let Err(e) = self.settings.save() {
            self.atlas.notify(tf!("Nie udało się zapisać ustawień: {}", e));
        }
        if moved {
            self.store = Store::new(&self.settings.data_dir);
            self.heights.set_store(self.store.clone());
            self.world.reset();
            if let Some(r) = &mut self.atlas.renderer {
                r.tiles.reset();
            }
        }
        self.collect_garbage();
    }

    fn start_move(&mut self, next: Settings) {
        let (from, to) = (self.settings.data_dir.clone(), next.data_dir.clone());
        let result = Arc::new(Mutex::new(None));
        let r = result.clone();
        thread::spawn(move || {
            *r.lock().unwrap() = Some(move_dir(&from, &to).map_err(|e| e.to_string()));
        });
        self.moving = Some(Moving {
            to: next.data_dir.clone(),
            result,
            settings: next,
        });
    }

    // ------------------------------------------------------------------ widoki

    /// Górny pasek z prawą częścią: ustawienia, pomoc, aktualizacja (rysowane od prawej).
    fn top_bar(&mut self, ctx: &egui::Context) {
        let mut offer = None;
        let mut settings = false;
        let update = &self.update;
        crate::ui::panels::top_bar(&mut self.atlas, ctx, |atlas, ui| {
            if theme::icon_button(ui, theme::icon::GEAR, false).on_hover_text(crate::i18n::tr("Ustawienia (Ctrl+,)")).clicked() {
                settings = true;
            }
            if theme::icon_button(ui, theme::icon::QUESTION, atlas.help).on_hover_text(crate::i18n::tr("Skróty i sterowanie (?)")).clicked() {
                atlas.help = !atlas.help;
            }
            if let Update::Available(rel) = update {
                if ui.add(theme::primary(&tf!("Aktualizacja {}", rel.version))).clicked() {
                    offer = Some(rel.clone());
                }
            }
        });
        if settings {
            self.open_settings(Page::General);
        }
        if let Some(rel) = offer {
            self.offer_update(rel);
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        let Some(view) = &mut self.settings_open else {
            return;
        };
        let mut close = false;
        let mut save = false;
        let mut check = false;
        let mut report = false;
        let mut tour = false;
        let used = self.usage.bytes.load(Ordering::Relaxed);
        let known = self.usage.known.load(Ordering::Relaxed);
        let current_limit = self.settings.limit_gb;
        let atlas = &mut self.atlas;
        let modal = egui::Modal::new(egui::Id::new("ustawienia")).frame(theme::glass().inner_margin(egui::Margin::same(18))).show(ctx, |ui| {
            ui.set_width(760.0);
            ui.horizontal(|ui| {
                ui.heading(crate::i18n::tr("Ustawienia"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(theme::icon::X).on_hover_text(crate::i18n::tr("Zamknij bez zapisywania (Esc)")).clicked() {
                        close = true;
                    }
                });
            });
            ui.add_space(8.0);
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(egui::vec2(180.0, 440.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_width(180.0);
                    for (page, glyph, name) in PAGES {
                        if theme::row_button(ui, glyph, crate::i18n::tr(name), view.page == page).clicked() {
                            view.page = page;
                        }
                    }
                });
                ui.add_space(12.0);
                ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 440.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_height(440.0);
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match view.page {
                        Page::General => {
                            ui.label(theme::kicker(crate::i18n::tr("Język / Language")));
                            ui.horizontal(|ui| {
                                let cur = if view.draft.lang.is_empty() { crate::i18n::system() } else { view.draft.lang.as_str() };
                                let mut pick = None;
                                for l in crate::i18n::LANGS {
                                    if ui.selectable_label(cur == l.code, l.name).clicked() {
                                        pick = Some(l.code);
                                    }
                                }
                                if let Some(c) = pick {
                                    view.draft.lang = c.to_string();
                                }
                            });
                            ui.add_space(12.0);
                            ui.label(theme::kicker(crate::i18n::tr("Program")));
                            theme::switch(ui, &mut view.draft.auto_update, crate::i18n::tr("Sprawdzaj aktualizacje"));
                            theme::switch(ui, &mut view.draft.vsync, crate::i18n::tr("Synchronizacja z monitorem"));
                            theme::para(ui, crate::i18n::tr("Bez synchronizacji więcej klatek na sekundę, ale możliwe rozrywanie obrazu. Zmiana działa po ponownym uruchomieniu."));
                            ui.add_space(12.0);
                            ui.label(theme::kicker(crate::i18n::tr("Pomoc")));
                            if ui.button(theme::ic(theme::icon::SIGNPOST, "Pokaż przewodnik od początku")).clicked() {
                                tour = true;
                            }
                        }
                        Page::Map => crate::ui::panels::map_settings(atlas, ui),
                        Page::Graphics => crate::ui::panels::graphics(atlas, ui),
                        Page::Performance => {
                            ui.label(theme::kicker(crate::i18n::tr("Limit zużycia zasobów")));
                            theme::para(ui, crate::i18n::tr("Ile pamięci RAM, pamięci karty graficznej i wątków pobierania może zająć GRAŃ. Niższy poziom odciąża słabszy komputer kosztem wolniejszego doczytywania mapy."));
                            ui.add_space(6.0);
                            for (i, (name, desc)) in crate::limits::LEVELS.iter().enumerate() {
                                let on = view.draft.resources == i as u8;
                                if theme::option_card(ui, name, desc, on).clicked() {
                                    view.draft.resources = i as u8;
                                }
                            }
                            theme::para(ui, crate::i18n::tr("Limit pamięci działa po zapisaniu; liczba wątków pobierania zmienia się po ponownym uruchomieniu."));
                            ui.add_space(12.0);
                            ui.label(theme::kicker(crate::i18n::tr("Doczytywanie z wyprzedzeniem")));
                            theme::para(ui, crate::i18n::tr("W tle, z najniższym priorytetem, program pobiera okolicę poza kadrem, żeby po obrocie lub przesunięciu obraz był od razu."));
                            ui.horizontal(|ui| {
                                for (v, name) in [(0u8, crate::i18n::tr("Wyłączone")), (1, crate::i18n::tr("Lekkie")), (2, crate::i18n::tr("Mocne"))] {
                                    if ui.selectable_label(atlas.prefs.prefetch == v, name).clicked() {
                                        atlas.prefs.prefetch = v;
                                    }
                                }
                            });
                        }
                        Page::Data => {
                            ui.label(theme::kicker(crate::i18n::tr("Zajęte miejsce")));
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(if known { fmt_bytes(used) } else { "…".into() }).size(22.0).strong());
                                ui.label(RichText::new(tf!("z {} GB", current_limit)).color(theme::PAPER_3));
                            });
                            let limit = current_limit as u64 * (1 << 30);
                            theme::bar(ui, used as f32 / limit as f32, theme::BRASS);
                            ui.add_space(12.0);
                            ui.label(theme::kicker(crate::i18n::tr("Limit danych")));
                            let mut idx = LIMIT_STEPS_GB.iter().position(|&g| g >= view.draft.limit_gb).unwrap_or(LIMIT_STEPS_GB.len() - 1);
                            ui.horizontal(|ui| {
                                if ui.add(egui::Slider::new(&mut idx, 0..=LIMIT_STEPS_GB.len() - 1).show_value(false)).changed() {
                                    view.draft.limit_gb = LIMIT_STEPS_GB[idx];
                                }
                                ui.label(RichText::new(format!("{} GB", view.draft.limit_gb)).strong());
                            });
                            theme::para(ui, crate::i18n::tr("Mapy zapisują się same podczas oglądania. Po przekroczeniu limitu najdawniej używane dane są usuwane."));
                            ui.add_space(12.0);
                            ui.label(theme::kicker(crate::i18n::tr("Folder danych")));
                            ui.label(RichText::new(view.draft.data_dir.display().to_string()).monospace());
                            if let Some(f) = free_space(&view.draft.data_dir) {
                                ui.label(RichText::new(tf!("Wolne miejsce: {}", fmt_bytes(f))).small().color(theme::PAPER_3));
                            }
                            if ui.button(theme::ic(theme::icon::FOLDER_OPEN, "Zmień folder…")).clicked() {
                                if let Some(p) = rfd::FileDialog::new().set_title(crate::i18n::tr("Folder na dane map GRANIA")).pick_folder() {
                                    view.draft.data_dir = data_dir_in(&p);
                                }
                            }
                        }
                        Page::About => {
                            ui.label(RichText::new(tf!("GRAŃ {}", env!("CARGO_PKG_VERSION"))).size(22.0).strong());
                            if ui.button(theme::ic(theme::icon::ARROWS_CLOCKWISE, "Sprawdź aktualizacje")).clicked() {
                                check = true;
                            }
                            if ui.button(theme::ic(theme::icon::BUG, "Zgłoś błąd lub sugestię…")).clicked() {
                                report = true;
                            }
                            if ui.button(theme::ic(theme::icon::FILE_TEXT, "Pokaż dziennik programu")).on_hover_text(crate::i18n::tr("Plik gran.log – przydaje się przy zgłaszaniu błędów")).clicked() {
                                let _ = open::that(crate::log_path().parent().unwrap_or(std::path::Path::new(".")));
                            }
                            ui.add_space(12.0);
                            ui.label(theme::kicker(crate::i18n::tr("Autor")));
                            if ui.link("redzikowski.com").clicked() {
                                let _ = open::that("https://redzikowski.com");
                            }
                            ui.label(RichText::new("kontakt@redzikowski.com").small().color(theme::PAPER_3));
                            if ui.add(theme::primary(&theme::ic(theme::icon::COFFEE, "Postaw kawę"))).clicked() {
                                let _ = open::that("https://buycoffee.to/adamredzikowski");
                            }
                            ui.add_space(12.0);
                            ui.label(theme::kicker(crate::i18n::tr("Źródła danych")));
                            theme::para(ui, crate::i18n::tr("Teren: Mapterhorn z modeli DMR 5.0 ÚGKK SR (CC BY 4.0), NMT GUGiK i DMR 4G ČÚZK (CC BY 4.0), zapasowo Terrarium/AWS Open Data. Zdjęcia: Esri World Imagery (© Esri, Maxar, Earthstar Geographics), opcjonalne ortofoto: GUGiK, ČÚZK, ÚGKK SR. Nazwy: OpenFreeMap, OpenMapTiles. Szlaki i punkty: © OpenStreetMap (ODbL). Wyszukiwanie: Nominatim."));
                        }
                    });
                });
            });
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                save = ui.add(theme::primary(crate::i18n::tr("Zapisz"))).clicked();
                if ui.button(crate::i18n::tr("Anuluj")).clicked() {
                    close = true;
                }
            });
        });
        if save {
            let next = self.settings_open.take().unwrap().draft;
            self.apply_settings(next);
        } else if close || modal.should_close() {
            self.settings_open = None;
        }
        if check {
            self.check_updates(true);
        }
        if report {
            self.feedback.open();
        }
        if tour {
            self.settings_open = None;
            self.tour = Some(0);
        }
    }

    fn moving_overlay(&mut self, ctx: &egui::Context) {
        let Some(m) = &self.moving else { return };
        let done = m.result.lock().unwrap().take();
        egui::Window::new(crate::i18n::tr("Przenoszenie danych"))
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_width(460.0);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tf!("Przenoszenie danych do {}…", m.to.display()));
                });
            });
        ctx.request_repaint_after(Duration::from_millis(200));
        if let Some(r) = done {
            let m = self.moving.take().unwrap();
            match r {
                Ok(()) => self.commit_settings(m.settings),
                Err(e) => {
                    rfd::MessageDialog::new().set_title(crate::i18n::tr("GRAŃ")).set_level(rfd::MessageLevel::Error).set_description(tf!("Nie udało się przenieść danych:\n{}\n\nDane, których nie przeniesiono, zostały w dotychczasowym folderze.", e)).show();
                }
            }
        }
    }

    /// Sprawdzenie aktualizacji w tle; `manual` – komunikat także gdy brak nowszej wersji.
    fn check_updates(&mut self, manual: bool) {
        if matches!(self.update, Update::Checking(..) | Update::Downloading(..)) {
            return;
        }
        let slot = Arc::new(Mutex::new(None));
        let s = slot.clone();
        thread::spawn(move || {
            *s.lock().unwrap() = Some(updater::latest());
        });
        self.update = Update::Checking(slot, manual);
    }

    fn offer_update(&mut self, rel: updater::Release) {
        let size = if rel.size > 0 { format!(" ({} MB)", rel.size / (1 << 20)) } else { String::new() };
        let ok = rfd::MessageDialog::new()
            .set_title(crate::i18n::tr("GRAŃ – aktualizacja"))
            .set_description(tf!("Dostępna jest nowa wersja {}. Masz wersję {}.\n\nPobrać i zainstalować aktualizację{}? Dane map i ustawienia zostaną zachowane. Instalacja wymaga potwierdzenia uprawnień administratora.", rel.version, updater::current(), size))
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();
        if ok != rfd::MessageDialogResult::Yes {
            self.update = Update::Available(rel);
            return;
        }
        let progress = Arc::new(AtomicU64::new(0));
        let slot = Arc::new(Mutex::new(None));
        let (p, s, r) = (progress.clone(), slot.clone(), rel.clone());
        thread::spawn(move || {
            *s.lock().unwrap() = Some(updater::download(&r, p));
        });
        self.update = Update::Downloading(rel, progress, slot);
    }

    fn update_ui(&mut self, ctx: &egui::Context) {
        let mut next = None;
        match &self.update {
            Update::Checking(slot, manual) => {
                let manual = *manual;
                let r = slot.lock().unwrap().take();
                match r {
                    Some(Ok(rel)) if updater::cmp_version(&rel.version, updater::current()) > 0 => {
                        next = Some(if manual {
                            Err(rel)
                        } else {
                            self.atlas.notify(tf!("Dostępna jest nowa wersja {} – przycisk „Aktualizacja” u góry.", rel.version));
                            Ok(Update::Available(rel))
                        });
                    }
                    Some(Ok(_)) => {
                        if manual {
                            self.atlas.notify(tf!("Masz najnowszą wersję ({}).", updater::current()));
                        }
                        next = Some(Ok(Update::Idle));
                    }
                    Some(Err(e)) => {
                        if manual {
                            self.atlas.notify(tf!("Nie udało się sprawdzić aktualizacji: {}", e));
                        } else {
                            log::warn!("aktualizacje: {e}");
                        }
                        next = Some(Ok(Update::Idle));
                    }
                    None => ctx.request_repaint_after(Duration::from_millis(300)),
                }
            }
            Update::Downloading(rel, progress, slot) => {
                let got = progress.load(Ordering::Relaxed);
                let total = rel.size.max(1);
                let version = rel.version.clone();
                egui::Window::new(crate::i18n::tr("Aktualizacja"))
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.set_width(380.0);
                        ui.label(tf!("Pobieranie wersji {}…", version));
                        theme::bar(ui, got as f32 / total as f32, theme::BRASS);
                        ui.label(RichText::new(format!("{} z {}", fmt_bytes(got), fmt_bytes(total))).small().color(theme::PAPER_3));
                    });
                ctx.request_repaint_after(Duration::from_millis(200));
                let done = slot.lock().unwrap().take();
                if let Some(r) = done {
                    next = Some(Ok(Update::Idle));
                    match r.and_then(|p| updater::run_installer(&p)) {
                        Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                        Err(e) => {
                            rfd::MessageDialog::new()
                                .set_title(crate::i18n::tr("GRAŃ"))
                                .set_level(rfd::MessageLevel::Error)
                                .set_description(tf!("Nie udało się zainstalować aktualizacji: {}", e))
                                .show();
                        }
                    }
                }
            }
            _ => {}
        }
        match next {
            Some(Ok(u)) => self.update = u,
            Some(Err(rel)) => {
                self.update = Update::Idle;
                self.offer_update(rel);
            }
            None => {}
        }
    }

    /// Przewodnik pierwszego uruchomienia: kilka kart z podstawami.
    fn tour_ui(&mut self, ctx: &egui::Context) {
        use crate::i18n::tr;
        let Some(step) = self.tour else { return };
        if self.settings_open.is_some() || self.moving.is_some() {
            return;
        }
        let steps: [(&str, &str, &str); 5] = [
            (theme::icon::MOUNTAINS, crate::i18n::tr("Witaj w GRANI"), crate::i18n::tr("Trójwymiarowy atlas szlaków górskich z prawdziwym modelem terenu. W kilku krokach pokażę, jak z niego korzystać.")),
            (theme::icon::HAND_GRABBING, crate::i18n::tr("Poruszanie mapą"), crate::i18n::tr("Lewy przycisk – przesuwanie (chwytasz teren). Prawy przycisk – obrót wokół chwyconego punktu. Kółko – przybliżanie do kursora, dwuklik – przybliżenie do punktu. Klawisze: strzałki, Q E, PgUp PgDn.")),
            (theme::icon::PATH, crate::i18n::tr("Planowanie trasy"), crate::i18n::tr("Wybierz narzędzie trasy (klawisz 2) i klikaj na mapie: start, punkty pośrednie, cel. Punkty można przeciągać, Ctrl+Z cofa zmianę, Ctrl+Y ją ponawia. Trasę zapiszesz w bibliotece (Ctrl+S) albo wyeksportujesz do GPX, KML lub GeoJSON.")),
            (theme::icon::PERSON_SIMPLE_WALK, crate::i18n::tr("Spacer"), crate::i18n::tr("Przycisk z idącą postacią stawia Cię na terenie: W A S D – chodzenie, Shift – bieg, spacja – skok, prawy przycisk – rozglądanie, Esc – wyjście.")),
            (theme::icon::BOOKMARKS, crate::i18n::tr("Biblioteka i skróty"), crate::i18n::tr("Biblioteka (B) trzyma zapisane trasy, ulubione miejsca i widoki. F11 – pełny ekran, ? – wszystkie skróty. Przewodnik obejrzysz ponownie z ustawień.")),
        ];
        let step = step.min(steps.len() - 1);
        let (glyph, title, body) = steps[step];
        let mut next = Some(step);
        egui::Modal::new(egui::Id::new("przewodnik")).frame(theme::glass().inner_margin(egui::Margin::same(22))).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(glyph).size(30.0).color(theme::BRASS));
                ui.vertical(|ui| {
                    ui.label(RichText::new(tf!("Krok {} z {}", step + 1, steps.len())).small().color(theme::PAPER_3));
                    ui.label(RichText::new(tr(title)).size(20.0).strong());
                });
            });
            ui.add_space(10.0);
            ui.add(egui::Label::new(RichText::new(tr(body)).size(15.0).color(theme::PAPER_2)).wrap());
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                // postęp: kropki
                for i in 0..steps.len() {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(r.center(), 4.0, if i == step { theme::BRASS } else { theme::INK_4 });
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let last = step + 1 == steps.len();
                    if ui.add(theme::primary(if last { crate::i18n::tr("Zaczynamy") } else { crate::i18n::tr("Dalej") })).clicked() {
                        next = if last { None } else { Some(step + 1) };
                    }
                    if step > 0 && ui.button(tr("Wstecz")).clicked() {
                        next = Some(step - 1);
                    }
                    if !last && ui.button(tr("Pomiń")).clicked() {
                        next = None;
                    }
                });
            });
        });
        self.tour = next;
        if next.is_none() && !self.settings.tour_done {
            self.settings.tour_done = true;
            let _ = self.settings.save();
        }
    }

    fn capture_frame(&mut self, ctx: &egui::Context) {
        let Some((path, at, after, sent)) = &mut self.capture else {
            return;
        };
        ctx.request_repaint_after(Duration::from_millis(100));
        if !*sent && at.elapsed().as_secs_f32() > *after {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            *sent = true;
        }
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| {
                if let egui::Event::Screenshot { image, .. } = e {
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        if let Some(img) = shot {
            let [w, h] = img.size;
            let raw: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, raw) {
                if let Err(e) = buf.save(&*path) {
                    log::error!("zrzut: {e}");
                }
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, PREFS_KEY, &self.atlas.save_prefs());
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.capture_frame(ctx);
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Comma)) {
            self.open_settings(Page::General);
        }
        if self.last_gc.elapsed() > Duration::from_secs(120) {
            self.collect_garbage();
        }
        // ślad GPX upuszczony na okno
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        for p in dropped {
            if crate::formats::is_track_file(&p) {
                self.atlas.import_track(&p);
            } else {
                self.atlas.notify(crate::i18n::tr("GRAŃ wczytuje pliki GPX, KML i GeoJSON."));
            }
        }
        let r = std::mem::take(&mut self.atlas.requests);
        if r.open_settings {
            self.open_settings(Page::Data);
        }
        if r.check_updates {
            self.check_updates(true);
        }
        self.update_ui(ctx);

        self.top_bar(ctx);
        crate::ui::panels::sidebar(&mut self.atlas, ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::INK_0))
            .show(ctx, |ui| {
                self.atlas.view_ui(ui, frame);
            });
        crate::ui::panels::nav(&mut self.atlas, ctx);
        crate::ui::panels::profile_panel(&mut self.atlas, ctx);
        crate::ui::search::show(&mut self.atlas, ctx);
        crate::ui::panels::help(&mut self.atlas, ctx);
        crate::ui::panels::load_window(&mut self.atlas, ctx);
        self.settings_window(ctx);
        self.moving_overlay(ctx);
        self.feedback.show(ctx);
        self.tour_ui(ctx);
    }
}

/// Przeniesienie zawartości folderu: zmiana nazwy na tym samym dysku, kopia + usunięcie między dyskami.
fn move_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let target = to.join(e.file_name());
        if std::fs::rename(e.path(), &target).is_err() {
            copy_tree(&e.path(), &target)?;
            if e.file_type()?.is_dir() {
                std::fs::remove_dir_all(e.path())?;
            } else {
                std::fs::remove_file(e.path())?;
            }
        }
    }
    let _ = std::fs::remove_dir_all(from);
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for e in std::fs::read_dir(from)? {
            let e = e?;
            copy_tree(&e.path(), &to.join(e.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}
