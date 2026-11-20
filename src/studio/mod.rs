//! Studio – rozbudowane wycinki terenu na komputerze (Windows): modele 3D do druku i do programów
//! 3D, przekroje płaszczyzną, mapy wektorowe i eksport w wielu formatach.
//!
//! Dostęp mają tylko konta, którym redzikowski.com nadał go w panelu (`studio` w odpowiedzi
//! `/me`); bez tego program nie pokazuje Studio nigdzie.
//!
//! Studio nie korzysta z kafli map: teren bierze bezpośrednio z usług urzędowych (GUGiK, ČÚZK)
//! i z Copernicus DEM (licencje pozwalające na użycie komercyjne z podaniem źródła), a obiekty
//! z OpenStreetMap (ODbL) – jednym zapytaniem Overpass. Źródła są podpisane w każdym eksporcie.
//!
//! Przebieg: obszar wskazany na mapie ([`geom::Area`]) → przygotowanie danych w tle ([`Scene`]) →
//! wygląd, model, przekroje i mapy liczone z gotowej sceny → zapis plików w tle.

pub mod color;
pub mod contour;
pub mod draw;
pub mod features;
pub mod geom;
pub mod mapdraw;
pub mod mesh;
pub mod preview;
pub mod section;
pub mod terrain;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use color::Look;
use features::Features;
use geom::{Area, Frame, Region};
use mapdraw::MapOpts;
use mesh::ModelOpts;
use section::{Plane, SectionOpts};
use terrain::{Grid, Progress, Source, Used};

/// Studio działa tylko na komputerze z Windows.
pub const SUPPORTED: bool = cfg!(windows);

/// Czy pokazywać Studio temu użytkownikowi.
pub fn allowed(acc: &crate::account::Account) -> bool {
    SUPPORTED && acc.studio()
}

/// Co robi kliknięcie mapy przy otwartym Studio.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Pick {
    /// Nic – obszar zmienia się tylko na zakładce Obszar.
    #[default]
    None,
    /// Punkty obszaru (zakładka Obszar).
    Area,
    /// Linia A–B przekroju.
    Section,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Tab {
    #[default]
    Area,
    Terrain,
    Look,
    Model,
    Section,
    Map,
}

impl Tab {
    pub const ALL: [Tab; 6] = [Tab::Area, Tab::Terrain, Tab::Look, Tab::Model, Tab::Section, Tab::Map];
    pub fn label(self) -> &'static str {
        match self {
            Tab::Area => crate::i18n::tr("Obszar"),
            Tab::Terrain => crate::i18n::tr("Dane"),
            Tab::Look => crate::i18n::tr("Wygląd"),
            Tab::Model => crate::i18n::tr("Model 3D"),
            Tab::Section => crate::i18n::tr("Przekrój"),
            Tab::Map => crate::i18n::tr("Mapa SVG"),
        }
    }
    pub fn icon(self) -> &'static str {
        use crate::ui::theme::icon;
        match self {
            Tab::Area => icon::SELECTION,
            Tab::Terrain => icon::DOWNLOAD_SIMPLE,
            Tab::Look => icon::PALETTE,
            Tab::Model => icon::CUBE,
            Tab::Section => icon::KNIFE,
            Tab::Map => icon::MAP_TRIFOLD,
        }
    }
    /// Krótki opis pod nazwą zakładki.
    pub fn hint(self) -> &'static str {
        match self {
            Tab::Area => crate::i18n::tr("kształt na mapie"),
            Tab::Terrain => crate::i18n::tr("teren i obiekty"),
            Tab::Look => crate::i18n::tr("kolory i światło"),
            Tab::Model => crate::i18n::tr("bryła i materiały"),
            Tab::Section => crate::i18n::tr("profil i płaszczyzny"),
            Tab::Map => crate::i18n::tr("arkusz i warstwy"),
        }
    }
}

