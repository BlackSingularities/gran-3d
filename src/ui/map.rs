//! Mapa 2D: cieniowany relief z modelu terenu albo obraz satelitarny, przesuwanie, przybliżanie,
//! zaznaczanie prostokąta. Kafle wczytują wątki w tle (dysk, a gdy brak – sieć), a dopóki kafel
//! nie jest gotowy, rysowany jest powiększony fragment kafla nadrzędnego.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use eframe::egui::{
    self, Color32, ColorImage, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, Vec2,
};

use crate::download;
use crate::geo::{lat_to_y, lon_to_x, meters_per_pixel, x_to_lon, y_to_lat, BBox, TileId, TILE};
use crate::store::{Layer, Store};
use crate::ui::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapLayer {
    Relief,
    Satellite,
}

impl MapLayer {
    fn source(self) -> Layer {
        match self {
            MapLayer::Relief => Layer::Dem,
            MapLayer::Satellite => Layer::Sat,
        }
    }
}

/// Element rysowany na mapie (zapisany obszar, propozycja, zaznaczenie).
pub struct Overlay {
    pub bbox: BBox,
    pub color: Color32,
    pub label: String,
    pub strong: bool,
}

#[derive(Default)]
pub struct MapResponse {
    /// Prostokąt zaznaczony przeciągnięciem (tryb zaznaczania).
    pub selected: Option<BBox>,
    /// Kliknięty punkt (dł., szer.).
    pub clicked: Option<[f64; 2]>,
    /// Punkt pod kursorem (dł., szer.).
    pub hover: Option<[f64; 2]>,
}

enum Slot {
    Pending,
    Ready(TextureHandle),
    Missing,
}

struct Loaded {
    layer: Layer,
    tile: TileId,
    image: Option<ColorImage>,
}

pub struct MapView {
    /// Środek widoku we współrzędnych świata 0..1.
    center: [f64; 2],
    /// Poziom przybliżenia (ciągły).
    zoom: f64,
    pub layer: MapLayer,
    slots: HashMap<(Layer, TileId), (Slot, u64)>,
    frame: u64,
    tx: Sender<(Layer, TileId)>,
    rx: Receiver<Loaded>,
    store: Arc<Mutex<Store>>,
    drag_from: Option<[f64; 2]>,
}

const MAX_TEXTURES: usize = 900;
const WORKERS: usize = 6;

impl MapView {
    pub fn new(store: Store, ctx: &egui::Context) -> Self {
        let (tx, req_rx) = channel::<(Layer, TileId)>();
        let (done_tx, rx) = channel::<Loaded>();
        let store = Arc::new(Mutex::new(store));
        let req_rx = Arc::new(Mutex::new(req_rx));
        for i in 0..WORKERS {
            let (req_rx, done_tx, store, ctx) =
                (req_rx.clone(), done_tx.clone(), store.clone(), ctx.clone());
            thread::Builder::new()
                .name(format!("mapa-{i}"))
                .spawn(move || loop {
                    let Ok((layer, tile)) = req_rx.lock().unwrap().recv() else {
                        break;
                    };
                    let st = store.lock().unwrap().clone();
                    let image = download::get(&st, layer, tile)
                        .ok()
                        .and_then(|b| decode(layer, tile, &b));
                    if done_tx.send(Loaded { layer, tile, image }).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                })
                .expect("wątek mapy");
        }
        Self {
            center: [lon_to_x(19.5), lat_to_y(50.0)],
            zoom: 6.3,
            layer: MapLayer::Relief,
            slots: HashMap::new(),
            frame: 0,
            tx,
            rx,
            store,
            drag_from: None,
        }
    }

    pub fn set_store(&mut self, store: Store) {
        *self.store.lock().unwrap() = store;
        self.slots.clear();
    }

    /// Wyśrodkowanie i dopasowanie przybliżenia do prostokąta.
    pub fn fit(&mut self, b: &BBox, size: Vec2) {
        let (x0, x1, y0, y1) = (lon_to_x(b.w), lon_to_x(b.e), lat_to_y(b.n), lat_to_y(b.s));
        self.center = [(x0 + x1) / 2.0, (y0 + y1) / 2.0];
        let zx = (size.x as f64 / TILE / (x1 - x0).max(1e-9)).log2();
        let zy = (size.y as f64 / TILE / (y1 - y0).max(1e-9)).log2();
        self.zoom = (zx.min(zy) - 0.35).clamp(3.0, 15.0);
    }

    fn scale(&self) -> f64 {
        TILE * self.zoom.exp2()
    }

