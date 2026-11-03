//! Główna aplikacja: kreator przy pierwszym starcie, potem atlas 3D; menedżer map (mapa 2D z panelem
//! danych) pod klawiszem D. Pobieranie obszarów idzie kolejką w tle; sprzątacz pilnuje limitu rozmiaru.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText};

use crate::areas::{self, Area, Level};
use crate::download::{fmt_duration, Job, JobState, Progress};
use crate::engine::{self, Engine};
use crate::geo::{BBox, TileId};
use crate::library;
use crate::settings::{data_dir_in, Settings, LIMIT_STEPS_GB};
use crate::store::{detect, fmt_bytes, free_space, legacy_tiles, Layer, Store, IMPORTED_MARK};
use crate::ui::map::{MapLayer, MapView, Overlay};
use crate::ui::theme;
use crate::ui::wizard::{self, DownloadView, Wizard};
use crate::heights::Heights;
use crate::osm::SectorId;
use crate::ui::atlas::Atlas;
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

struct NewArea {
    bbox: BBox,
    name: String,
    level: Level,
    /// Plan dla każdego poziomu liczony w tle: (bajty do pobrania, kafle do pobrania, część już obecna).
    plans: Arc<Mutex<HashMap<Level, (u64, u64, f32)>>>,
}

/// Przeniesienie folderu danych w tle.
struct Moving {
    to: PathBuf,
    result: Arc<Mutex<Option<Result<(), String>>>>,
    settings: Settings,
}

pub struct App {
    settings: Settings,
    store: Store,
    areas: Vec<Area>,
    map: MapView,
    wizard: Option<Wizard>,
    engine: Engine,
    /// Import kafli terenu wersji 1.x z katalogu `.cache\dem`.
    import: Option<(PathBuf, Arc<Job>)>,
    /// Część każdego obszaru obecna w bibliotece, liczona w tle.
    area_cov: Arc<Mutex<HashMap<String, f32>>>,
    last_cov: Option<Instant>,
    usage: Arc<Usage>,
    last_gc: Instant,
    selecting: bool,
    new_area: Option<NewArea>,
    settings_open: Option<Settings>,
    moving: Option<Moving>,
    hover: Option<[f64; 2]>,
    toast: Option<(String, Instant)>,
    /// Zrzut okna do pliku (GRAN_CAPTURE=plik.png, GRAN_CAPTURE_AFTER=sekundy) – zrzuty do dokumentacji i testy.
    capture: Option<(PathBuf, Instant, f32, bool)>,
    heights: Heights,
    world: World,
    atlas: Atlas,
    /// Menedżer map (mapa 2D z obszarami) zamiast widoku 3D.
    manager: bool,
    update: Update,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let settings = Settings::load();
        let store = Store::new(&settings.data_dir);
        store.start_scan();
        let areas = areas::load(store.root());
        let map = MapView::new(store.clone(), &cc.egui_ctx);
        let heights = Heights::new(store.clone());
        let world = World::new(heights.clone(), cc.egui_ctx.clone());
        let prefs = cc.storage.and_then(|s| eframe::get_value::<crate::ui::atlas::Prefs>(s, "atlas"));
        let atlas = Atlas::new(heights.clone(), world.clone(), prefs);
        updater::cleanup();
        let mut app = Self {
            heights,
            world,
            atlas,
            manager: false,
            update: Update::Idle,
            wizard: (!settings.setup_done).then(|| Wizard::new(settings.clone())),
            engine: Engine::new(store.clone()),
            import: None,
            area_cov: Arc::default(),
            last_cov: None,
            settings,
            store,
            areas,
            map,
            usage: Arc::new(Usage {
                bytes: AtomicU64::new(0),
                known: AtomicBool::new(false),
                busy: AtomicBool::new(false),
            }),
            last_gc: Instant::now(),
            selecting: false,
            new_area: None,
            settings_open: None,
            moving: None,
            hover: None,
            toast: None,
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
        if let (Some(w), Ok(page)) = (&mut app.wizard, std::env::var("GRAN_WIZARD_PAGE")) {
            w.jump(&page);
        }
        if app.wizard.is_none() {
            // dokończenie przerwanych pobrań z poprzedniej sesji
            app.resume_downloads();
            app.queue_import();
            app.refresh_usage();
            // link gran:// z wiersza poleceń (kliknięty w systemie)
            if let Some(l) = std::env::args().find(|a| a.starts_with("gran://")) {
                app.atlas.apply_link(&l);
            }
            let sectors = sectors_of(&app.areas);
            app.world.persist(sectors);
            if app.settings.auto_update && std::env::var_os("GRAN_CAPTURE").is_none() {
                app.check_updates(false);
            }
        }
        app
    }

