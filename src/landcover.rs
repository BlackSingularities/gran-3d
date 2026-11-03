//! Pokrycie terenu z OpenStreetMap (wody, lasy, piargi i skały, zarośla) wypiekane do maski rastrowej
//! sektora: R – woda, G – las, B – skały (255) / zarośla (110). Zapis: `land\9_x_y.png` w folderze danych.

use std::path::{Path, PathBuf};

use crate::geo::{lat_to_y, lon_to_x};
use crate::osm::{self, SectorId, SECTOR_Z};

/// Rozdzielczość maski sektora (~25 m na piksel w Karpatach).
pub const SIZE: u32 = 2048;

pub fn path(root: &Path, id: SectorId) -> PathBuf {
    root.join("land").join(format!("{}_{}_{}.png", SECTOR_Z, id.x, id.y))
}

pub fn exists(root: &Path, id: SectorId) -> bool {
    path(root, id).exists()
}

fn query(part: &crate::geo::BBox, full: &crate::geo::BBox) -> String {
    let qb = format!("{:.6},{:.6},{:.6},{:.6}", part.s, part.w, part.n, part.e);
    let bb = format!("{:.6},{:.6},{:.6},{:.6}", full.s, full.w, full.n, full.e);
    format!(
        "[out:json][timeout:300];(way[\"natural\"=\"water\"]({qb});relation[\"natural\"=\"water\"]({qb});way[\"waterway\"=\"riverbank\"]({qb});way[\"landuse\"=\"forest\"]({qb});relation[\"landuse\"=\"forest\"]({qb});way[\"natural\"=\"wood\"]({qb});relation[\"natural\"=\"wood\"]({qb});way[\"natural\"~\"^(scree|bare_rock|shingle)$\"]({qb});relation[\"natural\"~\"^(scree|bare_rock)$\"]({qb});way[\"natural\"=\"scrub\"]({qb});relation[\"natural\"=\"scrub\"]({qb}););out geom({bb});"
    )
}

/// Wypełnianie parzysto-nieparzyste jednego obiektu (z dziurami) liniami skanowania.
fn fill(img: &mut [u8], rings: &[Vec<(f64, f64)>], ch: usize, val: u8) {
    let s = SIZE as usize;
    let mut segs = Vec::new();
    let (mut min_y, mut max_y) = (f64::MAX, f64::MIN);
    for r in rings {
        for w in r.windows(2) {
            let (a, b) = (w[0], w[1]);
            if a.1 == b.1 {
                continue;
            }
            segs.push((a.0, a.1, b.0, b.1));
            min_y = min_y.min(a.1.min(b.1));
            max_y = max_y.max(a.1.max(b.1));
        }
    }
    if segs.is_empty() {
        return;
    }
    let y0 = min_y.floor().max(0.0) as usize;
    let y1 = (max_y.ceil() as isize).min(s as isize - 1);
    if y1 < 0 {
        return;
    }
    let mut xs: Vec<f64> = Vec::new();
    for y in y0..=y1 as usize {
        let cy = y as f64 + 0.5;
        xs.clear();
        for &(ax, ay, bx, by) in &segs {
            if (ay <= cy && by > cy) || (by <= cy && ay > cy) {
                xs.push(ax + (cy - ay) / (by - ay) * (bx - ax));
            }
        }
        xs.sort_by(|a, b| a.total_cmp(b));
        for k in (0..xs.len().saturating_sub(1)).step_by(2) {
            let xa = (xs[k] - 0.5).ceil().max(0.0) as isize;
            let xb = ((xs[k + 1] - 0.5).floor() as isize).min(s as isize - 1);
            for x in xa.max(0)..=xb {
                let i = (y * s + x as usize) * 3 + ch;
                if img[i] < val {
                    img[i] = val;
                }
            }
        }
    }
}

