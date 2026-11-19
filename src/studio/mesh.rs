//! Model 3D wycinka: zamknięta bryła (do druku 3D) – powierzchnia terenu przycięta gładko do kształtu
//! obszaru, pionowe ściany i płaska podstawa – oraz zapis do STL, OBJ (+MTL i tekstura PNG), PLY
//! z kolorami wierzchołków, GLB (glTF z teksturą) i mapy wysokości PNG 16-bit.

use std::collections::HashMap;

use image::RgbaImage;
use serde::{Deserialize, Serialize};

use super::terrain::Grid;

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
}

impl Default for ModelOpts {
    fn default() -> Self {
        Self { width_mm: 150.0, exag: 1.5, base_mm: 4.0, cells: 400 }
    }
}

/// Bryła: wspólne wierzchołki, trójkąty powierzchni terenu i trójkąty boków/podstawy.
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    /// Milimetry: x – wschód, y – północ, z – w górę (podstawa na z = 0).
    pub pos: Vec<[f32; 3]>,
    /// Współrzędne tekstury (0..1, v = 0 na północy).
    pub uv: Vec<[f32; 2]>,
    pub top: Vec<[u32; 3]>,
    pub side: Vec<[u32; 3]>,
    /// Skala: milimetry modelu na metr terenu (poziomo).
    pub mm_per_m: f64,
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
    let d: Vec<f64> = (0..nx * ny).map(|k| sd(m.x(k % nx), m.y(k / nx), m.z[k])).collect();
    let zmin = (0..nx * ny).filter(|&k| d[k] <= 0.0).map(|k| m.z[k]).fold(f32::INFINITY, f32::min);
    let zmin = if zmin.is_finite() { zmin } else { m.range().0 };
    let (gx0, gx1, gy0, gy1) = (g.x0, g.x1(), g.y0(), g.y1);
    let tx = |x: f64, y: f64, z: f64| -> ([f32; 3], [f32; 2]) {
        (
            [((x - cx) * s) as f32, ((y - cy) * s) as f32, ((z - zmin as f64) * s * opts.exag as f64 + opts.base_mm as f64) as f32],
            [((x - gx0) / (gx1 - gx0)) as f32, ((gy1 - y) / (gy1 - gy0)) as f32],
        )
    };
    let mut mesh = Mesh { mm_per_m: s, ..Default::default() };
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
    // podstawa: kopia powierzchni na z = 0 z odwróconą kolejnością
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
        mesh.side.push([low(t[0]), low(t[2]), low(t[1])]);
    }
    // ściany wzdłuż brzegu
    for (a, b) in boundary {
        mesh.side.push([a, low(a), low(b)]);
        mesh.side.push([a, low(b), b]);
    }
    mesh
}

