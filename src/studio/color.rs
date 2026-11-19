//! Kolorowanie wycinka: tekstura (RGBA) z wysokości, nachylenia, ekspozycji albo pokrycia terenu,
//! z cieniowaniem, warstwicami i obiektami OSM. Ta sama tekstura idzie na model 3D, do podglądu
//! i jako podkład map.

use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

use super::features::{AreaKind, Features, LineKind};
use super::terrain::Grid;

/// Co wyznacza kolor terenu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Mode {
    #[default]
    Elevation,
    Slope,
    Aspect,
    /// Pokrycie terenu z OSM (lasy, skały, wody, łąki).
    Cover,
    /// Jednolity kolor – samo cieniowanie (jak gips).
    Plain,
}

impl Mode {
    pub const ALL: [Mode; 5] = [Mode::Elevation, Mode::Slope, Mode::Aspect, Mode::Cover, Mode::Plain];
    pub fn label(self) -> &'static str {
        match self {
            Mode::Elevation => crate::i18n::tr("Wysokość"),
            Mode::Slope => crate::i18n::tr("Nachylenie"),
            Mode::Aspect => crate::i18n::tr("Ekspozycja"),
            Mode::Cover => crate::i18n::tr("Pokrycie terenu"),
            Mode::Plain => crate::i18n::tr("Jednolity"),
        }
    }
}

/// Palety hipsometryczne (kolor od wysokości).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Palette {
    #[default]
    Atlas,
    Alpine,
    Topo,
    Desert,
    Glacier,
    Gray,
    Night,
}

impl Palette {
    pub const ALL: [Palette; 7] = [Palette::Atlas, Palette::Alpine, Palette::Topo, Palette::Desert, Palette::Glacier, Palette::Gray, Palette::Night];
    pub fn label(self) -> &'static str {
        match self {
            Palette::Atlas => crate::i18n::tr("Atlas"),
            Palette::Alpine => crate::i18n::tr("Alpejska"),
            Palette::Topo => crate::i18n::tr("Topograficzna"),
            Palette::Desert => crate::i18n::tr("Pustynna"),
            Palette::Glacier => crate::i18n::tr("Lodowa"),
            Palette::Gray => crate::i18n::tr("Szarości"),
            Palette::Night => crate::i18n::tr("Nocna"),
        }
    }

    /// Przystanki (udział 0..1, kolor).
    fn stops(self) -> &'static [(f32, [u8; 3])] {
        match self {
            Palette::Atlas => &[(0.0, [62, 122, 76]), (0.2, [128, 166, 92]), (0.42, [214, 204, 136]), (0.62, [190, 142, 92]), (0.82, [150, 120, 104]), (1.0, [248, 248, 250])],
            Palette::Alpine => &[(0.0, [86, 128, 72]), (0.35, [112, 146, 84]), (0.5, [168, 160, 120]), (0.7, [150, 146, 140]), (0.88, [205, 205, 210]), (1.0, [255, 255, 255])],
            Palette::Topo => &[(0.0, [176, 214, 162]), (0.25, [226, 236, 186]), (0.5, [244, 226, 176]), (0.75, [228, 196, 156]), (1.0, [240, 232, 226])],
            Palette::Desert => &[(0.0, [214, 182, 128]), (0.4, [196, 140, 86]), (0.75, [150, 92, 60]), (1.0, [96, 64, 52])],
            Palette::Glacier => &[(0.0, [70, 92, 120]), (0.4, [128, 160, 190]), (0.75, [196, 220, 236]), (1.0, [255, 255, 255])],
            Palette::Gray => &[(0.0, [96, 96, 96]), (1.0, [250, 250, 250])],
            Palette::Night => &[(0.0, [16, 22, 40]), (0.5, [52, 70, 110]), (0.8, [122, 150, 200]), (1.0, [220, 232, 255])],
        }
    }

    pub fn at(self, t: f32) -> [f32; 3] {
        ramp(self.stops(), t)
    }
}

fn ramp(stops: &[(f32, [u8; 3])], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    for w in stops.windows(2) {
        let ((a, ca), (b, cb)) = (w[0], w[1]);
        if t <= b {
            let k = if b > a { (t - a) / (b - a) } else { 0.0 };
            return [0, 1, 2].map(|i| ca[i] as f32 + (cb[i] as f32 - ca[i] as f32) * k);
        }
    }
    let c = stops.last().map(|s| s.1).unwrap_or([255, 255, 255]);
    [c[0] as f32, c[1] as f32, c[2] as f32]
}

