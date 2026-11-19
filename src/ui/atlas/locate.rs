//! Położenie GPS na mapie (kropka z dokładnością i kierunkiem, kamera podążająca za Tobą)
//! i nagrywanie przejść: start, przerwa, zapis w bibliotece, eksport i wczytywanie z zegarka.

use super::*;
use crate::gps::{Fix, Permission};
use crate::recording::{Recorder, Recording};
use crate::ui::dialog::Ask;

/// Kamera a położenie.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Follow {
    /// Mapa swobodna (po przesunięciu palcem).
    #[default]
    Free,
    /// Kamera trzyma położenie na środku widocznej mapy.
    Follow,
    /// Jak wyżej i mapa obraca się zgodnie z kompasem (kierunkiem marszu).
    Compass,
}

/// Stan położenia na mapie.
#[derive(Default)]
pub struct GpsState {
    /// Odczyty włączone.
    pub on: bool,
    /// Ostatni odczyt i chwila jego odebrania.
    pub fix: Option<Fix>,
    pub fix_at: Option<Instant>,
    /// Kierunek z kompasu (stopnie od północy geograficznej).
    pub heading: Option<f32>,
    pub follow: Follow,
    /// Czeka na zgodę systemu: po niej nagrywanie (`true`) albo samo położenie.
    pending: Option<bool>,
    /// Po pierwszym odczycie przelot do położenia.
    fly_first: bool,
    /// Sprawdzono, czy jest nagranie przerwane zamknięciem programu.
    recovered: bool,
    /// Następny wczytany plik trafia do nagranych przejść (z zegarka, innej aplikacji).
    pub import_rec: bool,
}

impl GpsState {
    /// Świeży odczyt (ostatnie 20 s).
    pub fn fresh(&self) -> bool {
        self.fix_at.is_some_and(|t| t.elapsed() < Duration::from_secs(20))
    }
}

impl Atlas {
    /// Co klatkę: odczyty z systemu, punkty nagrania, kamera podążająca za położeniem.
    pub(super) fn gps_tick(&mut self, ctx: &egui::Context) {
        if !crate::platform::LOCATION {
            return;
        }
        if !self.gps.recovered {
            self.gps.recovered = true;
            self.recover_recording();
        }
        if !self.gps.on {
            return;
        }
        let (fixes, heading) = crate::platform::location_poll();
        if let Some(h) = heading {
            self.gps.heading = Some(h);
        }
        if !fixes.is_empty() {
            let heights = self.heights.clone();
            let ele = move |lon: f64, lat: f64| heights.sample_cached(lon, lat);
            for f in &fixes {
                if let Some(r) = &mut self.rec {
                    r.push(f, &ele);
                }
                // na mapie najnowszy odczyt; z sieci tylko, gdy satelitarny jest stary albo mniej dokładny
                let take = match self.gps.fix {
                    None => true,
                    Some(o) => f.t >= o.t && (f.gnss || !o.gnss || f.t - o.t > 15.0 || f.acc < o.acc),
                };
                if take {
                    self.gps.fix = Some(*f);
                    self.gps.fix_at = Some(Instant::now());
                }
            }
        }
        if let Some(r) = &mut self.rec {
            r.refresh(false);
        }
        if let Some(f) = self.gps.fix {
            if self.gps.fly_first {
                self.gps.fly_first = false;
                let h = self.heights.sample_cached(f.lon, f.lat).map(|v| v as f64).unwrap_or(self.cam.height);
                let mut to = self.cam;
                to.distance = self.cam.distance.clamp(900.0, 4000.0);
                to.height = h;
                let (lon, lat) = self.target_for_visible(ctx, f.lon, f.lat, h, &to);
                to.lon = lon;
                to.lat = lat;
                self.start_fly(to, 1.2);
            } else if self.gps.follow != Follow::Free && self.fly.is_none() && self.flight.is_none() && self.panorama.is_none() {
                self.follow_step(ctx, f);
            }
        }
        // zegar nagrania i nowe odczyty także przy nieruchomej mapie
        ctx.request_repaint_after(Duration::from_millis(if self.rec.is_some() { 500 } else { 1000 }));
    }

