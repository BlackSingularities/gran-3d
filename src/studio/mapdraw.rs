//! Mapa wektorowa obszaru: podkład rastrowy (opcjonalnie), pokrycie terenu, warstwice z opisami,
//! wody, drogi i szlaki, szczyty i miejscowości, siatka geograficzna, podziałka, strzałka północy,
//! legenda i źródła danych. Wynik to [`Drawing`] – zapis do SVG, PDF albo DXF.
//!
//! Zasady jak na mapach drukowanych:
//! - wszystko, co należy do mapy, jest przycięte do obszaru (ścieżka przycinająca w SVG i PDF,
//!   geometria w DXF), a każdy napis mieści się w całości wewnątrz obszaru (z odstępem od brzegu);
//! - napisy nie nachodzą na siebie ani na znaki punktowe – dla nazw sprawdzanych jest kilka
//!   położeń wokół punktu, opisy warstwic i siatki przesuwają się wzdłuż linii;
//! - warstwy w grupach (Podkład, Pokrycie terenu, Rzeźba, Wody, Komunikacja…), każda z własnymi
//!   ustawieniami widoczności, koloru, grubości i krycia ([`LayerCfg`]).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::color::{self, Look};
use super::contour::{self, Field};
use super::draw::{text_width, Anchor, Drawing, Font, Item, LayerCfg, Rgb};
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
    B2,
    B1,
}

