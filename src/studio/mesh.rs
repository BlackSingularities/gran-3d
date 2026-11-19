//! Model 3D wycinka: zamknięta bryła (do druku 3D) – powierzchnia terenu przycięta gładko do kształtu
//! obszaru, pionowe ściany i płaska podstawa – oraz zapis do STL, OBJ (+MTL i tekstury PNG), PLY
//! z kolorami wierzchołków, GLB (glTF z teksturą i materiałami PBR) i mapy wysokości PNG 16-bit.
//!
//! Materiały: powierzchnia terenu dzieli się na łąki, lasy, skały, piargi, śnieg i lodowce, wody
//! i budynki (z OpenStreetMap, nachylenia i wysokości), a ściany i spód mają własne materiały.
//! Każdy materiał ma kolor (albo teksturę terenu z odcieniem), szorstkość i metaliczność.

use std::collections::HashMap;

use image::RgbaImage;
use serde::{Deserialize, Serialize};

use super::terrain::Grid;

/// Materiał części bryły.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Mat {
    Ground = 0,
    Forest,
    Rock,
    Scree,
    Snow,
    Water,
    Building,
    Walls,
    Bottom,
}

impl Mat {
    pub const ALL: [Mat; 9] = [Mat::Ground, Mat::Forest, Mat::Rock, Mat::Scree, Mat::Snow, Mat::Water, Mat::Building, Mat::Walls, Mat::Bottom];
    pub fn label(self) -> &'static str {
        use crate::i18n::tr;
        match self {
            Mat::Ground => tr("Łąki i teren otwarty"),
            Mat::Forest => tr("Lasy"),
            Mat::Rock => tr("Skały"),
            Mat::Scree => tr("Piargi"),
            Mat::Snow => tr("Śnieg i lodowce"),
            Mat::Water => tr("Wody"),
            Mat::Building => tr("Budynki"),
            Mat::Walls => tr("Ściany boczne"),
            Mat::Bottom => tr("Spód"),
        }
    }
    /// Nazwa materiału w plikach (ASCII).
    pub fn id(self) -> &'static str {
        match self {
            Mat::Ground => "teren",
            Mat::Forest => "las",
            Mat::Rock => "skaly",
            Mat::Scree => "piargi",
            Mat::Snow => "snieg",
            Mat::Water => "woda",
            Mat::Building => "budynki",
            Mat::Walls => "sciany",
            Mat::Bottom => "spod",
        }
    }
    fn from_u8(v: u8) -> Mat {
        Mat::ALL.get(v as usize).copied().unwrap_or(Mat::Ground)
    }
}

/// Ustawienia materiału.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MatCfg {
    /// Kolor: odcień mnożony przez teksturę terenu albo kolor jednolity.
    pub color: [u8; 3],
    /// Tekstura terenu (kolory z zakładki Wygląd) zamiast koloru jednolitego.
    pub texture: bool,
    /// Szorstkość PBR 0..1 (0 – lustro, 1 – mat).
    pub roughness: f32,
    /// Metaliczność PBR 0..1.
    pub metallic: f32,
}

impl Default for MatCfg {
    fn default() -> Self {
        Self { color: [255, 255, 255], texture: true, roughness: 0.9, metallic: 0.0 }
    }
}

/// Wygląd ścian bocznych.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Walls {
    /// Jednolity kolor materiału ścian.
    #[default]
    Plain,
    /// Poziome warstwy skalne (tekstura), jak przekrój geologiczny.
    Strata,
}

impl Walls {
    pub const ALL: [Walls; 2] = [Walls::Plain, Walls::Strata];
    pub fn label(self) -> &'static str {
        match self {
            Walls::Plain => crate::i18n::tr("Jednolity kolor"),
            Walls::Strata => crate::i18n::tr("Warstwy skalne"),
        }
    }
}

/// Gotowe zestawy materiałów.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatPreset {
    Natural,
    Matte,
    Plaster,
    Ceramic,
}

impl MatPreset {
    pub const ALL: [MatPreset; 4] = [MatPreset::Natural, MatPreset::Matte, MatPreset::Plaster, MatPreset::Ceramic];
    pub fn label(self) -> &'static str {
        match self {
            MatPreset::Natural => crate::i18n::tr("Naturalne"),
            MatPreset::Matte => crate::i18n::tr("Matowa makieta"),
            MatPreset::Plaster => crate::i18n::tr("Biały gips"),
            MatPreset::Ceramic => crate::i18n::tr("Ceramika"),
        }
    }
}