    fn to_screen(&self, rect: Rect, wx: f64, wy: f64) -> Pos2 {
        let s = self.scale();
        let c = rect.center();
        Pos2::new(
            c.x + ((wx - self.center[0]) * s) as f32,
            c.y + ((wy - self.center[1]) * s) as f32,
        )
    }

    fn to_world(&self, rect: Rect, p: Pos2) -> [f64; 2] {
        let s = self.scale();
        let c = rect.center();
        [
            self.center[0] + (p.x - c.x) as f64 / s,
            self.center[1] + (p.y - c.y) as f64 / s,
        ]
    }

    fn lonlat(&self, rect: Rect, p: Pos2) -> [f64; 2] {
        let w = self.to_world(rect, p);
        [
            x_to_lon(w[0].clamp(0.0, 1.0)),
            y_to_lat(w[1].clamp(0.0, 1.0)),
        ]
    }

    fn bbox_rect(&self, rect: Rect, b: &BBox) -> Rect {
        Rect::from_two_pos(
            self.to_screen(rect, lon_to_x(b.w), lat_to_y(b.n)),
            self.to_screen(rect, lon_to_x(b.e), lat_to_y(b.s)),
        )
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        overlays: &[Overlay],
        selecting: bool,
    ) -> MapResponse {
        self.frame += 1;
        self.receive(ui.ctx());
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme::INK_0);
        let mut out = MapResponse::default();

        // sterowanie: przeciąganie (przesuwanie albo zaznaczanie), kółko (przybliżenie do kursora)
        let pointer = resp.hover_pos();
        if let Some(p) = pointer {
            out.hover = Some(self.lonlat(rect, p));
            let scroll = ui.input(|i| i.smooth_scroll_delta.y + (i.zoom_delta() - 1.0) * 200.0);
            if scroll.abs() > 0.0 {
                let before = self.to_world(rect, p);
                self.zoom = (self.zoom + scroll as f64 / 240.0).clamp(3.0, 16.0);
                let after = self.to_world(rect, p);
                self.center[0] += before[0] - after[0];
                self.center[1] += before[1] - after[1];
            }
        }
        if selecting {
            if resp.drag_started_by(egui::PointerButton::Primary) {
                self.drag_from = pointer.map(|p| self.lonlat(rect, p));
            }
            if let (Some(a), Some(p)) = (self.drag_from, pointer) {
                let b = BBox::from_corners(a, self.lonlat(rect, p));
                if resp.drag_stopped() {
                    self.drag_from = None;
                    if b.km2() > 1.0 {
                        out.selected = Some(b);
                    }
                } else {
                    let r = self.bbox_rect(rect, &b);
                    painter.rect_filled(r, 0.0, theme::BRASS.gamma_multiply(0.12));
                    painter.rect_stroke(
                        r,
                        0.0,
                        Stroke::new(1.5_f32, theme::BRASS),
                        egui::StrokeKind::Inside,
                    );
                }
            }
        }
        if resp.dragged_by(egui::PointerButton::Secondary)
            || resp.dragged_by(egui::PointerButton::Middle)
            || (!selecting && resp.dragged_by(egui::PointerButton::Primary))
        {
            let s = self.scale();
            let d = resp.drag_delta();
            self.center[0] -= d.x as f64 / s;
            self.center[1] -= d.y as f64 / s;
        }
        self.center[1] = self.center[1].clamp(0.0, 1.0);
        self.center[0] = self.center[0].rem_euclid(1.0);
        if resp.clicked() {
            out.clicked = pointer.map(|p| self.lonlat(rect, p));
        }

        self.paint_tiles(&painter, rect);

        for o in overlays {
            let r = self.bbox_rect(rect, &o.bbox);
            if !r.intersects(rect) {
                continue;
            }
            painter.rect_filled(
                r,
                2.0,
                o.color.gamma_multiply(if o.strong { 0.18 } else { 0.07 }),
            );
            painter.rect_stroke(
                r,
                2.0,
                Stroke::new(if o.strong { 2.0_f32 } else { 1.0 }, o.color),
                egui::StrokeKind::Inside,
            );
            if r.width() > 46.0 && !o.label.is_empty() {
                painter.text(
                    r.left_top() + Vec2::new(6.0, 5.0),
                    egui::Align2::LEFT_TOP,
                    &o.label,
                    egui::FontId::proportional(12.0),
                    o.color,
                );
            }
        }