impl Paper {
    pub const ALL: [Paper; 8] = [Paper::Scale, Paper::A4, Paper::A3, Paper::A2, Paper::A1, Paper::A0, Paper::B2, Paper::B1];
    pub fn label(self) -> &'static str {
        match self {
            Paper::Scale => crate::i18n::tr("Według skali"),
            Paper::A4 => "A4 (210 × 297 mm)",
            Paper::A3 => "A3 (297 × 420 mm)",
            Paper::A2 => "A2 (420 × 594 mm)",
            Paper::A1 => "A1 (594 × 841 mm)",
            Paper::A0 => "A0 (841 × 1189 mm)",
            Paper::B2 => "B2 (500 × 707 mm)",
            Paper::B1 => "B1 (707 × 1000 mm)",
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
            Paper::B2 => Some((500.0, 707.0)),
            Paper::B1 => Some((707.0, 1000.0)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Orient {
    /// Według kształtu obszaru.
    #[default]
    Auto,
    Portrait,
    Landscape,
}

impl Orient {
    pub const ALL: [Orient; 3] = [Orient::Auto, Orient::Portrait, Orient::Landscape];
    pub fn label(self) -> &'static str {
        match self {
            Orient::Auto => crate::i18n::tr("Według obszaru"),
            Orient::Portrait => crate::i18n::tr("Pionowo"),
            Orient::Landscape => crate::i18n::tr("Poziomo"),
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

/// Styl kartograficzny – zestaw kolorów wszystkich warstw.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MapStyle {
    /// Mapa turystyczna (ciepłe warstwice, zielone lasy, kolorowe szlaki).
    #[default]
    Tourist,
    /// Topograficzna (chłodniejsza, mocniejsze drogi i wody).
    Topo,
    /// Minimalistyczna w odcieniach szarości – do druku i dalszej obróbki.
    Minimal,
    /// Ciemna (plakat, ekran).
    Night,
}

impl MapStyle {
    pub const ALL: [MapStyle; 4] = [MapStyle::Tourist, MapStyle::Topo, MapStyle::Minimal, MapStyle::Night];
    pub fn label(self) -> &'static str {
        match self {
            MapStyle::Tourist => crate::i18n::tr("Turystyczny"),
            MapStyle::Topo => crate::i18n::tr("Topograficzny"),
            MapStyle::Minimal => crate::i18n::tr("Minimalistyczny (szarości)"),
            MapStyle::Night => crate::i18n::tr("Ciemny (plakat)"),
        }
    }
    fn pal(self) -> Pal {
        match self {
            MapStyle::Tourist => Pal {
                paper: [255, 255, 255],
                ink: [28, 30, 34],
                ink2: [90, 96, 104],
                contour: [176, 120, 72],
                water: [70, 140, 210],
                water_fill: [178, 214, 240],
                forest: [196, 226, 182],
                rock: [214, 208, 200],
                scree: [228, 224, 216],
                glacier: [232, 244, 252],
                building: [140, 128, 124],
                major: [200, 120, 40],
                road: [110, 100, 92],
                track: [120, 96, 70],
                path: [90, 70, 60],
                rail: [40, 40, 40],
                grid: [90, 110, 150],
                hut: [210, 60, 50],
                relief: [246, 244, 240],
            },
            MapStyle::Topo => Pal {
                paper: [252, 252, 248],
                ink: [20, 22, 26],
                ink2: [80, 84, 92],
                contour: [150, 100, 60],
                water: [30, 110, 190],
                water_fill: [160, 204, 236],
                forest: [208, 232, 196],
                rock: [222, 218, 212],
                scree: [234, 232, 226],
                glacier: [226, 240, 252],
                building: [90, 86, 84],
                major: [220, 60, 40],
                road: [40, 40, 40],
                track: [70, 70, 70],
                path: [40, 40, 40],
                rail: [20, 20, 20],
                grid: [60, 100, 170],
                hut: [200, 40, 40],
                relief: [250, 250, 246],
            },
            MapStyle::Minimal => Pal {
                paper: [255, 255, 255],
                ink: [20, 20, 20],
                ink2: [100, 100, 100],
                contour: [150, 150, 150],
                water: [110, 110, 110],
                water_fill: [215, 215, 215],
                forest: [236, 236, 236],
                rock: [224, 224, 224],
                scree: [232, 232, 232],
                glacier: [246, 246, 246],
                building: [120, 120, 120],
                major: [40, 40, 40],
                road: [70, 70, 70],
                track: [100, 100, 100],
                path: [90, 90, 90],
                rail: [30, 30, 30],
                grid: [160, 160, 160],
                hut: [40, 40, 40],
                relief: [252, 252, 252],
            },
            MapStyle::Night => Pal {
                paper: [20, 24, 30],
                ink: [236, 232, 222],
                ink2: [160, 166, 176],
                contour: [196, 150, 98],
                water: [90, 160, 230],
                water_fill: [30, 52, 76],
                forest: [32, 52, 40],
                rock: [56, 56, 60],
                scree: [48, 48, 52],
                glacier: [70, 86, 100],
                building: [96, 96, 104],
                major: [236, 170, 80],
                road: [200, 190, 170],
                track: [170, 150, 120],
                path: [190, 170, 150],
                rail: [210, 210, 210],
                grid: [90, 120, 170],
                hut: [240, 100, 80],
                relief: [44, 50, 60],
            },
        }
    }
}

/// Kolory stylu.
#[derive(Clone, Copy)]
struct Pal {
    paper: Rgb,
    ink: Rgb,
    ink2: Rgb,
    contour: Rgb,
    water: Rgb,
    water_fill: Rgb,
    forest: Rgb,
    rock: Rgb,
    scree: Rgb,
    glacier: Rgb,
    building: Rgb,
    major: Rgb,
    road: Rgb,
    track: Rgb,
    path: Rgb,
    rail: Rgb,
    grid: Rgb,
    hut: Rgb,
    relief: Rgb,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MapOpts {
    pub title: String,
    pub subtitle: String,
    pub author: String,
    pub scale: f32,
    pub paper: Paper,
    pub orient: Orient,
    /// Margines arkusza (mm).
    pub margin: f32,
    pub relief: Relief,
    pub dpi: u32,
    pub style: MapStyle,
    pub font: Font,
    /// Mnożnik wielkości napisów.
    pub text_scale: f32,
    /// Mnożnik grubości linii.
    pub line_scale: f32,
    /// Gęstość opisów warstwic (1 – co ok. 14 cm wzdłuż linii).
    pub label_density: f32,
    pub contours: bool,
    pub step: f32,
    /// Co która warstwica jest główna (pogrubiona, opisana).
    pub index_every: u8,
    pub contour_labels: bool,
    /// Wygładzanie warstwic (krzywe zamiast łamanych).
    pub smooth: bool,
    pub forest: bool,
    pub water: bool,
    pub rock: bool,
    pub buildings: bool,
    pub roads: bool,
    pub paths: bool,
    pub trails: bool,
    pub peaks: bool,
    /// Wysokości przy szczytach.
    pub peak_ele: bool,
    pub places: bool,
    pub graticule: bool,
    pub legend: bool,
    pub scalebar: bool,
    pub north: bool,
    /// Ramka obszaru (linia brzegu mapy).
    pub frame: bool,
    /// Ustawienia poszczególnych warstw (identyfikator → ustawienia).
    pub layers: BTreeMap<String, LayerCfg>,
    /// Przy zapisie SVG także każda warstwa w osobnym pliku.
    pub split_layers: bool,
}

impl Default for MapOpts {
    fn default() -> Self {
        Self {
            title: String::new(),
            subtitle: String::new(),
            author: String::new(),
            scale: 25_000.0,
            paper: Paper::Scale,
            orient: Orient::Auto,
            margin: 12.0,
            relief: Relief::Shade,
            dpi: 200,
            style: MapStyle::Tourist,
            font: Font::Sans,
            text_scale: 1.0,
            line_scale: 1.0,
            label_density: 1.0,
            contours: true,
            step: 20.0,
            index_every: 5,
            contour_labels: true,
            smooth: true,
            forest: true,
            water: true,
            rock: true,
            buildings: true,
            roads: true,
            paths: true,
            trails: true,
            peaks: true,
            peak_ele: true,
            places: true,
            graticule: true,
            legend: true,
            scalebar: true,
            north: true,
            frame: true,
            layers: BTreeMap::new(),
            split_layers: false,
        }
    }
}

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
            let probe: Vec<(f64, f64)> = (1..SAMPLES)
                .map(|i| {
                    let t = i as f64 / SAMPLES as f64;
                    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
                })
                .collect();
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

/// „25 000” – liczba z odstępami tysięcy.
pub fn group(n: f64) -> String {
    let s = format!("{:.0}", n.abs());
    let mut out = String::new();
    for (k, c) in s.chars().enumerate() {
        if k > 0 && (s.len() - k) % 3 == 0 {
            out.push('\u{2009}');
        }
        out.push(c);
    }
    if n < 0.0 {
        format!("−{out}")
    } else {
        out
    }
}

/// Wygładzenie łamanej (Chaikin, dwa przebiegi) z zachowaniem końców otwartej linii.
fn chaikin(pts: &[(f64, f64)]) -> Vec<(f64, f64)> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let closed = pts.first() == pts.last();
    let mut cur = pts.to_vec();
    for _ in 0..2 {
        let mut next = Vec::with_capacity(cur.len() * 2);
        if !closed {
            next.push(cur[0]);
        }
        for w in cur.windows(2) {
            let (a, b) = (w[0], w[1]);
            next.push((a.0 * 0.75 + b.0 * 0.25, a.1 * 0.75 + b.1 * 0.25));
            next.push((a.0 * 0.25 + b.0 * 0.75, a.1 * 0.25 + b.1 * 0.75));
        }
        if closed {
            let f = next[0];
            next.push(f);
        } else {
            next.push(*cur.last().unwrap());
        }
        cur = next;
    }
    cur
}

/// Rozmieszczanie napisów: całe wewnątrz obszaru (z odstępem od brzegu) i bez nakładania się.
struct Placer<'a> {
    region: &'a Region,
    /// Strona (mm) → metry obszaru.
    inv: &'a dyn Fn(f64, f64) -> (f64, f64),
    /// Odstęp od brzegu obszaru (m).
    margin_m: f64,
    placed: Vec<[f64; 4]>,
}

impl Placer<'_> {
    fn inside(&self, p: (f64, f64)) -> bool {
        let (x, y) = (self.inv)(p.0, p.1);
        self.region.sd(x, y) <= -self.margin_m
    }

    /// Czy czworokąt (strona, mm) leży cały w obszarze – próbki co ~0,8 mm po obwodzie.
    fn fits(&self, q: &[(f64, f64); 4]) -> bool {
        for k in 0..4 {
            let (a, b) = (q[k], q[(k + 1) % 4]);
            let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
            let n = (len / 0.8).ceil().max(1.0) as usize;
            for i in 0..n {
                let t = i as f64 / n as f64;
                if !self.inside((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)) {
                    return false;
                }
            }
        }
        true
    }

    fn free(&self, bb: [f64; 4]) -> bool {
        !self.placed.iter().any(|p| bb[0] < p[2] && bb[2] > p[0] && bb[1] < p[3] && bb[3] > p[1])
    }

    /// Próbuje postawić napis; zwraca `true`, gdy się udało (miejsce zostaje zajęte).
    fn try_text(&mut self, x: f64, y: f64, w: f64, size: f64, anchor: Anchor, angle: f64) -> bool {
        let q = text_quad(x, y, w, size, anchor, angle);
        let bb = aabb(&q);
        if self.free(bb) && self.fits(&q) {
            self.placed.push(bb);
            true
        } else {
            false
        }
    }
}

/// Czworokąt napisu (z marginesem na otoczkę) obrócony wokół punktu wstawienia.
fn text_quad(x: f64, y: f64, w: f64, size: f64, anchor: Anchor, angle: f64) -> [(f64, f64); 4] {
    let pad = size * 0.18;
    let x0 = match anchor {
        Anchor::Start => 0.0,
        Anchor::Middle => -w / 2.0,
        Anchor::End => -w,
    } - pad;
    let x1 = x0 + w + 2.0 * pad;
    let (y0, y1) = (-size * 0.78 - pad, size * 0.24 + pad);
    let (s, c) = angle.to_radians().sin_cos();
    let rot = |dx: f64, dy: f64| (x + dx * c - dy * s, y + dx * s + dy * c);
    [rot(x0, y0), rot(x1, y0), rot(x1, y1), rot(x0, y1)]
}

fn aabb(q: &[(f64, f64); 4]) -> [f64; 4] {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for p in q {
        b = [b[0].min(p.0), b[1].min(p.1), b[2].max(p.0), b[3].max(p.1)];
    }
    b
}

/// Wpis legendy: znak i opis.
enum Sym {
    Line(Rgb, f64, Option<f64>),
    Rect(Rgb),
    Peak(Rgb),
    Hut(Rgb, Rgb),
    Dot(Rgb),
}

fn legend_rows(o: &MapOpts, p: &Pal, osm: bool, lw: f64) -> Vec<(Sym, String)> {
    let tr = |s: &str| crate::i18n::tr(s).to_string();
    let mut rows = Vec::new();
    if o.contours {
        rows.push((Sym::Line(p.contour, 0.24 * lw, None), tf!("warstwica główna co {} m", o.step * o.index_every.max(1) as f32)));
        rows.push((Sym::Line(p.contour, 0.1 * lw, None), tf!("warstwica co {} m", o.step)));
    }
    if osm {
        if o.trails {
            rows.push((Sym::Line([214, 40, 40], 0.55 * lw, None), tr("szlak turystyczny (w kolorze znakowania)")));
        }
        if o.roads {
            rows.push((Sym::Line(p.major, 0.7 * lw, None), tr("droga główna")));
            rows.push((Sym::Line(p.road, 0.35 * lw, None), tr("droga")));
            rows.push((Sym::Line(p.rail, 0.45 * lw, None), tr("kolej, wyciąg")));
        }
        if o.paths {
            rows.push((Sym::Line(p.track, 0.22 * lw, Some(1.2)), tr("droga gruntowa")));
            rows.push((Sym::Line(p.path, 0.16 * lw, Some(0.7)), tr("ścieżka")));
        }
        if o.water {
            rows.push((Sym::Line(p.water, 0.45 * lw, None), tr("rzeka, potok")));
            rows.push((Sym::Rect(p.water_fill), tr("jezioro, zbiornik")));
        }
        if o.forest {
            rows.push((Sym::Rect(p.forest), tr("las")));
        }
        if o.rock {
            rows.push((Sym::Rect(p.rock), tr("skały, piargi")));
        }
        if o.buildings {
            rows.push((Sym::Rect(p.building), tr("budynek")));
        }
        if o.peaks {
            rows.push((Sym::Peak(p.ink), tr("szczyt (wysokość m n.p.m.)")));
            rows.push((Sym::Hut(p.ink, p.hut), tr("schronisko")));
        }
        if o.places {
            rows.push((Sym::Dot(p.ink), tr("miejscowość")));
        }
    }
    rows
}

fn draw_sym(out: &mut Vec<Item>, sym: &Sym, x: f64, y: f64, k: f64) {
    match *sym {
        Sym::Line(c, w, dash) => out.push(Item::Path { pts: vec![(x, y), (x + 7.0 * k, y)], closed: false, stroke: Some((c, w)), fill: None, dash }),
        Sym::Rect(c) => out.push(Item::Path { pts: vec![(x, y - 1.2 * k), (x + 7.0 * k, y - 1.2 * k), (x + 7.0 * k, y + 1.2 * k), (x, y + 1.2 * k)], closed: true, stroke: Some(([128, 128, 128], 0.08)), fill: Some(c), dash: None }),
        Sym::Peak(c) => out.push(Item::Path { pts: vec![(x + 2.5 * k, y + 0.9 * k), (x + 4.5 * k, y + 0.9 * k), (x + 3.5 * k, y - 1.1 * k)], closed: true, stroke: None, fill: Some(c), dash: None }),
        Sym::Hut(ink, c) => out.push(Item::Path { pts: vec![(x + 2.7 * k, y - 0.8 * k), (x + 4.3 * k, y - 0.8 * k), (x + 4.3 * k, y + 0.8 * k), (x + 2.7 * k, y + 0.8 * k)], closed: true, stroke: Some((ink, 0.15)), fill: Some(c), dash: None }),
        Sym::Dot(c) => out.push(Item::Circle { x: x + 3.5 * k, y, r: 0.7 * k, fill: Some(c), stroke: None }),
    }
}

/// Mapa obszaru jako rysunek. `dpi_cap` ogranicza rozdzielczość podkładu (podgląd na żywo).
#[allow(clippy::too_many_arguments)]
pub fn map(g: &Grid, feats: Option<&Features>, frame: &Frame, region: &Region, look: &Look, o: &MapOpts, credits: &[String], dpi_cap: Option<u32>) -> Drawing {
    let tr = crate::i18n::tr;
    let p = o.style.pal();
    let k = o.text_scale.clamp(0.4, 3.0) as f64;
    let lw = o.line_scale.clamp(0.2, 4.0) as f64;
    let fk = o.font.width_k();
    let tw = |s: &str, size: f64, bold: bool| text_width(s, size) * fk * if bold { 1.07 } else { 1.0 };
    let b = region.bounds();
    let (wm, hm) = ((b[2] - b[0]).max(1.0), (b[3] - b[1]).max(1.0));
    let mut credits: Vec<String> = credits.to_vec();
    if !o.author.trim().is_empty() {
        credits.insert(0, tf!("Opracowanie: {}", o.author.trim()));
    }
    let osm = feats.is_some();
    let legend_items = if o.legend { legend_rows(o, &p, osm, lw) } else { Vec::new() };

    // układ arkusza: margines, pas tytułu u góry, podziałka, legenda i źródła u dołu
    let m = o.margin.clamp(3.0, 60.0) as f64;
    let (ml, mr) = (m, m);
    let has_sub = !o.subtitle.trim().is_empty();
    let mt = m + (12.5 + if has_sub { 4.5 } else { 0.0 }) * k.min(1.8);
    let row_h = 4.6 * k;
    let col_w = 52.0 * k;
    let foot = |pw: f64| -> (f64, usize, usize) {
        let lx = ml + 46.0 * k;
        let cols = if legend_items.is_empty() { 1 } else { (((pw - mr - lx) / col_w).floor() as usize).clamp(1, 8) };
        let rows = legend_items.len().div_ceil(cols);
        let legend_h = (rows as f64 * row_h).max(if o.scalebar { 11.0 * k } else { 0.0 });
        let credits_h = credits.len() as f64 * 2.4 * k;
        (6.0 + legend_h + 3.0 + credits_h + m * 0.5, cols, rows)
    };
    let landscape = match o.orient {
        Orient::Auto => wm >= hm,
        Orient::Portrait => false,
        Orient::Landscape => true,
    };
    let (s, pw, ph, cols) = match o.paper.size() {
        Some((short, long)) => {
            let (pw, ph) = if landscape { (long, short) } else { (short, long) };
            let (mb, cols, _) = foot(pw);
            (((pw - ml - mr) / wm).min((ph - mt - mb) / hm).max(1e-6), pw, ph, cols)
        }
        None => {
            let s = 1000.0 / o.scale.max(500.0) as f64;
            let pw = (wm * s + ml + mr).max(160.0 * k);
            let (mb, cols, _) = foot(pw);
            (s, pw, hm * s + mt + mb, cols)
        }
    };
    let (mb, _, _) = foot(pw);
    // mapa wyśrodkowana w polu arkusza
    let ox = ml + ((pw - ml - mr) - wm * s) / 2.0;
    let oy = mt + ((ph - mt - mb) - hm * s) / 2.0;
    let tp = |x: f64, y: f64| (ox + (x - b[0]) * s, oy + (b[3] - y) * s);
    let inv = |px: f64, py: f64| (b[0] + (px - ox) / s, b[3] - (py - oy) / s);
    let title = if o.title.trim().is_empty() { tr("Mapa obszaru").to_string() } else { o.title.trim().to_string() };
    let scale_n = (1000.0 / s).round();
    let mut d = Drawing::new(pw, ph, &title);
    d.font = o.font;
    d.paper = p.paper;
    d.background = true;
    d.credits = credits.clone();
    let (clon, clat) = frame.to_geo((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
    d.desc = tf!("Skala 1:{} · środek {} · warstwice co {} m · wysokości w m n.p.m.", group(scale_n), crate::coords::format_pos(clon, clat), o.step);
    let outline: Vec<(f64, f64)> = region.outline(240).into_iter().map(|(x, y)| tp(x, y)).collect();
    d.clip = Some(outline.clone());

    // kolejność i grupy warstw
    let lay: &[(&'static str, &str, &'static str, &str, bool)] = &[
        ("g_podklad", tr("Podkład"), "podklad", tr("Cieniowanie rzeźby"), true),
        ("g_pokrycie", tr("Pokrycie terenu"), "lasy", tr("Lasy"), true),
        ("g_pokrycie", tr("Pokrycie terenu"), "skaly", tr("Skały i piargi"), true),
        ("g_pokrycie", tr("Pokrycie terenu"), "lodowce", tr("Lodowce"), true),
        ("g_pokrycie", tr("Pokrycie terenu"), "budynki", tr("Budynki"), true),
        ("g_rzezba", tr("Rzeźba terenu"), "warstwice", tr("Warstwice"), true),
        ("g_rzezba", tr("Rzeźba terenu"), "warstwice_g", tr("Warstwice główne"), true),
        ("g_wody", tr("Wody"), "wody_pow", tr("Jeziora i zbiorniki"), true),
        ("g_wody", tr("Wody"), "rzeki", tr("Rzeki i potoki"), true),
        ("g_komunikacja", tr("Komunikacja"), "drogi_lesne", tr("Drogi gruntowe"), true),
        ("g_komunikacja", tr("Komunikacja"), "sciezki", tr("Ścieżki"), true),
        ("g_komunikacja", tr("Komunikacja"), "drogi", tr("Drogi"), true),
        ("g_komunikacja", tr("Komunikacja"), "koleje", tr("Koleje i wyciągi"), true),
        ("g_szlaki", tr("Szlaki"), "szlaki", tr("Szlaki turystyczne"), true),
        ("g_siatka", tr("Siatka"), "siatka", tr("Siatka geograficzna"), true),
        ("g_symbole", tr("Znaki punktowe"), "punkty", tr("Szczyty, schroniska, miejscowości"), false),
        ("g_opisy", tr("Opisy"), "warstwice_opisy", tr("Opisy warstwic"), false),
        ("g_opisy", tr("Opisy"), "siatka_opisy", tr("Opisy siatki"), false),
        ("g_opisy", tr("Opisy"), "nazwy_obiektow", tr("Nazwy szczytów i obiektów"), false),
        ("g_opisy", tr("Opisy"), "nazwy_miejscowosci", tr("Nazwy miejscowości"), false),
        ("g_ramka", tr("Ramka i opis mapy"), "ramka", tr("Ramka obszaru"), false),
        ("g_ramka", tr("Ramka i opis mapy"), "tytul", tr("Tytuł i opis"), false),
        ("g_ramka", tr("Ramka i opis mapy"), "podzialka", tr("Podziałka"), false),
        ("g_ramka", tr("Ramka i opis mapy"), "polnoc", tr("Strzałka północy"), false),
        ("g_ramka", tr("Ramka i opis mapy"), "legenda", tr("Legenda"), false),
        ("g_ramka", tr("Ramka i opis mapy"), "zrodla", tr("Źródła danych"), false),
    ];
    for &(gid, gname, id, name, clipped) in lay {
        d.declare(gid, gname, id, name, clipped);
    }

    // podkład rastrowy
    if o.relief != Relief::None {
        let dpi = dpi_cap.map(|c| c.min(o.dpi)).unwrap_or(o.dpi).max(24) as f64;
        let px_w = ((wm * s / 25.4 * dpi) as u32).clamp(64, 7000);
        let px_h = ((hm * s / 25.4 * dpi) as u32).clamp(64, 7000);
        let mut lk = look.clone();
        lk.contours = false;
        if o.relief == Relief::Shade {
            lk.mode = color::Mode::Plain;
            lk.plain = p.relief;
            lk.shade = lk.shade.max(0.6);
        }
        // podkład z obiektami tylko w trybie kolorowym (wektorowe i tak będą na wierzchu)
        let sub = crop(g, b);
        let tex = color::texture(&sub, if o.relief == Relief::Color { feats } else { None }, &lk, px_w, px_h);
        let tm = color::TexMap::for_grid(&sub, px_w, px_h);
        let mut rgb = image::RgbImage::new(px_w, px_h);
        for (x, y, px) in rgb.enumerate_pixels_mut() {
            let (mx, my) = tm.m(x as f64 + 0.5, y as f64 + 0.5);
            let c = tex.get_pixel(x, y).0;
            // tuż za brzegiem zostaje kolor terenu (przycięcie ścieżką nie zostawia jasnej obwódki)
            *px = if region.sd(mx, my) <= sub.cell * 2.0 { image::Rgb([c[0], c[1], c[2]]) } else { image::Rgb(p.paper) };
        }
        let (x0, y0) = tp(sub.x0, sub.y1);
        let (x1, y1) = tp(sub.x1(), sub.y0());
        d.layer("podklad", "").push(Item::Image { x: x0, y: y0, w: x1 - x0, h: y1 - y0, img: rgb });
    }

    // pokrycie terenu
    if let Some(f) = feats {
        for poly in &f.polys {
            let (fill, layer, on) = match poly.kind {
                AreaKind::Forest => (p.forest, "lasy", o.forest),
                AreaKind::Rock => (p.rock, "skaly", o.rock),
                AreaKind::Scree => (p.scree, "skaly", o.rock),
                AreaKind::Glacier => (p.glacier, "lodowce", o.rock),
                AreaKind::Water => (p.water_fill, "wody_pow", o.water),
                AreaKind::Building => (p.building, "budynki", o.buildings),
            };
            if !on {
                continue;
            }
            let rings: Vec<Vec<(f64, f64)>> = poly
                .rings
                .iter()
                .map(|r| clip_rect(r, b))
                .filter(|r| r.len() >= 3)
                .map(|r| contour::simplify(&r.into_iter().map(|(x, y)| tp(x, y)).collect::<Vec<_>>(), 0.04))
                .filter(|r| r.len() >= 3)
                .collect();
            if rings.is_empty() {
                continue;
            }
            let stroke = if poly.kind == AreaKind::Water { Some((p.water, 0.12 * lw)) } else { None };
            d.layer(layer, "").push(Item::Rings { rings, fill, stroke });
        }
    }

    let margin_m = 0.8 / s;
    let mut placer = Placer { region, inv: &inv, margin_m, placed: Vec::new() };

    // znaki punktowe najpierw zajmują miejsce (napisy ich nie zasłonią)
    struct PointLabel {
        x: f64,
        y: f64,
        text: String,
        size: f64,
        bold: bool,
        color: Rgb,
        layer: &'static str,
        r: f64,
    }
    let mut point_labels: Vec<PointLabel> = Vec::new();
    if let Some(f) = feats {
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
            let (x, y) = tp(lb.x, lb.y);
            let rs = 1.1 * k;
            if !placer.fits(&[(x - rs, y - rs), (x + rs, y - rs), (x + rs, y + rs), (x - rs, y + rs)]) {
                continue;
            }
            let ele = lb.ele.filter(|_| o.peak_ele).map(|e| format!("{:.0}", e));
            let (text, size, bold, sym, layer) = match lb.kind {
                PointKind::Peak if o.peaks => {
                    let t = match (lb.name.trim().is_empty(), ele) {
                        (false, Some(e)) => format!("{} {}", lb.name.trim(), e),
                        (false, None) => lb.name.trim().to_string(),
                        (true, Some(e)) => e,
                        (true, None) => String::new(),
                    };
                    (t, 2.4, true, 0, "nazwy_obiektow")
                }
                PointKind::Saddle if o.peaks => (lb.name.clone(), 2.0, false, 1, "nazwy_obiektow"),
                PointKind::Hut if o.peaks => (lb.name.clone(), 2.1, false, 2, "nazwy_obiektow"),
                PointKind::Place(r) if o.places => (lb.name.clone(), [4.2, 3.4, 2.8, 2.3][r.min(3) as usize], r < 2, 3, "nazwy_miejscowosci"),
                _ => continue,
            };
            let sym_bb = [x - rs, y - rs, x + rs, y + rs];
            if !placer.free(sym_bb) {
                continue;
            }
            placer.placed.push(sym_bb);
            let ink = p.ink;
            let sl = d.layer("punkty", "");
            match sym {
                0 => sl.push(Item::Path { pts: vec![(x - 1.0 * k, y + 0.6 * k), (x + 1.0 * k, y + 0.6 * k), (x, y - 1.1 * k)], closed: true, stroke: None, fill: Some(ink), dash: None }),
                1 => sl.push(Item::Path { pts: vec![(x - 0.9 * k, y - 0.5 * k), (x, y + 0.3 * k), (x + 0.9 * k, y - 0.5 * k)], closed: false, stroke: Some((ink, 0.2 * lw)), fill: None, dash: None }),
                2 => sl.push(Item::Path { pts: vec![(x - 0.8 * k, y - 0.8 * k), (x + 0.8 * k, y - 0.8 * k), (x + 0.8 * k, y + 0.8 * k), (x - 0.8 * k, y + 0.8 * k)], closed: true, stroke: Some((ink, 0.15)), fill: Some(p.hut), dash: None }),
                _ => sl.push(Item::Circle { x, y, r: 0.6 * k, fill: Some(ink), stroke: None }),
            }
            let text = text.trim().to_string();
            if !text.is_empty() {
                point_labels.push(PointLabel { x, y, text, size: size * k, bold, color: ink, layer, r: rs });
            }
        }
    }

    // warstwice
    let mut contour_lines: Vec<(Vec<(f64, f64)>, String)> = Vec::new();
    if o.contours && o.step > 0.0 {
        // oczko co najmniej 0,35 mm na papierze
        let cell = (0.35 / s).max(g.cell);
        let sub = crop(g, b).resample(cell);
        let v: Vec<f64> = (0..sub.nx * sub.ny)
            .map(|q| {
                let (x, y) = (sub.x(q % sub.nx), sub.y(q / sub.nx));
                // pas poza obszarem (2 oczka) – linie dochodzą do brzegu, resztę przycina ścieżka
                if region.sd(x, y) <= cell * 2.0 {
                    sub.z[q] as f64
                } else {
                    f64::NAN
                }
            })
            .collect();
        let field = Field { nx: sub.nx, ny: sub.ny, v: &v };
        let (lo, hi) = sub.range();
        let step = o.step as f64;
        let every = o.index_every.max(1) as i64;
        let mut lvl = (lo as f64 / step).ceil() * step;
        while lvl <= hi as f64 {
            let index = ((lvl / step).round() as i64).rem_euclid(every) == 0;
            for l in contour::lines(&field, lvl) {
                let pts: Vec<(f64, f64)> = l.iter().map(|&(i, j)| tp(sub.x0 + i * sub.cell, sub.y1 - j * sub.cell)).collect();
                let pts = contour::simplify(&pts, 0.06);
                let pts = if o.smooth { chaikin(&pts) } else { pts };
                if contour::length(&pts) < 1.5 {
                    continue;
                }
                if index && o.contour_labels {
                    contour_lines.push((pts.clone(), format!("{:.0}", lvl)));
                }
                let (layer, w) = if index { ("warstwice_g", 0.24) } else { ("warstwice", 0.1) };
                d.layer(layer, "").push(Item::Path { pts, closed: false, stroke: Some((p.contour, w * lw)), fill: None, dash: None });
            }
            lvl += step;
        }
    }

    // linie: wody, drogi, ścieżki, szlaki (przycięte geometrycznie do obszaru)
    if let Some(f) = feats {
        for l in &f.lines {
            let (c, w, dash, layer, on) = match l.kind {
                LineKind::River => (p.water, 0.45, None, "rzeki", o.water),
                LineKind::Stream => (p.water, 0.2, None, "rzeki", o.water),
                LineKind::MajorRoad => (p.major, 0.7, None, "drogi", o.roads),
                LineKind::Road => (p.road, 0.35, None, "drogi", o.roads),
                LineKind::Track => (p.track, 0.22, Some(1.2), "drogi_lesne", o.paths),
                LineKind::Path => (p.path, 0.16, Some(0.7), "sciezki", o.paths),
                LineKind::Rail => (p.rail, 0.45, None, "koleje", o.roads),
                LineKind::Aerialway => (p.rail, 0.25, None, "koleje", o.roads),
                LineKind::Trail(c) => (if o.style == MapStyle::Minimal { [60, 60, 60] } else { c }, 0.55, None, "szlaki", o.trails),
            };
            if !on {
                continue;
            }
            for part in clip_line(&l.pts, region) {
                let pts: Vec<(f64, f64)> = part.into_iter().map(|(x, y)| tp(x, y)).collect();
                let pts = contour::simplify(&pts, 0.04);
                d.layer(layer, "").push(Item::Path { pts, closed: false, stroke: Some((c, w * lw)), fill: None, dash: dash.map(|v| v * lw.sqrt()) });
            }
        }
    }

    // nazwy: kilka położeń wokół punktu, pierwsze wolne i całe w obszarze
    for pl in &point_labels {
        let w = tw(&pl.text, pl.size, pl.bold);
        let gap = 0.9 * k + pl.r * 0.5;
        let cands = [
            (pl.x + gap, pl.y + pl.size * 0.34, Anchor::Start),
            (pl.x - gap, pl.y + pl.size * 0.34, Anchor::End),
            (pl.x + gap * 0.6, pl.y - gap, Anchor::Start),
            (pl.x + gap * 0.6, pl.y + gap + pl.size * 0.7, Anchor::Start),
            (pl.x - gap * 0.6, pl.y - gap, Anchor::End),
            (pl.x - gap * 0.6, pl.y + gap + pl.size * 0.7, Anchor::End),
            (pl.x, pl.y - gap - pl.size * 0.25, Anchor::Middle),
            (pl.x, pl.y + gap + pl.size * 0.8, Anchor::Middle),
        ];
        for (x, y, anchor) in cands {
            if placer.try_text(x, y, w, pl.size, anchor, 0.0) {
                d.layer(pl.layer, "").push(Item::Text { x, y, text: pl.text.clone(), size: pl.size, color: pl.color, anchor, angle: 0.0, halo: true, bold: pl.bold });
                break;
            }
        }
    }

    // opisy warstwic: wzdłuż linii, obrócone, czytelne (nie do góry nogami), całe w obszarze
    let csize = 2.0 * k;
    let spacing = 140.0 / o.label_density.clamp(0.2, 5.0) as f64;
    contour_lines.sort_by(|a, b| contour::length(&b.0).total_cmp(&contour::length(&a.0)));
    for (pts, text) in &contour_lines {
        let total = contour::length(pts);
        let w = tw(text, csize, false);
        if total < w * 3.0 {
            continue;
        }
        let mut next = (total.min(spacing) / 2.0).max(w);
        let mut acc = 0.0;
        for seg in pts.windows(2) {
            let l = ((seg[1].0 - seg[0].0).powi(2) + (seg[1].1 - seg[0].1).powi(2)).sqrt();
            while l > 0.0 && acc + l >= next && next <= total - w {
                let t = (next - acc) / l;
                let (x, y) = (seg[0].0 + (seg[1].0 - seg[0].0) * t, seg[0].1 + (seg[1].1 - seg[0].1) * t);
                let mut ang = (seg[1].1 - seg[0].1).atan2(seg[1].0 - seg[0].0).to_degrees();
                if ang > 90.0 {
                    ang -= 180.0;
                } else if ang < -90.0 {
                    ang += 180.0;
                }
                // środek napisu na linii: punkt wstawienia przesunięty o pół wysokości w dół napisu
                let (sa, ca) = ang.to_radians().sin_cos();
                let (tx, ty) = (x - sa * csize * 0.35, y + ca * csize * 0.35);
                if placer.try_text(tx, ty, w, csize, Anchor::Middle, ang) {
                    d.layer("warstwice_opisy", "").push(Item::Text { x: tx, y: ty, text: text.clone(), size: csize, color: p.contour, anchor: Anchor::Middle, angle: ang, halo: true, bold: false });
                    next += spacing;
                } else {
                    next += 8.0;
                }
            }
            acc += l;
        }
    }

    // siatka geograficzna: linie przycięte do obszaru, opisy wewnątrz obszaru
    if o.graticule {
        graticule(&mut d, frame, region, &tp, &mut placer, p.grid, k, lw, fk);
    }

    // ramka obszaru
    if o.frame {
        d.layer("ramka", "").push(Item::Path { pts: outline, closed: true, stroke: Some((p.ink, 0.35 * lw)), fill: None, dash: None });
    }

    // tytuł i opis
    {
        let head = d.layer("tytul", "");
        let ty = m + 6.2 * k.min(1.8);
        head.push(Item::Text { x: ml, y: ty, text: title.clone(), size: 6.4 * k.min(1.8), color: p.ink, anchor: Anchor::Start, angle: 0.0, halo: false, bold: true });
        let mut y = ty + 5.0 * k.min(1.8);
        if has_sub {
            head.push(Item::Text { x: ml, y, text: o.subtitle.trim().to_string(), size: 3.4 * k.min(1.8), color: p.ink, anchor: Anchor::Start, angle: 0.0, halo: false, bold: false });
            y += 4.4 * k.min(1.8);
        }
        let info = tf!("Skala 1:{} · środek {} · warstwice co {} m · wysokości w m n.p.m.", group(scale_n), crate::coords::format_pos(clon, clat), o.step);
        head.push(Item::Text { x: ml, y, text: info, size: 2.5 * k.min(1.6), color: p.ink2, anchor: Anchor::Start, angle: 0.0, halo: false, bold: false });
    }
    // strzałka północy
    if o.north {
        let (nx, ny) = (pw - mr - 4.0 * k, m + 6.0 * k);
        let n = d.layer("polnoc", "");
        n.push(Item::Path { pts: vec![(nx, ny - 5.0 * k), (nx + 2.2 * k, ny + 2.5 * k), (nx, ny + 1.2 * k)], closed: true, stroke: None, fill: Some(p.ink), dash: None });
        n.push(Item::Path { pts: vec![(nx, ny - 5.0 * k), (nx, ny + 1.2 * k), (nx - 2.2 * k, ny + 2.5 * k)], closed: true, stroke: Some((p.ink, 0.2)), fill: Some(p.paper), dash: None });
        n.push(Item::Text { x: nx, y: ny - 6.0 * k, text: "N".into(), size: 3.0 * k, color: p.ink, anchor: Anchor::Middle, angle: 0.0, halo: false, bold: true });
    }
    // stopka: podziałka, legenda, źródła
    let yb = ph - mb + 6.0;
    if o.scalebar {
        let bar_m = nice((wm / 4.0).max(10.0).min(45.0 / s));
        let bw = bar_m * s;
        let by = yb + 2.0 * k;
        let sc = d.layer("podzialka", "");
        for q in 0..4 {
            let (a, c) = (ml + q as f64 * bw / 4.0, ml + (q + 1) as f64 * bw / 4.0);
            sc.push(Item::Path { pts: vec![(a, by), (c, by), (c, by + 1.4 * k), (a, by + 1.4 * k)], closed: true, stroke: Some((p.ink, 0.15)), fill: Some(if q % 2 == 0 { p.ink } else { p.paper }), dash: None });
        }
        let fs = 2.2 * k;
        sc.push(Item::Text { x: ml, y: by - 1.0, text: "0".into(), size: fs, color: p.ink, anchor: Anchor::Middle, angle: 0.0, halo: false, bold: false });
        sc.push(Item::Text { x: ml + bw / 2.0, y: by - 1.0, text: super::section::fmt_len(bar_m / 2.0), size: fs, color: p.ink, anchor: Anchor::Middle, angle: 0.0, halo: false, bold: false });
        sc.push(Item::Text { x: ml + bw, y: by - 1.0, text: super::section::fmt_len(bar_m), size: fs, color: p.ink, anchor: Anchor::Middle, angle: 0.0, halo: false, bold: false });
        sc.push(Item::Text { x: ml, y: by + 1.4 * k + 3.2 * k, text: tf!("Skala 1:{}", group(scale_n)), size: 2.4 * k, color: p.ink, anchor: Anchor::Start, angle: 0.0, halo: false, bold: true });
    }
    if !legend_items.is_empty() {
        let lx = ml + 46.0 * k;
        let rows = legend_items.len().div_ceil(cols);
        let mut items = Vec::new();
        for (q, (sym, text)) in legend_items.iter().enumerate() {
            let (cx, cy) = (lx + (q / rows) as f64 * col_w, yb + 1.0 * k + (q % rows) as f64 * row_h);
            draw_sym(&mut items, sym, cx, cy, k);
            items.push(Item::Text { x: cx + 9.0 * k, y: cy + 0.8 * k, text: text.clone(), size: 2.2 * k, color: p.ink, anchor: Anchor::Start, angle: 0.0, halo: false, bold: false });
        }
        d.layer("legenda", "").extend(items);
    }
    {
        let n = credits.len();
        let z = d.layer("zrodla", "");
        for (q, c) in credits.iter().enumerate() {
            z.push(Item::Text { x: ml, y: ph - m * 0.5 - (n - 1 - q) as f64 * 2.4 * k, text: c.clone(), size: 1.8 * k, color: p.ink2, anchor: Anchor::Start, angle: 0.0, halo: false, bold: false });
        }
    }
    d.apply(&o.layers);
    d
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

#[allow(clippy::too_many_arguments)]
fn graticule(d: &mut Drawing, frame: &Frame, region: &Region, tp: &dyn Fn(f64, f64) -> (f64, f64), placer: &mut Placer, grid_c: Rgb, k: f64, lw: f64, fk: f64) {
    let b = region.bounds();
    let (w, s) = frame.to_geo(b[0], b[1]);
    let (e, n) = frame.to_geo(b[2], b[3]);
    // krok w minutach kątowych: 4–8 linii
    let span = ((e - w).max(n - s)) * 60.0;
    let step = [0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0].into_iter().find(|st| span / st <= 7.0).unwrap_or(600.0) / 60.0;
    let fmt = |v: f64, pos: char, neg: char| {
        let a = v.abs();
        let deg = a.floor();
        let min = ((a - deg) * 60.0 * 2.0).round() / 2.0;
        let (deg, min) = if min >= 60.0 { (deg + 1.0, 0.0) } else { (deg, min) };
        if min.fract() != 0.0 {
            format!("{}°{:04.1}′{}", deg, min, if v >= 0.0 { pos } else { neg })
        } else {
            format!("{}°{:02}′{}", deg, min, if v >= 0.0 { pos } else { neg })
        }
    };
    let size = 2.0 * k;
    let mut lines = Vec::new();
    let mut labels = Vec::new();
    // południk: linia i opis tuż pod górnym brzegiem obszaru (szukany w dół wzdłuż linii)
    let mut lon = (w / step).ceil() * step;
    while lon <= e {
        let geo: Vec<(f64, f64)> = (0..=48).map(|q| frame.to_m(lon, s + (n - s) * q as f64 / 48.0)).collect();
        let text = fmt(lon, 'E', 'W');
        let tw = text_width(&text, size) * fk;
        for part in clip_line(&geo, region) {
            let pts: Vec<(f64, f64)> = part.iter().map(|&(x, y)| tp(x, y)).collect();
            // od góry: punkty strony o najmniejszym y
            let mut cand: Vec<(f64, f64)> = pts.clone();
            cand.sort_by(|a, b| a.1.total_cmp(&b.1));
            let top = cand.first().copied();
            if let Some(t) = top {
                let bottom = cand.last().map(|p| p.1).unwrap_or(t.1);
                let mut y = t.1 + size * 1.4;
                while y < bottom {
                    // x linii na tej wysokości (linia prawie pionowa)
                    let x = interp_x(&pts, y).unwrap_or(t.0);
                    if placer.try_text(x + 0.8, y, tw, size, Anchor::Start, 0.0) {
                        labels.push(Item::Text { x: x + 0.8, y, text: text.clone(), size, color: grid_c, anchor: Anchor::Start, angle: 0.0, halo: true, bold: false });
                        break;
                    }
                    y += 1.5;
                }
            }
            lines.push(Item::Path { pts, closed: false, stroke: Some((grid_c, 0.1 * lw)), fill: None, dash: Some(1.0) });
        }
        lon += step;
    }
    // równoleżnik: opis nad linią, tuż przy lewym brzegu obszaru (szukany w prawo)
    let mut lat = (s / step).ceil() * step;
    while lat <= n {
        let geo: Vec<(f64, f64)> = (0..=48).map(|q| frame.to_m(w + (e - w) * q as f64 / 48.0, lat)).collect();
        let text = fmt(lat, 'N', 'S');
        let tw = text_width(&text, size) * fk;
        for part in clip_line(&geo, region) {
            let pts: Vec<(f64, f64)> = part.iter().map(|&(x, y)| tp(x, y)).collect();
            let (x0, x1) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, c), p| (a.min(p.0), c.max(p.0)));
            let mut x = x0 + 1.0;
            while x + tw < x1 {
                let y = interp_y(&pts, x).unwrap_or(pts[0].1) - 0.8;
                if placer.try_text(x, y, tw, size, Anchor::Start, 0.0) {
                    labels.push(Item::Text { x, y, text: text.clone(), size, color: grid_c, anchor: Anchor::Start, angle: 0.0, halo: true, bold: false });
                    break;
                }
                x += 1.5;
            }
            lines.push(Item::Path { pts, closed: false, stroke: Some((grid_c, 0.1 * lw)), fill: None, dash: Some(1.0) });
        }
        lat += step;
    }
    d.layer("siatka", "").extend(lines);
    d.layer("siatka_opisy", "").extend(labels);
}

/// x łamanej na wysokości y (pierwsze przecięcie).
fn interp_x(pts: &[(f64, f64)], y: f64) -> Option<f64> {
    pts.windows(2).find_map(|w| {
        let (a, b) = (w[0], w[1]);
        ((a.1 - y) * (b.1 - y) <= 0.0 && (b.1 - a.1).abs() > 1e-9).then(|| a.0 + (b.0 - a.0) * (y - a.1) / (b.1 - a.1))
    })
}

/// y łamanej w punkcie x (pierwsze przecięcie).
fn interp_y(pts: &[(f64, f64)], x: f64) -> Option<f64> {
    pts.windows(2).find_map(|w| {
        let (a, b) = (w[0], w[1]);
        ((a.0 - x) * (b.0 - x) <= 0.0 && (b.0 - a.0).abs() > 1e-9).then(|| a.1 + (b.1 - a.1) * (x - a.0) / (b.0 - a.0))
    })
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
    use crate::studio::features::Label;

    #[test]
    fn clipping() {
        let r = Region::Circle { cx: 0.0, cy: 0.0, r: 10.0 };
        let parts = clip_line(&[(-20.0, 0.0), (20.0, 0.0)], &r);
        assert_eq!(parts.len(), 1);
        assert!((parts[0][0].0 + 10.0).abs() < 0.01 && (parts[0][1].0 - 10.0).abs() < 0.01);
        let sq = clip_rect(&[(-5.0, -5.0), (5.0, -5.0), (5.0, 5.0), (-5.0, 5.0)], [0.0, 0.0, 10.0, 10.0]);
        assert_eq!(sq.len(), 4);
        assert_eq!(group(25000.0), "25\u{2009}000");
    }

    fn slope() -> Grid {
        let mut g = Grid::new([0.0, 0.0, 3000.0, 2000.0], 25.0);
        for j in 0..g.ny {
            for i in 0..g.nx {
                g.z[j * g.nx + i] = 1000.0 + (g.x(i) / 10.0) as f32 + (g.y(j) / 20.0) as f32;
            }
        }
        g
    }

    #[test]
    fn whole_map() {
        let g = slope();
        let f = Frame::new(20.0, 49.2);
        let r = Region::Rect { x0: 100.0, y0: 100.0, x1: 2900.0, y1: 1900.0 };
        let o = MapOpts { relief: Relief::Shade, dpi: 40, ..Default::default() };
        let d = map(&g, None, &f, &r, &Look::default(), &o, &["źródło".into()], None);
        assert!(d.layers.iter().any(|l| l.id == "warstwice_g" && !l.items.is_empty()));
        assert!(!super::super::draw::svg(&d).is_empty());
        let gj = contours_geojson(&g, &f, &r, 50.0);
        assert!(gj.contains("LineString"));
    }

    /// Żaden napis mapy nie wystaje poza koło obszaru – także przy dużych napisach i gęstych opisach.
    #[test]
    fn labels_stay_inside_area() {
        let g = slope();
        let f = Frame::new(20.0, 49.2);
        let r = Region::Circle { cx: 1500.0, cy: 1000.0, r: 800.0 };
        let mut feats = Features::default();
        for (q, (x, y)) in [(1500.0, 1000.0), (2280.0, 1000.0), (1500.0, 1790.0), (900.0, 600.0), (1200.0, 1100.0)].into_iter().enumerate() {
            feats.labels.push(Label { kind: PointKind::Peak, x, y, name: format!("Bardzo długa nazwa szczytu {q}"), ele: Some(1500.0) });
        }
        let o = MapOpts { relief: Relief::None, text_scale: 1.8, label_density: 4.0, step: 10.0, paper: Paper::A4, ..Default::default() };
        let d = map(&g, Some(&feats), &f, &r, &Look::default(), &o, &[], None);
        let clip = d.clip.clone().unwrap();
        let (cx, cy) = clip.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
        let (cx, cy) = (cx / clip.len() as f64, cy / clip.len() as f64);
        let rad = clip.iter().map(|p| ((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt()).fold(f64::INFINITY, f64::min);
        let mut n = 0;
        for l in d.layers.iter().filter(|l| matches!(l.id, "nazwy_obiektow" | "warstwice_opisy" | "siatka_opisy")) {
            for it in &l.items {
                if let Item::Text { x, y, text, size, anchor, angle, .. } = it {
                    n += 1;
                    let q = text_quad(*x, *y, text_width(text, *size) * 1.07, *size, *anchor, *angle);
                    for c in q {
                        assert!(((c.0 - cx).powi(2) + (c.1 - cy).powi(2)).sqrt() < rad + 0.05, "napis „{text}” wystaje poza obszar");
                    }
                }
            }
        }
        assert!(n > 0, "brak napisów do sprawdzenia");
    }
}
