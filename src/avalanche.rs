//! Komunikaty lawinowe dla dowolnego miejsca:
//!  - Tatry: TOPR (lawiny.topr.pl) i SLP HZS (otwarte dane CAAML v6 z laviny.sk),
//!  - reszta Europy: służby zrzeszone w EAWS (Alpy, Pireneje, Skandynawia, Karpaty, Wyspy Brytyjskie…)
//!    – dzienne pliki CAAML v6 z static.avalanche.report i zarysy regionów z regions.avalanches.org (CC0),
//!  - USA: centra lawinowe z avalanche.org (stopień i zalecenia dla stref prognoz).
//!
//! Pobierane w tle, trzymane w pamięci przez 30 minut. Program pokazuje skrót komunikatu ze źródłem
//! i odnośnikiem do pełnej wersji.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::Value;

const TTL: Duration = Duration::from_secs(30 * 60);
pub const TOPR_URL: &str = "https://lawiny.topr.pl/";
pub const HZS_URL: &str = "https://www.laviny.sk/bulletin";

/// Problem lawinowy: rodzaj, wystawy (N, NE, E, SE, S, SW, W, NW), zakres wysokości.
#[derive(Clone, Debug, Default)]
pub struct Problem {
    pub kind: ProblemKind,
    pub aspects: [bool; 8],
    pub above: Option<u32>,
    pub below: Option<u32>,
    /// Pora dnia (np. „po południu”), gdy problem dotyczy jej części.
    pub period: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProblemKind {
    NewSnow,
    WindSlab,
    PersistentWeak,
    WetSnow,
    Gliding,
    Favourable,
    #[default]
    Other,
}

impl ProblemKind {
    pub fn label(self) -> &'static str {
        match self {
            ProblemKind::NewSnow => "Świeży śnieg",
            ProblemKind::WindSlab => "Deski śnieżne (wiatr)",
            ProblemKind::PersistentWeak => "Słabe warstwy w starym śniegu",
            ProblemKind::WetSnow => "Mokry śnieg",
            ProblemKind::Gliding => "Ślizgający się śnieg",
            ProblemKind::Favourable => "Brak wyraźnego problemu",
            ProblemKind::Other => "Inny problem",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Bulletin {
    /// Służba wydająca komunikat („TOPR”, „SLP HZS”, „Lawinenwarndienst Tirol”…).
    pub source: String,
    pub url: String,
    /// Obszar (np. „Tatry Polskie”, nazwy regionów słowackich).
    pub region: String,
    /// Stopień zagrożenia 0–5 (najwyższy w ciągu dnia).
    pub level: u8,
    /// Gdy stopień zależy od wysokości: granica (m) i stopnie powyżej / poniżej.
    pub split: Option<(u32, u8, u8)>,
    /// Stopnie rano i po południu, gdy się różnią.
    pub am_pm: Option<(u8, u8)>,
    pub problems: Vec<Problem>,
    /// Identyfikatory regionów EAWS z komunikatu (np. „AT-07-07”).
    pub region_ids: Vec<String>,
    pub issued: String,
    pub valid_until: String,
    /// Najważniejsze zdanie i pełny komentarz.
    pub highlight: String,
    pub comment: String,
    pub snowpack: String,
    pub tendency: String,
}

pub fn level_name(l: u8) -> &'static str {
    match l {
        1 => "Niskie",
        2 => "Umiarkowane",
        3 => "Znaczne",
        4 => "Wysokie",
        5 => "Bardzo wysokie",
        _ => "Brak zagrożenia",
    }
}

/// Barwy skali europejskiej (EAWS).
pub fn level_rgb(l: u8) -> [u8; 3] {
    match l {
        1 => [204, 255, 102],
        2 => [255, 255, 0],
        3 => [255, 153, 0],
        4 => [255, 0, 0],
        5 => [140, 0, 0],
        _ => [140, 147, 158],
    }
}

#[derive(Clone)]
pub enum State {
    Loading,
    Ready(Arc<Vec<Bulletin>>),
    Failed(String),
}

#[derive(Clone, Default)]
pub struct Avalanche {
    slot: Arc<Mutex<Option<(Instant, Option<Result<Arc<Vec<Bulletin>>, String>>)>>>,
    /// Komunikaty poza Tatrami: klucz regionu → wynik.
    regional: Arc<Mutex<HashMap<String, (Instant, Option<Result<Arc<Vec<Bulletin>>, String>>)>>>,
}

/// Skąd wziąć komunikat dla punktu.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// Tatry: TOPR i SLP HZS.
    Tatras,
    /// Region EAWS (np. „AT-07”, „CH”, „FR”).
    Eaws(String),
    /// Strefa prognozy avalanche.org (indeks w warstwie).
    Us,
    /// Brak otwartego źródła dla tego miejsca.
    None,
}

/// Komunikaty dla punktu albo informacja, że dla tego miejsca nie ma źródła.
pub enum AtState {
    Loading,
    Ready(Arc<Vec<Bulletin>>),
    Failed(String),
    NoService,
}

pub fn in_tatras(lon: f64, lat: f64) -> bool {
    (19.4..=20.6).contains(&lon) && (48.75..=49.45).contains(&lat)
}

fn in_ring(lon: f64, lat: f64, ring: &[[f64; 2]]) -> bool {
    let mut inside = false;
    let mut j = ring.len().wrapping_sub(1);
    for i in 0..ring.len() {
        let ([xi, yi], [xj, yj]) = (ring[i], ring[j]);
        if (yi > lat) != (yj > lat) && lon < (xj - xi) * (lat - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn ring_area(r: &[[f64; 2]]) -> f64 {
    let mut s = 0.0;
    for i in 0..r.len() {
        let j = (i + r.len() - 1) % r.len();
        s += r[i][0] * r[j][1] - r[j][0] * r[i][1];
    }
    s.abs() / 2.0
}

#[derive(serde::Deserialize)]
struct EawsRegion {
    id: String,
    rings: Vec<Vec<[f64; 2]>>,
}

#[derive(serde::Deserialize)]
struct EawsRegions {
    regions: Vec<EawsRegion>,
}

fn eaws_regions() -> &'static [EawsRegion] {
    static R: OnceLock<Vec<EawsRegion>> = OnceLock::new();
    R.get_or_init(|| serde_json::from_str::<EawsRegions>(include_str!("../assets/eaws_regions.json")).map(|r| r.regions).unwrap_or_default())
}

/// Najmniejszy region EAWS zawierający punkt (np. „PL-12” zamiast „PL”).
pub fn eaws_region_at(lon: f64, lat: f64) -> Option<&'static str> {
    eaws_regions()
        .iter()
        .flat_map(|r| r.rings.iter().filter(|g| in_ring(lon, lat, g)).map(move |g| (r.id.as_str(), ring_area(g))))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

/// Strefy prognoz avalanche.org (wczytane raz na 30 minut): właściwości i pierścienie.
type UsZones = Arc<Vec<(Value, Vec<Vec<[f64; 2]>>)>>;

fn us_zones() -> &'static Mutex<Option<(Instant, UsZones)>> {
    static Z: OnceLock<Mutex<Option<(Instant, UsZones)>>> = OnceLock::new();
    Z.get_or_init(Default::default)
}

/// Pierścienie zewnętrzne z geometrii GeoJSON (Polygon, MultiPolygon).
fn geo_rings(g: &Value) -> Vec<Vec<[f64; 2]>> {
    let ring = |r: &Value| -> Vec<[f64; 2]> {
        r.as_array().into_iter().flatten().filter_map(|p| Some([p[0].as_f64()?, p[1].as_f64()?])).collect()
    };
    match g["type"].as_str() {
        Some("Polygon") => g["coordinates"].as_array().and_then(|a| a.first()).map(ring).into_iter().collect(),
        Some("MultiPolygon") => g["coordinates"].as_array().into_iter().flatten().filter_map(|p| p.as_array().and_then(|a| a.first()).map(ring)).collect(),
        _ => Vec::new(),
    }
}

impl Avalanche {
    /// Skąd pochodzi komunikat dla punktu (bez pobierania danych, poza strefami USA w pamięci).
    pub fn source_at(&self, lon: f64, lat: f64) -> Source {
        if in_tatras(lon, lat) {
            return Source::Tatras;
        }
        if let Some(id) = eaws_region_at(lon, lat) {
            return Source::Eaws(id.to_string());
        }
        // USA i Alaska (zgrubnie); strefy sprawdzane po pobraniu warstwy
        if (-170.0..=-65.0).contains(&lon) && (24.0..=72.0).contains(&lat) {
            return Source::Us;
        }
        Source::None
    }

    /// Komunikaty dla punktu (pobierane w tle; wołaj co klatkę).
    pub fn at(&self, ctx: &eframe::egui::Context, lon: f64, lat: f64) -> AtState {
        let key = match self.source_at(lon, lat) {
            Source::Tatras => {
                return match self.get(ctx) {
                    State::Loading => AtState::Loading,
                    State::Ready(b) => AtState::Ready(b),
                    State::Failed(e) => AtState::Failed(e),
                }
            }
            Source::None => return AtState::NoService,
            Source::Eaws(id) => {
                let (day, _) = crate::tz::now_at(lon, lat);
                // mikroregion dokładniej niż co ~2 km
                format!("eaws:{id}:{:04}-{:02}-{:02}:{:.2}:{:.2}", day.y, day.m, day.d, lon, lat)
            }
            Source::Us => format!("us:{:.2}:{:.2}", lon, lat),
        };
        let mut g = self.regional.lock().unwrap();
        match g.get(&key) {
            Some((_, None)) => return AtState::Loading,
            Some((at, Some(r))) if at.elapsed() < TTL => {
                return match r {
                    Ok(b) if b.is_empty() && key.starts_with("us:") => AtState::NoService,
                    Ok(b) => AtState::Ready(b.clone()),
                    Err(e) => AtState::Failed(e.clone()),
                }
            }
            _ => {}
        }
        g.insert(key.clone(), (Instant::now(), None));
        drop(g);
        let (slot, ctx) = (self.regional.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = if key.starts_with("us:") { us_at(lon, lat) } else { eaws_at(&key, lon, lat) };
            if let Err(e) = &r {
                log::warn!("lawiny {key}: {e}");
            }
            slot.lock().unwrap().insert(key, (Instant::now(), Some(r.map(Arc::new))));
            ctx.request_repaint();
        });
        AtState::Loading
    }
    /// Komunikaty TOPR i HZS (pobrane w tle; wołaj co klatkę).
    pub fn get(&self, ctx: &eframe::egui::Context) -> State {
        let mut g = self.slot.lock().unwrap();
        match &*g {
            Some((_, None)) => return State::Loading,
            Some((at, Some(r))) if at.elapsed() < TTL => {
                return match r {
                    Ok(b) => State::Ready(b.clone()),
                    Err(e) => State::Failed(e.clone()),
                }
            }
            _ => {}
        }
        *g = Some((Instant::now(), None));
        drop(g);
        let (slot, ctx) = (self.slot.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut out = Vec::new();
            let mut errs = Vec::new();
            match topr() {
                Ok(b) => out.push(b),
                Err(e) => errs.push(format!("TOPR: {e}")),
            }
            match hzs() {
                Ok(mut b) => out.append(&mut b),
                Err(e) => errs.push(format!("HZS: {e}")),
            }
            for e in &errs {
                log::warn!("lawiny: {e}");
            }
            let r = if out.is_empty() && !errs.is_empty() { Err(errs.join("; ")) } else { Ok(Arc::new(out)) };
            *slot.lock().unwrap() = Some((Instant::now(), Some(r)));
            ctx.request_repaint();
        });
        State::Loading
    }
}

