//! Scena dla renderera: światło, warstwy, linie szlaków, tras i granic.

use super::*;

impl Atlas {
    pub(super) fn scene(&self, view: &View) -> Scene {
        let (az, alt) = self.sun();
        let sv = sun::vector(az, alt);
        let (day, _) = self.when();
        // barwy: ciepłe nisko nad horyzontem, nocą chłodne i ciemne
        let a = alt as f32;
        let day_k = ((a + 4.0) / 14.0).clamp(0.0, 1.0);
        let warm = 1.0 - ((a - 2.0) / 25.0).clamp(0.0, 1.0);
        let sun_color = [
            (1.0 + 0.25 * warm) * 2.2 * day_k,
            (0.96 - 0.25 * warm) * 2.1 * day_k,
            (0.88 - 0.5 * warm) * 1.95 * day_k,
        ];
        let night = [0.05, 0.07, 0.12];
        let mixc = |d: [f32; 3], n: [f32; 3]| [n[0] + (d[0] - n[0]) * day_k, n[1] + (d[1] - n[1]) * day_k, n[2] + (d[2] - n[2]) * day_k];
        let sky = mixc([0.32, 0.42, 0.58], night);
        let zenith = mixc([0.16, 0.32, 0.62], [0.01, 0.015, 0.035]);
        let horizon_day = [0.62 + 0.25 * warm, 0.70 + 0.05 * warm, 0.80 - 0.25 * warm];
        let horizon = mixc(horizon_day, [0.03, 0.04, 0.07]);
        let lens_mode = match self.lens {
            Lens::None => 0.0,
            Lens::Band => if self.prefs.band_tol > 0.0 { 2.0 } else { 1.0 },
            Lens::Rel => 3.0,
            Lens::Vis => 0.0,
        };
        let lens_src = self.lens_pin.or(self.hover);
        let lens = match (lens_src, lens_mode > 0.0) {
            (Some((lon, lat, e)), true) => {
                let (x, z) = self.frame.to_local(lon, lat);
                [x as f32, z as f32, e, lens_mode]
            }
            _ => [0.0, 0.0, 0.0, 0.0],
        };
        let cursor = if self.lens == Lens::None {
            self.hover.map(|(lon, lat, e)| {
                let (x, z) = self.frame.to_local(lon, lat);
                [x as f32, z as f32, e]
            })
        } else {
            None
        };
        let to_rect = |r: Option<[f64; 4]>| -> [f32; 4] {
            match r {
                Some([x0, y0, x1, y1]) => {
                    let (a, b) = self.frame.world_to_local(x0, y0);
                    let (c, d) = self.frame.world_to_local(x1, y1);
                    [a as f32, b as f32, (1.0 / (c - a)) as f32, (1.0 / (d - b)) as f32]
                }
                None => [0.0; 4],
            }
        };
        // piętra roślinności i śniegu: wyżej bliżej równika (Alpy ~1800 m, Himalaje ~4000 m), niżej na północy
        let lift = (49.5 - self.cam.lat.abs() as f32).clamp(-20.0, 25.0);
        let up = if lift > 0.0 { lift * 120.0 } else { lift * 60.0 };
        let forest = 1300.0 + up;
        let biome = [forest, forest + 180.0 + lift.max(0.0) * 15.0, forest + 350.0 + lift.max(0.0) * 30.0];
        let snow_lift = if lift > 0.0 { lift * 150.0 } else { lift * 60.0 };
        // cięcie poziomic zależne od odległości
        let d = self.cam.distance;
        let contour = if d < 2500.0 { 10.0 } else if d < 9000.0 { 20.0 } else if d < 30_000.0 { 50.0 } else { 100.0 };
        // siatka UTM dopasowana do ramki wokół środka widoku
        let gc = self.panorama.unwrap_or(self.cam);
        let utm_fit = if self.prefs.km_grid {
            let (x0, z0) = self.frame.to_local(gc.lon, gc.lat);
            let frame = self.frame;
            crate::coords::local_utm_fit(gc.lon, gc.lat, x0, z0, |x, z| frame.to_lonlat(x, z))
        } else {
            None
        }
        .map(|(zone, mut f)| {
            // proste linie: płaszczyzna siatki na wysokości środka widoku (względem oka);
            // w spacerze i gdy oko jest poniżej środka siatka zostaje na terenie
            let dy = view.target.y - view.eye.y;
            if self.panorama.is_none() && dy < -1.0 {
                f[3][2] = dy as f32;
                f[3][3] = 1.0;
            }
            (zone, f)
        });
        Scene {
            style: self.prefs.style.code(),
            contours: self.prefs.contours,
            contour_interval: contour,
            km_grid: utm_fit.is_some(),
            utm_grid: utm_fit.map(|f| f.1).unwrap_or([[0.0; 4]; 4]),
            sun_dir: sv,
            sun_power: if alt > -2.0 { 1.0 } else { 0.0 },
            sun_color,
            sky_color: sky,
            zenith,
            horizon,
            // z wysoka mgła słabnie (inaczej przy dużym oddaleniu cały widok jest mleczny)
            fog_density: 1.0 / (190_000.0 * (self.cam.distance / 30_000.0).max(1.0) as f32),
            snowline: sun::snowline(day) as f32 + snow_lift,
            snow: self.prefs.snow,
            autumn: sun::autumn(day),
            lens,
            band_tol: self.prefs.band_tol,
            rel_range: self.prefs.rel_range,
            cursor,
            shadows: self.prefs.shadows && self.shadow_rect.is_some(),
            viewshed: self.lens == Lens::Vis && self.vs_rect.is_some(),
            biome,
            render_scale: self.prefs.render_scale,
            detail_px: self.prefs.detail_px,
            imagery: self.prefs.style == Style::Satellite,
            ortho: self.prefs.national_ortho,
            detail: self.prefs.detail,
            max_dem: self.prefs.max_dem,
            max_img: self.prefs.max_img,
            prefetch: self.prefs.prefetch,
            shadow_rect: to_rect(self.shadow_rect),
            vs_rect: to_rect(self.vs_rect),
            line_fade: (self.cam.distance as f32 * 3.0).clamp(15_000.0, 90_000.0),
            landcover: self.prefs.landcover && self.land_rect.is_some(),
            land_rect: to_rect(self.land_rect),
            water_rect: to_rect(self.water_rect),
            time: self.epoch.elapsed().as_secs_f32() % 3600.0,
            trees: self.trees_on() && !self.trees.is_empty(),
            animate: self.fly.is_some() || self.flight.is_some(),
        }
    }