    // ------------------------------------------------------------------ dane

    fn save_areas(&self) {
        if let Err(e) = areas::save(self.store.root(), &self.areas) {
            log::error!("zapis listy obszarów: {e}");
        }
    }

    fn refresh_usage(&self) {
        if self.usage.busy.swap(true, Ordering::Relaxed) {
            return;
        }
        let (usage, store) = (self.usage.clone(), self.store.clone());
        thread::spawn(move || {
            usage.bytes.store(store.usage(), Ordering::Relaxed);
            usage.known.store(true, Ordering::Relaxed);
            usage.busy.store(false, Ordering::Relaxed);
        });
    }

    fn collect_garbage(&mut self) {
        self.last_gc = Instant::now();
        let (store, areas, limit, usage) = (
            self.store.clone(),
            self.areas.clone(),
            self.settings.limit_bytes(),
            self.usage.clone(),
        );
        thread::spawn(move || {
            let (n, freed) = store.collect_garbage(limit, &areas);
            if n > 0 {
                log::info!("sprzątanie: usunięto {n} kafli ({})", fmt_bytes(freed));
            }
            usage.bytes.store(store.usage(), Ordering::Relaxed);
            usage.known.store(true, Ordering::Relaxed);
        });
    }

    /// Model globalny i obszary niedokończone: plan liczy tylko to, czego brakuje.
    fn resume_downloads(&mut self) {
        self.engine.add_base();
        for a in self.areas.iter().filter(|a| !a.complete) {
            self.engine.add_area(a);
        }
    }

    /// Odświeżenie stanu biblioteki w menedżerze: mapa pokrycia i procent obszarów.
    fn refresh_library(&mut self) {
        if self.last_cov.is_some_and(|t| t.elapsed() < Duration::from_secs(15)) {
            return;
        }
        self.last_cov = Some(Instant::now());
        // poziom 11 ma każdy obszar, a komórka ~15 km jest dość dokładna na całą Europę
        self.map.coverage = self.store.covered_tiles(Layer::Sat, 11).iter().map(|t: &TileId| t.bbox()).collect();
        let (store, areas, cov) = (self.store.clone(), self.areas.clone(), self.area_cov.clone());
        thread::spawn(move || {
            for a in areas {
                let f = library::plan(&a.bbox, a.level, |l, t| store.has(l, t)).fraction();
                cov.lock().unwrap().insert(a.id, f);
            }
        });
    }

    fn pump_queue(&mut self) {
        if let Some((dir, job)) = self.import.clone() {
            let p = job.progress();
            if p.state != JobState::Running {
                if p.state == JobState::Done {
                    let _ = std::fs::write(dir.join(IMPORTED_MARK), b"");
                    self.notify(format!(
                        "Zaimportowano dane wersji 1.x: {} kafli terenu.",
                        p.done
                    ));
                }
                self.import = None;
                self.collect_garbage();
            }
        }
        for (id, label, p) in self.engine.take_finished() {
            if let Some(a) = self.areas.iter_mut().find(|a| a.id == id) {
                a.tiles_done += p.done as u64;
                a.complete = p.state == JobState::Done;
                self.save_areas();
                self.notify(match p.state {
                    JobState::Done => format!("Obszar gotowy do pracy bez internetu: {label}"),
                    _ => format!(
                        "Nie wszystko udało się pobrać ({} kafli). Spróbuj ponownie później.",
                        p.failed
                    ),
                });
            }
            self.last_cov = None;
            self.collect_garbage();
        }
    }

    fn enqueue_area(&mut self, a: Area) {
        self.world.persist(sectors_of(std::slice::from_ref(&a)));
        self.engine.add_area(&a);
        self.areas.push(a);
        self.save_areas();
    }

    fn delete_area(&mut self, id: &str) {
        self.engine.cancel(id);
        if let Some(i) = self.areas.iter().position(|a| a.id == id) {
            let a = self.areas.remove(i);
            self.save_areas();
            let (store, others, usage) =
                (self.store.clone(), self.areas.clone(), self.usage.clone());
            thread::spawn(move || {
                store.remove_area_tiles(&a, &others);
                usage.bytes.store(store.usage(), Ordering::Relaxed);
            });
        }
    }

    fn notify(&mut self, msg: String) {
        self.toast = Some((msg, Instant::now()));
    }