fn get_text(url: &str) -> Result<String, String> {
    crate::download::agent()
        .get(url)
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(c, _) => format!("HTTP {c}"),
            e => e.to_string(),
        })?
        .into_string()
        .map_err(|e| e.to_string())
}

fn num(v: &Value) -> u8 {
    v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).unwrap_or(0).min(5) as u8
}

fn topr_problem(code: &str) -> ProblemKind {
    match code {
        "prns" => ProblemKind::NewSnow,
        "prwd" => ProblemKind::WindSlab,
        "prwl" => ProblemKind::PersistentWeak,
        "prws" | "prwt" => ProblemKind::WetSnow,
        "prgs" => ProblemKind::Gliding,
        "prnn" => ProblemKind::Favourable,
        _ => ProblemKind::Other,
    }
}

/// Komunikat TOPR: dane osadzone w stronie jako obiekt `oLawReport`.
fn topr() -> Result<Bulletin, String> {
    let html = get_text(TOPR_URL)?;
    parse_topr(&html)
}

pub fn parse_topr(html: &str) -> Result<Bulletin, String> {
    let at = html.find("oLawReport").ok_or("brak komunikatu na stronie")?;
    let start = at + html[at..].find('{').ok_or("brak danych komunikatu")?;
    let v: Value = serde_json::Deserializer::from_str(&html[start..])
        .into_iter::<Value>()
        .next()
        .ok_or("brak danych komunikatu")?
        .map_err(|e| e.to_string())?;
    let mst = &v["mst"];
    let mut b = Bulletin {
        source: "TOPR".into(),
        url: TOPR_URL.into(),
        region: "Tatry Polskie".into(),
        level: num(&mst["lev"]),
        issued: v["iat"].as_str().unwrap_or("").to_string(),
        valid_until: v["exp"].as_str().unwrap_or("").to_string(),
        highlight: mst["desc2"].as_str().unwrap_or("").to_string(),
        comment: v["comment"].as_str().unwrap_or("").to_string(),
        ..Default::default()
    };
    let half = |k: &str| -> (u8, u8, Option<u32>, Vec<Problem>) {
        let o = &v[k];
        let (up, lo) = (&o["upper"], &o["lower"]);
        let mode = o["mode"].as_u64().unwrap_or(1);
        let height = o["height"].as_u64().map(|h| h as u32).filter(|_| mode == 2);
        let mut probs = Vec::new();
        for (part, above, below) in [(up, height, None), (lo, None, height)] {
            let code = part["prb"].as_str().unwrap_or("");
            if code.is_empty() || code == "brak" || num(&part["lev"]) == 0 {
                continue;
            }
            let exp = part["exp"].as_str().unwrap_or("");
            let mut aspects = [false; 8];
            for (i, c) in exp.chars().take(8).enumerate() {
                aspects[i] = c == '1';
            }
            probs.push(Problem { kind: topr_problem(code), aspects, above, below, period: String::new() });
        }
        (num(&up["lev"]), num(&lo["lev"]), height, probs)
    };
    let (am_up, am_lo, height, mut probs) = half("am");
    let (pm_up, pm_lo, _, pm_probs) = half("pm");
    let am = am_up.max(am_lo);
    let pm = pm_up.max(pm_lo);
    if am != pm && pm > 0 {
        b.am_pm = Some((am, pm));
        for mut p in pm_probs {
            if !probs.iter().any(|q| q.kind == p.kind && q.aspects == p.aspects) {
                p.period = "po południu".into();
                probs.push(p);
            }
        }
    }
    if let Some(h) = height {
        if am_lo > 0 && am_up != am_lo {
            b.split = Some((h, am_up, am_lo));
        }
    }
    b.level = b.level.max(am).max(pm);
    b.problems = probs;
    Ok(b)
}