    /// Linie zmienne: trasy, warianty, ślad GPX, pomiar.
    pub(super) fn dyn_lines(&self, view: &View) -> Vec<LineInst> {
        let mut out = Vec::new();
        let _ = view;
        let push_track = |out: &mut Vec<LineInst>, r: &Route, color: [f32; 4], width: f32, outline: bool| {
            let tr = &r.track;
            let n = tr.len();
            let step = (n / 6000).max(1);
            let pts: Vec<[f32; 3]> = (0..n)
                .step_by(step)
                .chain(std::iter::once(n - 1))
                .map(|i| {
                    let (x, z) = self.frame.to_local(tr.lon[i], tr.lat[i]);
                    [x as f32, tr.e[i] + 2.0, z as f32]
                })
                .collect();
            for w in pts.windows(2) {
                if outline {
                    out.push(LineInst { p0: w[0], p1: w[1], color: [0.02, 0.02, 0.02, 1.0], wo: [width + 4.0, 0.0] });
                }
            }
            for w in pts.windows(2) {
                out.push(LineInst { p0: w[0], p1: w[1], color, wo: [width, 0.0] });
            }
        };
        for (i, r) in self.routes.iter().enumerate() {
            if i != self.route_sel {
                push_track(&mut out, r, [0.75, 0.72, 0.66, 1.0], 3.0, false);
            }
        }
        if let Some(r) = &self.gpx {
            push_track(&mut out, r, lin([186, 104, 236], 1.0), 4.0, true);
        }
        if let Some((_, r)) = &self.compare {
            push_track(&mut out, r, lin([214, 120, 255], 1.0), 4.0, true);
        }
        // zaznaczony odcinek szlaku: jasna obwódka i niebieskie wyróżnienie
        if let Selection::Trail(ei) = self.selection {
            if let Some(ed) = self.snap.graph.edges.get(ei as usize) {
                let pts: Vec<[f32; 3]> = (0..ed.lon.len())
                    .map(|i| {
                        let (x, z) = self.frame.to_local(ed.lon[i], ed.lat[i]);
                        [x as f32, ed.e[i] + 2.5, z as f32]
                    })
                    .collect();
                for (width, color) in [(14.0, lin([255, 255, 255], 0.95)), (10.0, [0.01, 0.01, 0.012, 1.0]), (6.0, lin([122, 176, 255], 1.0))] {
                    for w in pts.windows(2) {
                        out.push(LineInst { p0: w[0], p1: w[1], color, wo: [width, 0.0] });
                    }
                }
            }
        }
        if let Some(r) = self.routes.get(self.route_sel) {
            // wybrana trasa: jasna obwódka, ciemny brzeg i barwy nachylenia jak na profilu
            let tr = &r.track;
            let n = tr.len();
            if n >= 2 {
                let step = (n / 6000).max(1);
                let idx: Vec<usize> = (0..n).step_by(step).chain(std::iter::once(n - 1)).collect();
                let pt = |i: usize| {
                    let (x, z) = self.frame.to_local(tr.lon[i], tr.lat[i]);
                    [x as f32, tr.e[i] + 2.5, z as f32]
                };
                let base = if r.kind == RouteKind::Terrain { [255, 140, 60] } else { [255, 186, 60] };
                for (width, color) in [(13.0, lin([255, 255, 255], 1.0)), (9.0, [0.01, 0.01, 0.012, 1.0])] {
                    for w in idx.windows(2) {
                        out.push(LineInst { p0: pt(w[0]), p1: pt(w[1]), color, wo: [width, 0.0] });
                    }
                }
                for w in idx.windows(2) {
                    // nachylenie w oknie ~60 m wokół odcinka
                    let mid = (tr.d2[w[0]] + tr.d2[w[1]]) * 0.5;
                    let (a, b) = (tr.at_dist((mid - 30.0).max(0.0)), tr.at_dist(mid + 30.0));
                    let (ea, eb) = (tr.lerp(a.0, a.1).2, tr.lerp(b.0, b.1).2);
                    let g = (eb - ea) as f64 / 60.0;
                    let k = crate::metrics::grade_class(g);
                    let rgb = if k <= 1 { base } else { crate::metrics::GRADE_RGB[k] };
                    out.push(LineInst { p0: pt(w[0]), p1: pt(w[1]), color: lin(rgb, 1.0), wo: [6.0, 0.0] });
                }
            }
        }
        // pomiar: linia po terenie
        if self.measure.len() >= 2 {
            for w in self.measure.windows(2) {
                let n = (haversine(w[0].0, w[0].1, w[1].0, w[1].1) / 30.0).ceil().clamp(2.0, 400.0) as usize;
                let mut prev: Option<[f32; 3]> = None;
                for k in 0..=n {
                    let f = k as f64 / n as f64;
                    let lon = w[0].0 + (w[1].0 - w[0].0) * f;
                    let lat = w[0].1 + (w[1].1 - w[0].1) * f;
                    let e = self.heights.sample_cached(lon, lat).unwrap_or(0.0);
                    let (x, z) = self.frame.to_local(lon, lat);
                    let p = [x as f32, e + 2.0, z as f32];
                    if let Some(q) = prev {
                        out.push(LineInst { p0: q, p1: p, color: [1.0, 1.0, 1.0, 1.0], wo: [3.0, 0.0] });
                    }
                    prev = Some(p);
                }
            }
            if self.measure.len() == 2 {
                if let Some((vis, _)) = self.los {
                    let (a, b) = (self.measure[0], self.measure[1]);
                    let pa = self.local(a.0, a.1, a.2 + 1.7);
                    let pb = self.local(b.0, b.1, b.2 + 1.7);
                    let c = if vis.is_none() { lin([110, 230, 120], 1.0) } else { lin([255, 90, 54], 1.0) };
                    let ex = self.prefs.exag;
                    out.push(LineInst {
                        p0: [pa.x as f32, (pa.y / ex) as f32, pa.z as f32],
                        p1: [pb.x as f32, (pb.y / ex) as f32, pb.z as f32],
                        color: c,
                        wo: [2.0, 0.0],
                    });
                }
            }
        }
        // zaznaczony odcinek profilu: turkusowe wyróżnienie na trasie
        if let (Some((s0, s1)), Some(r)) = (self.profile_sel, self.current_route()) {
            let tr = &r.track;
            let mut pts: Vec<[f32; 3]> = Vec::new();
            let pt = |(lon, lat, e): (f64, f64, f32)| {
                let (x, z) = self.frame.to_local(lon, lat);
                [x as f32, e + 3.0, z as f32]
            };
            let a = tr.at_dist(s0);
            pts.push(pt(tr.lerp(a.0, a.1)));
            for i in 0..tr.len() {
                if tr.d2[i] > s0 && tr.d2[i] < s1 {
                    pts.push(pt((tr.lon[i], tr.lat[i], tr.e[i])));
                }
            }
            let b = tr.at_dist(s1);
            pts.push(pt(tr.lerp(b.0, b.1)));
            for w in pts.windows(2) {
                out.push(LineInst { p0: w[0], p1: w[1], color: lin([80, 220, 255], 1.0), wo: [9.0, 0.0] });
            }
        }
        out
    }
}

