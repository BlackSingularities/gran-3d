//! Mapa wektorowa obszaru: podkład rastrowy (opcjonalnie), pokrycie terenu, warstwice z opisami,
//! wody, drogi i szlaki, szczyty i miejscowości, siatka geograficzna, podziałka, strzałka północy,
//! legenda i źródła danych. Wynik to [`Drawing`] – zapis do SVG, PDF albo DXF.

use serde::{Deserialize, Serialize};

use super::color::{self, Look};
use super::contour::{self, Field};
use super::draw::{text_width, Anchor, Drawing, Item, Rgb};
use super::features::{AreaKind, Features, LineKind, PointKind};
use super::geom::{Frame, Region};
use super::terrain::Grid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Paper {
    /// Rozmiar wynika ze skali.
    #[default]
    Scale,
    A4,
    A3,
    A2,
    A1,
    A0,
}

impl Paper {
    pub const ALL: [Paper; 6] = [Paper::Scale, Paper::A4, Paper::A3, Paper::A2, Paper::A1, Paper::A0];
    pub fn label(self) -> &'static str {
        match self {
            Paper::Scale => crate::i18n::tr("Według skali"),
            Paper::A4 => "A4",
            Paper::A3 => "A3",
            Paper::A2 => "A2",
            Paper::A1 => "A1",
            Paper::A0 => "A0",
        }
    }
    /// Krótszy i dłuższy bok (mm).
    fn size(self) -> Option<(f64, f64)> {
        match self {
            Paper::Scale => None,
            Paper::A4 => Some((210.0, 297.0)),
            Paper::A3 => Some((297.0, 420.0)),
            Paper::A2 => Some((420.0, 594.0)),
            Paper::A1 => Some((594.0, 841.0)),
            Paper::A0 => Some((841.0, 1189.0)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Relief {
    None,
    /// Samo cieniowanie (szare).
    #[default]
    Shade,
    /// Kolory i cieniowanie z ustawień wyglądu.
    Color,
}

impl Relief {
    pub const ALL: [Relief; 3] = [Relief::None, Relief::Shade, Relief::Color];
    pub fn label(self) -> &'static str {
        match self {
            Relief::None => crate::i18n::tr("Bez podkładu (czysty wektor)"),
            Relief::Shade => crate::i18n::tr("Cieniowanie"),
            Relief::Color => crate::i18n::tr("Kolory z wyglądu"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MapOpts {
    pub title: String,
    pub scale: f32,
    pub paper: Paper,
    pub relief: Relief,
    pub dpi: u32,
    pub contours: bool,
    pub step: f32,
    pub contour_labels: bool,
    pub forest: bool,
    pub water: bool,
    pub rock: bool,
    pub buildings: bool,
    pub roads: bool,
    pub paths: bool,
    pub trails: bool,
    pub peaks: bool,
    pub places: bool,
    pub graticule: bool,
    pub legend: bool,
}

impl Default for MapOpts {
    fn default() -> Self {
        Self {
            title: String::new(),
            scale: 25_000.0,
            paper: Paper::Scale,
            relief: Relief::Shade,
            dpi: 200,
            contours: true,
            step: 20.0,
            contour_labels: true,
            forest: true,
            water: true,
            rock: true,
            buildings: true,
            roads: true,
            paths: true,
            trails: true,
            peaks: true,
            places: true,
            graticule: true,
            legend: true,
        }
    }
}

const CONTOUR: Rgb = [176, 120, 72];
const WATER: Rgb = [70, 140, 210];
const WATER_FILL: Rgb = [178, 214, 240];
const FOREST: Rgb = [196, 226, 182];
const ROCK: Rgb = [214, 208, 200];
const SCREE: Rgb = [228, 224, 216];
const GLACIER: Rgb = [232, 244, 252];
const BUILDING: Rgb = [140, 128, 124];
const INK: Rgb = [28, 30, 34];

/// Dzieli łamaną na części wewnątrz obszaru (brzeg doprecyzowany bisekcją).
pub fn clip_line(pts: &[(f64, f64)], region: &Region) -> Vec<Vec<(f64, f64)>> {
    let mut out = Vec::new();
    let edge = |a: (f64, f64), b: (f64, f64)| {
        // a wewnątrz, b na zewnątrz
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..18 {
            let m = (lo + hi) / 2.0;
            if region.sd(a.0 + (b.0 - a.0) * m, a.1 + (b.1 - a.1) * m) <= 0.0 {
                lo = m;
            } else {
                hi = m;
            }
        }
        (a.0 + (b.0 - a.0) * lo, a.1 + (b.1 - a.1) * lo)
    };
    // odcinek z oboma końcami poza obszarem może go przecinać (cięciwa koła, róg wielokąta):
    // dokładamy próbki pomocnicze, które po przycięciu znikają (leżą na prostym odcinku)
    const SAMPLES: usize = 24;
    let is_in = |p: (f64, f64)| region.sd(p.0, p.1) <= 0.0;
    let mut dense: Vec<((f64, f64), bool)> = Vec::with_capacity(pts.len());
    for k in 0..pts.len() {
        if k > 0 && !is_in(pts[k - 1]) && !is_in(pts[k]) {
            let (a, b) = (pts[k - 1], pts[k]);
            let probe: Vec<(f64, f64)> = (1..SAMPLES).map(|i| {
                let t = i as f64 / SAMPLES as f64;
                (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
            }).collect();
            if probe.iter().any(|&q| is_in(q)) {
                dense.extend(probe.into_iter().map(|q| (q, true)));
            }
        }
        dense.push((pts[k], false));
    }
    let flush = |cur: &mut Vec<((f64, f64), bool)>, out: &mut Vec<Vec<(f64, f64)>>| {
        // końce części to punkty brzegu albo oryginalne, więc próbki pomocnicze można pominąć
        let part: Vec<(f64, f64)> = cur.drain(..).filter(|(_, synth)| !*synth).map(|(p, _)| p).collect();
        if part.len() >= 2 {
            out.push(part);
        }
    };
    let mut cur: Vec<((f64, f64), bool)> = Vec::new();
    for k in 0..dense.len() {
        let (p, synth) = dense[k];
        let inside = is_in(p);
        let prev_inside = k > 0 && is_in(dense[k - 1].0);
        if inside {
            if k > 0 && !prev_inside {
                cur.push((edge(p, dense[k - 1].0), false));
            }
            cur.push((p, synth));
        } else if prev_inside {
            cur.push((edge(dense[k - 1].0, p), false));
            flush(&mut cur, &mut out);
        }
    }
    flush(&mut cur, &mut out);
    out
}

/// Przycięcie wielokąta do prostokąta (Sutherland–Hodgman).
fn clip_rect(ring: &[(f64, f64)], b: [f64; 4]) -> Vec<(f64, f64)> {
    let mut poly = ring.to_vec();
    for side in 0..4 {
        let inside = |p: (f64, f64)| match side {
            0 => p.0 >= b[0],
            1 => p.0 <= b[2],
            2 => p.1 >= b[1],
            _ => p.1 <= b[3],
        };
        let cut = |a: (f64, f64), c: (f64, f64)| {
            let t = match side {
                0 => (b[0] - a.0) / (c.0 - a.0),
                1 => (b[2] - a.0) / (c.0 - a.0),
                2 => (b[1] - a.1) / (c.1 - a.1),
                _ => (b[3] - a.1) / (c.1 - a.1),
            };
            (a.0 + (c.0 - a.0) * t, a.1 + (c.1 - a.1) * t)
        };
        let src = std::mem::take(&mut poly);
        for k in 0..src.len() {
            let (a, c) = (src[k], src[(k + 1) % src.len()]);
            match (inside(a), inside(c)) {
                (true, true) => poly.push(c),
                (true, false) => poly.push(cut(a, c)),
                (false, true) => {
                    poly.push(cut(a, c));
                    poly.push(c);
                }
                _ => {}
            }
        }
        if poly.is_empty() {
            break;
        }
    }
    poly
}

fn nice(v: f64) -> f64 {
    let p = 10f64.powf(v.log10().floor());
    for k in [1.0, 2.0, 2.5, 5.0, 10.0] {
        if k * p >= v {
            return k * p;
        }
    }
    10.0 * p
}

/// Mapa obszaru jako rysunek.
pub fn map(g: &Grid, feats: Option<&Features>, frame: &Frame, region: &Region, look: &Look, o: &MapOpts, credits: &[String]) -> Drawing {
    let b = region.bounds();
    let (wm, hm) = ((b[2] - b[0]).max(1.0), (b[3] - b[1]).max(1.0));
    let (ml, mr, mt, mbot) = (14.0, 14.0, 24.0, if o.legend { 34.0 } else { 22.0 });
    // skala: z ustawień albo dopasowana do arkusza (orientacja według kształtu obszaru)
    let s = match o.paper.size() {
        Some((short, long)) => {
            let (pw, ph) = if wm >= hm { (long, short) } else { (short, long) };
            ((pw - ml - mr) / wm).min((ph - mt - mbot) / hm)
        }
        None => 1000.0 / o.scale.max(500.0) as f64,
    };
    let (pw, ph) = match o.paper.size() {
        Some((short, long)) => {
            if wm >= hm {
                (long, short)
            } else {
                (short, long)
            }
        }
        None => (wm * s + ml + mr, hm * s + mt + mbot),
    };
    // mapa wyśrodkowana w polu arkusza
    let ox = ml + ((pw - ml - mr) - wm * s) / 2.0;
    let oy = mt + ((ph - mt - mbot) - hm * s) / 2.0;
    let tp = |x: f64, y: f64| (ox + (x - b[0]) * s, oy + (b[3] - y) * s);
    let title = if o.title.trim().is_empty() { crate::i18n::tr("Mapa obszaru").to_string() } else { o.title.trim().to_string() };
    let mut d = Drawing::new(pw, ph, &title);
    let scale_n = (1000.0 / s).round();

    // podkład rastrowy
    if o.relief != Relief::None {
        let px_w = ((wm * s / 25.4 * o.dpi as f64) as u32).clamp(64, 7000);
        let px_h = ((hm * s / 25.4 * o.dpi as f64) as u32).clamp(64, 7000);
        let mut lk = look.clone();
        lk.contours = false;
        if o.relief == Relief::Shade {
            lk.mode = color::Mode::Plain;
            lk.plain = [246, 244, 240];
            lk.shade = lk.shade.max(0.6);
        }
        // podkład z obiektami tylko w trybie kolorowym (wektorowe i tak będą na wierzchu)
        let sub = crop(g, b);
        let tex = color::texture(&sub, if o.relief == Relief::Color { feats } else { None }, &lk, px_w, px_h);
        let tm = color::TexMap::for_grid(&sub, px_w, px_h);
        let mut rgb = image::RgbImage::new(px_w, px_h);
        for (x, y, p) in rgb.enumerate_pixels_mut() {
            let (mx, my) = tm.m(x as f64 + 0.5, y as f64 + 0.5);
            let c = tex.get_pixel(x, y).0;
            *p = if region.sd(mx, my) <= 0.0 { image::Rgb([c[0], c[1], c[2]]) } else { image::Rgb([255, 255, 255]) };
        }
        let (x0, y0) = tp(sub.x0, sub.y1);
        let (x1, y1) = tp(sub.x1(), sub.y0());
        d.layer("podklad", "Podkład (cieniowanie)").push(Item::Image { x: x0, y: y0, w: x1 - x0, h: y1 - y0, img: rgb });
    }

    // pokrycie terenu
    if let Some(f) = feats {
        for p in &f.polys {
            let (fill, layer, name, on) = match p.kind {
                AreaKind::Forest => (FOREST, "lasy", crate::i18n::tr("Lasy"), o.forest),
                AreaKind::Rock => (ROCK, "skaly", crate::i18n::tr("Skały i piargi"), o.rock),
                AreaKind::Scree => (SCREE, "skaly", crate::i18n::tr("Skały i piargi"), o.rock),
                AreaKind::Glacier => (GLACIER, "lodowce", crate::i18n::tr("Lodowce"), o.rock),
                AreaKind::Water => (WATER_FILL, "wody_pow", crate::i18n::tr("Wody (powierzchnie)"), o.water),
                AreaKind::Building => (BUILDING, "budynki", crate::i18n::tr("Budynki"), o.buildings),
            };
            if !on {
                continue;
            }
            let rings: Vec<Vec<(f64, f64)>> = p
                .rings
                .iter()
                .map(|r| clip_rect(r, b))
                .filter(|r| r.len() >= 3)
                .map(|r| r.into_iter().map(|(x, y)| tp(x, y)).collect())
                .collect();
            if rings.is_empty() {
                continue;
            }
            let stroke = if p.kind == AreaKind::Water { Some((WATER, 0.12)) } else { None };
            d.layer(layer, name).push(Item::Rings { rings, fill, stroke });
        }
    }

    // maska: biel poza kształtem obszaru (wycinek nie musi być prostokątem)
    let outline: Vec<(f64, f64)> = region.outline(180).into_iter().map(|(x, y)| tp(x, y)).collect();
    if !matches!(region, Region::Rect { .. }) {
        let page = vec![(0.0, 0.0), (pw, 0.0), (pw, ph), (0.0, ph)];
        d.layer("maska", "Maska poza obszarem").push(Item::Rings { rings: vec![page, outline.clone()], fill: [255, 255, 255], stroke: None });
    }

    // warstwice
    if o.contours && o.step > 0.0 {
        // oczko co najmniej 0,35 mm na papierze
        let cell = (0.35 / s).max(g.cell);
        let sub = crop(g, b).resample(cell);
        let v: Vec<f64> = (0..sub.nx * sub.ny)
            .map(|k| {
                let (x, y) = (sub.x(k % sub.nx), sub.y(k / sub.nx));
                if region.sd(x, y) <= 0.0 {
                    sub.z[k] as f64
                } else {
                    f64::NAN
                }
            })
            .collect();
        let field = Field { nx: sub.nx, ny: sub.ny, v: &v };
        let (lo, hi) = sub.range();
        let step = o.step as f64;
        let mut lvl = (lo as f64 / step).ceil() * step;
        let mut labels: Vec<Item> = Vec::new();
        while lvl <= hi as f64 {
            let index = ((lvl / step).round() as i64).rem_euclid(5) == 0;
            for l in contour::lines(&field, lvl) {
                let pts: Vec<(f64, f64)> = l.iter().map(|&(i, j)| tp(sub.x0 + i * sub.cell, sub.y1 - j * sub.cell)).collect();
                let pts = contour::simplify(&pts, 0.08);
                if contour::length(&pts) < 1.5 {
                    continue;
                }
                if index && o.contour_labels && contour::length(&pts) > 30.0 {
                    labels.extend(contour_labels(&pts, &format!("{:.0}", lvl)));
                }
                let (layer, name, w) = if index { ("warstwice_g", crate::i18n::tr("Warstwice główne"), 0.22) } else { ("warstwice", crate::i18n::tr("Warstwice"), 0.1) };
                d.layer(layer, name).push(Item::Path { pts, closed: false, stroke: Some((CONTOUR, w)), fill: None, dash: None });
            }
            lvl += step;
        }
        d.layer("warstwice_opisy", crate::i18n::tr("Opisy warstwic")).extend(labels);
    }

    // linie: wody, drogi, ścieżki, szlaki
    if let Some(f) = feats {
        for l in &f.lines {
            let (c, w, dash, layer, name, on) = match l.kind {
                LineKind::River => (WATER, 0.45, None, "rzeki", crate::i18n::tr("Rzeki i potoki"), o.water),
                LineKind::Stream => (WATER, 0.2, None, "rzeki", crate::i18n::tr("Rzeki i potoki"), o.water),
                LineKind::MajorRoad => ([200, 120, 40], 0.7, None, "drogi", crate::i18n::tr("Drogi"), o.roads),
                LineKind::Road => ([110, 100, 92], 0.35, None, "drogi", crate::i18n::tr("Drogi"), o.roads),
                LineKind::Track => ([120, 96, 70], 0.22, Some(1.2), "drogi_lesne", crate::i18n::tr("Drogi gruntowe"), o.paths),
                LineKind::Path => ([90, 70, 60], 0.16, Some(0.7), "sciezki", crate::i18n::tr("Ścieżki"), o.paths),
                LineKind::Rail => ([40, 40, 40], 0.45, None, "koleje", crate::i18n::tr("Koleje i wyciągi"), o.roads),
                LineKind::Aerialway => ([40, 40, 40], 0.25, None, "koleje", crate::i18n::tr("Koleje i wyciągi"), o.roads),
                LineKind::Trail(c) => (c, 0.55, None, "szlaki", crate::i18n::tr("Szlaki turystyczne"), o.trails),
            };
            if !on {
                continue;
            }
            for part in clip_line(&l.pts, region) {
                let pts: Vec<(f64, f64)> = part.into_iter().map(|(x, y)| tp(x, y)).collect();
                let pts = contour::simplify(&pts, 0.05);
                d.layer(layer, name).push(Item::Path { pts, closed: false, stroke: Some((c, w)), fill: None, dash });
            }
        }
        // punkty i nazwy
        let mut placed: Vec<[f64; 4]> = Vec::new();
        let mut labels = f.labels.clone();
        labels.sort_by(|a, b| {
            let rank = |l: &super::features::Label| match l.kind {
                PointKind::Place(r) => r as i32,
                PointKind::Peak => 4 - (l.ele.unwrap_or(0.0) / 1000.0) as i32,
                PointKind::Hut => 6,
                PointKind::Saddle => 7,
            };
            rank(a).cmp(&rank(b))
        });
        for lb in &labels {
            if region.sd(lb.x, lb.y) > 0.0 {
                continue;
            }
            let (x, y) = tp(lb.x, lb.y);
            let ele = lb.ele.map(|e| format!(" {:.0}", e)).unwrap_or_default();
            let (text, size, bold, sym) = match lb.kind {
                PointKind::Peak if o.peaks => (format!("{}{}", lb.name, ele), 2.4, true, 0),
                PointKind::Saddle if o.peaks => (lb.name.clone(), 2.0, false, 1),
                PointKind::Hut if o.peaks => (lb.name.clone(), 2.1, false, 2),
                PointKind::Place(r) if o.places => (lb.name.clone(), [4.2, 3.4, 2.8, 2.3][r.min(3) as usize], r < 2, 3),
                _ => continue,
            };
            let text = text.trim().to_string();
            let tw = text_width(&text, size);
            let bx = [x + 1.6, y - size, x + 1.6 + tw, y + 0.6];
            let free = !placed.iter().any(|p| bx[0] < p[2] && bx[2] > p[0] && bx[1] < p[3] && bx[3] > p[1]);
            let sym_layer = d.layer("punkty", crate::i18n::tr("Szczyty i obiekty"));
            match sym {
                0 => sym_layer.push(Item::Path { pts: vec![(x - 1.0, y + 0.6), (x + 1.0, y + 0.6), (x, y - 1.1)], closed: true, stroke: None, fill: Some(INK), dash: None }),
                1 => sym_layer.push(Item::Path { pts: vec![(x - 0.9, y - 0.5), (x, y + 0.3), (x + 0.9, y - 0.5)], closed: false, stroke: Some((INK, 0.2)), fill: None, dash: None }),
                2 => sym_layer.push(Item::Path { pts: vec![(x - 0.8, y - 0.8), (x + 0.8, y - 0.8), (x + 0.8, y + 0.8), (x - 0.8, y + 0.8)], closed: true, stroke: Some((INK, 0.15)), fill: Some([210, 60, 50]), dash: None }),
                _ => sym_layer.push(Item::Circle { x, y, r: 0.6, fill: Some(INK), stroke: None }),
            }
            if free && !text.is_empty() {
                placed.push(bx);
                d.layer("nazwy", crate::i18n::tr("Nazwy")).push(Item::Text { x: x + 1.6, y: y + 0.6, text, size, color: INK, anchor: Anchor::Start, angle: 0.0, halo: true, bold });
            }
        }
    }

    // siatka geograficzna z opisami na ramce
    if o.graticule {
        graticule(&mut d, frame, b, &tp);
    }

    // ramka obszaru
    d.layer("ramka", crate::i18n::tr("Ramka")).push(Item::Path { pts: outline, closed: true, stroke: Some((INK, 0.35)), fill: None, dash: None });

    // tytuł, skala, podziałka, północ, legenda, źródła
    let head = d.layer("opis", crate::i18n::tr("Tytuł i opis"));
    head.push(Item::Text { x: ml, y: 11.0, text: title.clone(), size: 6.0, color: INK, anchor: Anchor::Start, angle: 0.0, halo: false, bold: true });
    let (clon, clat) = frame.to_geo((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
    head.push(Item::Text {
        x: ml,
        y: 17.5,
        text: tf!("Skala 1:{} · środek {} · warstwice co {} m · wysokości w m n.p.m.", scale_n, crate::coords::format_pos(clon, clat), o.step),
        size: 2.6,
        color: [90, 96, 104],
        anchor: Anchor::Start,
        angle: 0.0,
        halo: false,
        bold: false,
    });
    let bar_m = nice((wm / 4.0).max(10.0));
    let (bx, by) = (ml, ph - mbot + 8.0);
    let sc = d.layer("podzialka", crate::i18n::tr("Podziałka"));
    for k in 0..4 {
        let (a, c) = (bx + k as f64 * bar_m * s / 4.0, bx + (k + 1) as f64 * bar_m * s / 4.0);
        sc.push(Item::Path { pts: vec![(a, by), (c, by), (c, by + 1.4), (a, by + 1.4)], closed: true, stroke: Some((INK, 0.15)), fill: Some(if k % 2 == 0 { INK } else { [255, 255, 255] }), dash: None });
    }
    sc.push(Item::Text { x: bx, y: by - 1.2, text: "0".into(), size: 2.2, color: INK, anchor: Anchor::Middle, angle: 0.0, halo: false, bold: false });
    sc.push(Item::Text { x: bx + bar_m * s, y: by - 1.2, text: super::section::fmt_len(bar_m), size: 2.2, color: INK, anchor: Anchor::Middle, angle: 0.0, halo: false, bold: false });
    // strzałka północy
    let (nx, ny) = (pw - mr - 6.0, 10.0);
    sc.push(Item::Path { pts: vec![(nx, ny - 5.0), (nx + 2.2, ny + 2.5), (nx, ny + 1.2), (nx - 2.2, ny + 2.5)], closed: true, stroke: None, fill: Some(INK), dash: None });
    sc.push(Item::Text { x: nx, y: ny - 6.0, text: "N".into(), size: 3.0, color: INK, anchor: Anchor::Middle, angle: 0.0, halo: false, bold: true });
    if o.legend {
        legend(&mut d, o, ml + bar_m * s + 18.0, ph - mbot + 6.0, feats.is_some());
    }
    super::section::credits_block(&mut d, credits);
    d
}

/// Opisy warstwicy: co ~120 mm, obrócone wzdłuż linii, zawsze czytelne (nie do góry nogami).
fn contour_labels(pts: &[(f64, f64)], text: &str) -> Vec<Item> {
    let total = contour::length(pts);
    let mut out = Vec::new();
    let mut next = total.min(120.0) / 2.0;
    let mut acc = 0.0;
    for w in pts.windows(2) {
        let l = ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt();
        while acc + l >= next && l > 0.0 {
            let t = (next - acc) / l;
            let (x, y) = (w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t);
            let mut ang = (w[1].1 - w[0].1).atan2(w[1].0 - w[0].0).to_degrees();
            if ang > 90.0 {
                ang -= 180.0;
            } else if ang < -90.0 {
                ang += 180.0;
            }
            out.push(Item::Text { x, y: y + 0.7, text: text.to_string(), size: 2.0, color: CONTOUR, anchor: Anchor::Middle, angle: ang, halo: true, bold: false });
            next += 120.0;
        }
        acc += l;
    }
    out
}

/// Fragment siatki obejmujący prostokąt (z zapasem dwóch oczek).
pub fn crop(g: &Grid, b: [f64; 4]) -> Grid {
    let i0 = (((b[0] - g.x0) / g.cell).floor() as i64 - 2).clamp(0, g.nx as i64 - 2) as usize;
    let i1 = (((b[2] - g.x0) / g.cell).ceil() as i64 + 2).clamp(i0 as i64 + 1, g.nx as i64 - 1) as usize;
    let j0 = (((g.y1 - b[3]) / g.cell).floor() as i64 - 2).clamp(0, g.ny as i64 - 2) as usize;
    let j1 = (((g.y1 - b[1]) / g.cell).ceil() as i64 + 2).clamp(j0 as i64 + 1, g.ny as i64 - 1) as usize;
    let (nx, ny) = (i1 - i0 + 1, j1 - j0 + 1);
    let mut z = Vec::with_capacity(nx * ny);
    for j in j0..=j1 {
        z.extend_from_slice(&g.z[j * g.nx + i0..=j * g.nx + i1]);
    }
    Grid { nx, ny, x0: g.x(i0), y1: g.y(j0), cell: g.cell, z }
}

fn graticule(d: &mut Drawing, frame: &Frame, b: [f64; 4], tp: &dyn Fn(f64, f64) -> (f64, f64)) {
    let (w, s) = frame.to_geo(b[0], b[1]);
    let (e, n) = frame.to_geo(b[2], b[3]);
    // krok w minutach kątowych: 4–8 linii
    let span = ((e - w).max(n - s)) * 60.0;
    let step = [0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0].into_iter().find(|st| span / st <= 7.0).unwrap_or(600.0) / 60.0;
    let fmt = |v: f64, pos: char, neg: char| {
        let a = v.abs();
        let deg = a.floor();
        let min = ((a - deg) * 60.0).round();
        let (deg, min) = if min >= 60.0 { (deg + 1.0, 0.0) } else { (deg, min) };
        format!("{}°{:02}′{}", deg, min, if v >= 0.0 { pos } else { neg })
    };
    let grid_c: Rgb = [90, 110, 150];
    let mut items = Vec::new();
    let mut lon = (w / step).ceil() * step;
    while lon <= e {
        let pts: Vec<(f64, f64)> = (0..=24).map(|k| {
            let lat = s + (n - s) * k as f64 / 24.0;
            let (x, y) = frame.to_m(lon, lat);
            tp(x, y)
        }).collect();
        if let Some(&top) = pts.last() {
            items.push(Item::Text { x: top.0, y: top.1 - 1.2, text: fmt(lon, 'E', 'W'), size: 2.0, color: grid_c, anchor: Anchor::Middle, angle: 0.0, halo: true, bold: false });
        }
        items.push(Item::Path { pts, closed: false, stroke: Some((grid_c, 0.1)), fill: None, dash: Some(1.0) });
        lon += step;
    }
    let mut lat = (s / step).ceil() * step;
    while lat <= n {
        let pts: Vec<(f64, f64)> = (0..=24).map(|k| {
            let lo = w + (e - w) * k as f64 / 24.0;
            let (x, y) = frame.to_m(lo, lat);
            tp(x, y)
        }).collect();
        if let Some(&left) = pts.first() {
            items.push(Item::Text { x: left.0 - 1.0, y: left.1 + 0.7, text: fmt(lat, 'N', 'S'), size: 2.0, color: grid_c, anchor: Anchor::End, angle: 0.0, halo: true, bold: false });
        }
        items.push(Item::Path { pts, closed: false, stroke: Some((grid_c, 0.1)), fill: None, dash: Some(1.0) });
        lat += step;
    }
    d.layer("siatka", crate::i18n::tr("Siatka geograficzna")).extend(items);
}

fn legend(d: &mut Drawing, o: &MapOpts, x: f64, y: f64, osm: bool) {
    let mut rows: Vec<(Item, String)> = Vec::new();
    let line = |c: Rgb, w: f64, dash: Option<f64>| Item::Path { pts: vec![(0.0, 0.0), (7.0, 0.0)], closed: false, stroke: Some((c, w)), fill: None, dash };
    let rect = |c: Rgb| Item::Path { pts: vec![(0.0, -1.2), (7.0, -1.2), (7.0, 1.2), (0.0, 1.2)], closed: true, stroke: Some(([120, 120, 120], 0.08)), fill: Some(c), dash: None };
    if o.contours {
        rows.push((line(CONTOUR, 0.22, None), tf!("warstwice co {} m", o.step)));
    }
    if osm {
        if o.trails {
            rows.push((line([214, 40, 40], 0.55, None), crate::i18n::tr("szlak turystyczny (w kolorze znakowania)").to_string()));
        }
        if o.paths {
            rows.push((line([90, 70, 60], 0.16, Some(0.7)), crate::i18n::tr("ścieżka").to_string()));
        }
        if o.roads {
            rows.push((line([110, 100, 92], 0.35, None), crate::i18n::tr("droga").to_string()));
        }
        if o.water {
            rows.push((line(WATER, 0.45, None), crate::i18n::tr("rzeka, potok").to_string()));
        }
        if o.forest {
            rows.push((rect(FOREST), crate::i18n::tr("las").to_string()));
        }
        if o.rock {
            rows.push((rect(ROCK), crate::i18n::tr("skały, piargi").to_string()));
        }
    }
    let lay = d.layer("legenda", crate::i18n::tr("Legenda"));
    let per_col = 3;
    for (k, (sym, text)) in rows.into_iter().enumerate() {
        let (cx, cy) = (x + (k / per_col) as f64 * 52.0, y + (k % per_col) as f64 * 5.0);
        let moved = match sym {
            Item::Path { pts, closed, stroke, fill, dash } => Item::Path { pts: pts.into_iter().map(|(a, b)| (a + cx, b + cy)).collect(), closed, stroke, fill, dash },
            other => other,
        };
        lay.push(moved);
        lay.push(Item::Text { x: cx + 9.0, y: cy + 0.8, text, size: 2.2, color: INK, anchor: Anchor::Start, angle: 0.0, halo: false, bold: false });
    }
}

/// Warstwice jako GeoJSON (LineString w stopniach, właściwość `ele`).
pub fn contours_geojson(g: &Grid, frame: &Frame, region: &Region, step: f32) -> String {
    let b = region.bounds();
    let sub = crop(g, b);
    let v: Vec<f64> = (0..sub.nx * sub.ny)
        .map(|k| if region.sd(sub.x(k % sub.nx), sub.y(k / sub.nx)) <= 0.0 { sub.z[k] as f64 } else { f64::NAN })
        .collect();
    let field = Field { nx: sub.nx, ny: sub.ny, v: &v };
    let (lo, hi) = sub.range();
    let step = step.max(1.0) as f64;
    let mut feats = Vec::new();
    let mut lvl = (lo as f64 / step).ceil() * step;
    while lvl <= hi as f64 {
        for l in contour::lines(&field, lvl) {
            let pts: Vec<(f64, f64)> = l.iter().map(|&(i, j)| (sub.x0 + i * sub.cell, sub.y1 - j * sub.cell)).collect();
            let pts = contour::simplify(&pts, sub.cell * 0.3);
            if pts.len() < 2 {
                continue;
            }
            let coords: Vec<[f64; 2]> = pts
                .iter()
                .map(|&(x, y)| {
                    let (lon, lat) = frame.to_geo(x, y);
                    [(lon * 1e6).round() / 1e6, (lat * 1e6).round() / 1e6]
                })
                .collect();
            feats.push(serde_json::json!({ "type": "Feature", "properties": { "ele": lvl }, "geometry": { "type": "LineString", "coordinates": coords } }));
        }
        lvl += step;
    }
    serde_json::json!({ "type": "FeatureCollection", "features": feats }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipping() {
        let r = Region::Circle { cx: 0.0, cy: 0.0, r: 10.0 };
        let parts = clip_line(&[(-20.0, 0.0), (20.0, 0.0)], &r);
        assert_eq!(parts.len(), 1);
        assert!((parts[0][0].0 + 10.0).abs() < 0.01 && (parts[0][1].0 - 10.0).abs() < 0.01);
        let sq = clip_rect(&[(-5.0, -5.0), (5.0, -5.0), (5.0, 5.0), (-5.0, 5.0)], [0.0, 0.0, 10.0, 10.0]);
        assert_eq!(sq.len(), 4);
    }

    #[test]
    fn whole_map() {
        let mut g = Grid::new([0.0, 0.0, 3000.0, 2000.0], 25.0);
        for j in 0..g.ny {
            for i in 0..g.nx {
                g.z[j * g.nx + i] = 1000.0 + (g.x(i) / 10.0) as f32 + (g.y(j) / 20.0) as f32;
            }
        }
        let f = Frame::new(20.0, 49.2);
        let r = Region::Rect { x0: 100.0, y0: 100.0, x1: 2900.0, y1: 1900.0 };
        let o = MapOpts { relief: Relief::Shade, dpi: 40, ..Default::default() };
        let d = map(&g, None, &f, &r, &Look::default(), &o, &["źródło".into()]);
        assert!(d.layers.iter().any(|l| l.id == "warstwice_g"));
        assert!(!super::super::draw::svg(&d).is_empty());
        let gj = contours_geojson(&g, &f, &r, 50.0);
        assert!(gj.contains("LineString"));
    }
}
