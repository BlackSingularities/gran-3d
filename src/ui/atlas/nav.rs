//! Ruch kamery: przeloty, przelot nad trasą, spacer, obrót i przesuwanie celu.

use super::*;

impl Atlas {
    pub fn fly_to(&mut self, lon: f64, lat: f64, distance: Option<f64>) {
        let mut to = self.cam;
        to.lon = lon;
        to.lat = lat;
        to.height = self.heights.sample_cached(lon, lat).map(|h| h as f64).unwrap_or(self.cam.height);
        if let Some(d) = distance {
            to.distance = d;
        }
        self.start_fly(to, 1.2);
    }

    pub fn start_fly(&mut self, to: Camera, dur: f32) {
        self.flight = None;
        self.fly = Some(FlyTo { from: self.cam, to, t0: Instant::now(), dur });
    }

    pub fn fit_bbox(&mut self, b: &BBox) {
        let c = b.center();
        let span = haversine(b.w, c[1], b.e, c[1]).max(haversine(c[0], b.s, c[0], b.n));
        self.fly_to(c[0], c[1], Some((span * 1.25).max(1500.0)));
    }

    /// Kamera płynnie obejmuje całą trasę: widok ukośny, trasa nad profilem i obok panelu szczegółów.
    pub fn fit_route(&mut self, r: &Route) {
        let tr = &r.track;
        if tr.len() == 0 {
            return;
        }
        let mut b = BBox::new(tr.lon[0], tr.lat[0], tr.lon[0], tr.lat[0]);
        for i in 0..tr.len() {
            b.w = b.w.min(tr.lon[i]);
            b.e = b.e.max(tr.lon[i]);
            b.s = b.s.min(tr.lat[i]);
            b.n = b.n.max(tr.lat[i]);
        }
        let c = b.center();
        let span = haversine(b.w, c[1], b.e, c[1]).max(haversine(c[0], b.s, c[0], b.n)).max(400.0);
        let side = crate::ui::panels::right_inset(self) > 0.0;
        let mut to = self.cam;
        to.pitch = self.cam.pitch.clamp(38.0, 60.0);
        to.distance = (span * if side { 1.9 } else { 1.6 }).clamp(1500.0, 400_000.0);
        // środek widoku bliżej patrzącego (trasa wyżej, nad profilem) i w prawo (trasa obok panelu)
        let h = to.heading.to_radians();
        let back = if self.prefs.profile_open { span * 0.18 } else { 0.0 };
        let right = if side { span * 0.28 } else { 0.0 };
        let east = -h.sin() * back + h.cos() * right;
        let north = -h.cos() * back - h.sin() * right;
        let kx = 111_320.0 * c[1].to_radians().cos();
        to.lon = c[0] + east / kx;
        to.lat = c[1] + north / 110_574.0;
        to.height = self.heights.sample_cached(to.lon, to.lat).map(|h| h as f64).unwrap_or(self.cam.height);
        self.start_fly(to, 1.4);
    }

    pub fn enter_panorama(&mut self, lon: f64, lat: f64) {
        if self.panorama.is_none() {
            self.panorama = Some(self.cam);
        }
        let h = self.heights.sample_cached(lon, lat).unwrap_or(0.0) as f64;
        self.fly = None;
        self.flight = None;
        self.cam.lon = lon;
        self.cam.lat = lat;
        self.cam.height = h + 1.7 / self.prefs.exag.max(1.0);
        // oko prawie w punkcie obrotu: przy rozglądaniu nie wędruje w zbocze za plecami
        self.cam.distance = 0.4;
        self.cam.pitch = -2.0;
        self.walk_up = 0.0;
        self.walk_vy = 0.0;
        self.notify(crate::i18n::tr("Spacer: W A S D – chodzenie, Shift – bieg, spacja – skok, prawy przycisk – rozglądanie · Esc – wyjście"));
    }

    pub fn exit_panorama(&mut self) {
        if let Some(c) = self.panorama.take() {
            self.cam = c;
        }
    }