/// Mikroregion EAWS zawierający punkt (z pliku regions.avalanches.org, w pamięci na czas działania).
fn micro_region(region: &str, lon: f64, lat: f64) -> Option<String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<Vec<(String, Vec<Vec<[f64; 2]>>)>>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let cached = cache.lock().unwrap().get(region).cloned();
    let list = match cached {
        Some(l) => l,
        None => {
            let text = get_text(&format!("https://regions.avalanches.org/micro-regions/{region}_micro-regions.geojson.json")).ok()?;
            let v: Value = serde_json::from_str(&text).ok()?;
            let l: Vec<(String, Vec<Vec<[f64; 2]>>)> = v["features"]
                .as_array()
                .into_iter()
                .flatten()
                // historyczne wersje granic mają datę końca
                .filter(|f| f["properties"]["end_date"].is_null())
                .filter_map(|f| Some((f["properties"]["id"].as_str().or_else(|| f["id"].as_str())?.to_string(), geo_rings(&f["geometry"]))))
                .collect();
            let l = Arc::new(l);
            cache.lock().unwrap().insert(region.to_string(), l.clone());
            l
        }
    };
    list.iter().find(|(_, rings)| rings.iter().any(|r| in_ring(lon, lat, r))).map(|(id, _)| id.clone())
}