    // ------------------------------------------------------------------ ustawienia i przenoszenie

    fn apply_settings(&mut self, next: Settings) {
        let moved = next.data_dir != self.settings.data_dir;
        if moved
            && Path::new(&self.settings.data_dir).exists()
            && self.usage.bytes.load(Ordering::Relaxed) > 0
        {
            let r = rfd::MessageDialog::new()
                .set_title("GRAŃ")
                .set_description(format!(
                    "Przenieść pobrane dane do nowego folderu?\n\nZ: {}\nDo: {}\n\nTak – zapisane obszary i kafle trafią do nowego folderu, a stary zostanie usunięty.\nNie – nowy folder zacznie się wypełniać od początku, stare dane zostaną w poprzednim miejscu.",
                    self.settings.data_dir.display(),
                    next.data_dir.display()
                ))
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
        self.settings = next;
        if let Err(e) = self.settings.save() {
            self.notify(format!("Nie udało się zapisać ustawień: {e}"));
        }
        if moved {
            self.store = Store::new(&self.settings.data_dir);
            self.store.start_scan();
            self.areas = areas::load(self.store.root());
            self.map.set_store(self.store.clone());
            self.heights.set_store(self.store.clone());
            self.world.reset();
            if let Some(r) = &mut self.atlas.renderer {
                r.tiles.reset();
            }
            self.engine = Engine::new(self.store.clone());
            self.resume_downloads();
        }
        self.queue_import();
        self.collect_garbage();
    }

    /// Kafle wersji 1.x we wskazanym folderze → import w tle (jednorazowo).
    fn queue_import(&mut self) {
        if let Some((dir, _)) = detect(&self.settings.data_dir).legacy_dem {
            if self.import.is_none() {
                let files = legacy_tiles(&dir);
                if !files.is_empty() {
                    self.import = Some((dir, Job::import_legacy(self.store.clone(), files)));
                }
            }
        }
    }

    fn start_move(&mut self, next: Settings) {
        if let Some((_, job)) = self.import.take() {
            job.cancel();
        }
        self.engine.shutdown();
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

    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top")
            .frame(
                egui::Frame::new()
                    .fill(theme::INK_0)
                    .inner_margin(egui::Margin::symmetric(16, 10)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Grań").size(22.0).strong());
                    ui.label(theme::kicker("biblioteka map"));
                    ui.add_space(20.0);
                    if ui.add(theme::primary(&theme::ic(theme::icon::CUBE, "Atlas 3D"))).on_hover_text("Powrót do widoku 3D (D, Esc)").clicked() {
                        self.manager = false;
                    }
                    ui.add_space(10.0);
                    ui.selectable_value(&mut self.map.layer, MapLayer::Relief, "Relief");
                    ui.selectable_value(&mut self.map.layer, MapLayer::Satellite, "Satelita");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button(theme::ic(theme::icon::GEAR, "Ustawienia"))
                            .on_hover_text("Folder i rozmiar danych map (Ctrl+,)")
                            .clicked()
                        {
                            self.settings_open = Some(self.settings.clone());
                        }
                        if let Some([lon, lat]) = self.hover {
                            ui.label(
                                RichText::new(format!("{lat:.5}° N  {lon:.5}° E"))
                                    .monospace()
                                    .color(theme::PAPER_3),
                            );
                        }
                    });
                });
            });
    }

    fn data_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("data").resizable(false).exact_width(340.0).frame(egui::Frame::new().fill(theme::INK_1).inner_margin(egui::Margin::same(16))).show(ctx, |ui| {
            ui.label(theme::kicker("Dane map"));
            let used = self.usage.bytes.load(Ordering::Relaxed);
            let limit = self.settings.limit_bytes();
            ui.horizontal(|ui| {
                let txt = if self.usage.known.load(Ordering::Relaxed) { fmt_bytes(used) } else { "…".into() };
                ui.label(RichText::new(txt).size(22.0).strong());
                ui.label(RichText::new(format!("z {} GB", self.settings.limit_gb)).color(theme::PAPER_3));
            });
            theme::bar(ui, used as f32 / limit as f32, if used > limit { theme::SIGNAL } else { theme::BRASS });
            ui.label(RichText::new(self.settings.data_dir.display().to_string()).small().monospace().color(theme::PAPER_3));
            ui.add_space(12.0);

            if self.engine.busy() {
                self.downloads_card(ui);
                ui.add_space(8.0);
            }
            ui.horizontal(|ui| {
                ui.label(theme::kicker("Obszary w bibliotece"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if self.selecting { theme::ic(theme::icon::CROSSHAIR, "Zaznacz na mapie…") } else { theme::ic(theme::icon::PLUS, "Dodaj") };
                    if ui.add(egui::Button::new(label).selected(self.selecting)).on_hover_text("Przeciągnij prostokąt na mapie").clicked() {
                        self.selecting = !self.selecting;
                    }
                });
            });
            if let Some((_, job)) = &self.import {
                let p = job.progress();
                theme::card(false).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("Import danych wersji 1.x").strong());
                    theme::bar(ui, p.fraction(), theme::TEAL);
                    ui.label(RichText::new(format!("{} z {} kafli", p.done, p.total)).small().monospace().color(theme::PAPER_3));
                });
            }
            if let Some(p) = self.engine.progress(engine::BASE) {
                theme::card(false).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("Model globalny Europy").strong());
                    theme::bar(ui, p.fraction(), theme::BRASS);
                });
            }
            let mut delete = None;
            let mut retry = None;
            let mut show3d = None;
            let mut upgrade = None;
            let cov = self.area_cov.lock().unwrap().clone();
            egui::ScrollArea::vertical().show(ui, |ui| {
                if self.areas.is_empty() {
                    ui.label(RichText::new("Brak zapisanych obszarów. Kliknij „Dodaj” i zaznacz prostokąt na mapie, a teren i zdjęcia będą dostępne bez internetu.").color(theme::PAPER_3));
                }
                for a in &self.areas {
                    let p = self.engine.progress(&a.id);
                    theme::card(false).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            if ui.link(RichText::new(&a.name).strong()).on_hover_text("Pokaż na mapie").clicked() {
                                self.map.fit(&a.bbox, egui::vec2(900.0, 700.0));
                            }
                            if ui.small_button(theme::icon::CUBE).on_hover_text("Pokaż w atlasie 3D").clicked() {
                                show3d = Some(a.bbox);
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button(theme::icon::TRASH).on_hover_text("Usuń obszar i jego kafle").clicked() {
                                    delete = Some(a.id.clone());
                                }
                                if a.level < Level::Lidar {
                                    ui.menu_button(theme::icon::ARROW_UP, |ui| {
                                        for l in Level::ALL.into_iter().filter(|l| *l > a.level) {
                                            if ui.button(l.name()).on_hover_text(l.hint()).clicked() {
                                                upgrade = Some((a.id.clone(), l));
                                                ui.close();
                                            }
                                        }
                                    }).response.on_hover_text("Podnieś poziom: pobierze tylko brakujące dane");
                                }
                                if let Some(p) = &p {
                                    let paused = p.state == JobState::Paused;
                                    if ui.small_button(if paused { theme::icon::PLAY } else { theme::icon::PAUSE }).on_hover_text(if paused { "Wznów" } else { "Wstrzymaj" }).clicked() {
                                        self.engine.set_paused(&a.id, !paused);
                                    }
                                }
                                if !a.complete && p.is_none() && ui.small_button(theme::icon::DOWNLOAD_SIMPLE).on_hover_text("Dokończ pobieranie").clicked() {
                                    retry = Some(a.id.clone());
                                }
                            });
                        });
                        let share = cov.get(&a.id).map(|f| format!(" · w bibliotece {:.0}%", f * 100.0)).unwrap_or_default();
                        ui.label(RichText::new(format!("{} · {:.0} km²{share}", a.level.name(), a.bbox.km2())).small().color(theme::PAPER_3));
                        match (&p, a.complete) {
                            (Some(p), _) => {
                                theme::bar(ui, p.fraction(), theme::BRASS);
                                let eta = p.eta.map(|e| format!(" · zostało {}", fmt_duration(e))).unwrap_or_default();
                                ui.label(RichText::new(format!("{:.0}%{eta}", p.fraction() * 100.0)).small().monospace().color(theme::PAPER_3));
                            }
                            (None, true) => {
                                ui.label(RichText::new(theme::ic(theme::icon::CHECK_CIRCLE, "dostępny bez internetu")).small().color(theme::OK));
                            }
                            (None, false) => {
                                ui.label(RichText::new(theme::ic(theme::icon::WARNING, "niepełny – pobierz brakujące")).small().color(theme::WARN));
                            }
                        }
                    });
                }
            });
            if let Some(id) = delete {
                let name = self.areas.iter().find(|a| a.id == id).map(|a| a.name.clone()).unwrap_or_default();
                let ok = rfd::MessageDialog::new().set_title("GRAŃ").set_description(format!("Usunąć obszar „{name}” i jego kafle z dysku?")).set_buttons(rfd::MessageButtons::YesNo).show();
                if ok == rfd::MessageDialogResult::Yes {
                    self.delete_area(&id);
                }
            }
            if let Some(id) = retry {
                if let Some(a) = self.areas.iter().find(|a| a.id == id) {
                    self.engine.add_area(a);
                }
            }
            if let Some((id, level)) = upgrade {
                self.engine.cancel(&id);
                if let Some(a) = self.areas.iter_mut().find(|a| a.id == id) {
                    a.level = level;
                    a.complete = false;
                }
                if let Some(a) = self.areas.iter().find(|a| a.id == id) {
                    self.engine.add_area(a);
                }
                self.save_areas();
                self.last_cov = None;
            }
            if let Some(b) = show3d {
                self.atlas.fit_bbox(&b);
                self.manager = false;
            }
        });
    }

    /// Postęp całego pobierania: prędkość, pauza i stan każdego serwera.
    fn downloads_card(&self, ui: &mut egui::Ui) {
        let st = self.engine.stats();
        theme::card(true).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(theme::kicker("Pobieranie"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (glyph, tip) = if st.paused { (theme::icon::PLAY, "Wznów wszystko") } else { (theme::icon::PAUSE, "Wstrzymaj wszystko") };
                    if ui.small_button(glyph).on_hover_text(tip).clicked() {
                        self.engine.set_all_paused(!st.paused);
                    }
                });
            });
            let frac = st.done as f32 / st.total.max(1) as f32;
            theme::bar(ui, frac, theme::BRASS);
            let left = st.total.saturating_sub(st.done);
            let eta = (st.tiles_per_s > 0.1).then(|| fmt_duration(Duration::from_secs_f64(left as f64 / st.tiles_per_s)));
            ui.label(RichText::new(format!(
                "{} · {:.0} kafli/s · {} kafli{}",
                fmt_rate(st.bytes_per_s),
                st.tiles_per_s,
                left,
                eta.map(|e| format!(" · zostało {e}")).unwrap_or_default()
            )).small().monospace().color(theme::PAPER_2));
            for (s, (name, _)) in st.sources.iter().zip(engine::SOURCES) {
                if s.active + s.queued == 0 && s.last_error.is_none() {
                    continue;
                }
                let line = ui.label(RichText::new(format!("{name}: {} połączeń · w kolejce {}", s.active, s.queued)).small().color(if s.last_error.is_some() { theme::WARN } else { theme::PAPER_3 }));
                if let Some(e) = &s.last_error {
                    line.on_hover_text(format!("Ostatni błąd: {e}"));
                }
            }
        });
    }

    fn new_area_window(&mut self, ctx: &egui::Context) {
        let Some(na) = &mut self.new_area else { return };
        let mut open = true;
        let mut add = false;
        egui::Window::new("Nowy obszar w bibliotece").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).open(&mut open).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(theme::kicker("Nazwa"));
            ui.text_edit_singleline(&mut na.name);
            ui.label(RichText::new(format!("{} · {:.0} km²", na.bbox.label(), na.bbox.km2())).small().color(theme::PAPER_3));
            ui.add_space(6.0);
            ui.label(theme::kicker("Poziom szczegółów"));
            let plans = na.plans.lock().unwrap().clone();
            if plans.len() < Level::ALL.len() {
                ui.ctx().request_repaint_after(Duration::from_millis(200));
            }
            for l in Level::ALL {
                let txt = match plans.get(&l) {
                    Some((_, 0, _)) => format!("{} – wszystko już w bibliotece", l.name()),
                    Some((bytes, _, have)) if *have > 0.0 => format!("{} – do pobrania ~{} · już masz {:.0}%", l.name(), fmt_bytes(*bytes), have * 100.0),
                    Some((bytes, _, _)) => format!("{} – do pobrania ~{}", l.name(), fmt_bytes(*bytes)),
                    None => format!("{} – liczenie…", l.name()),
                };
                ui.radio_value(&mut na.level, l, txt);
                ui.label(RichText::new(l.hint()).small().color(theme::PAPER_3));
            }
            let free = free_space(&self.settings.data_dir);
            let bytes = plans.get(&na.level).map_or_else(|| areas::estimate(&na.bbox, na.level).bytes, |p| p.0);
            let covered = library::covered_by(&self.areas, &na.bbox, na.level).map(|a| a.name.clone());
            if let Some(name) = &covered {
                ui.label(RichText::new(theme::ic(theme::icon::CHECK_CIRCLE, &format!("Masz już ten teren w obszarze „{name}”."))).color(theme::OK));
            }
            if free.is_some_and(|f| bytes > f) {
                ui.label(RichText::new("Za mało wolnego miejsca na dysku.").color(theme::SIGNAL));
            } else if bytes + self.usage.bytes.load(Ordering::Relaxed) > self.settings.limit_bytes() {
                ui.label(RichText::new("Obszar przekroczy limit danych – zwiększ go w ustawieniach, inaczej zabraknie miejsca na kafle oglądanych okolic.").color(theme::WARN));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                add = ui.add_enabled(covered.is_none(), theme::primary(&theme::ic(theme::icon::DOWNLOAD_SIMPLE, "Pobierz"))).clicked();
            });
        });
        if add {
            let na = self.new_area.take().unwrap();
            let name = if na.name.trim().is_empty() {
                "Mój obszar".to_string()
            } else {
                na.name.trim().to_string()
            };
            self.enqueue_area(Area::new(&name, na.bbox, na.level));
        } else if !open {
            self.new_area = None;
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        let Some(s) = &mut self.settings_open else {
            return;
        };
        let mut open = true;
        let mut save = false;
        egui::Window::new("Dane map").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).open(&mut open).show(ctx, |ui| {
            ui.set_width(520.0);
            ui.label(theme::kicker("Folder na dane map"));
            ui.label(RichText::new(s.data_dir.display().to_string()).monospace());
            if ui.button("Wybierz folder…").clicked() {
                if let Some(p) = rfd::FileDialog::new().set_title("Folder na dane map GRANIA").pick_folder() {
                    s.data_dir = data_dir_in(&p);
                }
            }
            if let Some(f) = free_space(&s.data_dir) {
                ui.label(RichText::new(format!("Wolne miejsce: {}", fmt_bytes(f))).small().color(theme::PAPER_3));
            }
            if s.data_dir != self.settings.data_dir {
                if let Some(msg) = detect(&s.data_dir).describe() {
                    ui.label(RichText::new(msg).small().color(theme::OK));
                }
            }
            ui.add_space(8.0);
            ui.label(theme::kicker("Maksymalny rozmiar danych"));
            let mut idx = LIMIT_STEPS_GB.iter().position(|&g| g >= s.limit_gb).unwrap_or(5);
            ui.horizontal(|ui| {
                if ui.add(egui::Slider::new(&mut idx, 0..=LIMIT_STEPS_GB.len() - 1).show_value(false)).changed() {
                    s.limit_gb = LIMIT_STEPS_GB[idx];
                }
                ui.label(RichText::new(format!("{} GB", s.limit_gb)).strong());
            });
            ui.label(RichText::new("Po przekroczeniu limitu najdawniej oglądane kafle są usuwane; zapisane obszary zostają.").small().color(theme::PAPER_3));
            ui.add_space(8.0);
            ui.checkbox(&mut s.auto_update, "Sprawdzaj aktualizacje na redzikowski.com");
            ui.add_space(10.0);
            save = ui.add(theme::primary("Zapisz")).clicked();
        });
        if save {
            let next = self.settings_open.take().unwrap();
            self.apply_settings(next);
        } else if !open {
            self.settings_open = None;
        }
    }

    fn moving_overlay(&mut self, ctx: &egui::Context) {
        let Some(m) = &self.moving else { return };
        let done = m.result.lock().unwrap().take();
        egui::Window::new("Przenoszenie danych")
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("Przenoszenie danych map do {}…", m.to.display()));
                });
            });
        ctx.request_repaint_after(Duration::from_millis(200));
        if let Some(r) = done {
            let m = self.moving.take().unwrap();
            match r {
                Ok(()) => self.commit_settings(m.settings),
                Err(e) => {
                    rfd::MessageDialog::new().set_title("GRAŃ").set_level(rfd::MessageLevel::Error).set_description(format!("Nie udało się przenieść danych:\n{e}\n\nDane, których nie przeniesiono, zostały w dotychczasowym folderze.")).show();
                    self.engine = Engine::new(self.store.clone());
                    self.resume_downloads();
                }
            }
        }
    }
}

