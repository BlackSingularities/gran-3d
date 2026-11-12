//! Analizy na lokalnej siatce wysokości: pole widoczności, linia wzroku, trasa przez teren (A*),
//! cienie rzucane przez rzeźbę i statystyki wysokości względnej.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use rayon::prelude::*;

use crate::geo::EARTH_R;
use crate::heights::Grid;

/// Maska widoczności: 0 – niewidoczne, 1..255 – widoczne (zapas kąta, do cieniowania krawędzi).
pub struct Viewshed {
    pub mask: Vec<u8>,
    pub count: usize,
    pub area_km2: f64,
    pub observer: f32,
}

/// Widoczność promieniami do każdej komórki obwodu (odmiana XDraw) z krzywizną Ziemi i refrakcją.
pub fn viewshed(g: &Grid, ox: f64, oy: f64, eye: f32, target: f32) -> Viewshed {
    let (w, h) = (g.w, g.h);
    let oz = g.sample(ox, oy) + eye;
    let k = (1.0 - 0.13) / (2.0 * EARTH_R as f32);
    let mpp = g.mpp;
    let ray = |tx: f64, ty: f64, out: &mut Vec<(u32, u8)>| {
        let (dx, dy) = (tx - ox, ty - oy);
        let steps = dx.abs().max(dy.abs()).ceil();
        if steps < 1.0 {
            return;
        }
        let (sx, sy) = (dx / steps, dy / steps);
        let step_m = (sx.hypot(sy) as f32) * mpp;
        let mut max_tan = f32::NEG_INFINITY;
        for s in 1..=steps as usize {
            let (x, y) = (ox + sx * s as f64, oy + sy * s as f64);
            let (ix, iy) = (x.round() as isize, y.round() as isize);
            if ix < 0 || iy < 0 || ix >= w as isize || iy >= h as isize {
                break;
            }
            let d = s as f32 * step_m;
            let idx = iy as usize * w + ix as usize;
            let hh = g.data[idx] - d * d * k;
            let tan_t = (hh + target - oz) / d;
            if tan_t >= max_tan {
                let margin = (1.0 + (tan_t - max_tan).min(1e6) * d * 2.0).min(254.0);
                out.push((idx as u32, margin.max(1.0) as u8));
            }
            let tan_g = (hh - oz) / d;
            if tan_g > max_tan {
                max_tan = tan_g;
            }
        }
    };
    let mut border: Vec<(f64, f64)> = Vec::with_capacity(2 * (w + h));
    for x in 0..w {
        border.push((x as f64, 0.0));
        border.push((x as f64, (h - 1) as f64));
    }
    for y in 0..h {
        border.push((0.0, y as f64));
        border.push(((w - 1) as f64, y as f64));
    }
    let hits: Vec<Vec<(u32, u8)>> = border
        .par_chunks(64)
        .map(|c| {
            let mut out = Vec::new();
            for &(x, y) in c {
                ray(x, y, &mut out);
            }
            out
        })
        .collect();
    let mut mask = vec![0u8; w * h];
    for list in hits {
        for (i, m) in list {
            let v = &mut mask[i as usize];
            if *v < m {
                *v = m;
            }
        }
    }
    let (cx, cy) = (ox.round() as isize, oy.round() as isize);
    if cx >= 0 && cy >= 0 && (cx as usize) < w && (cy as usize) < h {
        mask[cy as usize * w + cx as usize] = 255;
    }
    let count = mask.iter().filter(|&&v| v > 0).count();
    Viewshed {
        area_km2: count as f64 * (mpp as f64).powi(2) / 1e6,
        mask,
        count,
        observer: oz,
    }
}