/// Pobranie (w ćwiartkach) i wypieczenie maski sektora. Blokujące.
pub fn fetch(id: SectorId) -> Result<Vec<u8>, String> {
    let full = id.bbox();
    let s = SIZE as f64;
    let n = (1u64 << SECTOR_Z) as f64;
    let to_px = |lon: f64, lat: f64| ((lon_to_x(lon) * n - id.x as f64) * s, (lat_to_y(lat) * n - id.y as f64) * s);
    let mut img = vec![0u8; (SIZE * SIZE * 3) as usize];
    let mut ok = 0;
    for j in 0..2 {
        for i in 0..2 {
            let part = crate::geo::BBox::new(
                full.w + (full.e - full.w) * i as f64 / 2.0,
                full.s + (full.n - full.s) * j as f64 / 2.0,
                full.w + (full.e - full.w) * (i + 1) as f64 / 2.0,
                full.s + (full.n - full.s) * (j + 1) as f64 / 2.0,
            );
            let v = match osm::overpass(&query(&part, &full), i + j) {
                Ok(v) => v,
                Err(e) => {
                    log::warn!("pokrycie terenu {}/{} ({i},{j}): {e}", id.x, id.y);
                    continue;
                }
            };
            ok += 1;
            let empty = Vec::new();
            for el in v["elements"].as_array().unwrap_or(&empty) {
                let t = &el["tags"];
                let tag = |k: &str| t[k].as_str().unwrap_or("");
                let nat = tag("natural");
                let (ch, val) = if nat == "water" || tag("waterway") == "riverbank" {
                    (0, 255)
                } else if tag("landuse") == "forest" || nat == "wood" {
                    (1, 255)
                } else if nat == "scrub" {
                    (2, 110)
                } else if matches!(nat, "scree" | "bare_rock" | "shingle") {
                    (2, 255)
                } else {
                    continue;
                };
                let ring_of = |g: &serde_json::Value| -> Vec<(f64, f64)> {
                    g.as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|p| Some(to_px(p["lon"].as_f64()?, p["lat"].as_f64()?)))
                                .collect()
                        })
                        .unwrap_or_default()
                };
                let mut rings = Vec::new();
                if el["type"] == "way" {
                    let mut r = ring_of(&el["geometry"]);
                    if r.len() > 2 {
                        if r.first() != r.last() {
                            r.push(r[0]);
                        }
                        rings.push(r);
                    }
                } else {
                    for m in el["members"].as_array().unwrap_or(&empty) {
                        if m["type"] == "way" {
                            let r = ring_of(&m["geometry"]);
                            if r.len() > 1 {
                                rings.push(r);
                            }
                        }
                    }
                }
                fill(&mut img, &rings, ch, val);
            }
        }
    }
    if ok == 0 {
        return Err("Overpass nie odpowiada".into());
    }
    Ok(img)
}

pub fn save(root: &Path, id: SectorId, rgb: &[u8]) -> Result<(), String> {
    let p = path(root, id);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let img = image::RgbImage::from_raw(SIZE, SIZE, rgb.to_vec()).ok_or("rozmiar maski")?;
    let tmp = p.with_extension("tmp.png");
    img.save(&tmp).map_err(|e| e.to_string())?;
    std::fs::rename(tmp, p).map_err(|e| e.to_string())
}

/// Mozaika 3×3 sektorów wokół `center`, każdy zmniejszony do `cell` px (RGBA).
/// Zwraca dane, bok i prostokąt w układzie świata (0..1): x0, y0, x1, y1.
pub fn mosaic(root: &Path, center: SectorId, cell: u32) -> Option<(Vec<u8>, u32, [f64; 4])> {
    let side = cell * 3;
    let mut out = vec![0u8; (side * side * 4) as usize];
    let mut any = false;
    let n = 1i64 << SECTOR_Z;
    for dy in -1i64..=1 {
        for dx in -1i64..=1 {
            let y = center.y as i64 + dy;
            if y < 0 || y >= n {
                continue;
            }
            let id = SectorId { x: (center.x as i64 + dx).rem_euclid(n) as u32, y: y as u32 };
            let Ok(img) = image::open(path(root, id)) else { continue };
            let img = image::imageops::resize(&img.to_rgb8(), cell, cell, image::imageops::FilterType::Triangle);
            any = true;
            let (ox, oy) = (((dx + 1) as u32) * cell, ((dy + 1) as u32) * cell);
            for (x, yy, p) in img.enumerate_pixels() {
                let i = (((oy + yy) * side + ox + x) * 4) as usize;
                out[i..i + 3].copy_from_slice(&p.0);
                out[i + 3] = 255;
            }
        }
    }
    if !any {
        return None;
    }
    let nf = n as f64;
    Some((
        out,
        side,
        [
            (center.x as f64 - 1.0) / nf,
            (center.y as f64 - 1.0) / nf,
            (center.x as f64 + 2.0) / nf,
            (center.y as f64 + 2.0) / nf,
        ],
    ))
}