/// Linie szlaków z grafu: kolory relacji (kilka kolorów – równoległe pasy).
/// Szlaki; przy wyznaczonej trasie przyciemnione i cieńsze, żeby trasa się wyróżniała.
/// Łańcuchy i klamry: pomarańczowe odcinki z ciemnym brzegiem, nad kolorami szlaków.
pub(super) fn chain_lines(snap: &Snapshot, frame: &Frame, ppp: f32) -> Vec<LineInst> {
    let mut out = Vec::new();
    let w = 4.0 * ppp.max(1.0);
    for line in &snap.chains {
        let pts: Vec<[f32; 3]> = line
            .iter()
            .map(|&(lon, lat, e)| {
                let (x, z) = frame.to_local(lon, lat);
                [x as f32, e + 1.5, z as f32]
            })
            .collect();
        for (width, color) in [(w + 3.0, [0.02, 0.02, 0.02, 1.0]), (w, lin([255, 140, 20], 1.0))] {
            for p in pts.windows(2) {
                out.push(LineInst { p0: p[0], p1: p[1], color, wo: [width, 0.0] });
            }
        }
    }
    out
}

/// Zamknięte odcinki szlaków: czerwona przerywana linia z ciemną obwódką (tylko gdy zamknięcie
/// obowiązuje w wybranym dniu).
pub(super) fn closure_lines(snap: &Snapshot, frame: &Frame, ppp: f32, (m, d): (u32, u32)) -> Vec<LineInst> {
    let mut out = Vec::new();
    let w = 5.0 * ppp.max(1.0);
    for (c, lines) in &snap.closures {
        if !c.active(m, d) {
            continue;
        }
        for line in lines {
            let pts = dense(line, frame, 2.0, 12.0);
            for (k, p) in pts.windows(2).enumerate() {
                out.push(LineInst { p0: p[0], p1: p[1], color: [0.02, 0.02, 0.02, 1.0], wo: [w + 3.0, 0.0] });
                if k % 2 == 0 {
                    out.push(LineInst { p0: p[0], p1: p[1], color: lin([255, 60, 60], 1.0), wo: [w, 0.0] });
                }
            }
        }
    }
    out
}