/// Wszystkie ustawienia Studio (zapisywane w `%APPDATA%\GRAN\studio.json`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Opts {
    pub area: Area,
    pub source: Source,
    /// Oczko siatki terenu (m); 0 – dobierane do wielkości obszaru.
    pub cell: f32,
    pub osm: bool,
    pub buildings: bool,
    pub smooth: u8,
    pub look: Look,
    pub model: ModelOpts,
    /// Dłuższy bok tekstury modelu (px).
    pub tex_px: u32,
    pub section: SectionOpts,
    pub map: MapOpts,
    pub tab: Tab,
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            area: Area::default(),
            source: Source::Best,
            cell: 0.0,
            osm: true,
            buildings: true,
            smooth: 0,
            look: Look::default(),
            model: ModelOpts::default(),
            tex_px: 4096,
            section: SectionOpts::default(),
            map: MapOpts::default(),
            tab: Tab::Area,
        }
    }
}

fn opts_file() -> PathBuf {
    crate::settings::config_dir().join("studio.json")
}

/// Przygotowane dane obszaru (teren i obiekty) – wspólne dla wszystkich wyników.
pub struct Scene {
    pub frame: Frame,
    pub area: Area,
    pub region: Region,
    pub grid: Grid,
    pub used: Used,
    pub feats: Option<Features>,
}

impl Scene {
    /// Podpisy źródeł danych do eksportów.
    pub fn credits(&self) -> Vec<String> {
        let mut v: Vec<String> = self.used.credits().into_iter().map(String::from).collect();
        if self.feats.is_some() {
            v.push(features::OSM_CREDIT.to_string());
        }
        v.push("GRAŃ Studio · redzikowski.com".to_string());
        v
    }

    /// Odległość ze znakiem z uwzględnieniem progu wysokości (ujemna wewnątrz wycinka).
    pub fn sd(&self, x: f64, y: f64, z: f32) -> f64 {
        let d = self.region.sd(x, y);
        match self.area.above {
            Some(t) => d.max((t - z) as f64),
            None => d,
        }
    }

    pub fn inside(&self, x: f64, y: f64) -> bool {
        self.sd(x, y, self.grid.sample(x, y)) <= 0.0
    }

    /// Materiały powierzchni bryły: pokrycie z OpenStreetMap, nachylenie, granica śniegu.
    pub fn materials(&self, m: &mut mesh::Mesh, opts: &ModelOpts) {
        if !opts.materials {
            return;
        }
        let (w, h) = color::tex_size(&self.grid, 1024);
        let cover = self.feats.as_ref().map(|f| color::cover_classes(f, &self.grid, w, h));
        m.assign(
            &|u, v| match &cover {
                Some(c) => {
                    let x = ((u * w as f32) as u32).min(w - 1);
                    let y = ((v * h as f32) as u32).min(h - 1);
                    c[(y * w + x) as usize]
                }
                None => 0,
            },
            opts,
        );
    }
}

/// Oczko siatki dobierane do obszaru: ~1500 węzłów na dłuższy bok, nie gęściej niż 1 m.
pub fn auto_cell(region: &Region) -> f64 {
    let b = region.bounds();
    let longest = (b[2] - b[0]).max(b[3] - b[1]);
    (longest / 1500.0).clamp(1.0, 90.0)
}

/// Wynik zadania w tle.
type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;

/// Który arkusz pokazuje podgląd wektorowy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SheetKind {
    Map,
    Section,
}

/// Wynik podglądu arkusza liczonego w tle.
pub struct SheetOut {
    img: Option<image::RgbaImage>,
    w: f64,
    h: f64,
    layers: Vec<draw::LayerInfo>,
    svg_len: usize,
}

/// Podgląd arkusza (mapy albo przekroju): obraz prawdziwego pliku SVG, warstwy, wymiary.
#[derive(Default)]
pub struct Sheet {
    /// Klucz ustawień pokazanego obrazu.
    pub key: u64,
    pub kind: Option<SheetKind>,
    pub tex: Option<eframe::egui::TextureHandle>,
    pub size_mm: (f64, f64),
    pub layers: Vec<draw::LayerInfo>,
    pub svg_len: usize,
    pub error: Option<String>,
    job: Option<(u64, SheetKind, Slot<SheetOut>)>,
    /// Powiększenie i przesunięcie podglądu (1 – cały arkusz).
    pub zoom: f32,
    pub pan: (f32, f32),
}

impl Sheet {
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
}

