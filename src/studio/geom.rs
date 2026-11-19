//! Geometria Studio: lokalny układ w metrach wokół obszaru i kształty wycinka z odległością
//! ze znakiem (ujemna wewnątrz) – z niej powstają gładkie krawędzie modelu, maski map i przekroje.

use serde::{Deserialize, Serialize};

use crate::geo::{lat_to_y, lon_to_x, x_to_lon, y_to_lat};

/// Obwód równika w metrach (Web Mercator).
const EARTH: f64 = 40_075_016.686;

/// Lokalny układ: x na wschód, y na północ, w metrach (Mercator przeskalowany do środka obszaru –
/// na kilkudziesięciu kilometrach błąd skali poniżej 0,5%).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub lon0: f64,
    pub lat0: f64,
    k: f64,
}

impl Frame {
    pub fn new(lon0: f64, lat0: f64) -> Self {
        Self { lon0, lat0, k: EARTH * lat0.to_radians().cos() }
    }

    pub fn to_m(&self, lon: f64, lat: f64) -> (f64, f64) {
        ((lon_to_x(lon) - lon_to_x(self.lon0)) * self.k, -(lat_to_y(lat) - lat_to_y(self.lat0)) * self.k)
    }

    pub fn to_geo(&self, x: f64, y: f64) -> (f64, f64) {
        (x_to_lon(lon_to_x(self.lon0) + x / self.k), y_to_lat(lat_to_y(self.lat0) - y / self.k))
    }

    /// Metry Web Mercator (EPSG:3857) dla punktu lokalnego.
    pub fn to_3857(&self, x: f64, y: f64) -> (f64, f64) {
        let mx = (lon_to_x(self.lon0) + x / self.k - 0.5) * EARTH;
        let my = (0.5 - (lat_to_y(self.lat0) - y / self.k)) * EARTH;
        (mx, my)
    }

    /// Metry EPSG:3857 na jeden metr w terenie.
    pub fn merc_scale(&self) -> f64 {
        EARTH / self.k
    }
}

/// Kształt wycinka wybrany w Studio.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Shape {
    #[default]
    Rect,
    Circle,
    Polygon,
    /// Pas wzdłuż linii (grań, dolina, szlak) o zadanej szerokości.
    Ridge,
}

impl Shape {
    pub const ALL: [Shape; 4] = [Shape::Rect, Shape::Circle, Shape::Polygon, Shape::Ridge];

    pub fn label(self) -> &'static str {
        match self {
            Shape::Rect => crate::i18n::tr("Prostokąt"),
            Shape::Circle => crate::i18n::tr("Koło"),
            Shape::Polygon => crate::i18n::tr("Wielokąt"),
            Shape::Ridge => crate::i18n::tr("Pas wzdłuż grani"),
        }
    }

    pub fn icon(self) -> &'static str {
        use crate::ui::theme::icon;
        match self {
            Shape::Rect => icon::SQUARE,
            Shape::Circle => icon::CIRCLE,
            Shape::Polygon => icon::POLYGON,
            Shape::Ridge => icon::MOUNTAINS,
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Shape::Rect => crate::i18n::tr("Kliknij na mapie dwa przeciwległe narożniki."),
            Shape::Circle => crate::i18n::tr("Kliknij środek, potem punkt na brzegu koła."),
            Shape::Polygon => crate::i18n::tr("Klikaj kolejne wierzchołki obszaru (co najmniej trzy)."),
            Shape::Ridge => crate::i18n::tr("Klikaj kolejne punkty wzdłuż grani; szerokość pasa ustawisz niżej."),
        }
    }
}

/// Obszar w stopniach (to, co użytkownik wskazał na mapie).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Area {
    pub shape: Shape,
    pub pts: Vec<(f64, f64)>,
    /// Szerokość pasa (m) dla [`Shape::Ridge`].
    pub width: f64,
    /// Tylko teren powyżej tej wysokości (m n.p.m.) – np. sam masyw albo grań nad doliną.
    pub above: Option<f32>,
}

impl Default for Area {
    fn default() -> Self {
        Self { shape: Shape::Rect, pts: Vec::new(), width: 1200.0, above: None }
    }
}

impl Area {
    pub fn complete(&self) -> bool {
        match self.shape {
            Shape::Rect | Shape::Circle | Shape::Ridge => self.pts.len() >= 2,
            Shape::Polygon => self.pts.len() >= 3,
        }
    }

    /// Kliknięcie na mapie: prostokąt i koło mają dwa punkty (trzecie kliknięcie zaczyna od nowa).
    pub fn add(&mut self, lon: f64, lat: f64) {
        if matches!(self.shape, Shape::Rect | Shape::Circle) && self.pts.len() >= 2 {
            self.pts.clear();
        }
        self.pts.push((lon, lat));
    }