/// Wygląd tekstury.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Look {
    pub mode: Mode,
    pub palette: Palette,
    /// Paleta rozciągnięta na zakres wysokości obszaru (inaczej 0–3000 m).
    pub relative: bool,
    /// Siła cieniowania 0..1.
    pub shade: f32,
    /// Słońce: azymut (° od północy) i wysokość nad horyzontem.
    pub sun_az: f32,
    pub sun_alt: f32,
    /// Cieniowanie z kilku kierunków (łagodniejsze, czytelniejsze).
    pub multi: bool,
    pub contours: bool,
    pub contour_step: f32,
    pub forest: bool,
    pub water: bool,
    pub rock: bool,
    pub trails: bool,
    pub paths: bool,
    pub roads: bool,
    pub buildings: bool,
    /// Kolor jednolity (tryb [`Mode::Plain`]) i podstawy modelu.
    pub plain: [u8; 3],
    pub base: [u8; 3],
}

impl Default for Look {
    fn default() -> Self {
        Self {
            mode: Mode::Elevation,
            palette: Palette::Atlas,
            relative: true,
            shade: 0.75,
            sun_az: 315.0,
            sun_alt: 40.0,
            multi: true,
            contours: false,
            contour_step: 50.0,
            forest: true,
            water: true,
            rock: true,
            trails: true,
            paths: false,
            roads: true,
            buildings: true,
            plain: [236, 232, 222],
            base: [58, 52, 46],
        }
    }
}

/// Odwzorowanie metrów na piksele tekstury.
#[derive(Clone, Copy, Debug)]
pub struct TexMap {
    pub x0: f64,
    pub y1: f64,
    pub sx: f64,
    pub sy: f64,
}

impl TexMap {
    pub fn for_grid(g: &Grid, w: u32, h: u32) -> Self {
        Self { x0: g.x0, y1: g.y1, sx: w as f64 / (g.x1() - g.x0), sy: h as f64 / (g.y1 - g.y0()) }
    }
    pub fn px(&self, x: f64, y: f64) -> (f64, f64) {
        ((x - self.x0) * self.sx, (self.y1 - y) * self.sy)
    }
    pub fn m(&self, px: f64, py: f64) -> (f64, f64) {
        (self.x0 + px / self.sx, self.y1 - py / self.sy)
    }
}

/// Rozmiar tekstury dla siatki: dłuższy bok `max_px`.
pub fn tex_size(g: &Grid, max_px: u32) -> (u32, u32) {
    let (w, h) = (g.x1() - g.x0, g.y1 - g.y0());
    if w >= h {
        (max_px, ((max_px as f64 * h / w).round() as u32).max(2))
    } else {
        (((max_px as f64 * w / h).round() as u32).max(2), max_px)
    }
}

/// Cieniowanie 0..1 (1 – w pełnym słońcu) dla spadku (dz/dx, dz/dy).
fn hillshade(gx: f64, gy: f64, az: f32, alt: f32) -> f32 {
    let (az, alt) = ((az as f64).to_radians(), (alt as f64).to_radians());
    // kierunek do słońca: x – wschód, y – północ
    let l = (az.sin() * alt.cos(), az.cos() * alt.cos(), alt.sin());
    let n = (-gx, -gy, 1.0);
    let len = (n.0 * n.0 + n.1 * n.1 + n.2 * n.2).sqrt();
    (((n.0 * l.0 + n.1 * l.1 + n.2 * l.2) / len).max(0.0)) as f32
}

/// Kolor nachylenia (stopnie) – klasy jak na mapach lawinowych.
fn slope_color(deg: f32) -> [f32; 3] {
    ramp(&[(0.0, [236, 240, 230]), (0.2, [200, 226, 170]), (0.33, [250, 230, 120]), (0.43, [246, 160, 70]), (0.55, [222, 60, 50]), (0.7, [150, 40, 120]), (1.0, [60, 30, 60])], deg / 60.0)
}

