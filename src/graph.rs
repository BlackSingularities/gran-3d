//! Graf szlaków złożony z sektorów OSM: przyklejanie punktów do sieci, Dijkstra z różnymi kosztami
//! i zestaw wariantów (najszybsza, najkrótsza, najmniej podejść, najłatwiejsza, objazdy).

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use crate::geo::haversine;
use crate::metrics::{climb, sac_factor, walk_time, TrailColor, Track, TrackStats};
use crate::osm::{RouteInfo, SectorData};

pub struct Edge {
    pub a: u32,
    pub b: u32,
    pub lon: Vec<f64>,
    pub lat: Vec<f64>,
    pub x: Vec<f32>,
    pub z: Vec<f32>,
    pub e: Vec<f32>,
    pub len2: f64,
    pub len3: f64,
    pub up: f64,
    pub down: f64,
    pub colors: Vec<TrailColor>,
    /// Indeksy w `TrailGraph::routes`.
    pub routes: Vec<u32>,
    pub sac: u8,
    pub oneway: i8,
    pub name: String,
    pub way: i64,
}

impl Edge {
    pub fn n(&self) -> usize {
        self.lon.len()
    }
}

pub struct GNode {
    pub lon: f64,
    pub lat: f64,
    pub edges: Vec<u32>,
}