    pub fn center(&self) -> Option<(f64, f64)> {
        if self.pts.is_empty() {
            return None;
        }
        if self.shape == Shape::Circle {
            return Some(self.pts[0]);
        }
        let n = self.pts.len() as f64;
        Some((self.pts.iter().map(|p| p.0).sum::<f64>() / n, self.pts.iter().map(|p| p.1).sum::<f64>() / n))
    }

    /// Kształt w metrach w układzie `f`.
    pub fn local(&self, f: &Frame) -> Option<Region> {
        if !self.complete() {
            return None;
        }
        let pts: Vec<(f64, f64)> = self.pts.iter().map(|&(lon, lat)| f.to_m(lon, lat)).collect();
        Some(match self.shape {
            Shape::Rect => {
                let (a, b) = (pts[0], pts[1]);
                Region::Rect { x0: a.0.min(b.0), y0: a.1.min(b.1), x1: a.0.max(b.0), y1: a.1.max(b.1) }
            }
            Shape::Circle => {
                let (c, e) = (pts[0], pts[1]);
                Region::Circle { cx: c.0, cy: c.1, r: ((e.0 - c.0).powi(2) + (e.1 - c.1).powi(2)).sqrt().max(1.0) }
            }
            Shape::Polygon => Region::Polygon(pts),
            Shape::Ridge => Region::Ridge { line: pts, half: (self.width / 2.0).max(5.0) },
        })
    }

    /// Obrys do narysowania na mapie (stopnie, zamknięty).
    pub fn outline(&self) -> Vec<(f64, f64)> {
        let Some(c) = self.center() else { return Vec::new() };
        let f = Frame::new(c.0, c.1);
        match self.local(&f) {
            Some(r) => r.outline(96).into_iter().map(|(x, y)| f.to_geo(x, y)).collect(),
            None => Vec::new(),
        }
    }
}

/// Kształt w metrach.
#[derive(Clone, Debug, PartialEq)]
pub enum Region {
    Rect { x0: f64, y0: f64, x1: f64, y1: f64 },
    Circle { cx: f64, cy: f64, r: f64 },
    Polygon(Vec<(f64, f64)>),
    Ridge { line: Vec<(f64, f64)>, half: f64 },
}