fn aspect_color(gx: f64, gy: f64) -> [f32; 3] {
    if gx.abs() + gy.abs() < 0.02 {
        return [220.0, 220.0, 220.0];
    }
    // azymut spadku (kierunek, w który stok jest zwrócony)
    let a = ((-gx).atan2(-gy).to_degrees() + 360.0) % 360.0;
    let h = a as f32 / 360.0;
    hsv(h, 0.45, 0.95)
}

fn hsv(h: f32, s: f32, v: f32) -> [f32; 3] {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
    let (r, g, b) = match i as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [r * 255.0, g * 255.0, b * 255.0]
}

/// Tekstura terenu `w`×`h` dla siatki, z obiektami OSM (gdy są).
pub fn texture(g: &Grid, feats: Option<&Features>, look: &Look, w: u32, h: u32) -> RgbaImage {
    let tm = TexMap::for_grid(g, w, h);
    let (lo, hi) = if look.relative { g.range() } else { (0.0, 3000.0) };
    let span = (hi - lo).max(1.0);
    // krok pochodnej: piksel tekstury, ale nie mniej niż oczko siatki
    let hstep = (1.0 / tm.sx).max(g.cell * 0.75);
    // pokrycie terenu z OSM jako maska (0 – nic, 1 – las, 2 – piarg, 3 – skała, 4 – lodowiec, 5 – woda)
    let cover = feats.map(|f| cover_mask(f, &tm, w, h, look));
    let mut img = RgbaImage::new(w, h);
    let rows: Vec<Vec<u8>> = {
        use rayon::prelude::*;
        (0..h)
            .into_par_iter()
            .map(|py| {
                let mut row = vec![0u8; w as usize * 4];
                for px in 0..w {
                    let (x, y) = tm.m(px as f64 + 0.5, py as f64 + 0.5);
                    let z = g.sample(x, y);
                    let (gx, gy) = g.gradient(x, y, hstep);
                    let slope = (gx * gx + gy * gy).sqrt().atan().to_degrees() as f32;
                    let c_cover = cover.as_ref().map(|m| m[(py * w + px) as usize]).unwrap_or(0);
                    let mut c = match look.mode {
                        Mode::Elevation => look.palette.at((z - lo) / span),
                        Mode::Slope => slope_color(slope),
                        Mode::Aspect => aspect_color(gx, gy),
                        Mode::Plain => look.plain.map(|v| v as f32),
                        Mode::Cover => {
                            if slope > 38.0 && c_cover == 0 {
                                [190.0, 186.0, 180.0]
                            } else {
                                [214.0, 226.0, 180.0]
                            }
                        }
                    };
                    // pokrycie terenu z OSM nałożone na kolor
                    let over: Option<([f32; 3], f32)> = match c_cover {
                        1 if look.forest => Some(([96.0, 150.0, 92.0], if look.mode == Mode::Cover { 1.0 } else { 0.45 })),
                        2 if look.rock => Some(([196.0, 188.0, 176.0], if look.mode == Mode::Cover { 1.0 } else { 0.35 })),
                        3 if look.rock => Some(([170.0, 164.0, 160.0], if look.mode == Mode::Cover { 1.0 } else { 0.45 })),
                        4 => Some(([232.0, 244.0, 252.0], 0.85)),
                        5 if look.water => Some(([120.0, 176.0, 222.0], 1.0)),
                        6 if look.buildings => Some(([120.0, 104.0, 100.0], 0.95)),
                        _ => None,
                    };
                    if let Some((oc, k)) = over {
                        c = [0, 1, 2].map(|i| c[i] * (1.0 - k) + oc[i] * k);
                    }
                    // cieniowanie (woda płaska – bez cienia)
                    if look.shade > 0.0 && !(c_cover == 5 && look.water) {
                        let s = if look.multi {
                            let a = hillshade(gx, gy, look.sun_az, look.sun_alt);
                            let b = hillshade(gx, gy, look.sun_az - 60.0, look.sun_alt + 10.0);
                            let d = hillshade(gx, gy, look.sun_az + 60.0, look.sun_alt + 10.0);
                            0.55 * a + 0.25 * b + 0.2 * d
                        } else {
                            hillshade(gx, gy, look.sun_az, look.sun_alt)
                        };
                        let flat = hillshade(0.0, 0.0, look.sun_az, look.sun_alt).max(0.2);
                        let k = (0.35 + 0.65 * s / flat).clamp(0.0, 1.35);
                        let m = 1.0 + (k - 1.0) * look.shade;
                        c = c.map(|v| v * m);
                    }
                    // warstwice
                    if look.contours && look.contour_step > 0.0 {
                        let step = look.contour_step;
                        let lvl = |z: f32| (z / step).floor();
                        let (zx, _) = tm.m(px as f64 + 1.5, py as f64 + 0.5);
                        let (_, zy) = tm.m(px as f64 + 0.5, py as f64 + 1.5);
                        let (z1, z2) = (g.sample(zx, y), g.sample(x, zy));
                        let (l0, l1, l2) = (lvl(z), lvl(z1), lvl(z2));
                        if l0 != l1 || l0 != l2 {
                            let top = l0.max(l1).max(l2);
                            let index = (top as i64).rem_euclid(5) == 0;
                            let k = if index { 0.5 } else { 0.72 };
                            c = c.map(|v| v * k + 40.0 * (1.0 - k));
                        }
                    }
                    let i = px as usize * 4;
                    row[i] = c[0].clamp(0.0, 255.0) as u8;
                    row[i + 1] = c[1].clamp(0.0, 255.0) as u8;
                    row[i + 2] = c[2].clamp(0.0, 255.0) as u8;
                    row[i + 3] = 255;
                }
                row
            })
            .collect()
    };
    for (py, row) in rows.into_iter().enumerate() {
        for px in 0..w as usize {
            img.put_pixel(px as u32, py as u32, Rgba([row[px * 4], row[px * 4 + 1], row[px * 4 + 2], 255]));
        }
    }
    if let Some(f) = feats {
        draw_lines(&mut img, f, &tm, look);
    }
    img
}