/// Trasy narciarskie: zjazdowe w barwach trudności, biegowe fioletowe, skiturowe pomarańczowe (przerywane).
pub(super) fn piste_lines(snap: &Snapshot, frame: &Frame, ppp: f32) -> Vec<LineInst> {
    let mut out = Vec::new();
    for (p, line) in &snap.pistes {
        let (rgb, w, dashed) = match (p.kind.as_str(), p.difficulty.as_str()) {
            ("downhill", "novice" | "easy") => ([60, 130, 255], 3.2, false),
            ("downhill", "intermediate") => ([235, 50, 50], 3.2, false),
            ("downhill", "advanced" | "expert" | "freeride" | "extreme") => ([20, 20, 20], 3.2, false),
            ("downhill", _) => ([60, 130, 255], 3.2, false),
            ("nordic", _) => ([170, 90, 230], 2.2, true),
            _ => ([255, 150, 40], 2.2, true),
        };
        let pts = dense(line, frame, 1.5, 15.0);
        for (k, s) in pts.windows(2).enumerate() {
            if !dashed || k % 2 == 0 {
                out.push(LineInst { p0: s[0], p1: s[1], color: lin(rgb, 0.95), wo: [w * ppp.max(1.0), 0.0] });
            }
        }
    }
    out
}