/// Pliki komunikatów EAWS z danego dnia (nazwy dostawców, np. „AT-07”, „CH”, „IT-MeteoMont”).
fn eaws_providers(date: &str) -> Result<Vec<String>, String> {
    let html = get_text(&format!("https://static.avalanche.report/eaws_bulletins/{date}/"))?;
    let prefix = format!("./{date}-");
    let mut out: Vec<String> = html
        .split("href=\"")
        .skip(1)
        .filter_map(|s| s.split('"').next())
        .filter_map(|h| h.strip_prefix(&prefix))
        .filter_map(|h| h.strip_suffix(".json"))
        .filter(|h| !h.contains('.'))
        .map(String::from)
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}

/// Komunikat EAWS dla punktu: region z wbudowanych zarysów, mikroregion z regions.avalanches.org,
/// biuletyn dostawcy z dzisiejszego (albo wczorajszego) katalogu.
fn eaws_at(key: &str, lon: f64, lat: f64) -> Result<Vec<Bulletin>, String> {
    let region = key.split(':').nth(1).unwrap_or("").to_string();
    let micro = micro_region(&region, lon, lat);
    let (today, _) = crate::tz::now_at(lon, lat);
    let mut last = String::new();
    for d in [today, today.add_days(-1)] {
        let date = format!("{:04}-{:02}-{:02}", d.y, d.m, d.d);
        let providers = match eaws_providers(&date) {
            Ok(p) => p,
            Err(e) => {
                last = e;
                continue;
            }
        };
        // najpierw dostawcy o pasującej nazwie, potem pozostali (np. wspólne pliki kilku regionów)
        let country = region.split('-').next().unwrap_or("");
        let first: Vec<&String> = providers.iter().filter(|p| region.starts_with(p.as_str()) || p.starts_with(&region)).collect();
        let rest: Vec<&String> = providers.iter().filter(|p| !first.contains(p) && p.starts_with(country)).collect();
        let order: Vec<&String> = first.into_iter().chain(rest).collect();
        for p in order {
            let Ok(text) = get_text(&format!("https://static.avalanche.report/eaws_bulletins/{date}/{date}-{p}.json")) else { continue };
            let Ok(all) = parse_caaml_from(&text, p, "https://www.avalanches.org/") else { continue };
            let pick: Vec<Bulletin> = all
                .iter()
                .filter(|b| match &micro {
                    Some(m) => b.region_ids.iter().any(|r| r == m),
                    None => b.region_ids.iter().any(|r| r.starts_with(&region)),
                })
                .cloned()
                .collect();
            if !pick.is_empty() {
                return Ok(pick);
            }
        }
    }
    if !last.is_empty() && !last.contains("404") {
        return Err(last);
    }
    // poza sezonem albo region bez dzisiejszego komunikatu
    Ok(Vec::new())
}