pub fn default_mats() -> [MatCfg; 9] {
    let tex = |r: f32| MatCfg { color: [255, 255, 255], texture: true, roughness: r, metallic: 0.0 };
    [
        tex(0.92),
        tex(0.95),
        tex(0.78),
        tex(0.86),
        MatCfg { color: [250, 252, 255], texture: true, roughness: 0.5, metallic: 0.0 },
        MatCfg { color: [235, 245, 255], texture: true, roughness: 0.08, metallic: 0.0 },
        tex(0.7),
        MatCfg { color: [92, 80, 66], texture: false, roughness: 0.9, metallic: 0.0 },
        MatCfg { color: [44, 40, 36], texture: false, roughness: 0.95, metallic: 0.0 },
    ]
}

/// Ustawienia bryły.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelOpts {
    /// Szerokość modelu (dłuższy bok obszaru) w milimetrach.
    pub width_mm: f32,
    /// Przewyższenie (1 – naturalne proporcje).
    pub exag: f32,
    /// Grubość podstawy pod najniższym punktem (mm).
    pub base_mm: f32,
    /// Najwięcej oczek siatki modelu na dłuższy bok.
    pub cells: u32,
    /// Podział powierzchni na materiały (las, skały, woda…).
    pub materials: bool,
    pub mats: [MatCfg; 9],
    pub walls: Walls,
    /// Granica wiecznego śniegu (m n.p.m., 0 – wyłączona).
    pub snow_line: f32,
    /// Skała na stokach od tego nachylenia (°, 0 – wyłączone).
    pub rock_slope: f32,
}

impl Default for ModelOpts {
    fn default() -> Self {
        Self { width_mm: 150.0, exag: 1.5, base_mm: 4.0, cells: 400, materials: true, mats: default_mats(), walls: Walls::Plain, snow_line: 0.0, rock_slope: 45.0 }
    }
}

impl ModelOpts {
    pub fn mat(&self, m: Mat) -> &MatCfg {
        &self.mats[m as usize]
    }

    pub fn preset(&mut self, p: MatPreset) {
        let walls = self.mats[Mat::Walls as usize].color;
        self.mats = default_mats();
        match p {
            MatPreset::Natural => {
                self.mats[Mat::Walls as usize].color = walls;
            }
            MatPreset::Matte => {
                for m in &mut self.mats {
                    m.roughness = 1.0;
                    m.metallic = 0.0;
                }
            }
            MatPreset::Plaster => {
                for m in &mut self.mats {
                    *m = MatCfg { color: [244, 242, 236], texture: false, roughness: 0.96, metallic: 0.0 };
                }
                self.mats[Mat::Water as usize] = MatCfg { color: [200, 222, 240], texture: false, roughness: 0.3, metallic: 0.0 };
                self.mats[Mat::Bottom as usize].color = [220, 218, 212];
            }
            MatPreset::Ceramic => {
                for m in &mut self.mats {
                    m.roughness = 0.3;
                }
                self.mats[Mat::Walls as usize] = MatCfg { color: [30, 32, 36], texture: false, roughness: 0.25, metallic: 0.0 };
                self.mats[Mat::Bottom as usize] = MatCfg { color: [20, 20, 22], texture: false, roughness: 0.4, metallic: 0.0 };
            }
        }
    }
}

/// Bryła: wierzchołki, trójkąty powierzchni terenu (z materiałami), ścian i spodu. Ściany i spód
/// mają własne wierzchołki (ostre krawędzie przy cieniowaniu, osobne współrzędne tekstur).
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    /// Milimetry: x – wschód, y – północ, z – w górę (podstawa na z = 0).
    pub pos: Vec<[f32; 3]>,
    /// Współrzędne tekstury: teren 0..1 (v = 0 na północy); ściany – tekstura warstw (powtarzana).
    pub uv: Vec<[f32; 2]>,
    pub top: Vec<[u32; 3]>,
    /// Materiał każdego trójkąta powierzchni ([`Mat`] jako liczba).
    pub top_mat: Vec<u8>,
    pub wall: Vec<[u32; 3]>,
    pub bottom: Vec<[u32; 3]>,
    /// Skala: milimetry modelu na metr terenu (poziomo).
    pub mm_per_m: f64,
    /// Wysokość terenu (m n.p.m.) na wysokości podstawy i milimetry modelu na metr w pionie.
    pub zmin: f32,
    pub base_mm: f32,
    pub mm_per_m_z: f64,
}