pub struct Studio {
    pub open: bool,
    pub pick: Pick,
    pub o: Opts,
    saved: Opts,
    pub scene: Option<Arc<Scene>>,
    job: Option<(Arc<Progress>, Slot<Scene>)>,
    export: Option<(Arc<Progress>, Slot<String>)>,
    pub error: Option<String>,
    pub notice: Option<String>,
    /// Ostatnio zapisany plik (przycisk „Pokaż w folderze”).
    pub last_file: Option<PathBuf>,
    pub view: preview::View,
    /// Podgląd: klucz ustawień i gotowa tekstura egui.
    pub preview: Option<(u64, eframe::egui::TextureHandle)>,
    /// Zmniejszona bryła i tekstura do podglądu (klucz ustawień).
    pub preview_data: Option<(u64, Arc<mesh::Mesh>, Arc<image::RgbaImage>)>,
    /// Płaski podgląd tekstury (zakładka Wygląd).
    pub flat: Option<(u64, eframe::egui::TextureHandle)>,
    /// Podgląd arkusza mapy albo przekroju.
    pub sheet: Sheet,
    /// Tekstura warstw skalnych na ściany modelu.
    strata: Arc<image::RgbaImage>,
}

impl Default for Studio {
    fn default() -> Self {
        Self::load()
    }
}

fn key_of(v: &impl Serialize, extra: u64) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(v).unwrap_or_default().hash(&mut h);
    extra.hash(&mut h);
    h.finish()
}

/// Rysunek liczony w tle (podgląd albo zapis); argument – górna granica dpi podkładu.
type Maker = Box<dyn FnOnce(Option<u32>) -> Result<draw::Drawing, String> + Send>;