/// Komunikat avalanche.org dla punktu (strefa prognozy z warstwy mapy).
fn us_at(lon: f64, lat: f64) -> Result<Vec<Bulletin>, String> {
    let zones = {
        let g = us_zones().lock().unwrap();
        g.as_ref().filter(|(at, _)| at.elapsed() < TTL).map(|(_, z)| z.clone())
    };
    let zones = match zones {
        Some(z) => z,
        None => {
            let text = get_text("https://api.avalanche.org/v2/public/products/map-layer")?;
            let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            let z: UsZones = Arc::new(v["features"].as_array().into_iter().flatten().map(|f| (f["properties"].clone(), geo_rings(&f["geometry"]))).collect());
            *us_zones().lock().unwrap() = Some((Instant::now(), z.clone()));
            z
        }
    };
    Ok(zones
        .iter()
        .filter(|(_, rings)| rings.iter().any(|r| in_ring(lon, lat, r)))
        .take(1)
        .map(|(p, _)| us_bulletin(p))
        .collect())
}

fn us_bulletin(p: &Value) -> Bulletin {
    let s = |k: &str| p[k].as_str().unwrap_or("").trim().to_string();
    let off = p["off_season"].as_bool().unwrap_or(false);
    Bulletin {
        source: s("center"),
        url: if s("link").is_empty() { s("center_link") } else { s("link") },
        region: s("name"),
        level: p["danger_level"].as_i64().unwrap_or(0).clamp(0, 5) as u8,
        highlight: if off { crate::i18n::tr("Poza sezonem – centrum nie wydaje prognoz.").into() } else { s("travel_advice") },
        valid_until: s("end_date"),
        issued: s("start_date"),
        ..Default::default()
    }
}