fn seg_dist(px: f64, py: f64, a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 { (((px - a.0) * dx + (py - a.1) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    let (qx, qy) = (a.0 + t * dx, a.1 + t * dy);
    ((px - qx).powi(2) + (py - qy).powi(2)).sqrt()
}

/// Punkt wewnątrz wielokąta (reguła parzystości).
pub fn inside_polygon(px: f64, py: f64, poly: &[(f64, f64)]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.1 > py) != (b.1 > py) && px < (b.0 - a.0) * (py - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
        j = i;
    }
    inside
}

impl Region {
    /// Odległość ze znakiem od brzegu (m): ujemna wewnątrz.
    pub fn sd(&self, x: f64, y: f64) -> f64 {
        match self {
            Region::Rect { x0, y0, x1, y1 } => {
                let (cx, cy, hw, hh) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0, (x1 - x0) / 2.0, (y1 - y0) / 2.0);
                let (dx, dy) = ((x - cx).abs() - hw, (y - cy).abs() - hh);
                let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt();
                outside + dx.max(dy).min(0.0)
            }
            Region::Circle { cx, cy, r } => ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - r,
            Region::Polygon(p) => {
                let mut d = f64::INFINITY;
                for i in 0..p.len() {
                    d = d.min(seg_dist(x, y, p[i], p[(i + 1) % p.len()]));
                }
                if inside_polygon(x, y, p) {
                    -d
                } else {
                    d
                }
            }
            Region::Ridge { line, half } => {
                let mut d = f64::INFINITY;
                for w in line.windows(2) {
                    d = d.min(seg_dist(x, y, w[0], w[1]));
                }
                if line.len() == 1 {
                    d = ((x - line[0].0).powi(2) + (y - line[0].1).powi(2)).sqrt();
                }
                d - half
            }
        }
    }

    /// Prostokąt otaczający (x0, y0, x1, y1).
    pub fn bounds(&self) -> [f64; 4] {
        match self {
            Region::Rect { x0, y0, x1, y1 } => [*x0, *y0, *x1, *y1],
            Region::Circle { cx, cy, r } => [cx - r, cy - r, cx + r, cy + r],
            Region::Polygon(p) => bbox(p, 0.0),
            Region::Ridge { line, half } => bbox(line, *half),
        }
    }

    /// Obrys w metrach (zamknięty, przeciwnie do ruchu wskazówek zegara).
    pub fn outline(&self, n: usize) -> Vec<(f64, f64)> {
        match self {
            Region::Rect { x0, y0, x1, y1 } => vec![(*x0, *y0), (*x1, *y0), (*x1, *y1), (*x0, *y1)],
            Region::Circle { cx, cy, r } => (0..n).map(|i| {
                let a = i as f64 / n as f64 * std::f64::consts::TAU;
                (cx + r * a.cos(), cy + r * a.sin())
            }).collect(),
            Region::Polygon(p) => p.clone(),
            Region::Ridge { line, half } => ridge_outline(line, *half),
        }
    }

    /// Pole powierzchni (m²) – liczone siatką, wystarczy do opisu.
    pub fn area_m2(&self) -> f64 {
        match self {
            Region::Rect { x0, y0, x1, y1 } => (x1 - x0) * (y1 - y0),
            Region::Circle { r, .. } => std::f64::consts::PI * r * r,
            _ => {
                let o = self.outline(64);
                let mut s = 0.0;
                for i in 0..o.len() {
                    let (a, b) = (o[i], o[(i + 1) % o.len()]);
                    s += a.0 * b.1 - b.0 * a.1;
                }
                (s / 2.0).abs()
            }
        }
    }
}

fn bbox(p: &[(f64, f64)], pad: f64) -> [f64; 4] {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for &(x, y) in p {
        b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
    }
    [b[0] - pad, b[1] - pad, b[2] + pad, b[3] + pad]
}

/// Obrys pasa wokół linii: lewa strona w przód, półkole, prawa strona wstecz, półkole.
fn ridge_outline(line: &[(f64, f64)], half: f64) -> Vec<(f64, f64)> {
    if line.len() < 2 {
        return Vec::new();
    }
    let normal = |a: (f64, f64), b: (f64, f64)| {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let l = (dx * dx + dy * dy).sqrt().max(1e-9);
        (-dy / l, dx / l)
    };
    let n = line.len();
    let side = |sign: f64| -> Vec<(f64, f64)> {
        (0..n)
            .map(|i| {
                let n1 = if i > 0 { normal(line[i - 1], line[i]) } else { normal(line[0], line[1]) };
                let n2 = if i + 1 < n { normal(line[i], line[i + 1]) } else { n1 };
                let (mx, my) = (n1.0 + n2.0, n1.1 + n2.1);
                let l = (mx * mx + my * my).sqrt().max(1e-9);
                let cos = ((mx / l) * n1.0 + (my / l) * n1.1).max(0.35);
                (line[i].0 + sign * mx / l * half / cos, line[i].1 + sign * my / l * half / cos)
            })
            .collect()
    };
    let cap = |c: (f64, f64), from: (f64, f64)| -> Vec<(f64, f64)> {
        let a0 = from.1.atan2(from.0);
        (1..12).map(|k| {
            let a = a0 - std::f64::consts::PI * k as f64 / 12.0;
            (c.0 + half * a.cos(), c.1 + half * a.sin())
        }).collect()
    };
    let mut out = side(1.0);
    let last = normal(line[n - 2], line[n - 1]);
    out.extend(cap(line[n - 1], last));
    let mut right = side(-1.0);
    right.reverse();
    out.extend(right);
    let first = normal(line[0], line[1]);
    out.extend(cap(line[0], (-first.0, -first.1)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_roundtrip() {
        let f = Frame::new(19.95, 49.23);
        let (x, y) = f.to_m(20.0, 49.25);
        assert!((x - 3640.0).abs() < 40.0, "{x}");
        assert!((y - 2224.0).abs() < 30.0, "{y}");
        let (lon, lat) = f.to_geo(x, y);
        assert!((lon - 20.0).abs() < 1e-9 && (lat - 49.25).abs() < 1e-9);
    }

    #[test]
    fn signed_distance() {
        let r = Region::Rect { x0: 0.0, y0: 0.0, x1: 10.0, y1: 4.0 };
        assert!((r.sd(5.0, 2.0) + 2.0).abs() < 1e-9);
        assert!((r.sd(13.0, 2.0) - 3.0).abs() < 1e-9);
        let c = Region::Circle { cx: 0.0, cy: 0.0, r: 5.0 };
        assert!((c.sd(3.0, 4.0)).abs() < 1e-9);
        let p = Region::Polygon(vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
        assert!(p.sd(5.0, 5.0) < 0.0 && p.sd(15.0, 5.0) > 0.0);
        let g = Region::Ridge { line: vec![(0.0, 0.0), (100.0, 0.0)], half: 10.0 };
        assert!(g.sd(50.0, 5.0) < 0.0 && g.sd(50.0, 15.0) > 0.0);
        assert_eq!(g.bounds(), [-10.0, -10.0, 110.0, 10.0]);
        assert!(g.outline(0).len() > 8);
    }
}