    /// Krok kamery w stronę położenia (i kierunku w trybie kompasu).
    fn follow_step(&mut self, ctx: &egui::Context, f: Fix) {
        let mut moved = false;
        if self.gps.follow == Follow::Compass {
            // kierunek: kompas, a w szybkim ruchu – kierunek marszu z odbiornika
            let course = f.course.filter(|_| f.speed.unwrap_or(0.0) > 1.2).map(|c| c as f64);
            if let Some(w) = self.gps.heading.map(|v| v as f64).or(course) {
                let dh = ((w - self.cam.heading + 540.0).rem_euclid(360.0)) - 180.0;
                if dh.abs() > 0.3 {
                    self.cam.heading = (self.cam.heading + dh * 0.15).rem_euclid(360.0);
                    moved = true;
                }
            }
        }
        let h = self.heights.sample_cached(f.lon, f.lat).map(|v| v as f64).unwrap_or(self.cam.height);
        let cam = self.cam;
        let (lon, lat) = self.target_for_visible(ctx, f.lon, f.lat, h, &cam);
        let kx = 111_320.0 * lat.to_radians().cos();
        let off = ((lon - self.cam.lon) * kx).hypot((lat - self.cam.lat) * 110_574.0);
        if off > 0.3 {
            let k = if off > 2000.0 { 1.0 } else { 0.18 };
            self.cam.lon += (lon - self.cam.lon) * k;
            self.cam.lat += (lat - self.cam.lat) * k;
            self.cam.height += (h - self.cam.height) * k;
            moved = true;
        }
        if moved {
            ctx.request_repaint();
        }
    }

    /// Przycisk położenia: włącza i przelatuje → podąża → podąża z kompasem → wyłącza
    /// (podczas nagrywania odczyty zostają, tylko mapa jest swobodna).
    pub fn toggle_locate(&mut self) {
        if !crate::platform::LOCATION {
            return self.notify(crate::i18n::tr("Położenie GPS działa w aplikacji na telefon (Android, iPhone)."));
        }
        if !self.gps.on {
            self.gps.follow = Follow::Follow;
            self.gps.fly_first = true;
            self.start_gps(false);
            return;
        }
        self.gps.follow = match self.gps.follow {
            Follow::Free => {
                if self.gps.fix.is_some() {
                    self.gps.fly_first = true;
                }
                Follow::Follow
            }
            Follow::Follow => {
                self.notify(crate::i18n::tr("Mapa obraca się zgodnie z kierunkiem, w którym patrzysz."));
                Follow::Compass
            }
            Follow::Compass => {
                if self.rec.is_none() {
                    self.stop_gps();
                }
                Follow::Free
            }
        };
    }

    /// Przesunięcie mapy przez użytkownika: koniec podążania (położenie nadal widać).
    pub(super) fn stop_following(&mut self) {
        if self.gps.follow != Follow::Free {
            self.gps.follow = Follow::Free;
            self.gps.fly_first = false;
        }
    }

    /// Włączenie odczytów z pytaniem o zgodę, gdy jej jeszcze nie ma.
    fn start_gps(&mut self, record: bool) {
        use crate::i18n::tr;
        match crate::platform::location_permission() {
            Permission::Granted => self.begin_gps(record),
            Permission::Unknown => {
                self.gps.pending = Some(record);
                let text = if crate::platform::IOS {
                    tr("GRAŃ pokaże Twoje położenie na mapie i – gdy włączysz nagrywanie – zapisze przejście, także przy wyłączonym ekranie. Położenie zostaje na tym urządzeniu: nie jest wysyłane ani udostępniane. W następnym oknie systemu wybierz „Podczas używania aplikacji”.")
                } else {
                    tr("GRAŃ pokaże Twoje położenie na mapie i – gdy włączysz nagrywanie – zapisze przejście, także przy wyłączonym ekranie (z powiadomieniem na pasku). Położenie zostaje na tym urządzeniu: nie jest wysyłane ani udostępniane. W następnym oknie systemu zezwól na dokładną lokalizację.")
                };
                self.ask_choice(Ask::new(tr("Lokalizacja"), text, &["Dalej", "Nie teraz"]), |a, i| {
                    if i == 0 {
                        crate::platform::location_request_permission();
                    } else {
                        a.gps.pending = None;
                        a.gps.follow = Follow::Free;
                    }
                });
            }
            Permission::Denied => {
                self.gps.follow = Follow::Free;
                let ask = Ask::new(tr("Lokalizacja wyłączona"), tr("GRAŃ nie ma zgody na lokalizację. Włącz ją w ustawieniach systemu (Uprawnienia → Lokalizacja → „Zezwalaj podczas używania aplikacji”, dokładna lokalizacja)."), &["Otwórz ustawienia", "Anuluj"]);
                self.ask(ask, |_| crate::platform::location_settings());
            }
        }
    }