/// Linia wzroku między punktami siatki (oczy `eye` m nad terenem po obu stronach).
/// Zwraca `None` gdy widać, albo ułamek odległości do pierwszej przeszkody.
pub fn line_of_sight(g: &Grid, a: (f64, f64), b: (f64, f64), eye: f32) -> Option<f64> {
    let ha = g.sample(a.0, a.1) + eye;
    let hb = g.sample(b.0, b.1) + eye;
    let len_px = (b.0 - a.0).hypot(b.1 - a.1);
    let n = (len_px * 2.0).ceil().max(2.0) as usize;
    let total_m = len_px * g.mpp as f64;
    for i in 1..n {
        let f = i as f64 / n as f64;
        let ground = g.sample(a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f) as f64;
        let d = f * total_m;
        // krzywizna: spadek względem cięciwy
        let drop = d * (total_m - d) / (2.0 * EARTH_R) * (1.0 - 0.13);
        let line = ha as f64 + (hb - ha) as f64 * f - drop;
        if ground > line {
            return Some(f);
        }
    }
    None
}

/// Czas marszu wg funkcji Toblera (s) dla odcinka o długości poziomej `d` i przewyższeniu `dh`.
fn tobler(d: f32, dh: f32) -> f32 {
    let v = 6.0 * (-3.5 * (dh / d + 0.05).abs()).exp();
    d.hypot(dh) / 1000.0 / v * 3600.0
}