/// Komunikaty SLP HZS na dziś (a gdy jeszcze ich nie ma – z wczoraj).
fn hzs() -> Result<Vec<Bulletin>, String> {
    let today = crate::sun::Day::today();
    let mut last = String::new();
    for d in [today, today.add_days(-1)] {
        let date = format!("{:04}-{:02}-{:02}", d.y, d.m, d.d);
        let url = format!("https://static.laviny.sk/bulletins/{date}/{date}_SK_en_CAAMLv6_2022.json");
        match get_text(&url) {
            Ok(t) => return parse_caaml(&t),
            Err(e) => last = e,
        }
    }
    if last.contains("404") {
        // poza sezonem SLP HZS nie wydaje komunikatów
        return Ok(Vec::new());
    }
    Err(last)
}

fn aspect_index(a: &str) -> Option<usize> {
    ["N", "NE", "E", "SE", "S", "SW", "W", "NW"].iter().position(|x| *x == a)
}

fn caaml_level(s: &str) -> u8 {
    match s {
        "low" => 1,
        "moderate" => 2,
        "considerable" => 3,
        "high" => 4,
        "very_high" => 5,
        _ => 0,
    }
}

fn caaml_problem(s: &str) -> ProblemKind {
    match s {
        "new_snow" => ProblemKind::NewSnow,
        "wind_slab" => ProblemKind::WindSlab,
        "persistent_weak_layers" => ProblemKind::PersistentWeak,
        "wet_snow" => ProblemKind::WetSnow,
        "gliding_snow" => ProblemKind::Gliding,
        "favourable_situation" | "no_distinct_avalanche_problem" => ProblemKind::Favourable,
        _ => ProblemKind::Other,
    }
}

fn bound(v: &Value) -> Option<u32> {
    v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_u64().map(|x| x as u32))
}

pub fn parse_caaml(text: &str) -> Result<Vec<Bulletin>, String> {
    parse_caaml_from(text, "SLP HZS", HZS_URL)
}

