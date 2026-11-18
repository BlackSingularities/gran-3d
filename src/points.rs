//! Punkty społeczności: miejsca na mapie z komentarzami użytkowników GRANI.
//!
//! Punkty i komentarze trafiają na redzikowski.com (`/api/map-points`), gdzie przechodzą moderację
//! w panelu strony. Program pokazuje zatwierdzone punkty z okolicy widoku oraz własne wpisy
//! (także te czekające na zatwierdzenie). Bez sieci (albo w trybie offline) wpisy czekają w kolejce
//! i wysyłają się same po powrocie połączenia. Podpis jest opcjonalny, a program nie wysyła nic
//! poza treścią wpisu, położeniem, wersją programu i anonimowym identyfikatorem instalacji.
//!
//! Plik `%APPDATA%\GRAN\punkty.json`: identyfikator instalacji, podpis, własne wpisy, kolejka
//! i ostatnio widziane punkty (pokazywane także bez sieci).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

const API: &str = "https://redzikowski.com/api/map-points";
/// Bok komórki pobierania punktów (stopnie).
const CELL: f64 = 0.25;
/// Komórka jest odświeżana najwyżej co tyle.
const REFRESH: Duration = Duration::from_secs(600);
/// Najwięcej zapamiętanych punktów z serwera.
const CACHE_MAX: usize = 5000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Info,
    Warning,
    Water,
    View,
    Photo,
    /// Także rodzaje usunięte z programu (dawne „nocleg”, „parking”) i nieznane.
    #[serde(other)]
    Other,
}

impl Category {
    pub const ALL: [Category; 6] = [Category::Warning, Category::Info, Category::Water, Category::View, Category::Photo, Category::Other];

    pub fn label(self) -> &'static str {
        use crate::i18n::tr;
        match self {
            Category::Info => tr("Informacja"),
            Category::Warning => tr("Utrudnienie"),
            Category::Water => tr("Woda"),
            Category::View => tr("Punkt widokowy"),
            Category::Photo => tr("Miejsce na zdjęcie"),
            Category::Other => tr("Inne"),
        }
    }

    pub fn icon(self) -> &'static str {
        use crate::ui::theme::icon as i;
        match self {
            Category::Info => i::INFO,
            Category::Warning => i::WARNING,
            Category::Water => i::DROP,
            Category::View => i::BINOCULARS,
            Category::Photo => i::CAMERA,
            Category::Other => i::CHAT_CIRCLE_TEXT,
        }
    }

    pub fn color(self) -> eframe::egui::Color32 {
        use eframe::egui::Color32;
        match self {
            Category::Warning => Color32::from_rgb(247, 160, 60),
            Category::Water => Color32::from_rgb(90, 170, 255),
            Category::View => Color32::from_rgb(110, 214, 140),
            Category::Photo => Color32::from_rgb(240, 120, 200),
            Category::Info => Color32::from_rgb(122, 176, 255),
            Category::Other => Color32::from_rgb(190, 196, 206),
        }
    }
}

/// Stan wpisu z punktu widzenia autora.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Opublikowany (widoczny dla wszystkich).
    #[default]
    Approved,
    /// Wysłany, czeka na zatwierdzenie.
    Pending,
    /// Czeka w kolejce na połączenie.
    Queued,
    /// Odrzucony albo ukryty przez moderację.
    Hidden,
}