#[derive(Copy, Clone, PartialEq)]
struct Item(f32, u32);
impl Eq for Item {}
impl Ord for Item {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.partial_cmp(&self.0).unwrap_or(Ordering::Equal)
    }
}
impl PartialOrd for Item {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// Trasa przez teren: A* po siatce (16 kierunków), funkcja Toblera, limit nachylenia.
pub fn terrain_path(g: &Grid, a: (f64, f64), b: (f64, f64), max_slope_deg: f32) -> Option<Vec<(f64, f64)>> {
    let (w, h) = (g.w, g.h);
    let n = w * h;
    let mut cost = vec![f32::INFINITY; n];
    let mut from = vec![u32::MAX; n];
    let mut closed = vec![false; n];
    let max_tan = max_slope_deg.to_radians().tan();
    let idx = |p: (f64, f64)| {
        (p.1.round().clamp(0.0, (h - 1) as f64) as usize) * w
            + p.0.round().clamp(0.0, (w - 1) as f64) as usize
    };
    let (s, t) = (idx(a), idx(b));
    let (tx, ty) = ((t % w) as f32, (t / w) as f32);
    let vmax = 6.0 * (-3.5f32 * 0.05).exp() / 3.6;
    let mpp = g.mpp;
    let hcost = |i: usize| ((i % w) as f32 - tx).hypot((i / w) as f32 - ty) * mpp / vmax;
    let mut heap = BinaryHeap::new();
    cost[s] = 0.0;
    heap.push(Item(hcost(s), s as u32));
    const DX: [i32; 16] = [1, -1, 0, 0, 1, 1, -1, -1, 2, 2, -2, -2, 1, 1, -1, -1];
    const DY: [i32; 16] = [0, 0, 1, -1, 1, -1, 1, -1, 1, -1, 1, -1, 2, -2, 2, -2];
    let mut found = false;
    let mut iter = 0;
    while let Some(Item(_, u)) = heap.pop() {
        let u = u as usize;
        if closed[u] {
            continue;
        }
        closed[u] = true;
        if u == t {
            found = true;
            break;
        }
        iter += 1;
        if iter > 6_000_000 {
            break;
        }
        let (ux, uy) = ((u % w) as i32, (u / w) as i32);
        let hu = g.data[u];
        for k in 0..16 {
            let (vx, vy) = (ux + DX[k], uy + DY[k]);
            if vx < 0 || vy < 0 || vx >= w as i32 || vy >= h as i32 {
                continue;
            }
            let v = vy as usize * w + vx as usize;
            if closed[v] {
                continue;
            }
            let d = (DX[k] as f32).hypot(DY[k] as f32) * mpp;
            let dh = g.data[v] - hu;
            if k >= 8 {
                let mid = g.sample(ux as f64 + DX[k] as f64 / 2.0, uy as f64 + DY[k] as f64 / 2.0);
                if (mid - hu).abs() / (d / 2.0) > max_tan || (g.data[v] - mid).abs() / (d / 2.0) > max_tan {
                    continue;
                }
            } else if dh.abs() / d > max_tan {
                continue;
            }
            let nc = cost[u] + tobler(d, dh);
            if nc < cost[v] {
                cost[v] = nc;
                from[v] = u as u32;
                heap.push(Item(nc + hcost(v), v as u32));
            }
        }
    }
    if !found {
        return None;
    }
    let mut path = Vec::new();
    let mut v = t;
    while v != u32::MAX as usize {
        path.push(((v % w) as f64, (v / w) as f64));
        v = from[v] as usize;
    }
    path.reverse();
    // wygładzenie zygzaków siatki
    for _ in 0..3 {
        if path.len() < 3 {
            break;
        }
        let mut q = vec![path[0]];
        for i in 1..path.len() - 1 {
            q.push((
                (path[i - 1].0 + 2.0 * path[i].0 + path[i + 1].0) / 4.0,
                (path[i - 1].1 + 2.0 * path[i].1 + path[i + 1].1) / 4.0,
            ));
        }
        q.push(*path.last().unwrap());
        path = q;
    }
    if let Some(f) = path.first_mut() {
        *f = a;
    }
    if let Some(l) = path.last_mut() {
        *l = b;
    }
    Some(path)
}

/// Cienie rzucane przez rzeźbę (Słońce w kierunku `sun` = [wschód, góra, południe]).
/// Maska 0 – cień, 255 – słońce (miękka krawędź z zapasu kąta).
pub fn shadows(g: &Grid, sun: [f32; 3]) -> Vec<u8> {
    let (w, h) = (g.w, g.h);
    if sun[1] <= 0.0 {
        return vec![0; w * h];
    }
    let horiz = (sun[0] * sun[0] + sun[2] * sun[2]).sqrt().max(1e-6);
    let (dx, dy) = (sun[0] / horiz, sun[2] / horiz);
    let tan_sun = sun[1] / horiz;
    let mpp = g.mpp;
    let maxh = g.data.iter().cloned().fold(f32::MIN, f32::max);
    let mut out = vec![255u8; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, o) in row.iter_mut().enumerate() {
            let h0 = g.data[y * w + x] + 0.5;
            let mut step = 1.0f32;
            let mut s = 1.0f32;
            let mut min_gap = f32::INFINITY;
            loop {
                let (px, py) = (x as f32 + dx * s, y as f32 + dy * s);
                if px < 0.0 || py < 0.0 || px > (w - 1) as f32 || py > (h - 1) as f32 {
                    break;
                }
                let ray = h0 + s * mpp * tan_sun;
                if ray > maxh + 5.0 {
                    break;
                }
                let gh = g.sample(px as f64, py as f64);
                let gap = (ray - gh) / (s * mpp);
                min_gap = min_gap.min(gap);
                if gap < -0.02 {
                    break;
                }
                s += step;
                step = (step * 1.04).min(8.0);
            }
            *o = if min_gap.is_infinite() {
                255
            } else {
                (((min_gap + 0.02) / 0.06).clamp(0.0, 1.0) * 255.0) as u8
            };
        }
    });
    out
}

/// Nasłonecznienie: godziny bezpośredniego słońca w ciągu dnia z cieniami rzucanymi przez rzeźbę.
/// `suns` – chwile dnia ze Słońcem nad horyzontem: wektor do Słońca i czas, który reprezentuje (h).
/// Wynik: godziny / 24 jako 0..255 (ta sama tekstura co cienie).
pub fn insolation(g: &Grid, suns: &[([f32; 3], f32)]) -> Vec<u8> {
    let mut acc = vec![0f32; g.w * g.h];
    for (sun, dt) in suns {
        let m = shadows(g, *sun);
        acc.par_iter_mut().zip(m.par_iter()).for_each(|(a, &v)| *a += v as f32 / 255.0 * dt);
    }
    acc.iter().map(|h| (h / 24.0 * 255.0).round().clamp(0.0, 255.0) as u8).collect()
}