/// Bryła z siatki `g` przyciętej funkcją odległości `sd` (ujemna wewnątrz, metry).
pub fn build(g: &Grid, sd: &dyn Fn(f64, f64, f32) -> f64, opts: &ModelOpts, bounds: [f64; 4]) -> Mesh {
    let longest = (bounds[2] - bounds[0]).max(bounds[3] - bounds[1]).max(1.0);
    let cell = (longest / opts.cells.max(16) as f64).max(g.cell);
    let m = g.resample(cell);
    let (nx, ny) = (m.nx, m.ny);
    let s = opts.width_mm as f64 / longest;
    let (cx, cy) = ((bounds[0] + bounds[2]) / 2.0, (bounds[1] + bounds[3]) / 2.0);
    // wartości w węzłach
    // węzeł dokładnie na brzegu liczy się jako zewnętrzny (inaczej punkt przecięcia pokryłby się
    // z wierzchołkiem węzła i powstałyby zdegenerowane trójkąty)
    let d: Vec<f64> = (0..nx * ny).map(|k| sd(m.x(k % nx), m.y(k / nx), m.z[k])).map(|v| if v == 0.0 { 1e-7 } else { v }).collect();
    let zmin = (0..nx * ny).filter(|&k| d[k] <= 0.0).map(|k| m.z[k]).fold(f32::INFINITY, f32::min);
    let zmin = if zmin.is_finite() { zmin } else { m.range().0 };
    let (gx0, gx1, gy0, gy1) = (g.x0, g.x1(), g.y0(), g.y1);
    let tx = |x: f64, y: f64, z: f64| -> ([f32; 3], [f32; 2]) {
        (
            [((x - cx) * s) as f32, ((y - cy) * s) as f32, ((z - zmin as f64) * s * opts.exag as f64 + opts.base_mm as f64) as f32],
            [((x - gx0) / (gx1 - gx0)) as f32, ((gy1 - y) / (gy1 - gy0)) as f32],
        )
    };
    let mut mesh = Mesh { mm_per_m: s, zmin, base_mm: opts.base_mm, mm_per_m_z: s * opts.exag as f64, ..Default::default() };
    // wierzchołek węzła i punkt przecięcia krawędzi (wspólne dla sąsiednich trójkątów)
    let mut node: HashMap<usize, u32> = HashMap::new();
    let mut cut: HashMap<(usize, usize), u32> = HashMap::new();
    let mut push = |mesh: &mut Mesh, x: f64, y: f64, z: f64| -> u32 {
        let (p, uv) = tx(x, y, z);
        mesh.pos.push(p);
        mesh.uv.push(uv);
        (mesh.pos.len() - 1) as u32
    };
    let mut boundary: Vec<(u32, u32)> = Vec::new();
    for j in 0..ny - 1 {
        for i in 0..nx - 1 {
            let (nw, ne, sw, se) = (j * nx + i, j * nx + i + 1, (j + 1) * nx + i, (j + 1) * nx + i + 1);
            if d[nw] > 0.0 && d[ne] > 0.0 && d[sw] > 0.0 && d[se] > 0.0 {
                continue;
            }
            // dwa trójkąty przeciwnie do wskazówek zegara (patrząc z góry)
            for tri in [[sw, se, ne], [sw, ne, nw]] {
                let inside: Vec<bool> = tri.iter().map(|&k| d[k] <= 0.0).collect();
                if inside.iter().all(|v| !v) {
                    continue;
                }
                // Sutherland–Hodgman: (indeks wierzchołka, czy z przecięcia)
                let mut poly: Vec<(u32, bool)> = Vec::with_capacity(4);
                for e in 0..3 {
                    let (a, b) = (tri[e], tri[(e + 1) % 3]);
                    let (ia, ib) = (inside[e], inside[(e + 1) % 3]);
                    if ia {
                        let v = *node.entry(a).or_insert_with(|| push(&mut mesh, m.x(a % nx), m.y(a / nx), m.z[a] as f64));
                        poly.push((v, false));
                    }
                    if ia != ib {
                        let key = (a.min(b), a.max(b));
                        let v = *cut.entry(key).or_insert_with(|| {
                            let t = d[a] / (d[a] - d[b]);
                            let (xa, ya, za) = (m.x(a % nx), m.y(a / nx), m.z[a] as f64);
                            let (xb, yb, zb) = (m.x(b % nx), m.y(b / nx), m.z[b] as f64);
                            push(&mut mesh, xa + (xb - xa) * t, ya + (yb - ya) * t, za + (zb - za) * t)
                        });
                        poly.push((v, true));
                    }
                }
                if poly.len() < 3 {
                    continue;
                }
                for k in 1..poly.len() - 1 {
                    mesh.top.push([poly[0].0, poly[k].0, poly[k + 1].0]);
                }
                // krawędź brzegu: dwa kolejne punkty przecięcia (wnętrze po lewej)
                for k in 0..poly.len() {
                    let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
                    if a.1 && b.1 && a.0 != b.0 {
                        boundary.push((a.0, b.0));
                    }
                }
            }
        }
    }
    mesh.top_mat = vec![Mat::Ground as u8; mesh.top.len()];
    // spód: kopia powierzchni na z = 0 z odwróconą kolejnością (własne wierzchołki)
    let n = mesh.pos.len();
    for k in 0..n {
        let p = mesh.pos[k];
        mesh.pos.push([p[0], p[1], 0.0]);
        let uv = mesh.uv[k];
        mesh.uv.push(uv);
    }
    let low = |v: u32| v + n as u32;
    let tops = mesh.top.clone();
    for t in &tops {
        mesh.bottom.push([low(t[0]), low(t[2]), low(t[1])]);
    }
    // ściany wzdłuż brzegu: każdy odcinek ma własne cztery wierzchołki (ostre krawędzie);
    // współrzędne tekstury: u wzdłuż brzegu, v – wysokość (poziome warstwy)
    let zmax = mesh.pos.iter().map(|p| p[2]).fold(1.0f32, f32::max);
    for (a, b) in boundary {
        let (pa, pb) = (mesh.pos[a as usize], mesh.pos[b as usize]);
        let u = |p: [f32; 3]| (p[0] * 0.7 + p[1] * 0.3) / 40.0;
        let base = mesh.pos.len() as u32;
        for (p, z) in [(pa, pa[2]), (pa, 0.0), (pb, 0.0), (pb, pb[2])] {
            mesh.pos.push([p[0], p[1], z]);
            mesh.uv.push([u(p), 1.0 - z / zmax]);
        }
        // a, a_dół, b_dół, b – na zewnątrz (wnętrze po lewej krawędzi a→b)
        mesh.wall.push([base, base + 1, base + 2]);
        mesh.wall.push([base, base + 2, base + 3]);
    }
    mesh
}