    fn begin_gps(&mut self, record: bool) {
        let journal = record.then(crate::gps::journal_path);
        if let Some(j) = &journal {
            let _ = std::fs::create_dir_all(j.parent().unwrap_or(std::path::Path::new(".")));
        }
        crate::platform::location_start(journal.as_deref());
        self.gps.on = true;
    }

    fn stop_gps(&mut self) {
        crate::platform::location_stop();
        self.gps.on = false;
        self.gps.fix = None;
        self.gps.fix_at = None;
        self.gps.heading = None;
        self.gps.follow = Follow::Free;
    }

    /// Odpowiedź systemu na prośbę o zgodę.
    pub fn on_location_permission(&mut self, granted: bool) {
        let Some(record) = self.gps.pending.take() else { return };
        if !granted {
            self.gps.follow = Follow::Free;
            return self.notify(crate::i18n::tr("Bez zgody na lokalizację GRAŃ nie pokaże położenia. Zmienisz to w ustawieniach systemu."));
        }
        if record {
            self.start_recording();
        } else {
            self.begin_gps(false);
        }
    }

    // ------------------------------------------------------------------ nagrywanie

    /// Początek nagrywania (z prośbą o zgodę, gdy trzeba).
    pub fn start_recording(&mut self) {
        if !crate::platform::LOCATION {
            return self.notify(crate::i18n::tr("Nagrywanie przejść działa w aplikacji na telefon (Android, iPhone)."));
        }
        if self.rec.is_some() {
            return self.open_record_panel();
        }
        if crate::platform::location_permission() != Permission::Granted {
            self.gps.follow = Follow::Follow;
            self.gps.fly_first = true;
            return self.start_gps(true);
        }
        let id = self.library.next_id();
        let near = self.gps.fix.and_then(|f| self.name_near(f.lon, f.lat, 1500.0));
        let name = crate::recording::default_name(crate::gps::now(), near);
        let filter = crate::recording::Filter { max_acc: self.prefs.gps_max_acc.clamp(5.0, 100.0) };
        self.rec = Some(Recorder::new(id, name, filter));
        self.begin_gps(true);
        if self.gps.follow == Follow::Free {
            self.gps.follow = Follow::Follow;
            self.gps.fly_first = self.gps.fix.is_some();
        }
        if self.gps.fix.is_none() {
            self.gps.fly_first = true;
        }
        self.notify(crate::i18n::tr("Nagrywanie rozpoczęte. Działa także przy wyłączonym ekranie."));
    }

    pub fn pause_recording(&mut self, on: bool) {
        if let Some(r) = &mut self.rec {
            r.set_paused(on);
        }
    }

    /// Koniec nagrywania: zapis, odrzucenie albo powrót do nagrywania.
    pub fn stop_recording(&mut self) {
        use crate::i18n::tr;
        let Some(r) = &mut self.rec else { return };
        r.refresh(true);
        let s = r.stats;
        let text = tf!("{} · {} · ↑ {} · ↓ {}\n\nZapisane przejście trafi do biblioteki – możesz je obejrzeć na mapie, wyeksportować do GPX albo FIT (zegarki, Garmin Connect, Strava) i udostępnić.", crate::recording::clock(r.elapsed()), fmt::dist(s.dist), fmt::ele(s.up), fmt::ele(s.down));
        self.ask_choice(Ask::new(tr("Zakończyć nagrywanie?"), text, &["Zapisz", "Odrzuć", "Nagrywaj dalej"]), |a, i| match i {
            0 => a.finish_recording(true),
            1 => {
                let ask = Ask::new(crate::i18n::tr("Odrzucić nagranie?"), crate::i18n::tr("Nagrane przejście zostanie usunięte i nie da się go odzyskać."), &["Odrzuć", "Anuluj"]).danger();
                a.ask(ask, |a| a.finish_recording(false));
            }
            _ => {}
        });
    }