impl Status {
    pub fn label(self) -> &'static str {
        use crate::i18n::tr;
        match self {
            Status::Approved => tr("opublikowany"),
            Status::Pending => tr("czeka na zatwierdzenie"),
            Status::Queued => tr("czeka na połączenie z siecią"),
            Status::Hidden => tr("niewidoczny dla innych"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub author: String,
    pub text: String,
    #[serde(default)]
    pub created_at: String,
    /// Odpowiedź autora GRANI (z panelu strony).
    #[serde(default)]
    pub official: bool,
    #[serde(default)]
    pub status: Status,
    /// Własny komentarz (z tej instalacji).
    #[serde(default)]
    pub mine: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub id: String,
    pub lon: f64,
    pub lat: f64,
    #[serde(default)]
    pub ele: Option<f32>,
    pub category: Category,
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub comments: Vec<Comment>,
    #[serde(default)]
    pub status: Status,
    #[serde(default)]
    pub mine: bool,
    /// Oceny innych użytkowników: +1 i −1.
    #[serde(default)]
    pub up: u32,
    #[serde(default)]
    pub down: u32,
}

impl Point {
    /// Bilans ocen.
    pub fn score(&self) -> i64 {
        self.up as i64 - self.down as i64
    }

    /// Data utworzenia do wyświetlenia („2026-11-18”).
    pub fn date(&self) -> &str {
        self.created_at.get(..10).unwrap_or("")
    }
}

/// Wpis czekający na wysłanie.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
enum Outgoing {
    Point { local: String },
    Comment { point: String, local: String },
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    #[serde(default)]
    client: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    mine: Vec<Point>,
    #[serde(default)]
    outbox: Vec<Outgoing>,
    #[serde(default)]
    cache: Vec<Point>,
    /// Własne oceny punktów (+1 / −1).
    #[serde(default)]
    votes: HashMap<String, i8>,
}

#[derive(Default)]
struct State {
    saved: Saved,
    server: HashMap<String, Point>,
    cells: HashMap<(i32, i32), Instant>,
    busy: bool,
    sending: bool,
    last_flush: Option<Instant>,
    error: Option<String>,
    version: u64,
    ctx: Option<eframe::egui::Context>,
}

/// Punkty społeczności (tani do klonowania).
#[derive(Clone, Default)]
pub struct Points {
    inner: Arc<Mutex<State>>,
}

fn file() -> std::path::PathBuf {
    crate::settings::config_dir().join("punkty.json")
}

fn now_iso() -> String {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    chrono::DateTime::from_timestamp(s, 0).map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string()).unwrap_or_default()
}

fn random_id() -> String {
    use sha2::Digest;
    let seed = format!("{:?}{:?}{}", std::time::SystemTime::now(), std::thread::current().id(), std::process::id());
    let h = sha2::Sha256::digest(seed.as_bytes());
    h.iter().take(12).map(|b| format!("{b:02x}")).collect()
}

fn agent() -> &'static ureq::Agent {
    crate::download::agent()
}