/// Biuletyny CAAML v6 (JSON). `source` i `url` – gdy plik nie podaje własnych.
pub fn parse_caaml_from(text: &str, source: &str, url: &str) -> Result<Vec<Bulletin>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for b in v["bulletins"].as_array().into_iter().flatten() {
        let regions: Vec<String> = b["regions"].as_array().into_iter().flatten().filter_map(|r| r["name"].as_str().map(String::from)).collect();
        let region_ids: Vec<String> = b["regions"].as_array().into_iter().flatten().filter_map(|r| r["regionID"].as_str().map(String::from)).collect();
        let provider = b["source"]["provider"]["name"].as_str().or_else(|| b["source"]["person"]["name"].as_str()).unwrap_or(source).to_string();
        let website = b["source"]["provider"]["website"].as_str().unwrap_or(url).to_string();
        let mut level = 0u8;
        let mut split = None;
        let (mut am, mut pm) = (0u8, 0u8);
        for r in b["dangerRatings"].as_array().into_iter().flatten() {
            let l = caaml_level(r["mainValue"].as_str().unwrap_or(""));
            level = level.max(l);
            match r["validTimePeriod"].as_str() {
                Some("earlier") => am = am.max(l),
                Some("later") => pm = pm.max(l),
                _ => {}
            }
            if let Some(lo) = bound(&r["elevation"]["lowerBound"]) {
                let below = b["dangerRatings"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|x| bound(&x["elevation"]["upperBound"]).is_some())
                    .map(|x| caaml_level(x["mainValue"].as_str().unwrap_or("")))
                    .max()
                    .unwrap_or(0);
                if below > 0 && below != l {
                    split = Some((lo, l, below));
                }
            }
        }
        let problems = b["avalancheProblems"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| {
                let mut aspects = [false; 8];
                for a in p["aspects"].as_array().into_iter().flatten().filter_map(|a| a.as_str()) {
                    if let Some(i) = aspect_index(a) {
                        aspects[i] = true;
                    }
                }
                Problem {
                    kind: caaml_problem(p["problemType"].as_str().unwrap_or("")),
                    aspects,
                    above: bound(&p["elevation"]["lowerBound"]),
                    below: bound(&p["elevation"]["upperBound"]),
                    period: match p["validTimePeriod"].as_str() {
                        Some("later") => "po południu".into(),
                        Some("earlier") => "rano".into(),
                        _ => String::new(),
                    },
                }
            })
            .collect();
        let txt = |a: &Value| a.as_str().unwrap_or("").trim().to_string();
        let region = if regions.is_empty() {
            let mut ids: Vec<String> = region_ids.iter().take(4).cloned().collect();
            if region_ids.len() > 4 {
                ids.push("…".into());
            }
            ids.join(", ")
        } else {
            regions.join(", ")
        };
        out.push(Bulletin {
            source: provider,
            url: website,
            region,
            region_ids,
            level,
            split,
            am_pm: (am > 0 && pm > 0 && am != pm).then_some((am, pm)),
            problems,
            issued: txt(&b["publicationTime"]).replace('T', " ").trim_end_matches('Z').chars().take(16).collect(),
            valid_until: txt(&b["validTime"]["endTime"]).replace('T', " ").trim_end_matches('Z').chars().take(16).collect(),
            highlight: txt(&b["avalancheActivity"]["highlights"]),
            comment: txt(&b["avalancheActivity"]["comment"]),
            snowpack: txt(&b["snowpackStructure"]["comment"]),
            tendency: b["tendency"]
                .as_array()
                .and_then(|a| a.first())
                .map(|t| txt(&t["highlights"]))
                .or_else(|| Some(txt(&b["tendency"]["highlights"])))
                .unwrap_or_default(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topr_page() {
        let html = r#"<script>    const oLawReport = {"iat":"2026-11-07 16:01","exp":"2026-11-08 20:00","mst":{"lev":1,"desc2":"Na ogół dogodne warunki."},"am":{"mode":2,"height":1800,"upper":{"lev":2,"prb":"prwd","exp":"11000001"},"lower":{"lev":1,"prb":"prgs","exp":"00011100"}},"pm":{"mode":2,"height":1800,"upper":{"lev":3,"prb":"prws","exp":"00111000"},"lower":{"lev":1,"prb":"brak","exp":"00000000"}},"comment":"Uwaga."};
        const aCam = [];</script>"#;
        let b = parse_topr(html).unwrap();
        assert_eq!(b.level, 3);
        assert_eq!(b.split, Some((1800, 2, 1)));
        assert_eq!(b.am_pm, Some((2, 3)));
        assert_eq!(b.problems[0].kind, ProblemKind::WindSlab);
        assert!(b.problems[0].aspects[0] && b.problems[0].aspects[7] && !b.problems[0].aspects[3]);
        assert_eq!(b.problems.len(), 3);
        assert_eq!(b.comment, "Uwaga.");
    }

    #[test]
    fn caaml() {
        let j = r#"{"bulletins":[{"regions":[{"name":"Západné Tatry"}],"publicationTime":"2026-02-28T16:00:00Z","validTime":{"endTime":"2026-03-01T23:00:00Z"},"dangerRatings":[{"mainValue":"considerable","elevation":{"lowerBound":"1800"}},{"mainValue":"moderate","elevation":{"upperBound":"1800"}}],"avalancheProblems":[{"problemType":"wind_slab","aspects":["N","NE"],"elevation":{"lowerBound":"1800"}}],"avalancheActivity":{"highlights":"Pozor","comment":"Text"},"tendency":[{"highlights":"rising"}]}]}"#;
        let b = parse_caaml(j).unwrap();
        assert_eq!(b[0].level, 3);
        assert_eq!(b[0].split, Some((1800, 3, 2)));
        assert_eq!(b[0].problems[0].above, Some(1800));
        assert!(b[0].problems[0].aspects[1]);
        assert_eq!(b[0].tendency, "rising");
        assert_eq!(b[0].valid_until, "2026-03-01 23:00");
        assert_eq!(b[0].source, "SLP HZS");
    }

    #[test]
    fn eaws_regions_cover_europe() {
        assert_eq!(eaws_region_at(11.39, 47.26), Some("AT-07")); // Innsbruck
        assert_eq!(eaws_region_at(7.75, 46.02), Some("CH")); // Zermatt
        assert_eq!(eaws_region_at(6.87, 45.92), Some("FR")); // Chamonix
        assert_eq!(eaws_region_at(11.35, 46.5), Some("IT-32-BZ")); // Bolzano
        assert_eq!(eaws_region_at(-105.27, 40.01), None);
    }

    #[test]
    fn sources_by_place() {
        let a = Avalanche::default();
        assert_eq!(a.source_at(19.95, 49.23), Source::Tatras);
        assert_eq!(a.source_at(11.39, 47.26), Source::Eaws("AT-07".into()));
        assert_eq!(a.source_at(-106.0, 39.6), Source::Us);
        assert_eq!(a.source_at(86.92, 27.99), Source::None);
    }

    #[test]
    fn eaws_bulletin_with_region_ids() {
        let j = r#"{"bulletins":[{"regions":[{"regionID":"PL-12"}],"source":{"provider":{"name":"TOPR","website":"https://lawiny.topr.pl"}},"dangerRatings":[{"mainValue":"low","elevation":{"lowerBound":"1800"},"validTimePeriod":"all_day"},{"mainValue":"no_snow","elevation":{"upperBound":"1800"}}],"publicationTime":"2026-11-10T16:33:00"}]}"#;
        let b = parse_caaml_from(j, "PL-12", "https://www.avalanches.org/").unwrap();
        assert_eq!(b[0].region_ids, vec!["PL-12".to_string()]);
        assert_eq!(b[0].region, "PL-12");
        assert_eq!(b[0].source, "TOPR");
        assert_eq!(b[0].level, 1);
    }

    #[test]
    fn us_zone_bulletin() {
        let p: Value = serde_json::from_str(r#"{"name":"Vail & Summit County","center":"Colorado Avalanche Information Center","link":"https://avalanche.state.co.us/","danger_level":3,"travel_advice":"Dangerous conditions.","off_season":false}"#).unwrap();
        let b = us_bulletin(&p);
        assert_eq!(b.level, 3);
        assert_eq!(b.region, "Vail & Summit County");
        assert_eq!(b.url, "https://avalanche.state.co.us/");
    }
}
