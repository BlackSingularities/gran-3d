//! Izolinie (warstwice i przecięcia z płaszczyzną) metodą marching squares, połączone w łamane.

use std::collections::HashMap;

/// Pole wartości na regularnej siatce: `v[j·nx + i]`, NaN – poza obszarem (linie tam się urywają).
pub struct Field<'a> {
    pub nx: usize,
    pub ny: usize,
    pub v: &'a [f64],
}

/// Łamane izolinii `level` we współrzędnych siatki (i, j) – ułamkowych.
pub fn lines(f: &Field, level: f64) -> Vec<Vec<(f64, f64)>> {
    // odcinki z kluczami krawędzi (krawędź pozioma: (i, j, 0), pionowa: (i, j, 1))
    type Key = (usize, usize, u8);
    let mut segs: Vec<(Key, Key)> = Vec::new();
    let mut point: HashMap<Key, (f64, f64)> = HashMap::new();
    let val = |i: usize, j: usize| f.v[j * f.nx + i];
    let mut cross = |a: (usize, usize), b: (usize, usize), key: Key| -> Key {
        point.entry(key).or_insert_with(|| {
            let (va, vb) = (val(a.0, a.1), val(b.0, b.1));
            let t = ((level - va) / (vb - va)).clamp(0.0, 1.0);
            (a.0 as f64 + (b.0 as f64 - a.0 as f64) * t, a.1 as f64 + (b.1 as f64 - a.1 as f64) * t)
        });
        key
    };
    for j in 0..f.ny.saturating_sub(1) {
        for i in 0..f.nx.saturating_sub(1) {
            let (a, b, c, d) = (val(i, j), val(i + 1, j), val(i + 1, j + 1), val(i, j + 1));
            if !(a.is_finite() && b.is_finite() && c.is_finite() && d.is_finite()) {
                continue;
            }
            // poziom dokładnie w węźle przesuwamy minimalnie, żeby nie było przypadków zdegenerowanych
            let up = |v: f64| v > level || (v == level);
            let idx = (up(a) as u8) | (up(b) as u8) << 1 | (up(c) as u8) << 2 | (up(d) as u8) << 3;
            if idx == 0 || idx == 15 {
                continue;
            }
            let top = (i, j, 0u8);
            let right = (i + 1, j, 1u8);
            let bottom = (i, j + 1, 0u8);
            let left = (i, j, 1u8);
            let mut e = |k: Key| match k.2 {
                0 => cross((k.0, k.1), (k.0 + 1, k.1), k),
                _ => cross((k.0, k.1), (k.0, k.1 + 1), k),
            };
            let pairs: &[(Key, Key)] = match idx {
                1 | 14 => &[(left, top)],
                2 | 13 => &[(top, right)],
                3 | 12 => &[(left, right)],
                4 | 11 => &[(right, bottom)],
                6 | 9 => &[(top, bottom)],
                7 | 8 => &[(left, bottom)],
                5 | 10 => {
                    // siodło: rozstrzygnięcie średnią z narożników
                    let mid = (a + b + c + d) / 4.0;
                    if (mid >= level) == (idx == 5) {
                        &[(left, bottom), (top, right)]
                    } else {
                        &[(left, top), (right, bottom)]
                    }
                }
                _ => &[],
            };
            for &(p, q) in pairs {
                segs.push((e(p), e(q)));
            }
        }
    }
    join(segs, &point)
}

/// Łączy odcinki o wspólnych końcach w łamane (zamknięte mają pierwszy punkt na końcu).
fn join<K: std::hash::Hash + Eq + Copy>(segs: Vec<(K, K)>, point: &HashMap<K, (f64, f64)>) -> Vec<Vec<(f64, f64)>> {
    let mut adj: HashMap<K, Vec<usize>> = HashMap::new();
    for (k, (a, b)) in segs.iter().enumerate() {
        adj.entry(*a).or_default().push(k);
        adj.entry(*b).or_default().push(k);
    }
    let mut used = vec![false; segs.len()];
    let mut out = Vec::new();
    let walk = |start: K, first: usize, used: &mut Vec<bool>| -> Vec<K> {
        let mut chain = vec![start];
        let mut cur = start;
        let mut seg = Some(first);
        while let Some(s) = seg {
            used[s] = true;
            let (a, b) = segs[s];
            let next = if a == cur { b } else { a };
            chain.push(next);
            cur = next;
            seg = adj.get(&cur).and_then(|v| v.iter().copied().find(|&k| !used[k]));
        }
        chain
    };
    for k in 0..segs.len() {
        if used[k] {
            continue;
        }
        let (a, _) = segs[k];
        let mut fwd = walk(a, k, &mut used);
        // dalej w drugą stronę od początku
        if let Some(back) = adj.get(&a).and_then(|v| v.iter().copied().find(|&s| !used[s])) {
            let mut b = walk(a, back, &mut used);
            b.reverse();
            b.pop();
            b.extend(fwd);
            fwd = b;
        }
        out.push(fwd.iter().filter_map(|k| point.get(k).copied()).collect());
    }
    out
}

/// Długość łamanej.
pub fn length(l: &[(f64, f64)]) -> f64 {
    l.windows(2).map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt()).sum()
}

/// Upraszczanie łamanej (Douglas–Peucker) z tolerancją `eps`.
pub fn simplify(l: &[(f64, f64)], eps: f64) -> Vec<(f64, f64)> {
    if l.len() < 3 {
        return l.to_vec();
    }
    let mut keep = vec![false; l.len()];
    keep[0] = true;
    keep[l.len() - 1] = true;
    let mut stack = vec![(0usize, l.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (pa, pb) = (l[a], l[b]);
        let (dx, dy) = (pb.0 - pa.0, pb.1 - pa.1);
        let len = (dx * dx + dy * dy).sqrt();
        let mut best = (0.0, a);
        for k in a + 1..b {
            let d = if len > 0.0 { ((l[k].0 - pa.0) * dy - (l[k].1 - pa.1) * dx).abs() / len } else { ((l[k].0 - pa.0).powi(2) + (l[k].1 - pa.1).powi(2)).sqrt() };
            if d > best.0 {
                best = (d, k);
            }
        }
        if best.0 > eps {
            keep[best.1] = true;
            stack.push((a, best.1));
            stack.push((best.1, b));
        }
    }
    l.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_contour_is_closed() {
        let n = 21;
        let v: Vec<f64> = (0..n * n)
            .map(|k| {
                let (i, j) = ((k % n) as f64 - 10.0, (k / n) as f64 - 10.0);
                100.0 - (i * i + j * j).sqrt()
            })
            .collect();
        let f = Field { nx: n, ny: n, v: &v };
        let ls = lines(&f, 95.0);
        assert_eq!(ls.len(), 1);
        let l = &ls[0];
        assert!(l.len() > 20);
        let (a, b) = (l[0], l[l.len() - 1]);
        assert!((a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9, "linia nie jest zamknięta");
        assert!((length(l) - 2.0 * std::f64::consts::PI * 5.0).abs() < 1.5);
        assert!(simplify(l, 0.2).len() < l.len());
    }
}
