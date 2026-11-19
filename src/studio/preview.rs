//! Podgląd modelu w oknie Studio: prosty rasteryzator programowy (rzut prostokątny, bufor głębi,
//! tekstura terenu, materiały z ustawień – kolor, szorstkość i połysk, warstwy skalne na ścianach).
//! Działa w wątku interfejsu na zmniejszonej bryle.

use image::RgbaImage;

use super::mesh::{Mat, Mesh, ModelOpts};

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

fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
    [v[0] / l, v[1] / l, v[2] / l]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Obraz `w`×`h` z bryły (milimetry), tekstury terenu i materiałów.
#[allow(clippy::too_many_arguments)]
pub fn render(mesh: &Mesh, tex: &RgbaImage, opts: &ModelOpts, strata: Option<&RgbaImage>, light: [f32; 3], v: &View, w: usize, h: usize) -> Vec<[u8; 4]> {
    // tło: delikatny pionowy gradient
    let mut px: Vec<[u8; 4]> = (0..w * h)
        .map(|k| {
            let t = (k / w.max(1)) as f32 / h.max(1) as f32;
            let c = |a: f32, b: f32| (a + (b - a) * t) as u8;
            [c(34.0, 18.0), c(38.0, 21.0), c(46.0, 26.0), 255]
        })
        .collect();
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
    // kierunek do patrzącego w układzie modelu i wektor połówkowy (połysk)
    let eye = norm([-cp * sy, -cp * cy, sp]);
    let l = norm(light);
    let half = norm([l[0] + eye[0], l[1] + eye[1], l[2] + eye[2]]);
    let (tw, th) = (tex.width() as f32, tex.height() as f32);
    let groups = mesh.groups(opts);
    for (mat, list) in &groups {
        let cfg = *opts.mat(*mat);
        let kind = match mat {
            Mat::Walls => 1,
            Mat::Bottom => 2,
            _ => 0,
        };
        let textured = kind == 0 && cfg.texture;
        let strata_tex = if kind == 1 { strata } else { None };
        let gloss = (1.0 - cfg.roughness).clamp(0.0, 1.0);
        let shininess = 6.0 + gloss * gloss * 180.0;
        for t in list.iter() {
            let (a, b, cc) = (scr[t[0] as usize], scr[t[1] as usize], scr[t[2] as usize]);
            // tylne ściany pomijamy (rzut prostokątny: znak pola na ekranie)
            let area = (b[0] - a[0]) * (cc[1] - a[1]) - (b[1] - a[1]) * (cc[0] - a[0]);
            if area >= 0.0 {
                continue;
            }
            let n = mesh.normal(t);
            // tekstura terenu ma już cieniowanie – kolory jednolite cieniujemy światłem
            let diffuse = if textured { 1.0 } else { 0.42 + 0.58 * dot(n, l).max(0.0) };
            let spec = if gloss > 0.05 { gloss * gloss * dot(n, half).max(0.0).powf(shininess) * 255.0 } else { 0.0 };
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
                    let u = ua[0] * w0 + ub[0] * w1 + uc[0] * w2;
                    let vv = ua[1] * w0 + ub[1] * w1 + uc[1] * w2;
                    let base: [f32; 3] = if textured {
                        let p = tex.get_pixel(((u * tw) as u32).min(tex.width() - 1), ((vv * th) as u32).min(tex.height() - 1)).0;
                        [0, 1, 2].map(|i| p[i] as f32 * cfg.color[i] as f32 / 255.0)
                    } else if let Some(s) = strata_tex {
                        let p = s.get_pixel(0, ((vv.clamp(0.0, 1.0) * s.height() as f32) as u32).min(s.height() - 1)).0;
                        [0, 1, 2].map(|i| p[i] as f32 * cfg.color[i] as f32 / 255.0)
                    } else {
                        cfg.color.map(|v| v as f32)
                    };
                    // połysk metalu w kolorze materiału, pozostałych – biały
                    let sc = [0, 1, 2].map(|i| spec * (1.0 - cfg.metallic) + spec * cfg.metallic * base[i] / 255.0);
                    let col = [0, 1, 2].map(|i| (base[i] * diffuse * (1.0 - cfg.metallic * 0.5) + sc[i]).clamp(0.0, 255.0) as u8);
                    px[k] = [col[0], col[1], col[2], 255];
                }
            }
        }
    }
    px
}