    fn finish_recording(&mut self, keep: bool) {
        let Some(r) = self.rec.take() else { return };
        // koniec pracy w tle; położenie na mapie zostaje, gdy kamera za nim podąża
        if self.gps.follow == Follow::Free {
            self.stop_gps();
        } else {
            crate::platform::location_start(None);
        }
        match r.finish(keep) {
            Some(rec) => {
                if let Err(e) = rec.save() {
                    return self.notify(tf!("Nie udało się zapisać nagrania: {}", e));
                }
                self.library.recordings.push(crate::library::RecordingMeta::of(&rec, String::new()));
                self.save_library();
                self.notify(tf!("Zapisano przejście „{}”: {}, ↑ {}.", rec.name, fmt::dist(rec.stats.dist), fmt::ele(rec.stats.up)));
                self.show_recording(&rec, false);
            }
            None if keep => self.notify(crate::i18n::tr("Za mało punktów, żeby zapisać przejście.")),
            None => self.notify(crate::i18n::tr("Nagranie odrzucone.")),
        }
    }

    /// Nagranie przerwane zamknięciem programu: wraca i nagrywa dalej.
    fn recover_recording(&mut self) {
        let heights = self.heights.clone();
        let ele = move |lon: f64, lat: f64| heights.sample_cached(lon, lat);
        let Some(r) = Recorder::recover(&ele) else { return };
        let n = r.points.len();
        self.rec = Some(r);
        if crate::platform::location_permission() == Permission::Granted {
            self.begin_gps(true);
        }
        self.notify(tf!("Wznowiono nagrywanie przejścia ({} punktów) – program był zamknięty w trakcie.", n));
    }

    pub fn open_record_panel(&mut self) {
        self.prefs.panel_open = true;
        self.tab = crate::ui::panels::Tab::Record;
    }

    // ------------------------------------------------------------------ zapisane przejścia

    /// Przejście na mapie i w profilu (jak wczytany ślad).
    pub fn show_recording(&mut self, rec: &Recording, fly: bool) {
        let track = rec.track();
        if track.len() < 2 {
            return;
        }
        self.exit_panorama();
        if self.tool == Tool::Route {
            self.end_planning();
        }
        let r = route_from_points(&track, RouteKind::Gpx, &rec.name, 1.0);
        if fly {
            self.fit_route(&r);
        }
        self.gpx = Some(r);
        self.prefs.profile_open = true;
    }

    pub fn open_recording(&mut self, id: u64) {
        match Recording::load(id) {
            Some(rec) => {
                self.stop_following();
                self.show_recording(&rec, true);
                self.notify(tf!("Otwarto przejście „{}”.", rec.name));
            }
            None => self.notify(crate::i18n::tr("Nie znaleziono pliku nagrania.")),
        }
    }

    pub fn delete_recording(&mut self, id: u64) {
        Recording::delete(id);
        self.library.recordings.retain(|r| r.id != id);
        self.save_library();
    }

    /// Plik przejścia: `gpx`, `fit`, `kml` albo `geojson`.
    fn recording_bytes(rec: &Recording, ext: &str) -> Vec<u8> {
        match ext {
            "fit" => rec.to_fit(),
            "kml" | "geojson" => {
                let r = route_from_points(&rec.track(), RouteKind::Gpx, &rec.name, 1.0);
                if ext == "kml" {
                    crate::formats::export_kml(&r, &rec.name, &[]).into_bytes()
                } else {
                    crate::formats::export_geojson(&r, &rec.name, &[]).into_bytes()
                }
            }
            _ => rec.to_gpx().into_bytes(),
        }
    }

