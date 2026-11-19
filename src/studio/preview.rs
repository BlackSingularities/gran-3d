//! Podgląd modelu w oknie Studio: prosty rasteryzator programowy (rzut prostokątny, bufor głębi,
//! tekstura terenu, światło z ustawień wyglądu). Działa w wątku interfejsu na zmniejszonej bryle.

use image::RgbaImage;

use super::mesh::Mesh;

/// Kamera podglądu: obrót wokół pionu, nachylenie i powiększenie.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
}

impl Default for View {
    fn default() -> Self {
        Self { yaw: -25.0, pitch: 38.0, zoom: 1.0 }
    }
}

/// Obraz `w`×`h` z bryły (milimetry) i tekstury terenu.
pub fn render(mesh: &Mesh, tex: &RgbaImage, base: [u8; 3], light: [f32; 3], v: &View, w: usize, h: usize) -> Vec<[u8; 4]> {
    let bg = [24u8, 27, 33, 255];
    let mut px = vec![bg; w * h];
    let mut depth = vec![f32::NEG_INFINITY; w * h];
    if mesh.pos.is_empty() || w < 4 || h < 4 {
        return px;
    }
    // środek bryły i zasięg
    let size = mesh.size();
    let mut lo = [f32::INFINITY; 3];
    for p in &mesh.pos {
        for i in 0..3 {
            lo[i] = lo[i].min(p[i]);
        }
    }
    let c = [lo[0] + size[0] / 2.0, lo[1] + size[1] / 2.0, lo[2] + size[2] / 2.0];
    let radius = (size[0] * size[0] + size[1] * size[1] + size[2] * size[2]).sqrt().max(1.0) / 2.0;
    let scale = (w.min(h) as f32 / (2.0 * radius)) * 0.95 * v.zoom;
    let (sy, cy) = v.yaw.to_radians().sin_cos();
    let (sp, cp) = v.pitch.to_radians().sin_cos();
    // obrót wokół z (pion), potem pochylenie wokół osi x ekranu
    let tf = |p: [f32; 3]| -> [f32; 3] {
        let (x, y, z) = (p[0] - c[0], p[1] - c[1], p[2] - c[2]);
        let (x1, y1) = (x * cy - y * sy, x * sy + y * cy);
        // y1 – w głąb (od patrzącego), z – w górę
        let (sy2, d) = (z * cp + y1 * sp, -y1 * cp + z * sp);
        [w as f32 / 2.0 + x1 * scale, h as f32 / 2.0 - sy2 * scale, d]
    };
    let scr: Vec<[f32; 3]> = mesh.pos.iter().map(|&p| tf(p)).collect();
    let (tw, th) = (tex.width() as f32, tex.height() as f32);
    let normal = |t: &[u32; 3]| -> [f32; 3] {
        let (a, b, c) = (mesh.pos[t[0] as usize], mesh.pos[t[1] as usize], mesh.pos[t[2] as usize]);
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let q = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [u[1] * q[2] - u[2] * q[1], u[2] * q[0] - u[0] * q[2], u[0] * q[1] - u[1] * q[0]];
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
        [n[0] / l, n[1] / l, n[2] / l]
    };
    for (textured, list) in [(true, &mesh.top), (false, &mesh.side)] {
        for t in list.iter() {
            let (a, b, cc) = (scr[t[0] as usize], scr[t[1] as usize], scr[t[2] as usize]);
            // tylne ściany pomijamy (rzut prostokątny: znak pola na ekranie)
            let area = (b[0] - a[0]) * (cc[1] - a[1]) - (b[1] - a[1]) * (cc[0] - a[0]);
            if area >= 0.0 {
                continue;
            }
            let n = normal(t);
            let shade = if textured { 1.0 } else { 0.45 + 0.55 * (n[0] * light[0] + n[1] * light[1] + n[2] * light[2]).max(0.0) };
            let x0 = a[0].min(b[0]).min(cc[0]).floor().max(0.0) as i32;
            let x1 = a[0].max(b[0]).max(cc[0]).ceil().min(w as f32 - 1.0) as i32;
            let y0 = a[1].min(b[1]).min(cc[1]).floor().max(0.0) as i32;
            let y1 = a[1].max(b[1]).max(cc[1]).ceil().min(h as f32 - 1.0) as i32;
            if x0 > x1 || y0 > y1 {
                continue;
            }
            let inv = 1.0 / area;
            let (ua, ub, uc) = (mesh.uv[t[0] as usize], mesh.uv[t[1] as usize], mesh.uv[t[2] as usize]);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((b[0] - fx) * (cc[1] - fy) - (b[1] - fy) * (cc[0] - fx)) * inv;
                    let w1 = ((cc[0] - fx) * (a[1] - fy) - (cc[1] - fy) * (a[0] - fx)) * inv;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                        continue;
                    }
                    let z = a[2] * w0 + b[2] * w1 + cc[2] * w2;
                    let k = y as usize * w + x as usize;
                    // trzecia współrzędna rośnie ku patrzącemu – zostaje najbliższy
                    if z <= depth[k] {
                        continue;
                    }
                    depth[k] = z;
                    let col = if textured {
                        let u = ua[0] * w0 + ub[0] * w1 + uc[0] * w2;
                        let vv = ua[1] * w0 + ub[1] * w1 + uc[1] * w2;
                        let p = tex.get_pixel(((u * tw) as u32).min(tex.width() - 1), ((vv * th) as u32).min(tex.height() - 1)).0;
                        [p[0], p[1], p[2], 255]
                    } else {
                        [(base[0] as f32 * shade) as u8, (base[1] as f32 * shade) as u8, (base[2] as f32 * shade) as u8, 255]
                    };
                    px[k] = col;
                }
            }
        }
    }
    px
}