/// Czas dojścia pieszo z punktu (`sx`, `sy`) po siatce wysokości (Dijkstra, 16 kierunków, funkcja Toblera).
/// `on_trail` – komórki szlaków i dróg (pełne tempo); poza nimi tempo jak w terenie bez ścieżki (×0,6)
/// i stoki ponad `max_slope_deg` nieprzejezdne. `pace` – mnożnik czasu (1 – zwykłe tempo).
/// Wynik w sekundach, `f32::INFINITY` – nieosiągalne w `max_s`.
pub fn travel_time(g: &Grid, sx: f64, sy: f64, on_trail: &[bool], max_slope_deg: f32, pace: f32, max_s: f32) -> Vec<f32> {
    let (w, h) = (g.w, g.h);
    let mut cost = vec![f32::INFINITY; w * h];
    let (ix, iy) = (sx.round(), sy.round());
    if ix < 0.0 || iy < 0.0 || ix >= w as f64 || iy >= h as f64 {
        return cost;
    }
    let s = iy as usize * w + ix as usize;
    let max_tan = max_slope_deg.to_radians().tan();
    let mpp = g.mpp;
    let mut heap = BinaryHeap::new();
    cost[s] = 0.0;
    heap.push(Item(0.0, s as u32));
    const DX: [i32; 16] = [1, -1, 0, 0, 1, 1, -1, -1, 2, 2, -2, -2, 1, 1, -1, -1];
    const DY: [i32; 16] = [0, 0, 1, -1, 1, -1, 1, -1, 1, -1, 1, -1, 2, -2, 2, -2];
    while let Some(Item(c, u)) = heap.pop() {
        let u = u as usize;
        if c > cost[u] {
            continue;
        }
        let (ux, uy) = ((u % w) as i32, (u / w) as i32);
        let hu = g.data[u];
        for k in 0..16 {
            let (vx, vy) = (ux + DX[k], uy + DY[k]);
            if vx < 0 || vy < 0 || vx >= w as i32 || vy >= h as i32 {
                continue;
            }
            let v = vy as usize * w + vx as usize;
            let d = (DX[k] as f32).hypot(DY[k] as f32) * mpp;
            let dh = g.data[v] - hu;
            // szlak: obie komórki na szlaku (skok o 2 tylko przez środek na szlaku)
            let mid = if k >= 8 { Some(((uy + DY[k] / 2) as usize) * w + (ux + DX[k] / 2) as usize) } else { None };
            let trail = on_trail[u] && on_trail[v] && mid.is_none_or(|m| on_trail[m]);
            if !trail && dh.abs() / d > max_tan {
                continue;
            }
            let f = if trail { 1.0 } else { 1.0 / 0.6 };
            let nc = c + tobler(d, dh) * f * pace;
            if nc < cost[v] && nc <= max_s {
                cost[v] = nc;
                heap.push(Item(nc, v as u32));
            }
        }
    }
    cost
}

/// Odsetek terenu wyżej i niżej od poziomu `ref_e` w promieniu `r_px` pikseli od punktu.
pub fn relative_stats(g: &Grid, cx: f64, cy: f64, ref_e: f32, r_px: f64) -> (f64, f64) {
    let (mut above, mut below, mut total) = (0usize, 0usize, 0usize);
    let r2 = r_px * r_px;
    let (x0, x1) = ((cx - r_px).max(0.0) as usize, (cx + r_px).min(g.w as f64 - 1.0) as usize);
    let (y0, y1) = ((cy - r_px).max(0.0) as usize, (cy + r_px).min(g.h as f64 - 1.0) as usize);
    for y in (y0..=y1).step_by(2) {
        for x in (x0..=x1).step_by(2) {
            if (x as f64 - cx).powi(2) + (y as f64 - cy).powi(2) > r2 {
                continue;
            }
            total += 1;
            let e = g.data[y * g.w + x];
            if e > ref_e + 1.0 {
                above += 1;
            } else if e < ref_e - 1.0 {
                below += 1;
            }
        }
    }
    let t = total.max(1) as f64;
    (above as f64 / t, below as f64 / t)
}