/// Wypełnienie pierścieni (reguła parzystości) w masce wartością `v`.
pub fn fill_rings(mask: &mut [u8], w: u32, h: u32, rings: &[Vec<(f64, f64)>], tm: &TexMap, v: u8) {
    let pr: Vec<Vec<(f64, f64)>> = rings.iter().map(|r| r.iter().map(|&(x, y)| tm.px(x, y)).collect()).collect();
    let (mut y0, mut y1) = (f64::INFINITY, f64::NEG_INFINITY);
    for r in &pr {
        for p in r {
            y0 = y0.min(p.1);
            y1 = y1.max(p.1);
        }
    }
    let (ya, yb) = ((y0.floor().max(0.0)) as i64, (y1.ceil().min(h as f64 - 1.0)) as i64);
    let mut xs: Vec<f64> = Vec::new();
    for py in ya..=yb {
        let sy = py as f64 + 0.5;
        xs.clear();
        for r in &pr {
            let n = r.len();
            for i in 0..n {
                let (a, b) = (r[i], r[(i + 1) % n]);
                if (a.1 > sy) != (b.1 > sy) {
                    xs.push(a.0 + (sy - a.1) * (b.0 - a.0) / (b.1 - a.1));
                }
            }
        }
        xs.sort_by(|a, b| a.total_cmp(b));
        for pair in xs.chunks(2) {
            if pair.len() < 2 {
                break;
            }
            let (xa, xb) = ((pair[0] - 0.5).ceil().max(0.0) as i64, ((pair[1] - 0.5).floor()).min(w as f64 - 1.0) as i64);
            for px in xa..=xb {
                mask[(py as u32 * w + px as u32) as usize] = v;
            }
        }
    }
}

fn cover_mask(f: &Features, tm: &TexMap, w: u32, h: u32, look: &Look) -> Vec<u8> {
    let mut m = vec![0u8; (w * h) as usize];
    for p in &f.polys {
        let v = match p.kind {
            AreaKind::Forest => 1,
            AreaKind::Scree => 2,
            AreaKind::Rock => 3,
            AreaKind::Glacier => 4,
            AreaKind::Water => 5,
            AreaKind::Building => {
                if !look.buildings {
                    continue;
                }
                6
            }
        };
        fill_rings(&mut m, w, h, &p.rings, tm, v);
    }
    m
}