    /// Eksport przejścia do pliku (okno zapisu; na telefonie – w wybranym formacie).
    pub fn export_recording(&mut self, id: u64, ext: &str) {
        let Some(rec) = Recording::load(id) else {
            return self.notify(crate::i18n::tr("Nie znaleziono pliku nagrania."));
        };
        let mut formats: Vec<(&str, &str)> = vec![
            ("gpx", crate::i18n::tr("GPX z czasem (Strava, Komoot, mapy)")),
            ("fit", crate::i18n::tr("FIT – aktywność (Garmin Connect, Suunto, Coros, Strava)")),
            ("kml", crate::i18n::tr("KML (OsmAnd, Google Earth)")),
            ("geojson", "GeoJSON"),
        ];
        if let Some(i) = formats.iter().position(|(e, _)| *e == ext) {
            let f = formats.remove(i);
            formats.insert(0, f);
        }
        let bytes = |ext: &str| Self::recording_bytes(&rec, ext);
        match crate::platform::save_file(crate::i18n::tr("Eksportuj przejście"), &file_stem(&rec.name), &formats, bytes) {
            Some(Ok(p)) => self.notify(tf!("Zapisano {}", p)),
            Some(Err(e)) => self.notify(tf!("Nie udało się zapisać: {}", e)),
            None => {}
        }
    }

    /// Udostępnienie przejścia innej aplikacji (Garmin Connect, Strava, poczta…).
    pub fn share_recording(&mut self, id: u64, ext: &str) {
        let Some(rec) = Recording::load(id) else {
            return self.notify(crate::i18n::tr("Nie znaleziono pliku nagrania."));
        };
        let dir = std::env::temp_dir().join("eksport");
        let _ = std::fs::create_dir_all(&dir);
        let name = format!("{}.{ext}", file_stem(&rec.name));
        let path = dir.join(&name);
        match std::fs::write(&path, Self::recording_bytes(&rec, ext)) {
            Ok(()) => crate::platform::share_file(&path, &name),
            Err(e) => self.notify(tf!("Nie udało się zapisać: {}", e)),
        }
    }

    /// Wybór pliku z zegarka albo innej aplikacji (FIT, GPX z czasem) do nagranych przejść.
    pub fn pick_recording_file(&mut self) {
        self.gps.import_rec = true;
        if let Some(p) = crate::ui::panels::pick_track_file() {
            self.import_track(&p);
        }
    }

