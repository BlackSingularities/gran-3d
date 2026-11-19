//! Przekroje: płaszczyzna postawiona na linii A–B wskazanej na mapie.
//!
//!  - pionowa – klasyczny przekrój (profil) terenu z opisem szczytów, szlaków i wód na linii,
//!  - pozioma – warstwica na zadanej wysokości (kształt masywu na tym poziomie),
//!  - ukośna – płaszczyzna przez linię A–B na zadanej wysokości, nachylona o kąt upadu w prawo od
//!    kierunku A→B (np. warstwa skalna); wynik w prawdziwym kształcie na płaszczyźnie.
//!
//! Każdy przekrój to rysunek wektorowy ([`Drawing`]) – zapis do SVG, PDF i DXF – a pionowy także CSV.

use serde::{Deserialize, Serialize};

use super::contour::{self, Field};
use super::draw::{Anchor, Drawing, Item};
use super::features::{Features, LineKind, PointKind};
use super::terrain::Grid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Plane {
    #[default]
    Vertical,
    Horizontal,
    Oblique,
}

impl Plane {
    pub const ALL: [Plane; 3] = [Plane::Vertical, Plane::Horizontal, Plane::Oblique];
    pub fn label(self) -> &'static str {
        match self {
            Plane::Vertical => crate::i18n::tr("Pionowa (profil)"),
            Plane::Horizontal => crate::i18n::tr("Pozioma (warstwica)"),
            Plane::Oblique => crate::i18n::tr("Ukośna (upad)"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SectionOpts {
    pub plane: Plane,
    /// Linia A–B (stopnie).
    pub line: Vec<(f64, f64)>,
    /// Wysokość płaszczyzny poziomej i ukośnej na linii A–B (m n.p.m.).
    pub level: f32,
    /// Kąt upadu płaszczyzny ukośnej (°).
    pub dip: f32,
    /// Przewyższenie rysunku profilu.
    pub exag: f32,
    /// Skala rysunku (1 : `scale`).
    pub scale: f32,
    pub labels: bool,
}

impl Default for SectionOpts {
    fn default() -> Self {
        Self { plane: Plane::Vertical, line: Vec::new(), level: 1500.0, dip: 30.0, exag: 1.0, scale: 25_000.0, labels: true }
    }
}

/// Profil wzdłuż odcinka (metry lokalne): (odległość od A, wysokość, x, y).
pub fn profile(g: &Grid, a: (f64, f64), b: (f64, f64)) -> Vec<(f64, f32, f64, f64)> {
    let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
    let n = ((len / (g.cell * 0.5)).ceil() as usize).clamp(2, 20_000);
    (0..=n)
        .map(|k| {
            let t = k as f64 / n as f64;
            let (x, y) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            (len * t, g.sample(x, y), x, y)
        })
        .collect()
}

fn nice_step(range: f64, target: usize) -> f64 {
    let raw = range / target.max(1) as f64;
    let p = 10f64.powf(raw.log10().floor());
    for k in [1.0, 2.0, 2.5, 5.0, 10.0] {
        if k * p >= raw {
            return k * p;
        }
    }
    10.0 * p
}

/// Przecięcia odcinków A–B z łamaną (odległość od A).
fn crossings(a: (f64, f64), b: (f64, f64), pts: &[(f64, f64)]) -> Vec<f64> {
    let mut out = Vec::new();
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt();
    for w in pts.windows(2) {
        let (p, q) = (w[0], w[1]);
        let (ex, ey) = (q.0 - p.0, q.1 - p.1);
        let den = dx * ey - dy * ex;
        if den.abs() < 1e-12 {
            continue;
        }
        let t = ((p.0 - a.0) * ey - (p.1 - a.1) * ex) / den;
        let u = ((p.0 - a.0) * dy - (p.1 - a.1) * dx) / den;
        if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
            out.push(t * len);
        }
    }
    out
}

/// Kierunek A→B jako azymut (°, od północy).
pub fn azimuth(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((b.0 - a.0).atan2(b.1 - a.1).to_degrees() + 360.0) % 360.0
}

fn compass(az: f64) -> &'static str {
    const N: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    N[(((az + 22.5) % 360.0) / 45.0) as usize % 8]
}

/// Przekrój pionowy jako rysunek: siatka, oś odległości i wysokości, teren, opisy.
pub fn vertical(g: &Grid, feats: Option<&Features>, a: (f64, f64), b: (f64, f64), o: &SectionOpts, title: &str, credits: &[String]) -> Drawing {
    let prof = profile(g, a, b);
    let len = prof.last().map(|p| p.0).unwrap_or(1.0).max(1.0);
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for p in &prof {
        lo = lo.min(p.1);
        hi = hi.max(p.1);
    }
    let zstep = nice_step((hi - lo).max(10.0) as f64, 6);
    let zlo = ((lo as f64 - zstep * 0.5) / zstep).floor() * zstep;
    let zhi = ((hi as f64 + zstep * 0.3) / zstep).ceil() * zstep + zstep * 0.6;
    // skala pozioma z ustawień (mm na metr), pionowa razy przewyższenie
    let mut sx = 1000.0 / o.scale.max(100.0) as f64;
    // rysunek nie szerszy niż 1150 mm (A0) – wtedy skala się dopasowuje
    if len * sx > 1150.0 {
        sx = 1150.0 / len;
    }
    let sz = sx * o.exag.max(0.1) as f64;
    let (ml, mr, mt, mb) = (24.0, 12.0, 26.0, 26.0);
    let w = len * sx + ml + mr;
    let h = (zhi - zlo) * sz + mt + mb;
    let mut d = Drawing::new(w.max(120.0), h.max(70.0), title);
    let px = |dist: f64| ml + dist * sx;
    let pz = |z: f64| mt + (zhi - z) * sz;
    // siatka i osie
    let grid_c = [210, 214, 220];
    let ax = d.layer("siatka", "Siatka i osie");
    let mut z = zlo;
    while z <= zhi + 1e-6 {
        ax.push(Item::Path { pts: vec![(px(0.0), pz(z)), (px(len), pz(z))], closed: false, stroke: Some((grid_c, 0.15)), fill: None, dash: None });
        ax.push(Item::Text { x: px(0.0) - 2.0, y: pz(z) + 1.0, text: format!("{:.0}", z), size: 2.6, color: [80, 86, 96], anchor: Anchor::End, angle: 0.0, halo: false, bold: false });
        z += zstep;
    }
    let dstep = nice_step(len, 8);
    let mut dd = 0.0;
    while dd <= len + 1e-6 {
        ax.push(Item::Path { pts: vec![(px(dd), pz(zlo)), (px(dd), pz(zhi))], closed: false, stroke: Some((grid_c, 0.15)), fill: None, dash: None });
        let label = if dstep >= 1000.0 || len >= 3000.0 { format!("{:.1} km", dd / 1000.0).replace(".0 km", " km") } else { format!("{:.0} m", dd) };
        ax.push(Item::Text { x: px(dd), y: pz(zlo) + 5.0, text: label, size: 2.6, color: [80, 86, 96], anchor: Anchor::Middle, angle: 0.0, halo: false, bold: false });
        dd += dstep;
    }
    ax.push(Item::Path { pts: vec![(px(0.0), pz(zhi)), (px(0.0), pz(zlo)), (px(len), pz(zlo))], closed: false, stroke: Some(([60, 64, 72], 0.3)), fill: None, dash: None });
    // teren: wypełnienie i linia
    let line: Vec<(f64, f64)> = prof.iter().map(|p| (px(p.0), pz(p.1 as f64))).collect();
    let mut fill = line.clone();
    fill.push((px(len), pz(zlo)));
    fill.push((px(0.0), pz(zlo)));
    let t = d.layer("teren", "Teren");
    t.push(Item::Path { pts: fill, closed: true, stroke: None, fill: Some([226, 214, 192]), dash: None });
    t.push(Item::Path { pts: line, closed: false, stroke: Some(([70, 52, 36], 0.45)), fill: None, dash: None });
    // obiekty na linii przekroju
    if let (Some(f), true) = (feats, o.labels) {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let l2 = (dx * dx + dy * dy).max(1e-9);
        let mut marks: Vec<(f64, String, [u8; 3], bool)> = Vec::new();
        for lb in &f.labels {
            if !matches!(lb.kind, PointKind::Peak | PointKind::Saddle | PointKind::Hut) {
                continue;
            }
            let t = ((lb.x - a.0) * dx + (lb.y - a.1) * dy) / l2;
            if !(0.0..=1.0).contains(&t) {
                continue;
            }
            let (qx, qy) = (a.0 + t * dx, a.1 + t * dy);
            let off = ((lb.x - qx).powi(2) + (lb.y - qy).powi(2)).sqrt();
            if off > (g.cell * 4.0).max(120.0) || lb.name.is_empty() {
                continue;
            }
            let ele = lb.ele.map(|e| format!(" {:.0} m", e)).unwrap_or_default();
            marks.push((t * l2.sqrt(), format!("{}{}", lb.name, ele), [30, 30, 30], true));
        }
        for l in &f.lines {
            let (c, name) = match l.kind {
                LineKind::Trail(c) => (c, crate::i18n::tr("szlak").to_string()),
                LineKind::River => ([60, 120, 200], l.name.clone()),
                LineKind::MajorRoad | LineKind::Road => ([150, 120, 80], l.name.clone()),
                _ => continue,
            };
            for at in crossings(a, b, &l.pts) {
                marks.push((at, name.clone(), c, false));
            }
        }
        marks.sort_by(|x, y| x.0.total_cmp(&y.0));
        let lab = d.layer("opisy", "Opisy");
        let mut last_x = f64::NEG_INFINITY;
        for (at, name, c, peak) in marks {
            let z = prof[((at / len) * (prof.len() - 1) as f64).round() as usize].1 as f64;
            let (x, y) = (px(at), pz(z));
            if peak {
                lab.push(Item::Path { pts: vec![(x - 1.2, y - 0.6), (x + 1.2, y - 0.6), (x, y - 2.6)], closed: true, stroke: None, fill: Some(c), dash: None });
                if x - last_x > 4.0 {
                    lab.push(Item::Text { x, y: y - 4.0, text: name, size: 2.8, color: c, anchor: Anchor::Start, angle: -60.0, halo: true, bold: true });
                    last_x = x;
                }
            } else {
                lab.push(Item::Circle { x, y, r: 0.8, fill: Some(c), stroke: Some(([255, 255, 255], 0.2)) });
            }
        }
    }
    // nagłówek i opisy końców
    let az = azimuth(a, b);
    let head = d.layer("naglowek", "Nagłówek");
    head.push(Item::Text { x: ml, y: 9.0, text: title.to_string(), size: 4.6, color: [20, 24, 30], anchor: Anchor::Start, angle: 0.0, halo: false, bold: true });
    head.push(Item::Text {
        x: ml,
        y: 15.0,
        text: tf!("Długość {} · azymut {}° ({}) · skala 1:{} · przewyższenie ×{}", fmt_len(len), az.round(), compass(az), (1000.0 / sx).round(), o.exag),
        size: 2.8,
        color: [80, 86, 96],
        anchor: Anchor::Start,
        angle: 0.0,
        halo: false,
        bold: false,
    });
    head.push(Item::Text { x: px(0.0), y: pz(zhi) - 2.0, text: "A".into(), size: 4.0, color: [200, 40, 40], anchor: Anchor::Middle, angle: 0.0, halo: true, bold: true });
    head.push(Item::Text { x: px(len), y: pz(zhi) - 2.0, text: "B".into(), size: 4.0, color: [200, 40, 40], anchor: Anchor::Middle, angle: 0.0, halo: true, bold: true });
    credits_block(&mut d, credits);
    d
}

pub fn fmt_len(m: f64) -> String {
    if m >= 1000.0 {
        format!("{:.2} km", m / 1000.0)
    } else {
        format!("{:.0} m", m)
    }
}

/// Stopka ze źródłami danych (wymagane licencjami).
pub fn credits_block(d: &mut Drawing, credits: &[String]) {
    d.credits = credits.to_vec();
    d.background = true;
    let h = d.h;
    let w = d.w;
    let lay = d.layer("zrodla", "Źródła danych");
    for (k, c) in credits.iter().rev().enumerate() {
        lay.push(Item::Text { x: w - 4.0, y: h - 3.0 - k as f64 * 2.6, text: c.clone(), size: 1.8, color: [110, 116, 126], anchor: Anchor::End, angle: 0.0, halo: false, bold: false });
    }
}

/// CSV profilu: odległość, wysokość, długość i szerokość geograficzna.
pub fn profile_csv(g: &Grid, frame: &super::geom::Frame, a: (f64, f64), b: (f64, f64)) -> String {
    let mut s = String::from("odleglosc_m;wysokosc_m;lon;lat\n");
    for (d, z, x, y) in profile(g, a, b) {
        let (lon, lat) = frame.to_geo(x, y);
        s.push_str(&format!("{:.1};{:.1};{:.6};{:.6}\n", d, z, lon, lat));
    }
    s
}

/// Przekrój płaszczyzną poziomą albo ukośną: linie przecięcia w układzie płaszczyzny.
/// Zwraca łamane w metrach (u – wzdłuż A→B, v – w prawo od linii, po płaszczyźnie).
pub fn plane_lines(g: &Grid, a: (f64, f64), b: (f64, f64), o: &SectionOpts, mask: &dyn Fn(f64, f64) -> bool) -> Vec<Vec<(f64, f64)>> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt().max(1e-9);
    let (ux, uy) = (dx / len, dy / len);
    // „w prawo” od kierunku A→B
    let (rx, ry) = (uy, -ux);
    let tan = if o.plane == Plane::Oblique { (o.dip as f64).to_radians().tan() } else { 0.0 };
    let cos = if o.plane == Plane::Oblique { (o.dip as f64).to_radians().cos() } else { 1.0 };
    let v: Vec<f64> = (0..g.nx * g.ny)
        .map(|k| {
            let (x, y) = (g.x(k % g.nx), g.y(k / g.nx));
            if !mask(x, y) {
                return f64::NAN;
            }
            let right = (x - a.0) * rx + (y - a.1) * ry;
            g.z[k] as f64 - (o.level as f64 - tan * right)
        })
        .collect();
    let f = Field { nx: g.nx, ny: g.ny, v: &v };
    contour::lines(&f, 0.0)
        .into_iter()
        .map(|l| {
            l.into_iter()
                .map(|(i, j)| {
                    let (x, y) = (g.x0 + i * g.cell, g.y1 - j * g.cell);
                    let u = (x - a.0) * ux + (y - a.1) * uy;
                    let r = (x - a.0) * rx + (y - a.1) * ry;
                    if o.plane == Plane::Horizontal {
                        // kształt w planie (x na wschód, y na północ)
                        (x, y)
                    } else {
                        (u, r / cos)
                    }
                })
                .collect()
        })
        .filter(|l: &Vec<(f64, f64)>| l.len() >= 2)
        .collect()
}