/// Gatunek drzewa rysowanego w lesie (numer = rodzaj bryły w `trees.wgsl`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TreeSpecies {
    Spruce = 0,
    Beech = 1,
    DwarfPine = 2,
    Pine = 3,
    Larch = 4,
    Birch = 5,
    Umbrella = 6,
}

impl TreeSpecies {
    /// Typowa wysokość i promień korony (m).
    pub fn size(self) -> (f32, f32) {
        match self {
            TreeSpecies::Spruce => (24.0, 3.5),
            TreeSpecies::Beech => (20.0, 5.2),
            // płat zarośli, nie pojedynczy krzaczek: szeroki, żeby sąsiednie zlewały się w kępy
            TreeSpecies::DwarfPine => (2.6, 5.5),
            TreeSpecies::Pine => (21.0, 3.8),
            TreeSpecies::Larch => (23.0, 3.1),
            TreeSpecies::Birch => (16.0, 2.9),
            TreeSpecies::Umbrella => (14.0, 6.2),
        }
    }
}

/// Przybliżona górna granica lasu (m n.p.m.) na danej szerokości geograficznej: około 3900 m
/// w strefie międzyzwrotnikowej, 2100 m w Alpach, 1900 m w Karpatach, kilkaset metrów w Skandynawii.
pub fn treeline(lat: f64) -> f64 {
    (3900.0 - 95.0 * (lat.abs() - 28.0).max(0.0)).clamp(250.0, 3900.0)
}