/// Klasy pokrycia z maski (0 – nic, 1 – las, 2 – piarg, 3 – skała, 4 – lodowiec, 5 – woda, 6 – budynek).
pub fn mat_of_cover(c: u8) -> Mat {
    match c {
        1 => Mat::Forest,
        2 => Mat::Scree,
        3 => Mat::Rock,
        4 => Mat::Snow,
        5 => Mat::Water,
        6 => Mat::Building,
        _ => Mat::Ground,
    }
}

impl Mesh {
    pub fn triangles(&self) -> usize {
        self.top.len() + self.wall.len() + self.bottom.len()
    }

    /// Wszystkie trójkąty: powierzchnia, ściany, spód.
    pub fn all(&self) -> impl Iterator<Item = &[u32; 3]> {
        self.top.iter().chain(self.wall.iter()).chain(self.bottom.iter())
    }

    /// Wymiary (mm): szerokość, głębokość, wysokość.
    pub fn size(&self) -> [f32; 3] {
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for p in &self.pos {
            for i in 0..3 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
        if lo[0] > hi[0] {
            return [0.0; 3];
        }
        [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]]
    }

    pub fn normal(&self, t: &[u32; 3]) -> [f32; 3] {
        let (a, b, c) = (self.pos[t[0] as usize], self.pos[t[1] as usize], self.pos[t[2] as usize]);
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
        [n[0] / l, n[1] / l, n[2] / l]
    }

    /// Wysokość terenu (m n.p.m.) punktu modelu.
    pub fn elevation(&self, z_mm: f32) -> f32 {
        if self.mm_per_m_z <= 0.0 {
            return self.zmin;
        }
        self.zmin + ((z_mm - self.base_mm) as f64 / self.mm_per_m_z) as f32
    }