/// Poziomy lustra wody na siatce o połowę rzadszej od mozaiki: każdy spójny zbiornik z maski
/// dostaje wysokość swojego brzegu (dolny kwartyl próbek DEM tuż za linią wody) – model terenu
/// w miejscu jezior ma dno albo szum zamiast płaskiej tafli. Poza wodą `no_water`.
pub fn water_levels(
    rgba: &[u8],
    side: u32,
    rect: [f64; 4],
    heights: &crate::heights::Heights,
    no_water: f32,
) -> (Vec<f32>, u32) {
    use crate::geo::{x_to_lon, y_to_lat};
    let w = side / 2;
    let n = w as usize;
    // woda w komórce: którykolwiek z 4 pikseli mozaiki
    let wet: Vec<bool> = (0..n * n)
        .map(|i| {
            let (x, y) = ((i % n) * 2, (i / n) * 2);
            [(0, 0), (1, 0), (0, 1), (1, 1)].iter().any(|(dx, dy)| rgba[(((y + dy) * side as usize) + x + dx) * 4] > 127)
        })
        .collect();
    let mut out = vec![no_water; n * n];
    let mut comp = vec![u32::MAX; n * n];
    let to_ll = |x: usize, y: usize| {
        let fx = rect[0] + (rect[2] - rect[0]) * (x as f64 + 0.5) / n as f64;
        let fy = rect[1] + (rect[3] - rect[1]) * (y as f64 + 0.5) / n as f64;
        (x_to_lon(fx), y_to_lat(fy))
    };
    let mut stack = Vec::new();
    let mut cells = Vec::new();
    let mut shore = Vec::new();
    for start in 0..n * n {
        if !wet[start] || comp[start] != u32::MAX {
            continue;
        }
        // zbiornik: wypełnianie 4-sąsiedztwem, brzeg = suche komórki obok
        cells.clear();
        shore.clear();
        comp[start] = start as u32;
        stack.push(start);
        while let Some(i) = stack.pop() {
            cells.push(i);
            let (x, y) = (i % n, i / n);
            for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
                let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                if nx < 0 || ny < 0 || nx >= n as i64 || ny >= n as i64 {
                    continue;
                }
                let j = ny as usize * n + nx as usize;
                if wet[j] {
                    if comp[j] == u32::MAX {
                        comp[j] = start as u32;
                        stack.push(j);
                    }
                } else {
                    shore.push(j);
                }
            }
        }
        shore.sort_unstable();
        shore.dedup();
        // najwyżej ~300 próbek brzegu na zbiornik
        let step = (shore.len() / 300).max(1);
        let mut hs: Vec<f32> = shore
            .iter()
            .step_by(step)
            .filter_map(|&j| {
                let (lon, lat) = to_ll(j % n, j / n);
                heights.sample(lon, lat)
            })
            .collect();
        if hs.is_empty() {
            continue;
        }
        hs.sort_by(|a, b| a.total_cmp(b));
        let level = hs[hs.len() / 4];
        for &i in &cells {
            out[i] = level;
        }
    }
    (out, w)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_square_with_hole() {
        let mut img = vec![0u8; (SIZE * SIZE * 3) as usize];
        let outer = vec![(10.0, 10.0), (110.0, 10.0), (110.0, 110.0), (10.0, 110.0), (10.0, 10.0)];
        let hole = vec![(40.0, 40.0), (80.0, 40.0), (80.0, 80.0), (40.0, 80.0), (40.0, 40.0)];
        fill(&mut img, &[outer, hole], 1, 255);
        let at = |x: usize, y: usize| img[(y * SIZE as usize + x) * 3 + 1];
        assert_eq!(at(20, 20), 255);
        assert_eq!(at(60, 60), 0);
        assert_eq!(at(200, 200), 0);
    }
}