impl Mesh {
    pub fn triangles(&self) -> usize {
        self.top.len() + self.side.len()
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

    fn normal(&self, t: &[u32; 3]) -> [f32; 3] {
        let (a, b, c) = (self.pos[t[0] as usize], self.pos[t[1] as usize], self.pos[t[2] as usize]);
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
        [n[0] / l, n[1] / l, n[2] / l]
    }

    /// Gładkie normalne wierzchołków (średnia trójkątów powierzchni; boki – płaskie).
    pub fn vertex_normals(&self) -> Vec<[f32; 3]> {
        let mut acc = vec![[0.0f32; 3]; self.pos.len()];
        for t in self.top.iter().chain(self.side.iter()) {
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
        for t in self.top.iter().chain(self.side.iter()) {
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

    /// OBJ z dwoma materiałami (teren z teksturą, podstawa w jednym kolorze) i plik MTL.
    pub fn obj(&self, stem: &str, credits: &str, base: [u8; 3]) -> (String, String) {
        use std::fmt::Write;
        let mut o = String::with_capacity(self.pos.len() * 60);
        let _ = writeln!(o, "# GRAN Studio – {stem}\n# {credits}\n# jednostki: milimetry, z w górę\nmtllib {stem}.mtl");
        for p in &self.pos {
            let _ = writeln!(o, "v {:.4} {:.4} {:.4}", p[0], p[1], p[2]);
        }
        for t in &self.uv {
            let _ = writeln!(o, "vt {:.6} {:.6}", t[0], 1.0 - t[1]);
        }
        let _ = writeln!(o, "o teren\nusemtl teren");
        for t in &self.top {
            let _ = writeln!(o, "f {a}/{a} {b}/{b} {c}/{c}", a = t[0] + 1, b = t[1] + 1, c = t[2] + 1);
        }
        let _ = writeln!(o, "o podstawa\nusemtl podstawa");
        for t in &self.side {
            let _ = writeln!(o, "f {} {} {}", t[0] + 1, t[1] + 1, t[2] + 1);
        }
        let c = base.map(|v| v as f32 / 255.0);
        let mtl = format!(
            "# GRAN Studio\nnewmtl teren\nKa 1 1 1\nKd 1 1 1\nKs 0 0 0\nillum 1\nmap_Kd {stem}.png\n\nnewmtl podstawa\nKa {r} {g} {b}\nKd {r} {g} {b}\nKs 0 0 0\nillum 1\n",
            r = c[0],
            g = c[1],
            b = c[2]
        );
        (o, mtl)
    }

    /// PLY binarny z kolorami wierzchołków (teren z tekstury, podstawa jednym kolorem).
    pub fn ply(&self, tex: &RgbaImage, base: [u8; 3]) -> Vec<u8> {
        let n = self.pos.len();
        let mut col = vec![base; n];
        let mut on_top = vec![false; n];
        for t in &self.top {
            for &v in t {
                on_top[v as usize] = true;
            }
        }
        for (k, c) in col.iter_mut().enumerate() {
            if on_top[k] {
                let uv = self.uv[k];
                let x = ((uv[0] * tex.width() as f32) as u32).min(tex.width() - 1);
                let y = ((uv[1] * tex.height() as f32) as u32).min(tex.height() - 1);
                let p = tex.get_pixel(x, y).0;
                *c = [p[0], p[1], p[2]];
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
        for t in self.top.iter().chain(self.side.iter()) {
            out.push(3);
            for &v in t {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out
    }

    /// GLB (glTF 2.0, metry, oś Y w górę): teren z teksturą PNG i podstawa w kolorze.
    pub fn glb(&self, tex_png: &[u8], base: [u8; 3], credits: &str) -> Vec<u8> {
        let normals = self.vertex_normals();
        let mut bin: Vec<u8> = Vec::new();
        let align = |b: &mut Vec<u8>| {
            while b.len() % 4 != 0 {
                b.push(0);
            }
        };
        // glTF: x = x, y = z (w górę), z = −y (południe); milimetry → metry
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        let pos_off = bin.len();
        for p in &self.pos {
            let q = [p[0] / 1000.0, p[2] / 1000.0, -p[1] / 1000.0];
            for i in 0..3 {
                lo[i] = lo[i].min(q[i]);
                hi[i] = hi[i].max(q[i]);
                bin.extend_from_slice(&q[i].to_le_bytes());
            }
        }
        let pos_len = bin.len() - pos_off;
        let nrm_off = bin.len();
        for n in &normals {
            for v in [n[0], n[2], -n[1]] {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        let nrm_len = bin.len() - nrm_off;
        let uv_off = bin.len();
        for t in &self.uv {
            bin.extend_from_slice(&t[0].to_le_bytes());
            bin.extend_from_slice(&t[1].to_le_bytes());
        }
        let uv_len = bin.len() - uv_off;
        let top_off = bin.len();
        for t in &self.top {
            for v in t {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        let top_len = bin.len() - top_off;
        let side_off = bin.len();
        for t in &self.side {
            for v in t {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        let side_len = bin.len() - side_off;
        align(&mut bin);
        let img_off = bin.len();
        bin.extend_from_slice(tex_png);
        let img_len = tex_png.len();
        align(&mut bin);
        let n = self.pos.len();
        let c = base.map(|v| (v as f32 / 255.0).powf(2.2));
        let json = serde_json::json!({
            "asset": { "version": "2.0", "generator": "GRAN Studio", "copyright": credits },
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": [{ "mesh": 0, "name": "wycinek" }],
            "meshes": [{ "name": "wycinek", "primitives": [
                { "attributes": { "POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2 }, "indices": 3, "material": 0 },
                { "attributes": { "POSITION": 0, "NORMAL": 1 }, "indices": 4, "material": 1 }
            ]}],
            "materials": [
                { "name": "teren", "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 }, "metallicFactor": 0.0, "roughnessFactor": 0.95 } },
                { "name": "podstawa", "pbrMetallicRoughness": { "baseColorFactor": [c[0], c[1], c[2], 1.0], "metallicFactor": 0.0, "roughnessFactor": 0.9 } }
            ],
            "textures": [{ "source": 0, "sampler": 0 }],
            "samplers": [{ "magFilter": 9729, "minFilter": 9987, "wrapS": 33071, "wrapT": 33071 }],
            "images": [{ "bufferView": 5, "mimeType": "image/png" }],
            "buffers": [{ "byteLength": bin.len() }],
            "bufferViews": [
                { "buffer": 0, "byteOffset": pos_off, "byteLength": pos_len, "target": 34962 },
                { "buffer": 0, "byteOffset": nrm_off, "byteLength": nrm_len, "target": 34962 },
                { "buffer": 0, "byteOffset": uv_off, "byteLength": uv_len, "target": 34962 },
                { "buffer": 0, "byteOffset": top_off, "byteLength": top_len, "target": 34963 },
                { "buffer": 0, "byteOffset": side_off, "byteLength": side_len, "target": 34963 },
                { "buffer": 0, "byteOffset": img_off, "byteLength": img_len }
            ],
            "accessors": [
                { "bufferView": 0, "componentType": 5126, "count": n, "type": "VEC3", "min": lo, "max": hi },
                { "bufferView": 1, "componentType": 5126, "count": n, "type": "VEC3" },
                { "bufferView": 2, "componentType": 5126, "count": n, "type": "VEC2" },
                { "bufferView": 3, "componentType": 5125, "count": self.top.len() * 3, "type": "SCALAR" },
                { "bufferView": 4, "componentType": 5125, "count": self.side.len() * 3, "type": "SCALAR" }
            ]
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

    /// Szczelność: każda krawędź należy do dokładnie dwóch trójkątów, w przeciwnych kierunkach.
    fn watertight(m: &Mesh) -> bool {
        let mut edges: HashMap<(u32, u32), i32> = HashMap::new();
        for t in m.top.iter().chain(m.side.iter()) {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                *edges.entry((a, b)).or_default() += 1;
            }
        }
        edges.iter().all(|(&(a, b), &n)| n == 1 && edges.get(&(b, a)) == Some(&1))
    }

    #[test]
    fn circle_model_is_closed() {
        let g = bump();
        let sd = |x: f64, y: f64, _z: f32| ((x - 500.0).powi(2) + (y - 500.0).powi(2)).sqrt() - 400.0;
        let m = build(&g, &sd, &ModelOpts { cells: 60, ..Default::default() }, [100.0, 100.0, 900.0, 900.0]);
        assert!(!m.top.is_empty());
        assert!(watertight(&m), "bryła nie jest szczelna");
        let s = m.size();
        assert!((s[0] - 150.0).abs() < 2.0, "{s:?}");
        let stl = m.stl("test");
        assert_eq!(stl.len(), 84 + m.triangles() * 50);
        let glb = m.glb(&[0u8; 8], [10, 10, 10], "x");
        assert_eq!(&glb[..4], b"glTF");
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
