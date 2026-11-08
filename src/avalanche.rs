//! Komunikaty lawinowe dla Tatr: TOPR (Tatry Polskie, lawiny.topr.pl) i SLP HZS (Słowacja,
//! otwarte dane CAAML v6 z laviny.sk). Pobierane w tle, trzymane w pamięci przez 30 minut.
//! Program pokazuje skrót komunikatu ze źródłem i odnośnikiem do pełnej wersji.

use std::sync::{Arc, Mutex};
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
    /// „TOPR” albo „SLP HZS”.
    pub source: &'static str,
    pub url: &'static str,
    /// Obszar (np. „Tatry Polskie”, nazwy regionów słowackich).
    pub region: String,
    /// Stopień zagrożenia 0–5 (najwyższy w ciągu dnia).
    pub level: u8,
    /// Gdy stopień zależy od wysokości: granica (m) i stopnie powyżej / poniżej.
    pub split: Option<(u32, u8, u8)>,
    /// Stopnie rano i po południu, gdy się różnią.
    pub am_pm: Option<(u8, u8)>,
    pub problems: Vec<Problem>,
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
}

impl Avalanche {
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
        source: "TOPR",
        url: TOPR_URL,
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
    let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for b in v["bulletins"].as_array().into_iter().flatten() {
        let regions: Vec<String> = b["regions"].as_array().into_iter().flatten().filter_map(|r| r["name"].as_str().map(String::from)).collect();
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
        out.push(Bulletin {
            source: "SLP HZS",
            url: HZS_URL,
            region: regions.join(", "),
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
    }
}