    pub(super) fn animate(&mut self, ctx: &egui::Context) {
        if let Some(f) = &self.fly {
            let t = (f.t0.elapsed().as_secs_f32() / f.dur).min(1.0);
            let e = (t * t * (3.0 - 2.0 * t)) as f64;
            let (a, b) = (f.from, f.to);
            let mut c = b;
            c.lon = a.lon + (b.lon - a.lon) * e;
            c.lat = a.lat + (b.lat - a.lat) * e;
            c.height = a.height + (b.height - a.height) * e;
            // odległość: logarytmicznie, z lekkim „podskokiem” przy dalekich przelotach
            let hop = haversine(a.lon, a.lat, b.lon, b.lat);
            let lift = (hop * 0.6 - a.distance.max(b.distance)).max(0.0) * (std::f64::consts::PI * e).sin();
            c.distance = (a.distance.ln() + (b.distance.ln() - a.distance.ln()) * e).exp() + lift;
            let dh = ((b.heading - a.heading + 540.0) % 360.0) - 180.0;
            c.heading = (a.heading + dh * e).rem_euclid(360.0);
            c.pitch = a.pitch + (b.pitch - a.pitch) * e;
            self.cam = c;
            if t >= 1.0 {
                self.fly = None;
            }
            ctx.request_repaint();
        }
        self.animate_flight(ctx);
    }

    pub fn toggle_flight(&mut self) {
        if self.flight.is_some() {
            self.flight = None;
            return;
        }
        if self.current_route().is_none() {
            self.notify(crate::i18n::tr("Przelot: najpierw wyznacz trasę albo wczytaj GPX."));
            return;
        }
        self.fly = None;
        self.flight = Some(Flight { dist: 0.0, paused: false, last: Instant::now(), heading: self.cam.heading, pitch: self.cam.pitch });
    }

    pub(super) fn animate_flight(&mut self, ctx: &egui::Context) {
        let Some(route) = self.current_route().cloned() else {
            self.flight = None;
            return;
        };
        let Some(fl) = &mut self.flight else { return };
        let dt = fl.last.elapsed().as_secs_f32().min(0.1);
        fl.last = Instant::now();
        let tr = &route.track;
        let total = *tr.d2.last().unwrap_or(&0.0);
        // tempo: cała trasa w ~40–90 s
        let speed = (total / 60.0).clamp(30.0, 400.0);
        if !fl.paused {
            fl.dist += speed * dt;
        }
        if fl.dist >= total {
            self.flight = None;
            return;
        }
        let (i, f) = tr.at_dist(fl.dist);
        let (lon, lat, e) = tr.lerp(i, f);
        let (ia, fa) = tr.at_dist((fl.dist + 250.0).min(total));
        let (lon2, lat2, _) = tr.lerp(ia, fa);
        let want = bearing(lon, lat, lon2, lat2);
        let turn = |a: f64, b: f64| ((b - a + 540.0).rem_euclid(360.0)) - 180.0;
        // punkt trasy zawsze widoczny, także za 2 s lotu: kamera obraca się zawczasu
        let here = (lon, lat, e);
        let (ib, fb) = tr.at_dist((fl.dist + speed * 2.0).min(total));
        let soon = tr.lerp(ib, fb);
        let exag = self.prefs.exag;
        let heights = &self.heights;
        let free = |heading: f64, pitch: f64| sight_clear(heights, exag, here, heading, pitch) && sight_clear(heights, exag, soon, heading, pitch);
        let blocked = !sight_clear(heights, exag, here, fl.heading, fl.pitch);
        let current = fl.heading;
        // najmniejsze odejście od kierunku lotu, z lekką preferencją obecnego ustawienia (bez przeskoków)
        let pick = FLIGHT_PITCHES.iter().find_map(|&pitch| {
            (0..24)
                .map(|k| (want + k as f64 * 15.0).rem_euclid(360.0))
                .filter(|&h| free(h, pitch))
                .min_by(|&a, &b| {
                    let cost = |h: f64| turn(want, h).abs() + 0.5 * turn(current, h).abs();
                    cost(a).total_cmp(&cost(b))
                })
                .map(|h| (h, pitch))
        });
        let (want, want_pitch) = pick.unwrap_or((want, 80.0));
        let dh = turn(fl.heading, want);
        let rate = if blocked { 5.0 } else { 2.0 };
        fl.heading = (fl.heading + dh * (dt as f64 * rate).min(1.0)).rem_euclid(360.0);
        let pitch_rate = if want_pitch > fl.pitch { 3.0 } else { 0.7 };
        fl.pitch += (want_pitch - fl.pitch) * (dt as f64 * pitch_rate).min(1.0);
        self.cam.lon = lon;
        self.cam.lat = lat;
        self.cam.height = e as f64;
        self.cam.heading = fl.heading;
        self.cam.pitch = fl.pitch;
        self.cam.distance = FLIGHT_DIST;
        self.profile_cursor = Some(fl.dist);
        ctx.request_repaint();
    }