/// Gatunek drzewa w punkcie lasu. `forest` – rodzaj lasu z OSM, `scrub` – zarośla (kosodrzewina),
/// `ele` – wysokość terenu, `lat` – szerokość geograficzna; `pick` i `roll` – losowania 0..1
/// (pierwsze wspólne dla kępy drzew, drugie osobne dla drzewa).
pub fn tree_species(forest: crate::landcover::Forest, scrub: bool, ele: f64, lat: f64, pick: f64, roll: f64) -> TreeSpecies {
    use crate::landcover::Forest;
    let line = treeline(lat);
    if scrub || ele > line {
        return TreeSpecies::DwarfPine;
    }
    // 0 – niziny, 1 – górna granica lasu
    let zone = (ele / line).clamp(0.0, 1.0);
    let tropics = lat.abs() < 23.5;
    let boreal = lat.abs() > 57.0;
    let needle = |p: f64| {
        if zone > 0.72 {
            // regiel górny: świerki, przy granicy lasu modrzewie
            if p < 0.78 { TreeSpecies::Spruce } else { TreeSpecies::Larch }
        } else if zone < 0.3 {
            // niziny i pogórza: bory sosnowe
            if p < 0.65 { TreeSpecies::Pine } else { TreeSpecies::Spruce }
        } else if p < 0.72 {
            TreeSpecies::Spruce
        } else if p < 0.86 {
            TreeSpecies::Pine
        } else {
            TreeSpecies::Larch
        }
    };
    let birch_share = if boreal { 0.7 } else { 0.22 };
    let broad = |p: f64| {
        if tropics && zone < 0.6 {
            if p < 0.55 { TreeSpecies::Umbrella } else { TreeSpecies::Beech }
        } else if p < birch_share {
            TreeSpecies::Birch
        } else {
            TreeSpecies::Beech
        }
    };
    // udział drzew liściastych, gdy OSM nie podaje rodzaju: maleje z wysokością; w tropikach duży, w tajdze mały
    let broad_share = if tropics {
        0.9
    } else if boreal {
        0.25
    } else {
        (0.72 - zone * 1.05).clamp(0.04, 0.62)
    };
    match forest {
        Forest::Needle => needle(pick),
        Forest::Broad => broad(pick),
        Forest::Mixed => {
            if roll < 0.5 { needle(pick) } else { broad(pick) }
        }
        Forest::Unknown => {
            if pick < broad_share { broad(roll) } else { needle(roll) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cone() -> Grid {
        let (w, h) = (101, 101);
        let mut data = vec![0f32; w * h];
        for y in 0..h {
            for x in 0..w {
                let d = ((x as f32 - 50.0).hypot(y as f32 - 50.0)) * 30.0;
                data[y * w + x] = (1000.0 - d).max(0.0);
            }
        }
        Grid {
            w,
            h,
            z: 12,
            px0: 0.0,
            py0: 0.0,
            mpp: 30.0,
            data,
        }
    }

    #[test]
    fn cone_hides_far_side() {
        let g = cone();
        let v = viewshed(&g, 10.0, 50.0, 1.7, 0.0);
        assert!(v.mask[50 * 101 + 20] > 0);
        assert_eq!(v.mask[50 * 101 + 90], 0);
        assert!(line_of_sight(&g, (10.0, 50.0), (90.0, 50.0), 1.7).is_some());
        assert!(line_of_sight(&g, (10.0, 50.0), (10.0, 90.0), 1.7).is_none());
    }

    #[test]
    fn path_goes_around_steep_cone() {
        let g = cone();
        let p = terrain_path(&g, (5.0, 50.0), (95.0, 50.0), 30.0).unwrap();
        assert!(p.len() > 10);
    }

    #[test]
    fn low_sun_casts_shadow_behind_cone() {
        let g = cone();
        // Słońce nisko na wschodzie: cień na zachód od szczytu
        let s = shadows(&g, [0.97, 0.24, 0.0]);
        assert!(s[50 * 101 + 10] < 128, "{}", s[50 * 101 + 10]);
        assert!(s[50 * 101 + 80] > 128);
    }

    #[test]
    fn insolation_sums_hours_on_flat_ground() {
        let mut g = cone();
        g.data.iter_mut().for_each(|v| *v = 0.0);
        let suns = [([0.0, 1.0, 0.0], 6.0), ([0.7, 0.7, 0.0], 6.0)];
        let m = insolation(&g, &suns);
        let hours = m[50 * 101 + 50] as f32 / 255.0 * 24.0;
        assert!((hours - 12.0).abs() < 0.2, "{hours}");
    }

    #[test]
    fn travel_time_grows_with_distance_and_trails_are_faster() {
        let mut g = cone();
        g.data.iter_mut().for_each(|v| *v = 500.0);
        let off = vec![false; g.w * g.h];
        let t = travel_time(&g, 50.0, 50.0, &off, 40.0, 1.0, 1e9);
        assert_eq!(t[50 * 101 + 50], 0.0);
        assert!(t[50 * 101 + 60] < t[50 * 101 + 80]);
        // 30 px × 30 m = 900 m po płaskim bez ścieżki: ok. 900 m / (5 km/h × 0,6) ≈ 1080 s
        assert!((t[50 * 101 + 80] - 1080.0).abs() < 120.0, "{}", t[50 * 101 + 80]);
        let on = vec![true; g.w * g.h];
        let t2 = travel_time(&g, 50.0, 50.0, &on, 40.0, 1.0, 1e9);
        assert!(t2[50 * 101 + 80] < t[50 * 101 + 80] * 0.7);
        // limit czasu: dalej nieosiągalne
        let t3 = travel_time(&g, 50.0, 50.0, &off, 40.0, 1.0, 300.0);
        assert!(t3[50 * 101 + 95].is_infinite());
    }
}