impl Studio {
    pub fn load() -> Self {
        let o: Opts = std::fs::read(opts_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Self {
            open: false,
            pick: Pick::None,
            saved: o.clone(),
            o,
            scene: None,
            job: None,
            export: None,
            error: None,
            notice: None,
            last_file: None,
            view: preview::View::default(),
            preview: None,
            preview_data: None,
            flat: None,
            sheet: Sheet { zoom: 1.0, ..Default::default() },
            strata: Arc::new(mesh::strata_texture()),
        }
    }

    /// Zapis ustawień, gdy się zmieniły (raz na klatkę – tanie porównanie).
    pub fn persist(&mut self) {
        if self.o != self.saved {
            self.saved = self.o.clone();
            if let Ok(b) = serde_json::to_vec_pretty(&self.o) {
                let _ = std::fs::create_dir_all(crate::settings::config_dir());
                let _ = crate::pack::write_file(&opts_file(), &b);
            }
        }
    }

    /// Kliknięcie mapy przy narzędziu Studio (obszar tylko na zakładce Obszar).
    pub fn click(&mut self, lon: f64, lat: f64) {
        match self.pick {
            Pick::None => {}
            Pick::Area => self.o.area.add(lon, lat),
            Pick::Section => {
                let l = &mut self.o.section.line;
                if l.len() >= 2 {
                    l.clear();
                }
                l.push((lon, lat));
            }
        }
    }

    /// Usuwa ostatni punkt (Backspace).
    pub fn drop_last(&mut self) {
        match self.pick {
            Pick::None => {}
            Pick::Area => {
                self.o.area.pts.pop();
            }
            Pick::Section => {
                self.o.section.line.pop();
            }
        }
    }

    /// Punkty do przeciągania na mapie: obszar albo linia przekroju.
    pub fn handles(&self) -> &[(f64, f64)] {
        match self.pick {
            Pick::None => &[],
            Pick::Area => &self.o.area.pts,
            Pick::Section => &self.o.section.line,
        }
    }

    pub fn move_handle(&mut self, i: usize, lon: f64, lat: f64) {
        let v = match self.pick {
            Pick::None => return,
            Pick::Area => &mut self.o.area.pts,
            Pick::Section => &mut self.o.section.line,
        };
        if let Some(p) = v.get_mut(i) {
            *p = (lon, lat);
        }
    }

    /// Kształt obszaru w metrach (do opisu wielkości przed przygotowaniem danych).
    pub fn region(&self) -> Option<(Frame, Region)> {
        let c = self.o.area.center()?;
        let f = Frame::new(c.0, c.1);
        Some((f, self.o.area.local(&f)?))
    }

    pub fn cell(&self) -> Option<f64> {
        let (_, r) = self.region()?;
        Some(if self.o.cell > 0.0 { self.o.cell as f64 } else { auto_cell(&r) })
    }

    /// Scena nie odpowiada już wskazanemu obszarowi (trzeba przygotować dane od nowa).
    pub fn stale(&self) -> bool {
        match &self.scene {
            Some(s) => s.area.pts != self.o.area.pts || s.area.shape != self.o.area.shape || (s.area.shape == geom::Shape::Ridge && s.area.width != self.o.area.width),
            None => true,
        }
    }

    pub fn busy(&self) -> Option<(f32, String)> {
        self.job.as_ref().map(|(p, _)| p.frac.lock().unwrap().clone())
    }

    pub fn exporting(&self) -> Option<(f32, String)> {
        self.export.as_ref().map(|(p, _)| p.frac.lock().unwrap().clone())
    }

    pub fn cancel(&mut self) {
        if let Some((p, _)) = &self.job {
            p.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Przygotowanie danych obszaru w tle.
    pub fn prepare(&mut self, cache: PathBuf, ctx: &eframe::egui::Context) {
        let Some((frame, region)) = self.region() else { return };
        if self.job.is_some() {
            return;
        }
        let cell = self.cell().unwrap_or(10.0);
        let b = region.bounds();
        let longest = (b[2] - b[0]).max(b[3] - b[1]);
        if longest / cell > (terrain::MAX_SIDE - 8) as f64 {
            self.error = Some(crate::i18n::tr("Za gęsta siatka dla tak dużego obszaru – zwiększ oczko siatki.").into());
            return;
        }
        if longest > 120_000.0 {
            self.error = Some(crate::i18n::tr("Obszar jest za duży (najwyżej 120 km na dłuższym boku).").into());
            return;
        }
        self.error = None;
        let pad = cell * 3.0;
        let bounds = [b[0] - pad, b[1] - pad, b[2] + pad, b[3] + pad];
        let progress = Arc::new(Progress::default());
        let slot: Slot<Scene> = Arc::default();
        let (p, s) = (progress.clone(), slot.clone());
        let (area, source, osm, buildings, smooth) = (self.o.area.clone(), self.o.source, self.o.osm, self.o.buildings, self.o.smooth);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            p.set(0.02, crate::i18n::tr("Pobieranie terenu…"));
            let r = (|| -> Result<Scene, String> {
                let (mut grid, used) = terrain::build(&frame, bounds, cell, source, &cache.join("studio"), &p)?;
                if smooth > 0 {
                    grid.smooth(smooth);
                }
                let feats = if osm {
                    p.set(0.9, crate::i18n::tr("Obiekty z OpenStreetMap…"));
                    let geo = terrain::geo_bounds(&frame, bounds);
                    // budynki tylko do ~150 km² (inaczej odpowiedź Overpass jest ogromna)
                    let small = region.area_m2() < 150e6;
                    match features::fetch(&frame, geo, buildings && small) {
                        Ok(f) => Some(f),
                        Err(e) => {
                            log::warn!("studio: OSM: {e}");
                            None
                        }
                    }
                } else {
                    None
                };
                Ok(Scene { frame, area, region, grid, used, feats })
            })();
            *s.lock().unwrap() = Some(r);
            ctx.request_repaint();
        });
        self.job = Some((progress, slot));
    }

    /// Odbiór wyników zadań w tle (co klatkę).
    pub fn poll(&mut self, ctx: &eframe::egui::Context) {
        let done = self.job.as_ref().and_then(|(_, s)| s.lock().unwrap().take());
        if let Some(r) = done {
            self.job = None;
            match r {
                Ok(sc) => {
                    let osm_missing = self.o.osm && sc.feats.is_none();
                    self.scene = Some(Arc::new(sc));
                    self.preview = None;
                    self.preview_data = None;
                    self.flat = None;
                    self.sheet.key = 0;
                    self.notice = Some(if osm_missing {
                        crate::i18n::tr("Teren gotowy. Obiekty OpenStreetMap są chwilowo niedostępne – spróbuj ponownie później.").into()
                    } else {
                        crate::i18n::tr("Dane obszaru są gotowe.").into()
                    });
                    if self.o.tab == Tab::Area || self.o.tab == Tab::Terrain {
                        self.o.tab = Tab::Look;
                    }
                }
                Err(e) => self.error = Some(e),
            }
        } else if self.job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        let done = self.export.as_ref().and_then(|(_, s)| s.lock().unwrap().take());
        if let Some(r) = done {
            self.export = None;
            match r {
                Ok(m) => self.notice = Some(m),
                Err(e) => self.error = Some(e),
            }
        } else if self.export.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        // podgląd arkusza
        let done = self.sheet.job.as_ref().and_then(|(k, kind, s)| s.lock().unwrap().take().map(|r| (*k, *kind, r)));
        if let Some((key, kind, r)) = done {
            self.sheet.job = None;
            self.sheet.key = key;
            self.sheet.kind = Some(kind);
            match r {
                Ok(out) => {
                    self.sheet.error = None;
                    self.sheet.size_mm = (out.w, out.h);
                    self.sheet.layers = out.layers;
                    self.sheet.svg_len = out.svg_len;
                    match out.img {
                        Some(img) => {
                            let ci = eframe::egui::ColorImage::from_rgba_premultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                            self.sheet.tex = Some(ctx.load_texture("studio-sheet", ci, eframe::egui::TextureOptions::LINEAR));
                        }
                        None => self.sheet.error = Some(crate::i18n::tr("Podgląd arkusza jest niedostępny.").into()),
                    }
                }
                Err(e) => {
                    self.sheet.error = Some(e);
                    self.sheet.layers.clear();
                }
            }
            ctx.request_repaint();
        } else if self.sheet.job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }

    /// Klucz podglądu 3D (zmienia się z ustawieniami wyglądu i bryły).
    pub fn preview_key(&self) -> u64 {
        let id = self.scene.as_ref().map(|s| Arc::as_ptr(s) as u64).unwrap_or(0);
        key_of(&(&self.o.look, &self.o.model, &self.o.area.above), id)
    }

    pub fn flat_key(&self) -> u64 {
        let id = self.scene.as_ref().map(|s| Arc::as_ptr(s) as u64).unwrap_or(0);
        key_of(&(&self.o.look, &self.o.area.above, 7u8), id)
    }

    /// Dane podglądu 3D (zmniejszona bryła z materiałami i tekstura) – liczone, gdy zmieniły się ustawienia.
    pub fn preview_mesh(&mut self) -> Option<(Arc<mesh::Mesh>, Arc<image::RgbaImage>)> {
        let sc = self.scene.clone()?;
        let key = self.preview_key();
        if let Some((k, m, t)) = &self.preview_data {
            if *k == key {
                return Some((m.clone(), t.clone()));
            }
        }
        let b = sc.region.bounds();
        let opts = ModelOpts { cells: 160, ..self.o.model.clone() };
        let mut m = mesh::build(&sc.grid, &|x, y, z| sc.sd(x, y, z), &opts, b);
        sc.materials(&mut m, &opts);
        let m = Arc::new(m);
        let (w, h) = color::tex_size(&sc.grid, 768);
        let t = Arc::new(color::texture(&sc.grid, sc.feats.as_ref(), &self.o.look, w, h));
        self.preview_data = Some((key, m.clone(), t.clone()));
        Some((m, t))
    }

    /// Tekstura warstw skalnych (gdy ściany mają taki wygląd).
    pub fn strata(&self) -> Option<&image::RgbaImage> {
        (self.o.model.walls == mesh::Walls::Strata).then_some(self.strata.as_ref())
    }

    fn start_export(&mut self, label: &str, job: impl FnOnce(&Progress) -> Result<String, String> + Send + 'static) {
        if self.export.is_some() {
            return;
        }
        let progress = Arc::new(Progress::default());
        progress.set(0.05, label.to_string());
        let slot: Slot<String> = Arc::default();
        let (p, s) = (progress.clone(), slot.clone());
        std::thread::spawn(move || {
            let r = job(&p);
            *s.lock().unwrap() = Some(r);
        });
        self.export = Some((progress, slot));
    }

    fn stem(&self) -> String {
        let base = self.o.map.title.trim();
        let base = if base.is_empty() { "gran-studio" } else { base };
        base.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect::<String>().trim_matches('-').to_string()
    }

    /// Eksport modelu 3D w formacie wybranym w oknie zapisu.
    pub fn export_model(&mut self) {
        let Some(sc) = self.scene.clone() else { return };
        let formats = [
            ("glb", "glTF (GLB) – materiały PBR i tekstury"),
            ("obj", "OBJ + MTL + tekstury PNG"),
            ("stl", "STL – druk 3D"),
            ("ply", "PLY z kolorami"),
        ];
        let Some(path) = crate::platform::save_target(crate::i18n::tr("Zapisz model 3D"), &self.stem(), &formats) else { return };
        self.last_file = Some(path.clone());
        let (model, look, tex_px) = (self.o.model.clone(), self.o.look.clone(), self.o.tex_px);
        let strata = self.strata().cloned();
        self.start_export(crate::i18n::tr("Budowanie modelu…"), move |p| {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("stl").to_ascii_lowercase();
            let b = sc.region.bounds();
            let mut m = mesh::build(&sc.grid, &|x, y, z| sc.sd(x, y, z), &model, b);
            if m.top.is_empty() {
                return Err(crate::i18n::tr("Wycinek jest pusty – sprawdź obszar i próg wysokości.").into());
            }
            p.set(0.3, crate::i18n::tr("Materiały…"));
            sc.materials(&mut m, &model);
            p.set(0.45, crate::i18n::tr("Tekstura…"));
            let tex = || {
                let (w, h) = color::tex_size(&sc.grid, tex_px);
                color::texture(&sc.grid, sc.feats.as_ref(), &look, w, h)
            };
            let credits = sc.credits().join("; ");
            p.set(0.7, crate::i18n::tr("Zapisywanie…"));
            let write = |path: &Path, bytes: &[u8]| std::fs::write(path, bytes).map_err(|e| tf!("Nie udało się zapisać {}: {}", path.display(), e));
            let strata_png = strata.as_ref().map(mesh::png);
            match ext.as_str() {
                "obj" => {
                    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("model").to_string();
                    let (obj, mtl) = m.obj(&stem, &credits, &model, strata_png.is_some());
                    write(path.as_path(), obj.as_bytes())?;
                    write(path.with_extension("mtl").as_path(), mtl.as_bytes())?;
                    write(path.with_extension("png").as_path(), &mesh::png(&tex()))?;
                    if let Some(sp) = &strata_png {
                        write(path.with_file_name(format!("{stem}_sciany.png")).as_path(), sp)?;
                    }
                }
                "glb" => write(path.as_path(), &m.glb(&mesh::png(&tex()), strata_png.as_deref(), &model, &credits))?,
                "ply" => write(path.as_path(), &m.ply(&tex(), &model, strata.as_ref()))?,
                _ => write(path.as_path(), &m.stl(&credits))?,
            }
            let s = m.size();
            Ok(tf!("Zapisano model: {} ({} × {} × {} mm, {} trójkątów).", path.display(), s[0].round(), s[1].round(), s[2].round(), m.triangles()))
        });
    }

    /// Tekstura (PNG) albo mapa wysokości (PNG 16-bit + opis).
    pub fn export_raster(&mut self, heightmap: bool) {
        let Some(sc) = self.scene.clone() else { return };
        let title = if heightmap { crate::i18n::tr("Zapisz mapę wysokości") } else { crate::i18n::tr("Zapisz obraz terenu") };
        let Some(path) = crate::platform::save_target(title, &self.stem(), &[("png", "PNG")]) else { return };
        self.last_file = Some(path.clone());
        let (look, tex_px) = (self.o.look.clone(), self.o.tex_px);
        self.start_export(crate::i18n::tr("Zapisywanie…"), move |_| {
            if heightmap {
                let (png, info) = mesh::heightmap_png(&sc.grid);
                std::fs::write(&path, png).map_err(|e| e.to_string())?;
                let _ = std::fs::write(path.with_extension("txt"), info);
            } else {
                let (w, h) = color::tex_size(&sc.grid, tex_px);
                std::fs::write(&path, mesh::png(&color::texture(&sc.grid, sc.feats.as_ref(), &look, w, h))).map_err(|e| e.to_string())?;
            }
            Ok(tf!("Zapisano: {}", path.display()))
        });
    }

    /// Linia przekroju w metrach sceny, jeśli leży w przygotowanym obszarze.
    pub fn section_line(&self) -> Option<((f64, f64), (f64, f64))> {
        let sc = self.scene.as_ref()?;
        let l = &self.o.section.line;
        if l.len() < 2 {
            return None;
        }
        let a = sc.frame.to_m(l[0].0, l[0].1);
        let b = sc.frame.to_m(l[1].0, l[1].1);
        let g = &sc.grid;
        let ok = |p: (f64, f64)| p.0 >= g.x0 && p.0 <= g.x1() && p.1 >= g.y0() && p.1 <= g.y1;
        (ok(a) && ok(b)).then_some((a, b))
    }

    fn section_title(&self) -> String {
        if self.o.map.title.trim().is_empty() {
            crate::i18n::tr("Przekrój terenu").to_string()
        } else {
            tf!("{} – przekrój", self.o.map.title.trim())
        }
    }

    /// Przepis na rysunek arkusza (mapy albo przekroju) z bieżących ustawień.
    fn maker(&self, kind: SheetKind) -> Result<Maker, String> {
        let sc = self.scene.clone().ok_or_else(|| crate::i18n::tr("Najpierw przygotuj dane obszaru (zakładka Dane).").to_string())?;
        match kind {
            SheetKind::Map => {
                let (look, o) = (self.o.look.clone(), self.o.map.clone());
                let m: Maker = Box::new(move |dpi| Ok(mapdraw::map(&sc.grid, sc.feats.as_ref(), &sc.frame, &sc.region, &look, &o, &sc.credits(), dpi)));
                Ok(m)
            }
            SheetKind::Section => {
                let o = self.o.section.clone();
                let line = self.section_line();
                let title = self.section_title();
                if o.plane != Plane::Horizontal && line.is_none() {
                    return Err(crate::i18n::tr("Wskaż na mapie linię A–B przekroju wewnątrz przygotowanego obszaru.").into());
                }
                let m: Maker = Box::new(move |_| {
                    if o.plane == Plane::Vertical {
                        let (a, b) = line.unwrap_or(((0.0, 0.0), (1.0, 0.0)));
                        return Ok(section::vertical(&sc.grid, sc.feats.as_ref(), a, b, &o, &title, &sc.credits()));
                    }
                    let (a, b) = line.unwrap_or(((0.0, 0.0), (1.0, 0.0)));
                    let lines = section::plane_lines(&sc.grid, a, b, &o, &|x, y| sc.region.sd(x, y) <= 0.0);
                    if lines.is_empty() {
                        return Err(crate::i18n::tr("Płaszczyzna nie przecina terenu w tym obszarze – zmień wysokość albo kąt.").into());
                    }
                    let outline = if o.plane == Plane::Horizontal { Some(sc.region.outline(180)) } else { None };
                    Ok(section::plane_drawing(&lines, &o, &title, &sc.credits(), outline.as_deref()))
                });
                Ok(m)
            }
        }
    }

    /// Klucz podglądu arkusza: ustawienia, które zmieniają rysunek, i rozdzielczość obrazu.
    fn sheet_key(&self, kind: SheetKind, px: u32) -> u64 {
        let id = self.scene.as_ref().map(|s| Arc::as_ptr(s) as u64).unwrap_or(0);
        match kind {
            SheetKind::Map => key_of(&(1u8, &self.o.map, &self.o.look, px), id),
            SheetKind::Section => key_of(&(2u8, &self.o.section, &self.o.map.title, px), id),
        }
    }

    /// Zamawia podgląd arkusza o dłuższym boku `px` (liczony w tle; stary obraz zostaje do czasu nowego).
    pub fn sheet_request(&mut self, kind: SheetKind, px: u32, ctx: &eframe::egui::Context) {
        let key = self.sheet_key(kind, px);
        if self.sheet.key == key {
            return;
        }
        if let Some((k, _, _)) = &self.sheet.job {
            // trwa liczenie – nowe ustawienia zostaną policzone po nim
            let _ = k;
            return;
        }
        let maker = match self.maker(kind) {
            Ok(m) => m,
            Err(e) => {
                self.sheet.key = key;
                self.sheet.kind = Some(kind);
                self.sheet.error = Some(e);
                self.sheet.tex = None;
                self.sheet.layers.clear();
                return;
            }
        };
        let slot: Slot<SheetOut> = Arc::default();
        let s = slot.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = maker(Some(110)).map(|d| {
                let svg = draw::svg(&d);
                SheetOut { img: draw::raster(&svg, px), w: d.w, h: d.h, layers: d.info(), svg_len: svg.len() }
            });
            *s.lock().unwrap() = Some(r);
            ctx.request_repaint();
        });
        self.sheet.job = Some((key, kind, slot));
    }

    /// Zapis arkusza w formacie wybranym w oknie (SVG, PDF, DXF, PNG; przekrój pionowy także CSV).
    pub fn export_sheet(&mut self, kind: SheetKind) {
        let maker = match self.maker(kind) {
            Ok(m) => m,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let mut formats = vec![("svg", "SVG – grafika wektorowa z warstwami"), ("pdf", "PDF – do druku, z warstwami"), ("dxf", "DXF – CAD"), ("png", "PNG – obraz")];
        let vertical = kind == SheetKind::Section && self.o.section.plane == Plane::Vertical;
        if vertical {
            formats.push(("csv", "CSV – profil"));
        }
        let title = if kind == SheetKind::Map { crate::i18n::tr("Zapisz mapę") } else { crate::i18n::tr("Zapisz przekrój") };
        let Some(path) = crate::platform::save_target(title, &self.stem(), &formats) else { return };
        self.last_file = Some(path.clone());
        let split = kind == SheetKind::Map && self.o.map.split_layers;
        let dpi = self.o.map.dpi;
        let csv = match (vertical, self.scene.clone(), self.section_line()) {
            (true, Some(sc), Some((a, b))) => Some((sc, a, b)),
            _ => None,
        };
        self.start_export(crate::i18n::tr("Rysowanie…"), move |p| {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("svg").to_ascii_lowercase();
            if ext == "csv" {
                let Some((sc, a, b)) = csv else { return Err(crate::i18n::tr("Wskaż na mapie linię A–B przekroju wewnątrz przygotowanego obszaru.").into()) };
                std::fs::write(&path, section::profile_csv(&sc.grid, &sc.frame, a, b)).map_err(|e| e.to_string())?;
                return Ok(tf!("Zapisano: {}", path.display()));
            }
            let d = maker(None)?;
            p.set(0.7, crate::i18n::tr("Zapisywanie…"));
            let bytes = match ext.as_str() {
                "pdf" => draw::pdf(&d),
                "dxf" => draw::dxf(&d).into_bytes(),
                "png" => {
                    let px = ((d.w.max(d.h) / 25.4 * dpi as f64) as u32).clamp(256, 16_000);
                    let img = draw::raster(&draw::svg(&d), px).ok_or_else(|| crate::i18n::tr("Nie udało się narysować obrazu.").to_string())?;
                    mesh::png(&img)
                }
                _ => draw::svg(&d).into_bytes(),
            };
            std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
            let mut extra = 0;
            if split && ext == "svg" {
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("mapa").to_string();
                for l in d.layers.iter().filter(|l| !l.items.is_empty()) {
                    let file = path.with_file_name(format!("{stem}-{}.svg", l.id));
                    std::fs::write(&file, draw::svg(&d.only(l.id))).map_err(|e| e.to_string())?;
                    extra += 1;
                }
            }
            Ok(if extra > 0 { tf!("Zapisano: {} (i {} plików warstw).", path.display(), extra) } else { tf!("Zapisano: {}", path.display()) })
        });
    }

    pub fn export_contours(&mut self) {
        let Some(sc) = self.scene.clone() else { return };
        let step = self.o.map.step;
        let Some(path) = crate::platform::save_target(crate::i18n::tr("Zapisz warstwice"), &format!("{}-warstwice", self.stem()), &[("geojson", "GeoJSON")]) else { return };
        self.last_file = Some(path.clone());
        self.start_export(crate::i18n::tr("Warstwice…"), move |_| {
            std::fs::write(&path, mapdraw::contours_geojson(&sc.grid, &sc.frame, &sc.region, step)).map_err(|e| e.to_string())?;
            Ok(tf!("Zapisano: {}", path.display()))
        });
    }
}