    /// Wczytanie pliku z czasem punktów jako nagrane przejście.
    pub(super) fn import_recording(&mut self, path: &std::path::Path) {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => return self.notify(tf!("Nie można odczytać pliku: {}", e)),
        };
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let file = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        let stem = path.file_stem().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        let parsed: Result<(String, Vec<(f64, f64, Option<f32>)>, Vec<f64>), String> = if ext == "fit" || bytes.get(8..12) == Some(&b".FIT"[..]) {
            crate::fit::parse(&bytes).map(|t| (stem.clone(), t.points, t.times))
        } else {
            let text = String::from_utf8_lossy(&bytes).trim_start_matches('\u{feff}').to_string();
            crate::formats::parse_any(&text, &ext).map(|t| (t.name, t.points, t.times))
        };
        let (name, points, times) = match parsed {
            Ok(v) => v,
            Err(e) => return self.notify(e),
        };
        if times.len() != points.len() || times.iter().any(|t| *t <= 0.0) {
            self.notify(crate::i18n::tr("Plik nie ma czasu punktów, więc to trasa, a nie nagrane przejście – otwieram go jako trasę."));
            return self.import_track(path);
        }
        let h = &self.heights;
        let pts: Vec<(f64, f64, f32, f64)> = points.iter().zip(&times).map(|(&(lon, lat, e), &t)| (lon, lat, h.sample_cached(lon, lat).or(e).unwrap_or(0.0), t)).collect();
        let id = self.library.next_id();
        let name = if name.trim().is_empty() { crate::recording::default_name(times[0], None) } else { name };
        let Some(rec) = Recording::from_track(id, &name, &file, &pts) else {
            return self.notify(crate::i18n::tr("Plik nie zawiera śladu."));
        };
        if let Err(e) = rec.save() {
            return self.notify(tf!("Nie udało się zapisać nagrania: {}", e));
        }
        self.library.recordings.push(crate::library::RecordingMeta::of(&rec, String::new()));
        self.save_library();
        self.notify(tf!("Dodano przejście „{}”: {}, ↑ {}.", rec.name, fmt::dist(rec.stats.dist), fmt::ele(rec.stats.up)));
        self.show_recording(&rec, true);
    }

    // ------------------------------------------------------------------ na mapie

    /// Kropka położenia: promień dokładności, stożek kierunku, szara – gdy odczyt jest stary.
    pub(super) fn draw_location(&self, p: &egui::Painter, rect: Rect, view: &View) {
        let Some(f) = self.gps.fix.filter(|_| self.gps.on) else { return };
        let fresh = self.gps.fresh();
        let e = self.heights.sample_cached(f.lon, f.lat).unwrap_or(0.0);
        let Some(s) = view.project(self.local(f.lon, f.lat, e)) else { return };
        let c = Pos2::new(s[0] + rect.left(), s[1] + rect.top());
        let blue = if fresh { Color32::from_rgb(64, 156, 255) } else { Color32::from_rgb(140, 146, 158) };
        // promień dokładności: punkt odsunięty o `acc` metrów na wschód
        let kx = 111_320.0 * f.lat.to_radians().cos();
        let r = view.project(self.local(f.lon + f.acc as f64 / kx, f.lat, e)).map(|q| (q[0] - s[0]).hypot(q[1] - s[1])).unwrap_or(0.0);
        if r > 12.0 {
            let r = r.min(4000.0);
            p.circle_filled(c, r, blue.gamma_multiply(0.14));
            p.circle_stroke(c, r, Stroke::new(1.0, blue.gamma_multiply(0.5)));
        }
        // stożek kierunku (kompas albo kierunek marszu)
        let course = f.course.filter(|_| f.speed.unwrap_or(0.0) > 1.2);
        if let Some(h) = self.gps.heading.or(course).filter(|_| fresh) {
            let a = ((h as f64 - self.cam.heading).to_radians()) as f32;
            let dir = |da: f32| Vec2::new((a + da).sin(), -(a + da).cos());
            let w = 0.55;
            let mut fan = vec![c];
            for k in 0..=8 {
                let da = -w + 2.0 * w * k as f32 / 8.0;
                fan.push(c + dir(da) * 44.0);
            }
            p.add(egui::Shape::convex_polygon(fan, blue.gamma_multiply(0.38), Stroke::NONE));
        }
        p.circle_filled(c + Vec2::new(0.0, 1.5), 11.0, Color32::from_black_alpha(70));
        p.circle_filled(c, 10.5, Color32::WHITE);
        p.circle_filled(c, 7.5, blue);
    }

    /// Linia bieżącego nagrania (odcinki rozdzielone przerwami).
    pub(super) fn recording_lines(&self, out: &mut Vec<LineInst>) {
        let Some(r) = &self.rec else { return };
        let pts = &r.points;
        if pts.len() < 2 {
            return;
        }
        let step = (pts.len() / 6000).max(1);
        let idx: Vec<usize> = (0..pts.len()).step_by(step).chain(std::iter::once(pts.len() - 1)).collect();
        let at = |i: usize| {
            let (x, z) = self.frame.to_local(pts[i].lon, pts[i].lat);
            [x as f32, pts[i].ele + 2.5, z as f32]
        };
        for (width, color) in [(9.0, [0.02, 0.02, 0.02, 1.0]), (5.0, lin([255, 86, 66], 1.0))] {
            for w in idx.windows(2) {
                if pts[w[0]].seg != pts[w[1]].seg {
                    continue;
                }
                out.push(LineInst { p0: at(w[0]), p1: at(w[1]), color, wo: [width, 0.0] });
            }
        }
    }

    /// Bieżące nagranie względem trasy na mapie: (pozostały dystans, odległość od trasy) w metrach.
    pub fn route_progress(&self) -> Option<(f64, f64)> {
        let f = self.gps.fix?;
        let r = self.current_route()?;
        let tr = &r.track;
        if tr.len() < 2 {
            return None;
        }
        let kx = f.lat.to_radians().cos();
        let (mut best, mut bi) = (f64::INFINITY, 0usize);
        for i in 0..tr.len() {
            let d = ((tr.lon[i] - f.lon) * kx).powi(2) + (tr.lat[i] - f.lat).powi(2);
            if d < best {
                best = d;
                bi = i;
            }
        }
        let off = haversine(f.lon, f.lat, tr.lon[bi], tr.lat[bi]);
        let total = *tr.d2.last()? as f64;
        Some(((total - tr.d2[bi] as f64).max(0.0), off))
    }
}