impl App {
    fn toast_ui(&mut self, ui: &mut egui::Ui) {
        if let Some((msg, at)) = &self.toast {
            if at.elapsed() < Duration::from_secs(5) {
                let rect = ui.max_rect();
                egui::Area::new(egui::Id::new("toast"))
                    .fixed_pos(rect.center_bottom() + egui::vec2(-200.0, -110.0))
                    .show(ui.ctx(), |ui| {
                        theme::card(true).show(ui, |ui| {
                            ui.set_width(400.0);
                            ui.label(msg);
                        });
                    });
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            }
        }
    }

    fn atlas_top_bar(&mut self, ctx: &egui::Context) {
        let mut offer = None;
        egui::TopBottomPanel::top("top3d")
            .frame(egui::Frame::new().fill(theme::INK_0).inner_margin(egui::Margin::symmetric(16, 8)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Grań").size(22.0).strong());
                    ui.label(theme::kicker("atlas szlaków 3D"));
                    ui.add_space(16.0);
                    let search = egui::Button::new(RichText::new(theme::ic(theme::icon::MAGNIFYING_GLASS, "Szukaj szczytu, szlaku, miejscowości…   /")).color(theme::PAPER_3))
                        .min_size(egui::vec2(320.0, 30.0));
                    if ui.add(search).clicked() {
                        self.atlas.search = Some(Default::default());
                    }
                    ui.add_space(10.0);
                    let style = &mut self.atlas.prefs.style;
                    egui::ComboBox::from_id_salt("styl")
                        .selected_text(style.label())
                        .show_ui(ui, |ui| {
                            for s in crate::ui::atlas::Style::ALL {
                                ui.selectable_value(style, s, s.label());
                            }
                        });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(theme::icon::QUESTION).on_hover_text("Skróty i sterowanie (?)").clicked() {
                            self.atlas.help = !self.atlas.help;
                        }
                        if ui.button(theme::icon::GEAR).on_hover_text("Folder i rozmiar danych map (Ctrl+,)").clicked() {
                            self.settings_open = Some(self.settings.clone());
                        }
                        let label = if self.engine.busy() {
                            let st = self.engine.stats();
                            let pct = st.done as f32 / st.total.max(1) as f32 * 100.0;
                            format!("{}  {pct:.0}% · {}", theme::icon::DOWNLOAD_SIMPLE, fmt_rate(st.bytes_per_s))
                        } else {
                            theme::ic(theme::icon::MAP_TRIFOLD, "Biblioteka")
                        };
                        if ui.button(label).on_hover_text("Pobrane obszary, pobieranie, zajętość dysku (D)").clicked() {
                            self.manager = true;
                        }
                        if !self.atlas.prefs.panel_open && ui.button(theme::icon::SIDEBAR).on_hover_text("P").clicked() {
                            self.atlas.prefs.panel_open = true;
                        }
                        if let Update::Available(rel) = &self.update {
                            if ui.add(theme::primary(&format!("Aktualizacja {}", rel.version))).clicked() {
                                offer = Some(rel.clone());
                            }
                        }
                    });
                });
            });
        if let Some(rel) = offer {
            self.offer_update(rel);
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
            .set_title("GRAŃ – aktualizacja")
            .set_description(format!(
                "Dostępna jest nowa wersja {}. Masz wersję {}.\n\nPobrać i zainstalować aktualizację{size}? Dane map i ustawienia zostaną zachowane. Instalacja wymaga potwierdzenia uprawnień administratora.",
                rel.version,
                updater::current()
            ))
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
                            self.toast = Some((
                                format!("Dostępna jest nowa wersja {} – przycisk „Aktualizacja” u góry.", rel.version),
                                Instant::now(),
                            ));
                            Ok(Update::Available(rel))
                        });
                    }
                    Some(Ok(_)) => {
                        if manual {
                            self.toast = Some((format!("Masz najnowszą wersję ({}).", updater::current()), Instant::now()));
                        }
                        next = Some(Ok(Update::Idle));
                    }
                    Some(Err(e)) => {
                        if manual {
                            self.toast = Some((format!("Nie udało się sprawdzić aktualizacji: {e}"), Instant::now()));
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
                egui::Window::new("Aktualizacja")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.set_width(380.0);
                        ui.label(format!("Pobieranie wersji {version}…"));
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
                                .set_title("GRAŃ")
                                .set_level(rfd::MessageLevel::Error)
                                .set_description(format!("Nie udało się zainstalować aktualizacji: {e}"))
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
        eframe::set_value(storage, "atlas", &self.atlas.save_prefs());
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.capture_frame(ctx);
        self.pump_queue();
        if self.engine.busy() || self.import.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }

        if let Some(wiz) = &mut self.wizard {
            let dl = self.engine.busy() || wiz_started(&self.areas);
            let view = dl.then(|| DownloadView {
                base: Some(self.engine.progress(engine::BASE).unwrap_or_else(Progress::complete)),
                areas: self
                    .areas
                    .iter()
                    .map(|a| (a, self.engine.progress(&a.id)))
                    .collect(),
            });
            match wiz.show(ctx, &mut self.map, view) {
                Some(wizard::Action::Start(settings, list)) => {
                    self.commit_settings(settings);
                    self.engine.add_base();
                    for a in list {
                        self.enqueue_area(a);
                    }
                }
                Some(wizard::Action::Finish) => {
                    self.wizard = None;
                    self.refresh_usage();
                }
                None => {}
            }
            return;
        }

        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Comma)) {
            self.settings_open = Some(self.settings.clone());
        }
        if self.last_gc.elapsed() > Duration::from_secs(600) {
            self.collect_garbage();
        }
        // ślad GPX upuszczony na okno
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        for p in dropped {
            if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("gpx")) {
                self.manager = false;
                self.atlas.import_gpx(&p);
            }
        }
        let r = std::mem::take(&mut self.atlas.requests);
        if r.open_data {
            self.manager = !self.manager;
        }
        if r.open_settings {
            self.settings_open = Some(self.settings.clone());
        }
        if r.check_updates {
            self.check_updates(true);
        }
        self.update_ui(ctx);

        if self.manager {
            if !ctx.wants_keyboard_input()
                && self.new_area.is_none()
                && !self.selecting
                && self.settings_open.is_none()
                && ctx.input(|i| i.key_pressed(egui::Key::D) || i.key_pressed(egui::Key::Escape))
            {
                self.manager = false;
            }
            self.refresh_library();
            self.top_bar(ctx);
            self.data_panel(ctx);
            egui::CentralPanel::default()
                .frame(egui::Frame::new())
                .show(ctx, |ui| {
                    let overlays: Vec<Overlay> = self
                        .areas
                        .iter()
                        .map(|a| Overlay {
                            bbox: a.bbox,
                            color: if a.complete { theme::OK } else { theme::BRASS },
                            label: a.name.clone(),
                            strong: false,
                        })
                        .collect();
                    let r = self.map.show(ui, &overlays, self.selecting);
                    self.hover = r.hover;
                    if let Some(b) = r.selected {
                        self.selecting = false;
                        if b.km2() > areas::MAX_AREA_KM2 {
                            self.notify(format!(
                                "Obszar za duży: {:.0} km² (limit {:.0} km²).",
                                b.km2(),
                                areas::MAX_AREA_KM2
                            ));
                        } else {
                            let plans: Arc<Mutex<HashMap<Level, (u64, u64, f32)>>> = Arc::default();
                            let (store, slot) = (self.store.clone(), plans.clone());
                            thread::spawn(move || {
                                for l in Level::ALL {
                                    let p = library::plan(&b, l, |layer, t| store.has(layer, t));
                                    slot.lock().unwrap().insert(l, (p.bytes_new, p.tiles_new, p.fraction()));
                                }
                            });
                            self.new_area = Some(NewArea {
                                bbox: b,
                                name: String::new(),
                                level: Level::Standard,
                                plans,
                            });
                        }
                    }
                    self.toast_ui(ui);
                });
        } else {
            self.atlas_top_bar(ctx);
            crate::ui::panels::toolbar(&mut self.atlas, ctx);
            crate::ui::panels::side_panel(&mut self.atlas, ctx);
            crate::ui::panels::profile_panel(&mut self.atlas, ctx);
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(theme::INK_0))
                .show(ctx, |ui| {
                    self.atlas.view_ui(ui, frame);
                    self.toast_ui(ui);
                });
            crate::ui::search::show(&mut self.atlas, ctx);
            crate::ui::panels::help(&mut self.atlas, ctx);
        }
        self.new_area_window(ctx);
        self.settings_window(ctx);
        self.moving_overlay(ctx);
    }
}

/// Sektory danych okolicy (szlaki, punkty) pokrywające zapisane obszary.
fn sectors_of(areas: &[Area]) -> Vec<SectorId> {
    let mut out = Vec::new();
    for a in areas {
        for t in a.bbox.tiles(crate::osm::SECTOR_Z) {
            out.push(SectorId { x: t.x, y: t.y });
        }
    }
    out.sort();
    out.dedup();
    out
}

fn fmt_rate(bytes_per_s: f64) -> String {
    format!("{:.1} MB/s", bytes_per_s / 1_048_576.0).replace('.', ",")
}

fn wiz_started(areas: &[Area]) -> bool {
    !areas.is_empty()
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