impl Points {
    pub fn load() -> Self {
        let mut saved: Saved = std::fs::read(file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        if saved.client.is_empty() {
            saved.client = random_id();
        }
        let server = saved.cache.drain(..).map(|p| (p.id.clone(), p)).collect();
        let me = Self { inner: Arc::new(Mutex::new(State { saved, server, ..Default::default() })) };
        me.persist();
        me
    }

    fn persist(&self) {
        let s = self.inner.lock().unwrap();
        let mut cache: Vec<Point> = s.server.values().cloned().collect();
        cache.truncate(CACHE_MAX);
        let out = serde_json::json!({
            "client": s.saved.client,
            "author": s.saved.author,
            "mine": s.saved.mine,
            "outbox": s.saved.outbox,
            "cache": cache,
            "votes": s.saved.votes,
        });
        drop(s);
        let _ = std::fs::create_dir_all(crate::settings::config_dir());
        let _ = crate::pack::write_file(&file(), out.to_string().as_bytes());
    }

    fn changed(s: &mut State) {
        s.version += 1;
        if let Some(c) = &s.ctx {
            c.request_repaint();
        }
    }

    pub fn version(&self) -> u64 {
        self.inner.lock().unwrap().version
    }

    pub fn author(&self) -> String {
        self.inner.lock().unwrap().saved.author.clone()
    }

    pub fn busy(&self) -> bool {
        let s = self.inner.lock().unwrap();
        s.busy || s.sending
    }

    pub fn error(&self) -> Option<String> {
        self.inner.lock().unwrap().error.clone()
    }

    pub fn queued(&self) -> usize {
        self.inner.lock().unwrap().saved.outbox.len()
    }

    /// Wszystkie punkty do pokazania: z serwera i własne (własne wersje mają pierwszeństwo).
    pub fn all(&self) -> Vec<Point> {
        let s = self.inner.lock().unwrap();
        let mut out: Vec<Point> = s.saved.mine.iter().filter(|p| p.status != Status::Hidden || p.mine).cloned().collect();
        for p in s.server.values() {
            if let Some(m) = out.iter_mut().find(|m| m.id == p.id) {
                // własny punkt już na serwerze: komentarze z serwera + własne oczekujące
                let own: Vec<Comment> = m.comments.iter().filter(|c| c.mine && c.status != Status::Approved).cloned().collect();
                let mut merged = p.clone();
                merged.mine = m.mine;
                merged.comments.extend(own.into_iter().filter(|c| !p.comments.iter().any(|x| x.id == c.id && !c.id.is_empty())));
                *m = merged;
            } else {
                let mut p = p.clone();
                // własne komentarze pod cudzymi punktami
                if let Some(own) = s.saved.mine.iter().find(|m| m.id == p.id) {
                    p.comments.extend(own.comments.iter().filter(|c| c.mine && c.status != Status::Approved).cloned());
                }
                out.push(p);
            }
        }
        out
    }

    pub fn get(&self, id: &str) -> Option<Point> {
        self.all().into_iter().find(|p| p.id == id)
    }

    /// Punkty z okolicy prostokąta (w, s, e, n); pobiera brakujące komórki w tle.
    pub fn want(&self, ctx: &eframe::egui::Context, w: f64, s_: f64, e: f64, n: f64) {
        let cells: Vec<(i32, i32)> = {
            let mut st = self.inner.lock().unwrap();
            if st.ctx.is_none() {
                st.ctx = Some(ctx.clone());
            }
            if st.busy || crate::offline::is_offline() {
                return;
            }
            let (x0, x1) = ((w / CELL).floor() as i32, (e / CELL).floor() as i32);
            let (y0, y1) = ((s_ / CELL).floor() as i32, (n / CELL).floor() as i32);
            if (x1 - x0) * (y1 - y0) > 64 {
                return; // widok z bardzo daleka – punkty pokazujemy dopiero bliżej
            }
            let cells: Vec<(i32, i32)> = (y0..=y1)
                .flat_map(|y| (x0..=x1).map(move |x| (x, y)))
                .filter(|c| st.cells.get(c).is_none_or(|t| t.elapsed() > REFRESH))
                .collect();
            if cells.is_empty() {
                return;
            }
            for c in &cells {
                st.cells.insert(*c, Instant::now());
            }
            st.busy = true;
            cells
        };
        let me = self.clone();
        std::thread::spawn(move || {
            let (mut w, mut s, mut e, mut n) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for (x, y) in &cells {
                w = w.min(*x as f64 * CELL);
                e = e.max((*x + 1) as f64 * CELL);
                s = s.min(*y as f64 * CELL);
                n = n.max((*y + 1) as f64 * CELL);
            }
            let r = fetch(w, s, e, n);
            let mut st = me.inner.lock().unwrap();
            st.busy = false;
            match r {
                Ok(list) => {
                    st.error = None;
                    // punkty z tego prostokąta, których już nie ma (ukryte), znikają
                    st.server.retain(|_, p| !(p.lon >= w && p.lon <= e && p.lat >= s && p.lat <= n));
                    for p in list {
                        // własny punkt zatwierdzony
                        if let Some(m) = st.saved.mine.iter_mut().find(|m| m.id == p.id) {
                            m.status = Status::Approved;
                            for c in &mut m.comments {
                                if p.comments.iter().any(|x| x.id == c.id) {
                                    c.status = Status::Approved;
                                }
                            }
                        }
                        st.server.insert(p.id.clone(), p);
                    }
                    Self::changed(&mut st);
                    drop(st);
                    me.persist();
                }
                Err(e) => {
                    log::warn!("punkty społeczności: {e}");
                    st.error = Some(e);
                    for c in &cells {
                        st.cells.remove(c);
                    }
                    Self::changed(&mut st);
                }
            }
        });
        self.flush();
    }

    /// Nowy punkt z pierwszym komentarzem; wysyłany od razu albo po powrocie sieci.
    pub fn add_point(&self, lon: f64, lat: f64, ele: Option<f32>, category: Category, title: &str, text: &str, author: &str) {
        let local = format!("local-{}", random_id());
        {
            let mut s = self.inner.lock().unwrap();
            s.saved.author = author.trim().to_string();
            let comment = Comment { id: String::new(), author: author.trim().into(), text: text.trim().into(), created_at: now_iso(), official: false, status: Status::Queued, mine: true };
            s.saved.mine.push(Point {
                id: local.clone(),
                lon,
                lat,
                ele,
                category,
                title: title.trim().into(),
                author: author.trim().into(),
                created_at: now_iso(),
                comments: if text.trim().is_empty() { Vec::new() } else { vec![comment] },
                status: Status::Queued,
                mine: true,
                up: 0,
                down: 0,
            });
            s.saved.outbox.push(Outgoing::Point { local });
            Self::changed(&mut s);
        }
        self.persist();
        self.flush_now();
    }

    /// Komentarz pod punktem (własnym albo cudzym).
    pub fn add_comment(&self, point: &Point, text: &str, author: &str) {
        let local = format!("local-{}", random_id());
        {
            let mut s = self.inner.lock().unwrap();
            s.saved.author = author.trim().to_string();
            let c = Comment { id: local.clone(), author: author.trim().into(), text: text.trim().into(), created_at: now_iso(), official: false, status: Status::Queued, mine: true };
            match s.saved.mine.iter_mut().find(|m| m.id == point.id) {
                Some(m) => m.comments.push(c),
                None => {
                    // cudzy punkt: kopia jako nośnik własnego komentarza
                    let mut p = point.clone();
                    p.mine = false;
                    p.comments = vec![c];
                    s.saved.mine.push(p);
                }
            }
            s.saved.outbox.push(Outgoing::Comment { point: point.id.clone(), local });
            Self::changed(&mut s);
        }
        self.persist();
        self.flush_now();
    }

    /// Moja ocena punktu: 1, −1 albo 0 (brak).
    pub fn my_vote(&self, id: &str) -> i8 {
        self.inner.lock().unwrap().saved.votes.get(id).copied().unwrap_or(0)
    }

    /// Ocena punktu (+1, −1, 0 – cofnięcie) wysyłana w tle; liczniki odświeżają się po odpowiedzi.
    pub fn vote(&self, id: &str, value: i8) -> Result<(), String> {
        if crate::offline::is_offline() {
            return Err(crate::i18n::tr("Tryb offline – ocenisz punkt po połączeniu z siecią.").into());
        }
        if id.starts_with("local-") {
            return Err(crate::i18n::tr("punkt nie został jeszcze wysłany").into());
        }
        let (client, prev) = {
            let mut s = self.inner.lock().unwrap();
            let prev = s.saved.votes.get(id).copied().unwrap_or(0);
            if value == 0 {
                s.saved.votes.remove(id);
            } else {
                s.saved.votes.insert(id.to_string(), value);
            }
            // od razu w liczniku (poprawione odpowiedzią serwera)
            if let Some(p) = s.server.get_mut(id) {
                let dec = |v: &mut u32| *v = v.saturating_sub(1);
                match prev {
                    1 => dec(&mut p.up),
                    -1 => dec(&mut p.down),
                    _ => {}
                }
                match value {
                    1 => p.up += 1,
                    -1 => p.down += 1,
                    _ => {}
                }
            }
            Self::changed(&mut s);
            (s.saved.client.clone(), prev)
        };
        self.persist();
        let me = self.clone();
        let id = id.to_string();
        std::thread::spawn(move || {
            let body = serde_json::json!({ "app": "gran-3d", "client": client, "value": value });
            let r = agent()
                .post(&format!("{API}/{}/vote", crate::osm::urlencode(&id)))
                .timeout(Duration::from_secs(20))
                .set("Content-Type", "application/json")
                .send_string(&body.to_string())
                .map_err(err)
                .and_then(json);
            let mut s = me.inner.lock().unwrap();
            match r {
                Ok(v) => {
                    if let Some(p) = s.server.get_mut(&id) {
                        p.up = v["up"].as_u64().unwrap_or(p.up as u64) as u32;
                        p.down = v["down"].as_u64().unwrap_or(p.down as u64) as u32;
                    }
                }
                Err(e) => {
                    log::warn!("ocena punktu: {e}");
                    s.error = Some(e);
                    if prev == 0 {
                        s.saved.votes.remove(&id);
                    } else {
                        s.saved.votes.insert(id.clone(), prev);
                    }
                }
            }
            Self::changed(&mut s);
        });
        Ok(())
    }

    /// Zgłoszenie punktu do moderacji (spam, nieprawda, obraźliwe).
    pub fn report(&self, id: &str, reason: &str) -> Result<(), String> {
        if crate::offline::is_offline() {
            return Err(crate::i18n::tr("Tryb offline – zgłoszenie wyślesz po połączeniu z siecią.").into());
        }
        let client = self.inner.lock().unwrap().saved.client.clone();
        let body = serde_json::json!({ "app": "gran-3d", "reason": reason, "client": client });
        agent()
            .post(&format!("{API}/{}/report", crate::osm::urlencode(id)))
            .timeout(Duration::from_secs(20))
            .set("Content-Type", "application/json").send_string(&body.to_string())
            .map(|_| ())
            .map_err(err)
    }

    /// Usunięcie własnego wpisu z tej instalacji (oczekujący nie zostanie wysłany).
    pub fn forget(&self, id: &str) {
        {
            let mut s = self.inner.lock().unwrap();
            s.saved.mine.retain(|p| p.id != id);
            s.saved.outbox.retain(|o| !matches!(o, Outgoing::Point { local } if local == id));
            Self::changed(&mut s);
        }
        self.persist();
    }

    fn flush_now(&self) {
        self.inner.lock().unwrap().last_flush = None;
        self.flush();
    }

    /// Wysyłka kolejki (najwyżej raz na minutę, gdy poprzednia się nie udała).
    pub fn flush(&self) {
        let (items, client) = {
            let mut s = self.inner.lock().unwrap();
            if s.sending || s.saved.outbox.is_empty() || crate::offline::is_offline() || s.last_flush.is_some_and(|t| t.elapsed() < Duration::from_secs(60)) {
                return;
            }
            s.sending = true;
            s.last_flush = Some(Instant::now());
            (s.saved.outbox.clone(), s.saved.client.clone())
        };
        let me = self.clone();
        std::thread::spawn(move || {
            for item in items {
                let r = me.send(&item, &client);
                let mut s = me.inner.lock().unwrap();
                match r {
                    Ok(()) => {
                        s.saved.outbox.retain(|o| !same(o, &item));
                        s.error = None;
                    }
                    Err(e) => {
                        log::warn!("wysyłka punktu: {e}");
                        s.error = Some(e);
                        Self::changed(&mut s);
                        break;
                    }
                }
                Self::changed(&mut s);
            }
            me.inner.lock().unwrap().sending = false;
            me.persist();
        });
    }

    fn send(&self, item: &Outgoing, client: &str) -> Result<(), String> {
        let base = |m: &mut serde_json::Map<String, serde_json::Value>| {
            m.insert("app".into(), "gran-3d".into());
            m.insert("appVersion".into(), crate::app_version().into());
            m.insert("platform".into(), crate::platform::NAME.into());
            m.insert("locale".into(), crate::i18n::current().into());
            m.insert("client".into(), client.into());
        };
        match item {
            Outgoing::Point { local } => {
                let p = self.inner.lock().unwrap().saved.mine.iter().find(|p| &p.id == local).cloned();
                let Some(p) = p else { return Ok(()) };
                let mut m = serde_json::Map::new();
                base(&mut m);
                m.insert("lon".into(), p.lon.into());
                m.insert("lat".into(), p.lat.into());
                if let Some(e) = p.ele {
                    m.insert("ele".into(), (e as f64).into());
                }
                m.insert("category".into(), serde_json::to_value(p.category).unwrap_or_default());
                m.insert("title".into(), p.title.clone().into());
                m.insert("text".into(), p.comments.first().map(|c| c.text.clone()).unwrap_or_default().into());
                m.insert("author".into(), p.author.clone().into());
                let v: serde_json::Value = agent().post(API).timeout(Duration::from_secs(20)).set("Content-Type", "application/json").send_string(&serde_json::Value::Object(m).to_string()).map_err(err).and_then(json)?;
                let id = v["id"].as_str().unwrap_or_default().to_string();
                let status = if v["status"] == "approved" { Status::Approved } else { Status::Pending };
                let comment_id = v["commentId"].as_str().unwrap_or_default().to_string();
                let mut s = self.inner.lock().unwrap();
                // kolejne wpisy w kolejce odnoszą się już do identyfikatora z serwera
                for o in &mut s.saved.outbox {
                    if let Outgoing::Comment { point, .. } = o {
                        if point == local {
                            *point = id.clone();
                        }
                    }
                }
                if let Some(m) = s.saved.mine.iter_mut().find(|p| &p.id == local) {
                    m.id = id;
                    m.status = status;
                    if let Some(c) = m.comments.first_mut() {
                        c.status = status;
                        c.id = comment_id;
                    }
                }
                Ok(())
            }
            Outgoing::Comment { point, local } => {
                if point.starts_with("local-") {
                    return Err(crate::i18n::tr("punkt nie został jeszcze wysłany").into());
                }
                let c = self.inner.lock().unwrap().saved.mine.iter().flat_map(|p| p.comments.iter()).find(|c| &c.id == local).cloned();
                let Some(c) = c else { return Ok(()) };
                let mut m = serde_json::Map::new();
                base(&mut m);
                m.insert("text".into(), c.text.clone().into());
                m.insert("author".into(), c.author.clone().into());
                let v: serde_json::Value = agent()
                    .post(&format!("{API}/{}/comments", crate::osm::urlencode(point)))
                    .timeout(Duration::from_secs(20))
                    .set("Content-Type", "application/json").send_string(&serde_json::Value::Object(m).to_string())
                    .map_err(err)
                    .and_then(json)?;
                let status = if v["status"] == "approved" { Status::Approved } else { Status::Pending };
                let id = v["id"].as_str().unwrap_or_default().to_string();
                let mut s = self.inner.lock().unwrap();
                for p in &mut s.saved.mine {
                    for cm in &mut p.comments {
                        if &cm.id == local {
                            cm.status = status;
                            cm.id = id.clone();
                        }
                    }
                }
                Ok(())
            }
        }
    }
}

fn same(a: &Outgoing, b: &Outgoing) -> bool {
    match (a, b) {
        (Outgoing::Point { local: x }, Outgoing::Point { local: y }) => x == y,
        (Outgoing::Comment { local: x, .. }, Outgoing::Comment { local: y, .. }) => x == y,
        _ => false,
    }
}

fn json(r: ureq::Response) -> Result<serde_json::Value, String> {
    let t = r.into_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&t).map_err(|e| e.to_string())
}