    /// Pełny ekran (F11).
    pub fn toggle_fullscreen(&mut self, ctx: &egui::Context) {
        let on = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!on));
    }

    pub(super) fn ground(&self, view: &View, px: [f32; 2]) -> Option<(f64, f64, f32)> {
        let dir = view.ray(px);
        let to_geo = |w: DVec3| {
            let (lon, lat) = view.frame.to_lonlat(w.x, w.z);
            (lon, lat, (w.y / view.exag) as f32)
        };
        // najpierw głębia z karty graficznej – dokładnie to, co widać
        if let Some(pk) = self.renderer.as_ref().and_then(|r| r.pick) {
            let near = |a: [f32; 2], r: f32| (a[0] - px[0]).abs() <= r && (a[1] - px[1]).abs() <= r;
            // ten sam widok, ten sam piksel ±1,5 (także niebo)
            if pk.eye == view.eye && near(pk.at, 1.5) {
                return pk.world.map(to_geo);
            }
            if let Some(w) = pk.world.filter(|_| pk.when.elapsed() < Duration::from_millis(400)) {
                let shown = view.project(w);
                // punkt z poprzedniej klatki nadal pod kursorem (kamera drgnęła)
                if shown.is_some_and(|s| near(s, 1.5)) {
                    return Some(to_geo(w));
                }
                // kursor przesunął się o kilka pikseli od ostatniego odczytu: punkt na nowym promieniu
                // w tej samej odległości (z krzywizną jak w shaderze). Odczyt głębi przychodzi dopiero po
                // narysowaniu klatki, a samo przecięcie z modelem terenu przy szczytach i graniach bywało
                // trafiane za lub przed grzbietem – znacznik przeskakiwał wtedy daleko od kursora.
                if shown.is_some_and(|s| near(s, 60.0)) {
                    let drop = |d: DVec3| (d.x * d.x + d.z * d.z) / (2.0 * crate::geo::EARTH_R) * 0.87;
                    let d = w - view.eye;
                    let rendered = DVec3::new(d.x, d.y - drop(d), d.z);
                    let rel = dir * rendered.length();
                    let p = view.eye + DVec3::new(rel.x, rel.y + drop(rel), rel.z);
                    return Some(to_geo(p));
                }
            }
        }
        let frame = view.frame;
        let h = |x: f64, z: f64| {
            let (lon, lat) = frame.to_lonlat(x, z);
            // ten sam poziom terenu co na ekranie: dokładniejszy model z pamięci przesuwałby
            // punkt względem kursora, bo przy ukośnym widoku mała różnica wysokości to duże przesunięcie
            let shown = self.renderer.as_ref().and_then(|r| r.tiles.rendered_dem_z(lon, lat));
            shown
                .and_then(|lvl| self.heights.sample_at(lon, lat, lvl, false))
                .or_else(|| self.heights.sample_cached(lon, lat))
                .map(|v| v as f64)
        };
        let p = view.raycast(dir, &h)?;
        let (lon, lat) = frame.to_lonlat(p.x, p.z);
        Some((lon, lat, p.y as f32))
    }

    pub(super) fn local(&self, lon: f64, lat: f64, e: f32) -> DVec3 {
        let (x, z) = self.frame.to_local(lon, lat);
        DVec3::new(x, e as f64 * self.prefs.exag, z)
    }

    /// Spacer po terenie: W/S – przód/tył, A/D – w bok, Shift – bieg, spacja – skok (z grawitacją).
    pub(super) fn walk(&mut self, ctx: &egui::Context) {
        let (dt, fwd, side, run, jump) = ctx.input(|i| {
            let k = |a: egui::Key| i.key_down(a) as i32 as f64;
            (
                i.stable_dt.min(0.05) as f64,
                k(egui::Key::W) - k(egui::Key::S),
                k(egui::Key::D) - k(egui::Key::A),
                i.modifiers.shift,
                i.key_pressed(egui::Key::Space),
            )
        });
        let moving = fwd != 0.0 || side != 0.0;
        if moving {
            let speed = if run { 22.0 } else { 6.0 };
            let len = fwd.hypot(side);
            let (f, s) = (fwd / len * speed * dt, side / len * speed * dt);
            let h = self.cam.heading.to_radians();
            let east = h.sin() * f + h.cos() * s;
            let north = h.cos() * f - h.sin() * s;
            let kx = 111_320.0 * self.cam.lat.to_radians().cos();
            self.cam.lon = (self.cam.lon + east / kx + 540.0).rem_euclid(360.0) - 180.0;
            self.cam.lat = (self.cam.lat + north / 110_574.0).clamp(-84.0, 84.0);
        }
        if jump && self.walk_up <= 0.0 {
            self.walk_vy = 4.2;
        }
        if self.walk_up > 0.0 || self.walk_vy > 0.0 {
            self.walk_vy -= 9.81 * dt;
            self.walk_up = (self.walk_up + self.walk_vy * dt).max(0.0);
            if self.walk_up <= 0.0 {
                self.walk_vy = 0.0;
            }
        }
        // grunt: najwyższy z kilku próbek najdokładniejszego terenu wokół stóp i pod okiem – na ostrych
        // szczytach i graniach zgrubna wysokość bywa niższa od rysowanej i oko trafiało pod powierzchnię
        // (widok „od spodu”, jak do góry nogami)
        let kx = 111_320.0 * self.cam.lat.to_radians().cos();
        let h = self.cam.heading.to_radians();
        let mut g: Option<f32> = None;
        for (de, dn) in [(0.0, 0.0), (4.0, 0.0), (-4.0, 0.0), (0.0, 4.0), (0.0, -4.0), (-h.sin() * 2.0, -h.cos() * 2.0)] {
            let (lon, lat) = (self.cam.lon + de / kx, self.cam.lat + dn / 110_574.0);
            let v = self.heights.sample_at(lon, lat, 15, false).or_else(|| self.heights.sample_cached(lon, lat));
            if let Some(v) = v {
                g = Some(g.map_or(v, |x| x.max(v)));
            }
        }
        if let Some(g) = g {
            let exag = self.prefs.exag.max(1.0);
            self.cam.height = g as f64 + (1.7 + self.walk_up) / exag;
        }
        if moving || self.walk_up > 0.0 {
            ctx.request_repaint();
        }
    }

    pub fn shift_hour(&mut self, dh: f64) {
        let (day, hour) = self.when();
        let h = (hour + dh).clamp(0.0, 23.99);
        self.prefs.custom_time = Some((day.y, day.m, day.d, h));
    }

    pub(super) fn min_pitch(&self) -> f64 {
        if self.panorama.is_some() {
            -35.0
        } else {
            4.0
        }
    }

    pub fn min_dist(&self) -> f64 {
        if self.panorama.is_some() {
            0.4
        } else {
            120.0
        }
    }

    /// Przesuwa cel kamery tak, jak obrót całego widoku wokół chwyconego punktu (o `dh` w poziomie
    /// i `dp` w nachyleniu, stopnie): punkt zostaje w tym samym miejscu ekranu.
    pub(super) fn orbit(&mut self, dh: f64, dp: f64) {
        let Some((lon, lat, e)) = self.orbit_pivot else { return };
        let (tx, tz) = self.frame.to_local(self.cam.lon, self.cam.lat);
        let exag = self.prefs.exag;
        let pivot = self.local(lon, lat, e);
        let v = DVec3::new(tx, self.cam.height * exag, tz) - pivot;
        // nachylenie: obrót w płaszczyźnie kierunku patrzenia i pionu
        let (h, p) = (self.cam.heading.to_radians(), dp.to_radians());
        let (fwd, right) = (DVec3::new(h.sin(), 0.0, -h.cos()), DVec3::new(h.cos(), 0.0, h.sin()));
        let (a, b, c) = (v.dot(fwd), v.dot(right), v.y);
        let v = fwd * (a * p.cos() + c * p.sin()) + right * b + DVec3::Y * (c * p.cos() - a * p.sin());
        // kierunek: obrót wokół pionu
        let q = dh.to_radians();
        let v = DVec3::new(v.x * q.cos() - v.z * q.sin(), v.y, v.x * q.sin() + v.z * q.cos());
        let mut target = pivot + v;
        // cel wraca na teren wzdłuż linii patrzenia: obraz bez zmian, a wygładzanie wysokości nie szarpie
        let (h, p) = ((self.cam.heading + dh).to_radians(), (self.cam.pitch + dp).to_radians());
        let look = DVec3::new(h.sin() * p.cos(), -p.sin(), -h.cos() * p.cos());
        let mut distance = self.cam.distance;
        if p.sin() > 0.1 {
            for _ in 0..3 {
                let (l, t) = self.frame.to_lonlat(target.x, target.z);
                let Some(g) = self.heights.sample_cached(l, t) else { break };
                let step = (g as f64 * exag - target.y) / look.y;
                if distance + step < self.min_dist() {
                    break;
                }
                target += look * step;
                distance += step;
            }
        }
        let (l, t) = self.frame.to_lonlat(target.x, target.z);
        self.cam.lon = (l + 540.0).rem_euclid(360.0) - 180.0;
        self.cam.lat = t.clamp(-84.0, 84.0);
        self.cam.height = target.y / exag;
        self.cam.distance = distance;
    }

    pub(super) fn move_target(&mut self, east: f64, south: f64) {
        let kx = 111_320.0 * self.cam.lat.to_radians().cos();
        self.cam.lon += east / kx;
        self.cam.lat = (self.cam.lat - south / 110_574.0).clamp(-84.0, 84.0);
        self.cam.lon = (self.cam.lon + 540.0).rem_euclid(360.0) - 180.0;
        self.land_target(1.0);
    }

    /// Cel kamery wraca na teren wzdłuż linii patrzenia: oko zostaje na tej samej wysokości
    /// bezwzględnej, więc przesuwanie mapy nad dolinami i grzbietami nie podbija ani nie obniża kamery.
    pub(super) fn land_target(&mut self, k: f64) {
        if self.panorama.is_some() || self.flight.is_some() {
            return;
        }
        let exag = self.prefs.exag;
        let (h, p) = (self.cam.heading.to_radians(), self.cam.pitch.to_radians());
        if p.sin() <= 0.1 {
            return;
        }
        let look = DVec3::new(h.sin() * p.cos(), -p.sin(), -h.cos() * p.cos());
        let (tx, tz) = self.frame.to_local(self.cam.lon, self.cam.lat);
        let mut target = DVec3::new(tx, self.cam.height * exag, tz);
        let mut distance = self.cam.distance;
        for _ in 0..3 {
            let (l, t) = self.frame.to_lonlat(target.x, target.z);
            let Some(g) = self.heights.sample_cached(l, t) else { break };
            let step = (g as f64 * exag - target.y) / look.y * k;
            if distance + step < self.min_dist() || distance + step > 900_000.0 {
                break;
            }
            target += look * step;
            distance += step;
        }
        let (l, t) = self.frame.to_lonlat(target.x, target.z);
        self.cam.lon = (l + 540.0).rem_euclid(360.0) - 180.0;
        self.cam.lat = t.clamp(-84.0, 84.0);
        self.cam.height = target.y / exag;
        self.cam.distance = distance;
    }
}