/// Punkty linii co `step` m (żeby leżała na terenie i dało się ją przerywać), `lift` m nad ziemią.
fn dense(line: &[(f64, f64, f32)], frame: &Frame, lift: f32, step: f64) -> Vec<[f32; 3]> {
    let mut out = Vec::new();
    for w in line.windows(2) {
        let dist = haversine(w[0].0, w[0].1, w[1].0, w[1].1);
        let n = (dist / step).ceil().clamp(1.0, 200.0) as usize;
        for k in 0..n {
            let f = k as f64 / n as f64;
            let (lon, lat) = (w[0].0 + (w[1].0 - w[0].0) * f, w[0].1 + (w[1].1 - w[0].1) * f);
            let e = w[0].2 + (w[1].2 - w[0].2) * f as f32;
            let (x, z) = frame.to_local(lon, lat);
            out.push([x as f32, e + lift, z as f32]);
        }
    }
    if let Some(&(lon, lat, e)) = line.last() {
        let (x, z) = frame.to_local(lon, lat);
        out.push([x as f32, e + lift, z as f32]);
    }
    out
}

/// Granice: parki narodowe (zielone), rezerwaty (jasnozielone), państwa (jasne).
pub(super) fn boundary_lines(snap: &Snapshot, frame: &Frame, ppp: f32) -> Vec<LineInst> {
    use crate::osm::BoundaryKind as B;
    let mut out = Vec::new();
    for (kind, _, lines) in &snap.boundaries {
        let (rgb, w) = match kind {
            B::NationalPark => ([70, 190, 100], 2.4),
            B::Reserve => ([150, 215, 120], 1.6),
            B::Country => ([240, 240, 240], 2.8),
        };
        let color = lin(rgb, 0.9);
        for line in lines {
            let pts: Vec<[f32; 3]> = line
                .iter()
                .map(|&(lon, lat, e)| {
                    let (x, z) = frame.to_local(lon, lat);
                    [x as f32, e + 1.0, z as f32]
                })
                .collect();
            // przerywana: co drugi odcinek ~ co 30 m
            for (k, p) in pts.windows(2).enumerate() {
                if k % 2 == 0 || *kind == B::Country {
                    out.push(LineInst { p0: p[0], p1: p[1], color, wo: [w * ppp.max(1.0), 0.0] });
                }
            }
        }
    }
    out
}

pub(super) fn trail_lines(snap: &Snapshot, frame: &Frame, ppp: f32, dim: bool) -> Vec<LineInst> {
    let mut out = Vec::new();
    let w = if dim { 1.6 } else { 2.6 } * ppp.max(1.0);
    for ed in &snap.graph.edges {
        let n = ed.n();
        // co ~25 m wystarczy do rysowania
        let step = 2usize;
        let idx: Vec<usize> = (0..n).step_by(step).chain(std::iter::once(n - 1)).collect();
        let pts: Vec<[f32; 3]> = idx
            .iter()
            .map(|&i| {
                let (x, z) = frame.to_local(ed.lon[i], ed.lat[i]);
                [x as f32, ed.e[i] + 1.0, z as f32]
            })
            .collect();
        let k = ed.colors.len() as f32;
        for (ci, c) in ed.colors.iter().enumerate() {
            let off = (ci as f32 - (k - 1.0) / 2.0) * w;
            let mut col = lin(c.rgb(), 0.98);
            if *c == crate::metrics::TrailColor::Other {
                col = lin([214, 196, 160], 0.98);
            }
            if dim {
                // wyblakłe: zbliżone do szarości i ciemniejsze
                let l = col[0] * 0.2126 + col[1] * 0.7152 + col[2] * 0.0722;
                for k in 0..3 {
                    col[k] = (col[k] * 0.35 + l * 0.65) * 0.32;
                }
            }
            for p in pts.windows(2) {
                if p[0] == p[1] {
                    continue;
                }
                out.push(LineInst { p0: p[0], p1: p[1], color: col, wo: [w, off] });
            }
        }
    }
    out
}