fn err(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(429, _) => crate::i18n::tr("Za dużo wpisów naraz – spróbuj za kilka minut.").into(),
        ureq::Error::Status(503, _) if crate::offline::is_offline() => crate::i18n::tr("tryb offline").into(),
        ureq::Error::Status(code, r) => {
            let msg = json(r).ok().and_then(|v| v["error"].as_str().map(String::from)).unwrap_or_default();
            if msg.is_empty() {
                tf!("serwer odpowiedział kodem {}", code)
            } else {
                msg
            }
        }
        ureq::Error::Transport(t) => tf!("brak połączenia z redzikowski.com: {}", t),
    }
}

/// Zatwierdzone punkty w prostokącie.
fn fetch(w: f64, s: f64, e: f64, n: f64) -> Result<Vec<Point>, String> {
    let url = format!("{API}?app=gran-3d&bbox={w:.4},{s:.4},{e:.4},{n:.4}");
    let v: serde_json::Value = agent().get(&url).timeout(Duration::from_secs(20)).call().map_err(err).and_then(json)?;
    let list = v["points"].as_array().cloned().unwrap_or_default();
    Ok(list.into_iter().filter_map(|p| serde_json::from_value::<Point>(p).ok()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_server_point() {
        let old: Point = serde_json::from_value(serde_json::json!({ "id": "x", "lon": 1.0, "lat": 2.0, "category": "parking", "title": "Stary" })).unwrap();
        assert_eq!(old.category, Category::Other, "usunięte rodzaje jako „Inne”");
        let v = serde_json::json!({
            "id": "abc", "lon": 20.0, "lat": 49.2, "ele": 1987.5, "category": "water", "title": "Źródło", "up": 5, "down": 2,
            "author": "Ania", "createdAt": "2026-11-18T10:00:00Z",
            "comments": [{ "id": "c1", "author": "Ania", "text": "Płynie", "createdAt": "2026-11-18T10:00:00Z" },
                         { "id": "c2", "author": "GRAŃ", "text": "Dzięki!", "official": true }]
        });
        let p: Point = serde_json::from_value(v).unwrap();
        assert_eq!(p.category, Category::Water);
        assert_eq!(p.comments.len(), 2);
        assert!(p.comments[1].official);
        assert_eq!(p.status, Status::Approved);
        assert_eq!(p.date(), "2026-11-18");
        assert_eq!(p.score(), 3);
    }

    #[test]
    fn outbox_roundtrip() {
        let o = vec![Outgoing::Point { local: "local-1".into() }, Outgoing::Comment { point: "x".into(), local: "local-2".into() }];
        let s = serde_json::to_string(&o).unwrap();
        let back: Vec<Outgoing> = serde_json::from_str(&s).unwrap();
        assert!(same(&back[0], &o[0]) && same(&back[1], &o[1]));
    }
}