/// Rysunek przekroju poziomego/ukośnego.
pub fn plane_drawing(lines: &[Vec<(f64, f64)>], o: &SectionOpts, title: &str, credits: &[String], outline: Option<&[(f64, f64)]>) -> Drawing {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for p in lines.iter().flatten().chain(outline.unwrap_or(&[]).iter()) {
        b = [b[0].min(p.0), b[1].min(p.1), b[2].max(p.0), b[3].max(p.1)];
    }
    if b[0] > b[2] {
        b = [0.0, 0.0, 1.0, 1.0];
    }
    let mut s = 1000.0 / o.scale.max(100.0) as f64;
    let span = (b[2] - b[0]).max(b[3] - b[1]).max(1.0);
    if span * s > 1150.0 {
        s = 1150.0 / span;
    }
    let (m, top) = (16.0, 24.0);
    let w = (b[2] - b[0]) * s + 2.0 * m;
    let h = (b[3] - b[1]) * s + m + top;
    let mut d = Drawing::new(w.max(120.0), h.max(80.0), title);
    // y rośnie w górę w metrach – na stronie w dół
    let tp = |p: (f64, f64)| (m + (p.0 - b[0]) * s, top + (b[3] - p.1) * s);
    if let Some(o) = outline {
        d.layer("obszar", "Granica obszaru").push(Item::Path { pts: o.iter().map(|&p| tp(p)).collect(), closed: true, stroke: Some(([150, 150, 160], 0.25)), fill: None, dash: Some(1.5) });
    }
    let lay = d.layer("przekroj", "Linia przekroju");
    for l in lines {
        let closed = l.len() > 2 && l[0] == *l.last().unwrap();
        let pts: Vec<(f64, f64)> = l.iter().map(|&p| tp(p)).collect();
        lay.push(Item::Path { pts, closed, stroke: Some(([170, 40, 40], 0.4)), fill: if closed { Some([246, 226, 220]) } else { None }, dash: None });
    }
    let head = d.layer("naglowek", "Nagłówek");
    head.push(Item::Text { x: m, y: 9.0, text: title.to_string(), size: 4.6, color: [20, 24, 30], anchor: Anchor::Start, angle: 0.0, halo: false, bold: true });
    let info = match o.plane {
        Plane::Horizontal => tf!("Płaszczyzna pozioma na wysokości {} m n.p.m. · skala 1:{}", o.level, (1000.0 / s).round()),
        _ => tf!("Płaszczyzna przez linię A–B na wysokości {} m, upad {}° w prawo · prawdziwy kształt · skala 1:{}", o.level, o.dip, (1000.0 / s).round()),
    };
    head.push(Item::Text { x: m, y: 15.0, text: info, size: 2.8, color: [80, 86, 96], anchor: Anchor::Start, angle: 0.0, halo: false, bold: false });
    // podziałka
    let bar = nice_step(span / 4.0, 1);
    let (bx, by) = (m, h - 8.0);
    let sc = d.layer("podzialka", "Podziałka");
    sc.push(Item::Path { pts: vec![(bx, by), (bx + bar * s, by)], closed: false, stroke: Some(([30, 30, 30], 0.6)), fill: None, dash: None });
    sc.push(Item::Text { x: bx + bar * s / 2.0, y: by - 1.5, text: fmt_len(bar), size: 2.5, color: [30, 30, 30], anchor: Anchor::Middle, angle: 0.0, halo: false, bold: false });
    credits_block(&mut d, credits);
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cone() -> Grid {
        let mut g = Grid::new([0.0, 0.0, 2000.0, 2000.0], 20.0);
        for j in 0..g.ny {
            for i in 0..g.nx {
                let r = ((g.x(i) - 1000.0).powi(2) + (g.y(j) - 1000.0).powi(2)).sqrt();
                g.z[j * g.nx + i] = (2000.0 - r) as f32;
            }
        }
        g
    }

    #[test]
    fn vertical_profile_drawing() {
        let g = cone();
        let o = SectionOpts::default();
        let d = vertical(&g, None, (0.0, 1000.0), (2000.0, 1000.0), &o, "Test", &["x".into()]);
        assert!(!d.is_empty());
        let p = profile(&g, (0.0, 1000.0), (2000.0, 1000.0));
        let top = p.iter().map(|q| q.1).fold(f32::MIN, f32::max);
        assert!((top - 2000.0).abs() < 25.0);
        assert!((azimuth((0.0, 0.0), (1.0, 0.0)) - 90.0).abs() < 1e-9);
    }

    #[test]
    fn horizontal_and_oblique_planes() {
        let g = cone();
        let mut o = SectionOpts { plane: Plane::Horizontal, level: 1500.0, ..Default::default() };
        let h = plane_lines(&g, (500.0, 1000.0), (1500.0, 1000.0), &o, &|_, _| true);
        assert_eq!(h.len(), 1);
        o.plane = Plane::Oblique;
        o.dip = 20.0;
        let ob = plane_lines(&g, (500.0, 1000.0), (1500.0, 1000.0), &o, &|_, _| true);
        assert!(!ob.is_empty());
        let d = plane_drawing(&ob, &o, "Ukośny", &[], None);
        assert!(!d.is_empty());
    }
}