        // podziałka i atrybucja
        let lat = y_to_lat(self.center[1]);
        let mpp = meters_per_pixel(lat, 0) / self.zoom.exp2();
        let target = mpp * 110.0;
        let nice = [1.0, 2.0, 5.0]
            .iter()
            .flat_map(|m| (0..7).map(move |e| m * 10f64.powi(e)))
            .filter(|v| *v <= target)
            .fold(1.0, f64::max);
        let px = (nice / mpp) as f32;
        let base = rect.left_bottom() + Vec2::new(14.0, -16.0);
        painter.line_segment(
            [base, base + Vec2::new(px, 0.0)],
            Stroke::new(2.0_f32, theme::PAPER),
        );
        let label = if nice >= 1000.0 {
            format!("{} km", nice / 1000.0)
        } else {
            format!("{nice} m")
        };
        painter.text(
            base + Vec2::new(px + 6.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::monospace(11.0),
            theme::PAPER_2,
        );
        let attr = match self.layer {
            MapLayer::Relief => "Teren: Terrarium / AWS Open Data (SRTM, EU-DEM)",
            MapLayer::Satellite => "Sentinel-2 cloudless 2020 © EOX (CC BY-NC-SA 4.0)",
        };
        painter.text(
            rect.right_bottom() + Vec2::new(-10.0, -8.0),
            egui::Align2::RIGHT_BOTTOM,
            attr,
            egui::FontId::proportional(10.5),
            theme::PAPER_3,
        );
        out
    }

    fn receive(&mut self, ctx: &egui::Context) {
        while let Ok(l) = self.rx.try_recv() {
            let slot = match l.image {
                Some(img) => Slot::Ready(ctx.load_texture(
                    format!("{:?}-{}-{}-{}", l.layer, l.tile.z, l.tile.x, l.tile.y),
                    img,
                    TextureOptions::LINEAR,
                )),
                None => Slot::Missing,
            };
            self.slots.insert((l.layer, l.tile), (slot, self.frame));
        }
        if self.slots.len() > MAX_TEXTURES {
            // najdawniej rysowane tekstury zwalniamy (kafle zostają na dysku)
            let mut ages: Vec<u64> = self.slots.values().map(|s| s.1).collect();
            ages.sort_unstable();
            let cut = ages[self.slots.len() - MAX_TEXTURES * 3 / 4];
            self.slots
                .retain(|_, s| s.1 >= cut || matches!(s.0, Slot::Pending));
        }
    }

    fn request(&mut self, layer: Layer, t: TileId) -> Option<TextureHandle> {
        let frame = self.frame;
        match self.slots.get_mut(&(layer, t)) {
            Some((Slot::Ready(tex), seen)) => {
                *seen = frame;
                Some(tex.clone())
            }
            Some(_) => None,
            None => {
                self.slots.insert((layer, t), (Slot::Pending, frame));
                let _ = self.tx.send((layer, t));
                None
            }
        }
    }

    fn peek(&mut self, layer: Layer, t: TileId) -> Option<TextureHandle> {
        let frame = self.frame;
        match self.slots.get_mut(&(layer, t)) {
            Some((Slot::Ready(tex), seen)) => {
                *seen = frame;
                Some(tex.clone())
            }
            _ => None,
        }
    }