/// Odcinek z wygładzonymi brzegami, szerokość w pikselach.
pub fn stroke(img: &mut RgbaImage, a: (f64, f64), b: (f64, f64), width: f64, c: [u8; 3], alpha: f32) {
    let r = width / 2.0;
    let (x0, x1) = ((a.0.min(b.0) - r - 1.0).floor().max(0.0) as i64, (a.0.max(b.0) + r + 1.0).ceil().min(img.width() as f64 - 1.0) as i64);
    let (y0, y1) = ((a.1.min(b.1) - r - 1.0).floor().max(0.0) as i64, (a.1.max(b.1) + r + 1.0).ceil().min(img.height() as f64 - 1.0) as i64);
    if x0 > x1 || y0 > y1 {
        return;
    }
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = (dx * dx + dy * dy).max(1e-9);
    for py in y0..=y1 {
        for px in x0..=x1 {
            let (qx, qy) = (px as f64 + 0.5, py as f64 + 0.5);
            let t = (((qx - a.0) * dx + (qy - a.1) * dy) / l2).clamp(0.0, 1.0);
            let d = ((qx - a.0 - t * dx).powi(2) + (qy - a.1 - t * dy).powi(2)).sqrt();
            let k = ((r + 0.5 - d).clamp(0.0, 1.0) as f32) * alpha;
            if k <= 0.0 {
                continue;
            }
            let p = img.get_pixel_mut(px as u32, py as u32);
            for i in 0..3 {
                p.0[i] = (p.0[i] as f32 * (1.0 - k) + c[i] as f32 * k) as u8;
            }
        }
    }
}

fn draw_lines(img: &mut RgbaImage, f: &Features, tm: &TexMap, look: &Look) {
    // szerokości w pikselach rosną z rozdzielczością tekstury (ok. metrów na piksel)
    let mpp = 1.0 / tm.sx;
    let wpx = |m: f64, min: f64| (m / mpp).max(min);
    for l in &f.lines {
        let (c, w, alpha) = match l.kind {
            LineKind::River if look.water => ([90, 150, 210], wpx(8.0, 1.6), 1.0),
            LineKind::Stream if look.water => ([110, 165, 220], wpx(3.0, 1.0), 0.9),
            LineKind::MajorRoad if look.roads => ([250, 236, 200], wpx(10.0, 2.0), 1.0),
            LineKind::Road if look.roads => ([252, 250, 244], wpx(6.0, 1.3), 0.95),
            LineKind::Track if look.paths => ([150, 110, 70], wpx(3.0, 1.0), 0.8),
            LineKind::Path if look.paths => ([120, 70, 50], wpx(2.0, 0.9), 0.8),
            LineKind::Rail if look.roads => ([60, 60, 60], wpx(4.0, 1.2), 1.0),
            LineKind::Aerialway if look.roads => ([40, 40, 40], wpx(2.0, 1.0), 1.0),
            LineKind::Trail(c) if look.trails => (c, wpx(5.0, 1.8), 1.0),
            _ => continue,
        };
        let pts: Vec<(f64, f64)> = l.pts.iter().map(|&(x, y)| tm.px(x, y)).collect();
        for s in pts.windows(2) {
            stroke(img, s[0], s[1], w, c, alpha);
        }
    }
}

/// Jasność wektora normalnego pod słońcem – do cieniowania podglądu 3D.
pub fn light(n: [f32; 3], look: &Look) -> f32 {
    let (az, alt) = (look.sun_az.to_radians(), look.sun_alt.to_radians());
    let l = [az.sin() * alt.cos(), az.cos() * alt.cos(), alt.sin()];
    (n[0] * l[0] + n[1] * l[1] + n[2] * l[2]).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramps_and_texture() {
        assert_eq!(Palette::Gray.at(0.0), [96.0, 96.0, 96.0]);
        assert_eq!(Palette::Gray.at(1.0), [250.0, 250.0, 250.0]);
        let mut g = Grid::new([0.0, 0.0, 100.0, 100.0], 10.0);
        for j in 0..g.ny {
            for i in 0..g.nx {
                g.z[j * g.nx + i] = (i * 10 + j) as f32;
            }
        }
        let look = Look { contours: true, contour_step: 10.0, ..Default::default() };
        let t = texture(&g, None, &look, 64, 64);
        assert_eq!(t.dimensions(), (64, 64));
        let mut m = vec![0u8; 64 * 64];
        let tm = TexMap::for_grid(&g, 64, 64);
        fill_rings(&mut m, 64, 64, &[vec![(10.0, 10.0), (90.0, 10.0), (90.0, 90.0), (10.0, 90.0)]], &tm, 7);
        assert!(m.iter().filter(|&&v| v == 7).count() > 2000);
    }
}