    /// Materiały powierzchni: `cover(u, v)` – klasa pokrycia z mapy (współrzędne tekstury terenu),
    /// do tego skała na stromych stokach i śnieg powyżej granicy z ustawień.
    pub fn assign(&mut self, cover: &dyn Fn(f32, f32) -> u8, opts: &ModelOpts) {
        let rock_cos = if opts.rock_slope > 0.0 { opts.rock_slope.to_radians().cos() } else { -1.0 };
        let mats: Vec<u8> = self
            .top
            .iter()
            .map(|t| {
                let (a, b, c) = (self.uv[t[0] as usize], self.uv[t[1] as usize], self.uv[t[2] as usize]);
                let (u, v) = ((a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0);
                let mut m = mat_of_cover(cover(u, v));
                let z = (self.pos[t[0] as usize][2] + self.pos[t[1] as usize][2] + self.pos[t[2] as usize][2]) / 3.0;
                // nachylenie z normalnej (w modelu z przewyższeniem – liczone na prawdziwym terenie)
                let n = self.normal(t);
                let exag = (self.mm_per_m_z / self.mm_per_m.max(1e-12)) as f32;
                let horiz = (n[0] * n[0] + n[1] * n[1]).sqrt() / exag.max(0.01);
                let cos = n[2] / (n[2] * n[2] + horiz * horiz).sqrt().max(1e-6);
                if m == Mat::Ground && rock_cos > -1.0 && cos < rock_cos {
                    m = Mat::Rock;
                }
                if opts.snow_line > 0.0 && m != Mat::Water && self.elevation(z) >= opts.snow_line {
                    m = Mat::Snow;
                }
                m as u8
            })
            .collect();
        self.top_mat = mats;
    }

    /// Trójkąty pogrupowane według materiałów (bez podziału – cała powierzchnia jako teren).
    pub fn groups(&self, opts: &ModelOpts) -> Vec<(Mat, Vec<[u32; 3]>)> {
        let mut by: std::collections::BTreeMap<Mat, Vec<[u32; 3]>> = Default::default();
        for (k, t) in self.top.iter().enumerate() {
            let m = if opts.materials { Mat::from_u8(self.top_mat.get(k).copied().unwrap_or(0)) } else { Mat::Ground };
            by.entry(m).or_default().push(*t);
        }
        let mut out: Vec<(Mat, Vec<[u32; 3]>)> = by.into_iter().collect();
        if !self.wall.is_empty() {
            out.push((Mat::Walls, self.wall.clone()));
        }
        if !self.bottom.is_empty() {
            out.push((Mat::Bottom, self.bottom.clone()));
        }
        out
    }

    /// Gładkie normalne wierzchołków (średnia przylegających trójkątów).
    pub fn vertex_normals(&self) -> Vec<[f32; 3]> {
        let mut acc = vec![[0.0f32; 3]; self.pos.len()];
        for t in self.all() {
            let nrm = self.normal(t);
            for &v in t {
                for i in 0..3 {
                    acc[v as usize][i] += nrm[i];
                }
            }
        }
        acc.into_iter()
            .map(|n| {
                let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                if l > 0.0 {
                    [n[0] / l, n[1] / l, n[2] / l]
                } else {
                    [0.0, 0.0, 1.0]
                }
            })
            .collect()
    }

    /// STL binarny (milimetry).
    pub fn stl(&self, title: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(84 + self.triangles() * 50);
        let mut head = format!("GRAN Studio: {title}").into_bytes();
        head.resize(80, b' ');
        out.extend_from_slice(&head[..80]);
        out.extend_from_slice(&(self.triangles() as u32).to_le_bytes());
        for t in self.all() {
            for v in self.normal(t) {
                out.extend_from_slice(&v.to_le_bytes());
            }
            for &k in t {
                for v in self.pos[k as usize] {
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
            out.extend_from_slice(&[0, 0]);
        }
        out
    }

    /// OBJ z materiałami (osobne grupy) i plik MTL (z rozszerzeniem PBR: Pr, Pm).
    /// Tekstury: `<stem>.png` (teren) i `<stem>_sciany.png` (warstwy skalne, gdy `strata`).
    pub fn obj(&self, stem: &str, credits: &str, opts: &ModelOpts, strata: bool) -> (String, String) {
        use std::fmt::Write;
        let mut o = String::with_capacity(self.pos.len() * 90);
        let _ = writeln!(o, "# GRAN Studio – {stem}\n# {credits}\n# jednostki: milimetry, z w górę\nmtllib {stem}.mtl");
        for p in &self.pos {
            let _ = writeln!(o, "v {:.4} {:.4} {:.4}", p[0], p[1], p[2]);
        }
        for t in &self.uv {
            let _ = writeln!(o, "vt {:.6} {:.6}", t[0], 1.0 - t[1]);
        }
        for n in self.vertex_normals() {
            let _ = writeln!(o, "vn {:.4} {:.4} {:.4}", n[0], n[1], n[2]);
        }
        let mut mtl = String::from("# GRAN Studio – materiały (PBR: Pr – szorstkość, Pm – metaliczność)\n");
        for (m, tris) in self.groups(opts) {
            let id = m.id();
            let _ = writeln!(o, "o {id}\ng {id}\nusemtl {id}");
            for t in &tris {
                let (a, b, c) = (t[0] + 1, t[1] + 1, t[2] + 1);
                let _ = writeln!(o, "f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}");
            }
            let cfg = opts.mat(m);
            let c = cfg.color.map(|v| v as f32 / 255.0);
            let ns = ((1.0 - cfg.roughness).powi(2) * 900.0 + 4.0).round();
            let ks = (1.0 - cfg.roughness) * 0.5;
            let _ = write!(
                mtl,
                "\nnewmtl {id}\nKa {r:.3} {g:.3} {b:.3}\nKd {r:.3} {g:.3} {b:.3}\nKs {ks:.3} {ks:.3} {ks:.3}\nNs {ns}\nPr {:.3}\nPm {:.3}\nillum 2\n",
                cfg.roughness,
                cfg.metallic,
                r = c[0],
                g = c[1],
                b = c[2]
            );
            let texture = match m {
                Mat::Walls if strata => Some(format!("{stem}_sciany.png")),
                Mat::Walls | Mat::Bottom => None,
                _ if cfg.texture => Some(format!("{stem}.png")),
                _ => None,
            };
            if let Some(t) = texture {
                let _ = writeln!(mtl, "map_Kd {t}");
            }
        }
        (o, mtl)
    }

    /// Kolor wierzchołka powierzchni (tekstura z odcieniem albo kolor jednolity).
    fn top_color(&self, k: usize, tex: &RgbaImage, cfg: &MatCfg) -> [u8; 3] {
        if !cfg.texture {
            return cfg.color;
        }
        let uv = self.uv[k];
        let x = ((uv[0] * tex.width() as f32) as u32).min(tex.width() - 1);
        let y = ((uv[1] * tex.height() as f32) as u32).min(tex.height() - 1);
        let p = tex.get_pixel(x, y).0;
        [0, 1, 2].map(|i| (p[i] as u32 * cfg.color[i] as u32 / 255) as u8)
    }

    /// PLY binarny z kolorami wierzchołków (materiały i tekstury wypalone w kolorach).
    pub fn ply(&self, tex: &RgbaImage, opts: &ModelOpts, strata: Option<&RgbaImage>) -> Vec<u8> {
        let n = self.pos.len();
        let mut col = vec![opts.mat(Mat::Bottom).color; n];
        for (k, t) in self.top.iter().enumerate() {
            let m = if opts.materials { Mat::from_u8(self.top_mat.get(k).copied().unwrap_or(0)) } else { Mat::Ground };
            let cfg = opts.mat(m);
            for &v in t {
                col[v as usize] = self.top_color(v as usize, tex, cfg);
            }
        }
        let wall = opts.mat(Mat::Walls);
        for t in &self.wall {
            for &v in t {
                let uv = self.uv[v as usize];
                col[v as usize] = match strata {
                    Some(s) => {
                        let y = ((uv[1].clamp(0.0, 1.0) * s.height() as f32) as u32).min(s.height() - 1);
                        let p = s.get_pixel(0, y).0;
                        [0, 1, 2].map(|i| (p[i] as u32 * wall.color[i] as u32 / 255) as u8)
                    }
                    None => wall.color,
                };
            }
        }
        let head = format!(
            "ply\nformat binary_little_endian 1.0\ncomment GRAN Studio (milimetry)\nelement vertex {n}\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nelement face {}\nproperty list uchar uint vertex_indices\nend_header\n",
            self.triangles()
        );
        let mut out = head.into_bytes();
        for k in 0..n {
            for v in self.pos[k] {
                out.extend_from_slice(&v.to_le_bytes());
            }
            out.extend_from_slice(&col[k]);
        }
        for t in self.all() {
            out.push(3);
            for &v in t {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out
    }

    /// GLB (glTF 2.0, metry, oś Y w górę): osobny prymityw i materiał PBR dla każdego materiału,
    /// tekstura terenu i (opcjonalnie) tekstura warstw skalnych na ścianach.
    pub fn glb(&self, tex_png: &[u8], strata_png: Option<&[u8]>, opts: &ModelOpts, credits: &str) -> Vec<u8> {
        let normals = self.vertex_normals();
        let mut bin: Vec<u8> = Vec::new();
        let align = |b: &mut Vec<u8>| {
            while b.len() % 4 != 0 {
                b.push(0);
            }
        };
        let mut views: Vec<serde_json::Value> = Vec::new();
        let mut view = |bin: &mut Vec<u8>, off: usize, target: Option<u32>| {
            let len = bin.len() - off;
            let mut v = serde_json::json!({ "buffer": 0, "byteOffset": off, "byteLength": len });
            if let Some(t) = target {
                v["target"] = t.into();
            }
            views.push(v);
            align(bin);
            views.len() - 1
        };
        // glTF: x = x, y = z (w górę), z = −y (południe); milimetry → metry
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        let off = bin.len();
        for p in &self.pos {
            let q = [p[0] / 1000.0, p[2] / 1000.0, -p[1] / 1000.0];
            for i in 0..3 {
                lo[i] = lo[i].min(q[i]);
                hi[i] = hi[i].max(q[i]);
                bin.extend_from_slice(&q[i].to_le_bytes());
            }
        }
        let v_pos = view(&mut bin, off, Some(34962));
        let off = bin.len();
        for n in &normals {
            for v in [n[0], n[2], -n[1]] {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        let v_nrm = view(&mut bin, off, Some(34962));
        let off = bin.len();
        for t in &self.uv {
            bin.extend_from_slice(&t[0].to_le_bytes());
            bin.extend_from_slice(&t[1].to_le_bytes());
        }
        let v_uv = view(&mut bin, off, Some(34962));
        let n = self.pos.len();
        let mut accessors = vec![
            serde_json::json!({ "bufferView": v_pos, "componentType": 5126, "count": n, "type": "VEC3", "min": lo, "max": hi }),
            serde_json::json!({ "bufferView": v_nrm, "componentType": 5126, "count": n, "type": "VEC3" }),
            serde_json::json!({ "bufferView": v_uv, "componentType": 5126, "count": n, "type": "VEC2" }),
        ];
        let mut prims = Vec::new();
        let mut materials = Vec::new();
        for (m, tris) in self.groups(opts) {
            let off = bin.len();
            for t in &tris {
                for v in t {
                    bin.extend_from_slice(&v.to_le_bytes());
                }
            }
            let bv = view(&mut bin, off, Some(34963));
            accessors.push(serde_json::json!({ "bufferView": bv, "componentType": 5125, "count": tris.len() * 3, "type": "SCALAR" }));
            let cfg = opts.mat(m);
            let c = cfg.color.map(|v| (v as f32 / 255.0).powf(2.2));
            let mut pbr = serde_json::json!({ "baseColorFactor": [c[0], c[1], c[2], 1.0], "metallicFactor": cfg.metallic, "roughnessFactor": cfg.roughness });
            let texture = match m {
                Mat::Walls if strata_png.is_some() => Some(1),
                Mat::Walls | Mat::Bottom => None,
                _ if cfg.texture => Some(0),
                _ => None,
            };
            if let Some(t) = texture {
                pbr["baseColorTexture"] = serde_json::json!({ "index": t });
            }
            materials.push(serde_json::json!({ "name": m.id(), "pbrMetallicRoughness": pbr, "doubleSided": false }));
            prims.push(serde_json::json!({ "attributes": { "POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2 }, "indices": accessors.len() - 1, "material": materials.len() - 1 }));
        }
        let off = bin.len();
        bin.extend_from_slice(tex_png);
        let v_img = view(&mut bin, off, None);
        let mut images = vec![serde_json::json!({ "bufferView": v_img, "mimeType": "image/png" })];
        let mut textures = vec![serde_json::json!({ "source": 0, "sampler": 0 })];
        if let Some(sp) = strata_png {
            let off = bin.len();
            bin.extend_from_slice(sp);
            let v = view(&mut bin, off, None);
            images.push(serde_json::json!({ "bufferView": v, "mimeType": "image/png" }));
            textures.push(serde_json::json!({ "source": 1, "sampler": 1 }));
        }
        align(&mut bin);
        let json = serde_json::json!({
            "asset": { "version": "2.0", "generator": "GRAN Studio", "copyright": credits },
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": [{ "mesh": 0, "name": "wycinek" }],
            "meshes": [{ "name": "wycinek", "primitives": prims }],
            "materials": materials,
            "textures": textures,
            "samplers": [
                { "magFilter": 9729, "minFilter": 9987, "wrapS": 33071, "wrapT": 33071 },
                { "magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 33071 }
            ],
            "images": images,
            "buffers": [{ "byteLength": bin.len() }],
            "bufferViews": views,
            "accessors": accessors
        });
        let mut js = json.to_string().into_bytes();
        while js.len() % 4 != 0 {
            js.push(b' ');
        }
        let total = 12 + 8 + js.len() + 8 + bin.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(js.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&js);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        out
    }
}

/// Tekstura ścian: poziome warstwy skalne (gleba u góry, niżej piaskowce, łupki i wapienie).
pub fn strata_texture() -> RgbaImage {
    let (w, h) = (64u32, 512u32);
    let mut img = RgbaImage::new(w, h);
    // deterministyczny pseudolosowy ciąg (powtarzalny wynik)
    let mut seed = 0x9e37_79b9u32;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed % 10_000) as f32 / 10_000.0
    };
    let palette: [[f32; 3]; 6] = [[132.0, 110.0, 84.0], [168.0, 146.0, 112.0], [110.0, 104.0, 98.0], [150.0, 140.0, 128.0], [124.0, 96.0, 74.0], [178.0, 170.0, 156.0]];
    let mut bands: Vec<(u32, [f32; 3])> = Vec::new();
    let mut y = 0u32;
    // gleba
    bands.push((h / 40, [70.0, 54.0, 40.0]));
    y += h / 40;
    while y < h {
        let t = (6.0 + rnd() * 34.0) as u32;
        let c = palette[(rnd() * palette.len() as f32) as usize % palette.len()];
        let k = 0.85 + rnd() * 0.3;
        bands.push((t, c.map(|v| v * k)));
        y += t;
    }
    let mut row = 0u32;
    for (t, c) in bands {
        for dy in 0..t {
            if row + dy >= h {
                break;
            }
            for x in 0..w {
                // drobne ziarno i ciemniejsza spągowa linia warstwy
                let grain = 0.94 + rnd() * 0.12;
                let edge = if dy + 1 == t { 0.8 } else { 1.0 };
                let p = c.map(|v| (v * grain * edge).clamp(0.0, 255.0) as u8);
                img.put_pixel(x, row + dy, image::Rgba([p[0], p[1], p[2], 255]));
            }
        }
        row += t;
    }
    img
}

/// Mapa wysokości jako PNG 16-bit w skali szarości (0 – najniżej, 65535 – najwyżej) i opis zakresu.
pub fn heightmap_png(g: &Grid) -> (Vec<u8>, String) {
    let (lo, hi) = g.range();
    let span = (hi - lo).max(0.01);
    let mut img = image::ImageBuffer::<image::Luma<u16>, Vec<u16>>::new(g.nx as u32, g.ny as u32);
    for j in 0..g.ny {
        for i in 0..g.nx {
            let v = ((g.at(i, j) - lo) / span * 65535.0).round().clamp(0.0, 65535.0) as u16;
            img.put_pixel(i as u32, j as u32, image::Luma([v]));
        }
    }
    let mut out = Vec::new();
    let _ = image::DynamicImage::ImageLuma16(img).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png);
    let info = format!(
        "GRAN Studio – mapa wysokości\nrozmiar: {} × {} px\noczko: {:.2} m\nwysokość 0 = {:.2} m n.p.m.\nwysokość 65535 = {:.2} m n.p.m.\n",
        g.nx, g.ny, g.cell, lo, hi
    );
    (out, info)
}

/// PNG z obrazu RGBA.
pub fn png(img: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::new();
    let _ = img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bump() -> Grid {
        let mut g = Grid::new([0.0, 0.0, 1000.0, 1000.0], 20.0);
        for j in 0..g.ny {
            for i in 0..g.nx {
                let (x, y) = (g.x(i) - 500.0, g.y(j) - 500.0);
                g.z[j * g.nx + i] = 1000.0 + 300.0 * (-(x * x + y * y) / 80_000.0).exp() as f32;
            }
        }
        g
    }

    /// Szczelność (według położeń, bo ściany mają własne wierzchołki): każda krawędź należy do
    /// dokładnie dwóch trójkątów, w przeciwnych kierunkach.
    fn watertight(m: &Mesh) -> bool {
        let mut ids: HashMap<[i64; 3], u32> = HashMap::new();
        let mut id = |p: [f32; 3]| {
            let key = p.map(|v| (v as f64 * 1e4).round() as i64);
            let n = ids.len() as u32;
            *ids.entry(key).or_insert(n)
        };
        let vid: Vec<u32> = m.pos.iter().map(|&p| id(p)).collect();
        let mut edges: HashMap<(u32, u32), i32> = HashMap::new();
        for t in m.all() {
            for k in 0..3 {
                let (a, b) = (vid[t[k] as usize], vid[t[(k + 1) % 3] as usize]);
                if a != b {
                    *edges.entry((a, b)).or_default() += 1;
                }
            }
        }
        edges.iter().all(|(&(a, b), &n)| n == 1 && edges.get(&(b, a)) == Some(&1))
    }

    #[test]
    fn circle_model_is_closed() {
        let g = bump();
        let sd = |x: f64, y: f64, _z: f32| ((x - 500.0).powi(2) + (y - 500.0).powi(2)).sqrt() - 400.0;
        let mut m = build(&g, &sd, &ModelOpts { cells: 60, ..Default::default() }, [100.0, 100.0, 900.0, 900.0]);
        assert!(!m.top.is_empty() && !m.wall.is_empty() && !m.bottom.is_empty());
        assert!(watertight(&m), "bryła nie jest szczelna");
        let s = m.size();
        assert!((s[0] - 150.0).abs() < 2.0, "{s:?}");
        let stl = m.stl("test");
        assert_eq!(stl.len(), 84 + m.triangles() * 50);
        // materiały: woda w środku, śnieg wysoko
        let opts = ModelOpts { snow_line: 1250.0, ..Default::default() };
        m.assign(&|u, v| if (u - 0.5).abs() < 0.05 && (v - 0.5).abs() < 0.05 { 5 } else { 0 }, &opts);
        let groups = m.groups(&opts);
        assert!(groups.iter().any(|(g, _)| *g == Mat::Water));
        assert!(groups.iter().any(|(g, _)| *g == Mat::Snow));
        let total: usize = groups.iter().map(|(_, t)| t.len()).sum();
        assert_eq!(total, m.triangles());
        let strata = png(&strata_texture());
        let glb = m.glb(&[0u8; 8], Some(&strata), &opts, "x");
        assert_eq!(&glb[..4], b"glTF");
        let (obj, mtl) = m.obj("wycinek", "x", &opts, true);
        assert!(obj.contains("usemtl woda") && mtl.contains("newmtl sciany") && mtl.contains("wycinek_sciany.png"));
        let ply = m.ply(&RgbaImage::new(4, 4), &opts, Some(&strata_texture()));
        assert!(ply.starts_with(b"ply"));
    }

    #[test]
    fn above_threshold_cut_is_closed() {
        let g = bump();
        let sd = |x: f64, y: f64, z: f32| {
            let r = ((x - 500.0).abs() - 450.0).max((y - 500.0).abs() - 450.0);
            r.max((1100.0 - z) as f64)
        };
        let m = build(&g, &sd, &ModelOpts { cells: 50, ..Default::default() }, [50.0, 50.0, 950.0, 950.0]);
        assert!(!m.top.is_empty());
        assert!(watertight(&m));
    }
}