    fn paint_tiles(&mut self, painter: &egui::Painter, rect: Rect) {
        let layer = self.layer.source();
        let z = (self.zoom.round() as u8).clamp(2, layer.max_z());
        let n = 1u32 << z;
        let tl = self.to_world(rect, rect.left_top());
        let br = self.to_world(rect, rect.right_bottom());
        let (x0, x1) = (
            (tl[0] * n as f64).floor() as i64,
            (br[0] * n as f64).floor() as i64,
        );
        let (y0, y1) = (
            (tl[1] * n as f64).floor().max(0.0) as i64,
            ((br[1] * n as f64).floor() as i64).min(n as i64 - 1),
        );
        // od środka – najpierw to, na co patrzy użytkownik
        let mut want: Vec<(i64, i64)> = (y0..=y1)
            .flat_map(|y| (x0..=x1).map(move |x| (x, y)))
            .collect();
        let (cx, cy) = ((x0 + x1) as f64 / 2.0, (y0 + y1) as f64 / 2.0);
        want.sort_by(|a, b| {
            ((a.0 as f64 - cx).hypot(a.1 as f64 - cy))
                .total_cmp(&(b.0 as f64 - cx).hypot(b.1 as f64 - cy))
        });
        let mut seen = HashSet::new();
        for (xi, yi) in want {
            let wx = xi.rem_euclid(n as i64) as u32;
            let t = TileId::new(z, wx, yi as u32);
            if !seen.insert((xi, yi)) {
                continue;
            }
            let r = Rect::from_two_pos(
                self.to_screen(rect, xi as f64 / n as f64, yi as f64 / n as f64),
                self.to_screen(rect, (xi + 1) as f64 / n as f64, (yi + 1) as f64 / n as f64),
            );
            if let Some(tex) = self.request(layer, t) {
                painter.image(
                    tex.id(),
                    r,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
                continue;
            }
            // zastępczo: powiększony fragment przodka
            for dz in 1..=z.min(6) {
                let (p, off, k) = t.ancestor(dz);
                if let Some(tex) = self.peek(layer, p) {
                    let uv = Rect::from_min_size(Pos2::new(off[0], off[1]), Vec2::splat(k));
                    painter.image(tex.id(), r, uv, Color32::WHITE);
                    break;
                }
            }
        }
    }
}

/// Dekodowanie wypieczonego kafla do obrazu: zdjęcie wprost, teren jako cieniowany relief z hipsometrią.
fn decode(layer: Layer, t: TileId, blob: &[u8]) -> Option<ColorImage> {
    match layer {
        Layer::Sat => {
            let img = image::load_from_memory_with_format(
                crate::bake::jpeg(blob)?,
                image::ImageFormat::Jpeg,
            )
            .ok()?
            .to_rgb8();
            Some(ColorImage::from_rgb(
                [img.width() as usize, img.height() as usize],
                img.as_raw(),
            ))
        }
        Layer::Dem => {
            let d = crate::bake::decode_dem(blob)?;
            let lat = t.bbox().center()[1];
            let mpp = meters_per_pixel(lat, t.z) as f32 * (256.0 / d.w as f32);
            Some(ColorImage::from_rgb(
                [d.w, d.h],
                &relief(&d.elev, d.w, d.h, mpp),
            ))
        }
    }
}

/// Relief: oświetlenie z północnego zachodu (315°, 45°) z lekką przesadą pionową, kolory pięter wysokości.
pub fn relief(elev: &[f32], w: usize, h: usize, mpp: f32) -> Vec<u8> {
    let at = |x: isize, y: isize| {
        elev[(y.clamp(0, h as isize - 1) as usize) * w + x.clamp(0, w as isize - 1) as usize]
    };
    let (az, alt) = (315f32.to_radians(), 45f32.to_radians());
    let light = [alt.cos() * az.sin(), -alt.cos() * az.cos(), alt.sin()];
    // im mniejsze piksele, tym mocniejsza przesada – relief czytelny na każdym poziomie
    let exag = (1.0 + (mpp / 30.0).ln().max(0.0)) * 1.6;
    let mut out = Vec::with_capacity(w * h * 3);
    for y in 0..h as isize {
        for x in 0..w as isize {
            let e = at(x, y);
            let dx = (at(x + 1, y) - at(x - 1, y)) * exag / (2.0 * mpp);
            let dy = (at(x, y + 1) - at(x, y - 1)) * exag / (2.0 * mpp);
            let len = (dx * dx + dy * dy + 1.0).sqrt();
            let n = [-dx / len, -dy / len, 1.0 / len];
            let shade = (n[0] * light[0] + n[1] * light[1] + n[2] * light[2]).clamp(0.0, 1.0);
            let base = hypso(e);
            let k = 0.35 + 0.85 * shade;
            out.extend(base.iter().map(|c| (c * k).clamp(0.0, 255.0) as u8));
        }
    }
    out
}

/// Barwy wysokości: woda, niziny, regle, hale, turnie, śnieg.
fn hypso(e: f32) -> [f32; 3] {
    const STOPS: [(f32, [f32; 3]); 8] = [
        (-50.0, [58.0, 92.0, 110.0]),
        (0.5, [62.0, 98.0, 112.0]),
        (1.0, [92.0, 116.0, 84.0]),
        (400.0, [118.0, 132.0, 92.0]),
        (900.0, [96.0, 116.0, 82.0]),
        (1500.0, [140.0, 132.0, 104.0]),
        (2100.0, [168.0, 160.0, 148.0]),
        (3000.0, [236.0, 236.0, 240.0]),
    ];
    if e <= STOPS[0].0 {
        return STOPS[0].1;
    }
    for w in STOPS.windows(2) {
        let ((e0, c0), (e1, c1)) = (w[0], w[1]);
        if e <= e1 {
            let f = (e - e0) / (e1 - e0);
            return [
                c0[0] + (c1[0] - c0[0]) * f,
                c0[1] + (c1[1] - c0[1]) * f,
                c0[2] + (c1[2] - c0[2]) * f,
            ];
        }
    }
    STOPS[7].1
}