#[derive(Clone, Copy, Debug)]
pub struct Snap {
    pub edge: u32,
    /// Pozycja wzdłuż krawędzi w jednostkach indeksu (i + t).
    pub p: f64,
    pub lon: f64,
    pub lat: f64,
    pub e: f32,
    pub dist: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CostMode {
    Time,
    Distance,
    Ascent,
    Easy,
}

impl CostMode {
    pub fn label(self) -> &'static str {
        match self {
            CostMode::Time => crate::i18n::tr("Najszybsza"),
            CostMode::Distance => crate::i18n::tr("Najkrótsza"),
            CostMode::Ascent => crate::i18n::tr("Najmniej podejść"),
            CostMode::Easy => crate::i18n::tr("Najłatwiejsza"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Piece {
    pub edge: u32,
    pub p0: f64,
    pub p1: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteKind {
    Trail,
    Terrain,
    Gpx,
}

#[derive(Clone, Debug)]
pub struct Route {
    pub kind: RouteKind,
    pub labels: Vec<String>,
    pub pieces: Vec<Piece>,
    pub track: Track,
    pub stats: TrackStats,
    /// Indeksy punktów śladu odpowiadające kolejnym punktom trasy.
    pub leg_idx: Vec<usize>,
    /// Trasy (relacje) przebyte po drodze.
    pub route_refs: Vec<u32>,
}

/// Lokalny układ metrów do indeksu przestrzennego.
#[derive(Clone, Copy)]
struct Local {
    lon0: f64,
    lat0: f64,
    kx: f64,
}

impl Local {
    fn xz(&self, lon: f64, lat: f64) -> (f32, f32) {
        (
            ((lon - self.lon0) * self.kx) as f32,
            (-(lat - self.lat0) * 110_574.0) as f32,
        )
    }
}

const CELL: f32 = 250.0;

pub struct TrailGraph {
    pub nodes: Vec<GNode>,
    pub edges: Vec<Edge>,
    pub routes: Vec<RouteInfo>,
    index: HashMap<(i32, i32), Vec<(u32, u32)>>,
    local: Local,
    pub total_km: f64,
}

#[derive(Copy, Clone, PartialEq)]
struct HeapItem(f64, u32);
impl Eq for HeapItem {}
impl Ord for HeapItem {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.partial_cmp(&self.0).unwrap_or(Ordering::Equal)
    }
}
impl PartialOrd for HeapItem {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

struct Metrics {
    len2: f64,
    len3: f64,
    up: f64,
    down: f64,
}

impl TrailGraph {
    pub fn empty() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            routes: Vec::new(),
            index: HashMap::new(),
            local: Local {
                lon0: 0.0,
                lat0: 0.0,
                kx: 111_320.0,
            },
            total_km: 0.0,
        }
    }

    /// Złożenie grafu z sektorów; `height` próbkuje wysokość (zagęszczenie co ~10 m).
    pub fn build(sectors: &[&SectorData], height: &(dyn Fn(f64, f64) -> f32 + Sync)) -> Self {
        let mut g = Self::empty();
        let Some(first) = sectors.iter().find(|s| !s.nodes.is_empty()) else {
            return g;
        };
        let (lon0, lat0) = (first.nodes[0].1, first.nodes[0].2);
        g.local = Local {
            lon0,
            lat0,
            kx: 111_320.0 * lat0.to_radians().cos(),
        };
        // relacje (po identyfikatorze) i węzły (po identyfikatorze OSM)
        let mut route_idx: HashMap<i64, u32> = HashMap::new();
        let mut coords: HashMap<i64, (f64, f64)> = HashMap::new();
        struct Run<'a> {
            ids: Vec<i64>,
            routes: Vec<u32>,
            meta: &'a crate::osm::WayRun,
        }
        let mut runs: Vec<Run> = Vec::new();
        for s in sectors {
            let rmap: Vec<u32> = s
                .routes
                .iter()
                .map(|r| {
                    *route_idx.entry(r.id).or_insert_with(|| {
                        g.routes.push(r.clone());
                        (g.routes.len() - 1) as u32
                    })
                })
                .collect();
            for &(id, lon, lat) in &s.nodes {
                coords.insert(id, (lon, lat));
            }
            for r in &s.runs {
                runs.push(Run {
                    ids: r.nodes.iter().map(|&i| s.nodes[i as usize].0).collect(),
                    routes: r.routes.iter().map(|&i| rmap[i as usize]).collect(),
                    meta: r,
                });
            }
        }
        // węzły grafu: końce ciągów i punkty wspólne wielu dróg
        let mut uses: HashMap<i64, u32> = HashMap::new();
        for r in &runs {
            for id in &r.ids {
                *uses.entry(*id).or_default() += 1;
            }
            *uses.entry(r.ids[0]).or_default() += 2;
            *uses.entry(*r.ids.last().unwrap()).or_default() += 2;
        }
        let mut gidx: HashMap<i64, u32> = HashMap::new();
        let mut node_of = |g: &mut TrailGraph, id: i64| -> u32 {
            *gidx.entry(id).or_insert_with(|| {
                let (lon, lat) = coords[&id];
                g.nodes.push(GNode {
                    lon,
                    lat,
                    edges: Vec::new(),
                });
                (g.nodes.len() - 1) as u32
            })
        };
        struct Raw {
            a: u32,
            b: u32,
            pts: Vec<(f64, f64)>,
            run: usize,
        }
        let mut raws = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (ri, r) in runs.iter().enumerate() {
            let mut start = 0;
            for i in 1..r.ids.len() {
                if uses[&r.ids[i]] > 1 || i == r.ids.len() - 1 {
                    let seg = &r.ids[start..=i];
                    start = i;
                    let key: Vec<i64> = if seg[0] < seg[seg.len() - 1] {
                        seg.to_vec()
                    } else {
                        seg.iter().rev().copied().collect()
                    };
                    if !seen.insert(key) {
                        continue;
                    }
                    let a = node_of(&mut g, seg[0]);
                    let b = node_of(&mut g, seg[seg.len() - 1]);
                    // zagęszczenie geometrii co ~10 m
                    let mut pts = Vec::new();
                    for k in 0..seg.len() {
                        let c = coords[&seg[k]];
                        if k > 0 {
                            let p = coords[&seg[k - 1]];
                            let d = haversine(p.0, p.1, c.0, c.1);
                            let m = (d / 10.0).floor() as usize;
                            for j in 1..=m {
                                let f = j as f64 / (m + 1) as f64;
                                pts.push((p.0 + (c.0 - p.0) * f, p.1 + (c.1 - p.1) * f));
                            }
                        }
                        pts.push(c);
                    }
                    raws.push(Raw { a, b, pts, run: ri });
                }
            }
        }
        // wysokości równolegle (próbkowanie może czytać dysk albo pobierać kafle)
        use rayon::prelude::*;
        let heights: Vec<Vec<f32>> = raws
            .par_iter()
            .map(|r| r.pts.iter().map(|&(lon, lat)| height(lon, lat)).collect())
            .collect();
        let mut total = 0.0;
        for (raw, e) in raws.into_iter().zip(heights) {
            let r = &runs[raw.run];
            let n = raw.pts.len();
            let mut len2 = 0.0;
            let mut len3 = 0.0;
            let (mut x, mut z) = (Vec::with_capacity(n), Vec::with_capacity(n));
            for i in 0..n {
                let (xx, zz) = g.local.xz(raw.pts[i].0, raw.pts[i].1);
                x.push(xx);
                z.push(zz);
                if i > 0 {
                    let h = haversine(raw.pts[i - 1].0, raw.pts[i - 1].1, raw.pts[i].0, raw.pts[i].1);
                    len2 += h;
                    len3 += h.hypot((e[i] - e[i - 1]) as f64);
                }
            }
            let (up, down) = climb(&e);
            total += len2;
            let mut colors: Vec<TrailColor> = r
                .routes
                .iter()
                .map(|&ri| TrailColor::parse(&g.routes[ri as usize].color))
                .filter(|c| *c != TrailColor::Other)
                .collect();
            colors.sort();
            colors.dedup();
            if colors.is_empty() {
                colors.push(TrailColor::Other);
            }
            let id = g.edges.len() as u32;
            g.edges.push(Edge {
                a: raw.a,
                b: raw.b,
                lon: raw.pts.iter().map(|p| p.0).collect(),
                lat: raw.pts.iter().map(|p| p.1).collect(),
                x,
                z,
                e,
                len2,
                len3,
                up,
                down,
                colors,
                routes: r.routes.clone(),
                sac: r.meta.sac,
                oneway: r.meta.oneway,
                name: r.meta.name.clone(),
                way: r.meta.way,
            });
            g.nodes[raw.a as usize].edges.push(id);
            if raw.b != raw.a {
                g.nodes[raw.b as usize].edges.push(id);
            }
        }
        for (id, ed) in g.edges.iter().enumerate() {
            for i in 0..ed.n() - 1 {
                let k0 = cell(ed.x[i], ed.z[i]);
                let k1 = cell(ed.x[i + 1], ed.z[i + 1]);
                g.index.entry(k0).or_default().push((id as u32, i as u32));
                if k1 != k0 {
                    g.index.entry(k1).or_default().push((id as u32, i as u32));
                }
            }
        }
        g.total_km = total / 1000.0;
        g
    }

    /// Najbliższy punkt na sieci szlaków (w promieniu `max_dist` m).
    pub fn snap(&self, lon: f64, lat: f64, max_dist: f64) -> Option<Snap> {
        if self.edges.is_empty() {
            return None;
        }
        let (x, z) = self.local.xz(lon, lat);
        let r = (max_dist as f32 / CELL).ceil() as i32;
        let (cx, cz) = cell(x, z);
        let mut best: Option<Snap> = None;
        let mut best_d = max_dist;
        for i in -r..=r {
            for j in -r..=r {
                let Some(list) = self.index.get(&(cx + i, cz + j)) else {
                    continue;
                };
                for &(eid, s) in list {
                    let ed = &self.edges[eid as usize];
                    let s = s as usize;
                    let (ax, az, bx, bz) = (ed.x[s], ed.z[s], ed.x[s + 1], ed.z[s + 1]);
                    let (dx, dz) = (bx - ax, bz - az);
                    let l2 = dx * dx + dz * dz;
                    let t = if l2 > 0.0 {
                        (((x - ax) * dx + (z - az) * dz) / l2).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let d = ((ax + dx * t - x).hypot(az + dz * t - z)) as f64;
                    if d < best_d {
                        best_d = d;
                        let tt = t as f64;
                        best = Some(Snap {
                            edge: eid,
                            p: s as f64 + tt,
                            lon: ed.lon[s] + (ed.lon[s + 1] - ed.lon[s]) * tt,
                            lat: ed.lat[s] + (ed.lat[s + 1] - ed.lat[s]) * tt,
                            e: ed.e[s] + (ed.e[s + 1] - ed.e[s]) * t,
                            dist: d,
                        });
                    }
                }
            }
        }
        best
    }

    /// Punkty fragmentu krawędzi od p0 do p1: (dł., szer., wysokość).
    pub fn slice(&self, ed: &Edge, p0: f64, p1: f64) -> Vec<(f64, f64, f32)> {
        let n = ed.n();
        let at = |p: f64| {
            let i = (p.floor() as isize).clamp(0, n as isize - 2) as usize;
            let t = (p - i as f64).clamp(0.0, 1.0);
            (
                ed.lon[i] + (ed.lon[i + 1] - ed.lon[i]) * t,
                ed.lat[i] + (ed.lat[i + 1] - ed.lat[i]) * t,
                ed.e[i] + (ed.e[i + 1] - ed.e[i]) * t as f32,
            )
        };
        let mut out = vec![at(p0)];
        if p1 >= p0 {
            let mut i = p0.floor() as i64 + 1;
            while (i as f64) < p1 {
                out.push(at(i as f64));
                i += 1;
            }
        } else {
            let mut i = p0.ceil() as i64 - 1;
            while (i as f64) > p1 {
                out.push(at(i as f64));
                i -= 1;
            }
        }
        out.push(at(p1));
        out
    }

    fn part_metrics(&self, ed: &Edge, p0: f64, p1: f64) -> Metrics {
        let pts = self.slice(ed, p0, p1);
        let mut len2 = 0.0;
        let mut len3 = 0.0;
        for i in 1..pts.len() {
            let h = haversine(pts[i - 1].0, pts[i - 1].1, pts[i].0, pts[i].1);
            len2 += h;
            len3 += h.hypot((pts[i].2 - pts[i - 1].2) as f64);
        }
        let es: Vec<f32> = pts.iter().map(|p| p.2).collect();
        let (up, down) = climb(&es);
        Metrics {
            len2,
            len3,
            up,
            down,
        }
    }

    fn cost(mode: CostMode, m: &Metrics, sac: u8, pen: f64) -> f64 {
        let c = match mode {
            CostMode::Distance => m.len3,
            CostMode::Ascent => m.up * 25.0 + m.len3 * 0.6,
            CostMode::Easy => {
                let hard = if sac >= 5 {
                    6.0
                } else if sac >= 4 {
                    3.0
                } else if sac >= 3 {
                    1.5
                } else {
                    1.0
                };
                walk_time(m.len2, m.up, m.down) * hard * 3600.0
            }
            CostMode::Time => walk_time(m.len2, m.up, m.down) * sac_factor(sac) * 3600.0,
        };
        c * pen
    }

    fn edge_cost(&self, ed: &Edge, forward: bool, mode: CostMode, pen: f64) -> f64 {
        if (ed.oneway == 1 && !forward) || (ed.oneway == -1 && forward) {
            return f64::INFINITY;
        }
        let m = if forward {
            Metrics {
                len2: ed.len2,
                len3: ed.len3,
                up: ed.up,
                down: ed.down,
            }
        } else {
            Metrics {
                len2: ed.len2,
                len3: ed.len3,
                up: ed.down,
                down: ed.up,
            }
        };
        Self::cost(mode, &m, ed.sac, pen)
    }

    fn leg(&self, s: &Snap, t: &Snap, mode: CostMode, pen: &HashMap<u32, f64>) -> Option<Vec<Piece>> {
        let n = self.nodes.len() as u32;
        let (vs, vt) = (n, n + 1);
        let mut extra: HashMap<u32, Vec<(u32, Piece)>> = HashMap::new();
        let es = &self.edges[s.edge as usize];
        let et = &self.edges[t.edge as usize];
        let last = |e: &Edge| (e.n() - 1) as f64;
        extra.entry(vs).or_default().push((es.a, Piece { edge: s.edge, p0: s.p, p1: 0.0 }));
        extra.entry(vs).or_default().push((es.b, Piece { edge: s.edge, p0: s.p, p1: last(es) }));
        extra.entry(et.a).or_default().push((vt, Piece { edge: t.edge, p0: 0.0, p1: t.p }));
        extra.entry(et.b).or_default().push((vt, Piece { edge: t.edge, p0: last(et), p1: t.p }));
        if s.edge == t.edge {
            extra.entry(vs).or_default().push((vt, Piece { edge: s.edge, p0: s.p, p1: t.p }));
        }
        let part_cost = |p: &Piece| {
            let ed = &self.edges[p.edge as usize];
            let fwd = p.p1 >= p.p0;
            if (ed.oneway == 1 && !fwd) || (ed.oneway == -1 && fwd) {
                f64::INFINITY
            } else {
                Self::cost(
                    mode,
                    &self.part_metrics(ed, p.p0, p.p1),
                    ed.sac,
                    pen.get(&p.edge).copied().unwrap_or(1.0),
                )
            }
        };
        let total = (n + 2) as usize;
        let mut dist = vec![f64::INFINITY; total];
        let mut prev: Vec<Option<(u32, Piece)>> = vec![None; total];
        let mut heap = BinaryHeap::new();
        dist[vs as usize] = 0.0;
        heap.push(HeapItem(0.0, vs));
        while let Some(HeapItem(d, u)) = heap.pop() {
            if d > dist[u as usize] {
                continue;
            }
            if u == vt {
                break;
            }
            let mut relax = |v: u32, c: f64, piece: Piece, heap: &mut BinaryHeap<HeapItem>| {
                let nd = d + c;
                if nd < dist[v as usize] {
                    dist[v as usize] = nd;
                    prev[v as usize] = Some((u, piece));
                    heap.push(HeapItem(nd, v));
                }
            };
            if u < n {
                for &eid in &self.nodes[u as usize].edges {
                    let ed = &self.edges[eid as usize];
                    let p = pen.get(&eid).copied().unwrap_or(1.0);
                    if ed.a == u {
                        relax(ed.b, self.edge_cost(ed, true, mode, p), Piece { edge: eid, p0: 0.0, p1: last(ed) }, &mut heap);
                    }
                    if ed.b == u {
                        relax(ed.a, self.edge_cost(ed, false, mode, p), Piece { edge: eid, p0: last(ed), p1: 0.0 }, &mut heap);
                    }
                }
            }
            if let Some(ex) = extra.get(&u) {
                for (to, piece) in ex {
                    relax(*to, part_cost(piece), *piece, &mut heap);
                }
            }
        }
        if !dist[vt as usize].is_finite() {
            return None;
        }
        let mut pieces = Vec::new();
        let mut v = vt;
        while v != vs {
            let (u, p) = prev[v as usize]?;
            if (p.p1 - p.p0).abs() > 1e-6 {
                pieces.push(p);
            }
            v = u;
        }
        pieces.reverse();
        Some(pieces)
    }

    /// Trasa przez kolejne punkty (co najmniej dwa).
    pub fn route(&self, snaps: &[Snap], mode: CostMode, pen: &HashMap<u32, f64>) -> Option<Route> {
        let mut all = Vec::new();
        let mut leg_ends = Vec::new();
        for w in snaps.windows(2) {
            all.extend(self.leg(&w[0], &w[1], mode, pen)?);
            leg_ends.push(all.len());
        }
        Some(self.build_route(all, &leg_ends, vec![mode.label().to_string()]))
    }

    fn build_route(&self, pieces: Vec<Piece>, leg_ends: &[usize], labels: Vec<String>) -> Route {
        let mut tr = Track::default();
        let mut leg_idx = vec![0];
        let mut refs = Vec::new();
        for (k, pc) in pieces.iter().enumerate() {
            let ed = &self.edges[pc.edge as usize];
            refs.extend(ed.routes.iter().copied());
            let sl = self.slice(ed, pc.p0, pc.p1);
            let start = if tr.len() > 0 { 1 } else { 0 };
            for p in &sl[start..] {
                tr.push(p.0, p.1, p.2, ed.colors[0], ed.sac);
            }
            if leg_ends.contains(&(k + 1)) {
                leg_idx.push(tr.len().saturating_sub(1));
            }
        }
        // odcinki zerowej długości (punkty w tym samym miejscu)
        while leg_idx.len() < leg_ends.len() + 1 {
            leg_idx.push(tr.len().saturating_sub(1));
        }
        if tr.len() == 1 {
            let (lon, lat, e) = (tr.lon[0], tr.lat[0], tr.e[0]);
            tr.push(lon, lat, e, tr.color[0], tr.sac[0]);
        }
        tr.finalize(1.0);
        refs.sort_unstable();
        refs.dedup();
        let stats = tr.stats();
        Route {
            kind: RouteKind::Trail,
            labels,
            pieces,
            track: tr,
            stats,
            leg_idx,
            route_refs: refs,
        }
    }

    fn piece_len(&self, p: &Piece) -> f64 {
        let ed = &self.edges[p.edge as usize];
        (p.p1 - p.p0).abs() / (ed.n() - 1).max(1) as f64 * ed.len2
    }

    /// Długość wspólna dwóch tras / długość krótszej.
    fn overlap(&self, a: &Route, b: &Route) -> f64 {
        let mut la: HashMap<u32, f64> = HashMap::new();
        for p in &a.pieces {
            *la.entry(p.edge).or_default() += self.piece_len(p);
        }
        let shared: f64 = b
            .pieces
            .iter()
            .map(|p| self.piece_len(p).min(la.get(&p.edge).copied().unwrap_or(0.0)))
            .sum();
        shared / a.stats.len2.min(b.stats.len2).max(1.0)
    }

    /// Zestaw różnych wariantów; zbieżne są łączone z wieloma etykietami.
    pub fn alternatives(&self, snaps: &[Snap]) -> Vec<Route> {
        let mut out: Vec<Route> = Vec::new();
        let none = HashMap::new();
        let push = |out: &mut Vec<Route>, r: Route| -> bool {
            for o in out.iter_mut() {
                if self.overlap(o, &r) > 0.93
                    && (o.stats.len2 - r.stats.len2).abs() < 0.03 * o.stats.len2
                {
                    for l in r.labels {
                        if !o.labels.contains(&l) {
                            o.labels.push(l);
                        }
                    }
                    return false;
                }
            }
            out.push(r);
            true
        };
        for m in [CostMode::Time, CostMode::Distance, CostMode::Ascent, CostMode::Easy] {
            if let Some(r) = self.route(snaps, m, &none) {
                push(&mut out, r);
            }
        }
        if out.is_empty() {
            return out;
        }
        let mut pen: HashMap<u32, f64> = HashMap::new();
        let mut variant = 0;
        for _ in 0..3 {
            if out.len() >= 5 {
                break;
            }
            for r in &out {
                for p in &r.pieces {
                    *pen.entry(p.edge).or_insert(1.0) *= 1.7;
                }
            }
            let Some(mut alt) = self.route(snaps, CostMode::Time, &pen) else {
                break;
            };
            if alt.stats.time > out[0].stats.time * 1.8 {
                break;
            }
            variant += 1;
            alt.labels = vec![tf!("Wariant {}", variant)];
            if !push(&mut out, alt) {
                variant -= 1;
            }
        }
        out
    }
}

fn cell(x: f32, z: f32) -> (i32, i32) {
    ((x / CELL).floor() as i32, (z / CELL).floor() as i32)
}

/// Trasa z dowolnego śladu (wyznaczonego w terenie albo z pliku GPX).
pub fn route_from_points(
    pts: &[(f64, f64, f32)],
    kind: RouteKind,
    label: &str,
    time_factor: f64,
) -> Route {
    let mut tr = Track::default();
    let color = if kind == RouteKind::Terrain {
        TrailColor::Offtrail
    } else {
        TrailColor::Purple
    };
    let sac = if kind == RouteKind::Terrain { 2 } else { 1 };
    for &(lon, lat, e) in pts {
        tr.push(lon, lat, e, color, sac);
    }
    tr.finalize(time_factor);
    let stats = tr.stats();
    Route {
        kind,
        labels: vec![label.to_string()],
        pieces: Vec::new(),
        leg_idx: vec![0, pts.len().saturating_sub(1)],
        track: tr,
        stats,
        route_refs: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osm::{SectorData, WayRun};

    fn square() -> SectorData {
        // kwadrat 1 km z przekątną: dwie drogi z A do C
        let nodes = vec![
            (1, 20.0, 49.0),
            (2, 20.0137, 49.0),
            (3, 20.0137, 49.009),
            (4, 20.0, 49.009),
        ];
        let run = |ids: Vec<u32>, way| WayRun {
            way,
            nodes: ids,
            routes: vec![],
            sac: 1,
            oneway: 0,
            name: String::new(),
            highway: "path".into(),
        };
        SectorData {
            format: 2,
            fetched: 0,
            nodes,
            runs: vec![run(vec![0, 1, 2], 10), run(vec![0, 3, 2], 11), run(vec![0, 2], 12)],
            routes: vec![],
            pois: vec![],
            partial: false,
        }
    }

    #[test]
    fn shortest_takes_diagonal() {
        let s = square();
        let g = TrailGraph::build(&[&s], &|_, _| 1000.0);
        assert_eq!(g.nodes.len(), 2); // B i D leżą wewnątrz dróg
        let a = g.snap(20.0, 49.0, 100.0).unwrap();
        let c = g.snap(20.0137, 49.009, 100.0).unwrap();
        let r = g
            .route(&[a, c], CostMode::Distance, &HashMap::new())
            .unwrap();
        assert!((r.stats.len2 - 1414.0).abs() < 40.0, "{}", r.stats.len2);
        let alts = g.alternatives(&[a, c]);
        assert!(alts.len() >= 2);
    }
}
