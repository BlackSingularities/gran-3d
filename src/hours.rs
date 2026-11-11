//! Godziny otwarcia z OSM (`opening_hours`): najczęstsza część składni – miesiące i dni miesiąca,
//! dni tygodnia, przedziały godzin (także po północy), `24/7`, `off`. Reguły czytane po kolei,
//! późniejsza reguła dla tego samego dnia zastępuje wcześniejszą (jak w specyfikacji OSM).
//! Składnia, której program nie rozumie (wschód słońca, święta, tygodnie), daje `None` –
//! wtedy interfejs pokazuje sam tekst z OSM.

use crate::sun::{days_from_civil, Day};

const DAYS: [&str; 7] = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// Data w roku: (miesiąc 1–12, dzień 1–31).
type MonthDay = (u32, u32);

#[derive(Clone, Debug, PartialEq)]
struct Rule {
    /// Zakresy dat (włącznie; zakres może przechodzić przez Nowy Rok). Puste – cały rok.
    dates: Vec<(MonthDay, MonthDay)>,
    /// Dni tygodnia (0 = poniedziałek). Wszystkie `true`, gdy reguła ich nie podaje.
    days: [bool; 7],
    /// Przedziały w minutach od północy; koniec może przekraczać 1440 (po północy). Puste – zamknięte.
    spans: Vec<(u16, u16)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hours {
    rules: Vec<Rule>,
}

/// Stan w danej chwili.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    pub open: bool,
    /// Najbliższa zmiana: za ile dni (0 – dziś) i o której (minuty od północy).
    pub change: Option<(u32, u16)>,
}

fn month(s: &str) -> Option<u32> {
    MONTHS.iter().position(|m| m.eq_ignore_ascii_case(s)).map(|i| i as u32 + 1)
}

fn weekday(s: &str) -> Option<usize> {
    DAYS.iter().position(|d| d.eq_ignore_ascii_case(s))
}

fn time(s: &str) -> Option<u16> {
    let (h, m) = s.split_once(':')?;
    let (h, m): (u16, u16) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
    (h <= 24 && m < 60 && h * 60 + m <= 1440).then_some(h * 60 + m)
}

/// „May”, „May 15”, „Dec 24” → (miesiąc, dzień); `end` – dzień domyślny na koniec miesiąca.
fn month_day(s: &str, end: bool) -> Option<MonthDay> {
    let mut it = s.split_whitespace();
    let m = month(it.next()?)?;
    let d = match it.next() {
        Some(d) => d.parse().ok().filter(|d| (1..=31).contains(d))?,
        None if end => 31,
        None => 1,
    };
    it.next().is_none().then_some((m, d))
}

/// Selektor dat: „May-Sep”, „Nov 01-Jun 15”, „Jan,Mar”, „Dec 24”.
fn dates(s: &str) -> Option<Vec<(MonthDay, MonthDay)>> {
    let mut out = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        match part.split_once('-') {
            Some((a, b)) => {
                let a = month_day(a.trim(), false)?;
                let b = b.trim();
                // „Jun 01-15” – drugi koniec w tym samym miesiącu
                let b = match b.parse::<u32>() {
                    Ok(d) => (a.0, d),
                    Err(_) => month_day(b, true)?,
                };
                out.push((a, b));
            }
            None => {
                let a = month_day(part, false)?;
                let b = if part.split_whitespace().count() == 1 { (a.0, 31) } else { a };
                out.push((a, b));
            }
        }
    }
    Some(out)
}

/// Selektor dni tygodnia: „Mo-Fr”, „Sa,Su”, „Mo,We-Fr”. `PH` i `SH` (święta) są pomijane.
fn days(s: &str) -> Option<[bool; 7]> {
    let mut out = [false; 7];
    let mut any = false;
    for part in s.split(',') {
        let part = part.trim();
        if part.eq_ignore_ascii_case("PH") || part.eq_ignore_ascii_case("SH") {
            continue;
        }
        match part.split_once('-') {
            Some((a, b)) => {
                let (a, b) = (weekday(a)?, weekday(b)?);
                let mut i = a;
                loop {
                    out[i] = true;
                    if i == b {
                        break;
                    }
                    i = (i + 1) % 7;
                }
            }
            None => out[weekday(part)?] = true,
        }
        any = true;
    }
    any.then_some(out)
}

fn spans(s: &str) -> Option<Vec<(u16, u16)>> {
    let mut out = Vec::new();
    for part in s.split(',') {
        let (a, b) = part.trim().split_once('-')?;
        let (a, mut b) = (time(a)?, time(b)?);
        if b <= a {
            b += 1440;
        }
        out.push((a, b));
    }
    Some(out)
}

fn is_date_token(t: &str) -> bool {
    t.split(['-', ',']).next().is_some_and(|w| month(w.split_whitespace().next().unwrap_or("")).is_some())
}

fn is_day_token(t: &str) -> bool {
    t.split(['-', ',']).all(|w| weekday(w.trim()).is_some() || matches!(w.trim(), "PH" | "SH"))
}

fn rule(r: &str) -> Option<Option<Rule>> {
    // komentarze w cudzysłowie
    let mut r = r.to_string();
    while let (Some(a), Some(b)) = (r.find('"'), r.rfind('"')) {
        if b <= a {
            break;
        }
        r.replace_range(a..=b, "");
    }
    let r = r.trim().replace(": ", " ");
    let r = r.trim_end_matches(':').trim();
    if r.is_empty() {
        return Some(None);
    }
    if r == "24/7" {
        return Some(Some(Rule { dates: Vec::new(), days: [true; 7], spans: vec![(0, 1440)] }));
    }
    let mut rule = Rule { dates: Vec::new(), days: [true; 7], spans: Vec::new() };
    let tokens: Vec<&str> = r.split(' ').filter(|t| !t.is_empty()).collect();
    let mut i = 0;
    // daty mogą mieć spacje („Nov 01-Jun 15”): zbieramy wszystko do pierwszego tokenu z dniem lub godziną
    let mut date_text = String::new();
    while i < tokens.len() && (is_date_token(tokens[i]) || (!date_text.is_empty() && tokens[i].chars().next().is_some_and(|c| c.is_ascii_digit()) && !tokens[i].contains(':'))) {
        if !date_text.is_empty() {
            date_text.push(' ');
        }
        date_text.push_str(tokens[i]);
        i += 1;
    }
    if !date_text.is_empty() {
        rule.dates = dates(&date_text.replace(" -", "-").replace("- ", "-"))?;
    }
    let only_ph = i < tokens.len() && matches!(tokens[i], "PH" | "SH");
    if i < tokens.len() && is_day_token(tokens[i]) {
        if only_ph {
            // reguła tylko na święta – nie umiemy ich wyznaczyć, pomijamy
            return Some(None);
        }
        rule.days = days(tokens[i])?;
        i += 1;
    }
    let rest = tokens[i..].join(" ");
    let rest = rest.trim();
    if rest.is_empty() {
        rule.spans = vec![(0, 1440)];
    } else if rest == "off" || rest == "closed" {
        rule.spans = Vec::new();
    } else if rest == "24/7" {
        rule.spans = vec![(0, 1440)];
    } else {
        rule.spans = spans(&rest.replace(' ', ""))?;
    }
    Some(Some(rule))
}

impl Hours {
    pub fn parse(s: &str) -> Option<Hours> {
        let mut rules = Vec::new();
        for r in s.split([';', '|']) {
            if let Some(r) = rule(r)? {
                rules.push(r);
            }
        }
        (!rules.is_empty()).then_some(Hours { rules })
    }

    fn matches_date(r: &Rule, day: Day) -> bool {
        r.dates.is_empty()
            || r.dates.iter().any(|&(a, b)| {
                let x = (day.m, day.d);
                if a <= b {
                    a <= x && x <= b
                } else {
                    x >= a || x <= b
                }
            })
    }

    /// Przedziały otwarcia zaczynające się danego dnia (minuty; koniec może wykraczać za północ).
    pub fn day_spans(&self, day: Day) -> Vec<(u16, u16)> {
        let wd = (days_from_civil(day.y, day.m, day.d) + 3).rem_euclid(7) as usize;
        let mut out = Vec::new();
        for r in &self.rules {
            if r.days[wd] && Self::matches_date(r, day) {
                out = r.spans.clone();
            }
        }
        out
    }

    /// Stan o `minute` danego dnia i najbliższa zmiana (do tygodnia naprzód).
    pub fn status(&self, day: Day, minute: u16) -> Status {
        let m = minute as u32;
        let today = self.day_spans(day);
        let yesterday = self.day_spans(day.add_days(-1));
        let open_now = today.iter().find(|&&(a, b)| (a as u32) <= m && m < b as u32).map(|&(_, b)| b as u32).or_else(|| {
            yesterday.iter().find(|&&(_, b)| b > 1440 && m < b as u32 - 1440).map(|&(_, b)| b as u32 - 1440)
        });
        if let Some(end) = open_now {
            // ciągłe otwarcie (np. całą dobę) – szukamy pierwszej przerwy
            let mut end_abs = end;
            for k in 0..8u32 {
                let d = day.add_days(k as i64);
                let next = self.day_spans(d);
                let local = end_abs as i64 - (k * 1440) as i64;
                if let Some(&(_, b)) = next.iter().find(|&&(a, b)| (a as i64) <= local && local < b as i64 && b as i64 > local) {
                    end_abs = k * 1440 + b as u32;
                    continue;
                }
                // koniec dnia o 24:00 i otwarcie od 0:00 następnego
                if local == 1440 {
                    continue;
                }
                break;
            }
            if end_abs >= 8 * 1440 {
                return Status { open: true, change: None };
            }
            return Status { open: true, change: Some((end_abs / 1440, (end_abs % 1440) as u16)) };
        }
        for k in 0..8u32 {
            let spans = self.day_spans(day.add_days(k as i64));
            let next = spans.iter().filter(|&&(a, _)| k > 0 || a as u32 > m).map(|&(a, _)| a).min();
            if let Some(a) = next {
                return Status { open: false, change: Some((k, a)) };
            }
        }
        Status { open: false, change: None }
    }
}

/// Bieżący dzień i minuta czasu lokalnego w punkcie (`lon`, `lat`).
pub fn now_local(lon: f64, lat: f64) -> (Day, u16) {
    let (day, hour) = crate::tz::now_at(lon, lat);
    (day, ((hour * 60.0) as u16).min(1439))
}

/// Opis stanu po polsku, np. „otwarte · do 18:00”, „zamknięte · otwarcie jutro o 8:00”.
pub fn describe(s: Status) -> String {
    let hm = |m: u16| format!("{}:{:02}", m / 60, m % 60);
    let when = |(k, m): (u32, u16)| match k {
        0 => tf!("o {}", hm(m)),
        1 => tf!("jutro o {}", hm(m)),
        k => tf!("za {} dni o {}", k, hm(m)),
    };
    match (s.open, s.change) {
        (true, None) => crate::i18n::tr("otwarte całą dobę").into(),
        (true, Some((0, m))) => tf!("otwarte · do {}", hm(m)),
        (true, Some(c)) => tf!("otwarte · zamknięcie {}", when(c)),
        (false, Some(c)) => tf!("zamknięte · otwarcie {}", when(c)),
        (false, None) => crate::i18n::tr("zamknięte").into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, dd: u32) -> Day {
        Day { y, m, d: dd }
    }

    #[test]
    fn weekdays_and_times() {
        // 2026-11-11 to środa
        let h = Hours::parse("Mo-Fr 08:00-18:00; Sa,Su 09:00-17:00").unwrap();
        assert_eq!(h.status(d(2026, 11, 11), 10 * 60), Status { open: true, change: Some((0, 18 * 60)) });
        assert_eq!(h.status(d(2026, 11, 11), 19 * 60), Status { open: false, change: Some((1, 8 * 60)) });
        // piątek wieczorem → sobota 9:00
        assert_eq!(h.status(d(2026, 11, 13), 20 * 60), Status { open: false, change: Some((1, 9 * 60)) });
    }

    #[test]
    fn always_open() {
        let h = Hours::parse("24/7").unwrap();
        assert_eq!(h.status(d(2026, 1, 1), 3), Status { open: true, change: None });
    }

    #[test]
    fn seasons_and_off() {
        let h = Hours::parse("May-Sep: 07:00-21:00; Oct-Apr Mo-Su 08:00-16:00; Dec 24 off").unwrap();
        assert_eq!(h.status(d(2026, 7, 1), 20 * 60).open, true);
        assert_eq!(h.status(d(2026, 11, 11), 17 * 60), Status { open: false, change: Some((1, 8 * 60)) });
        assert_eq!(h.status(d(2026, 12, 24), 12 * 60), Status { open: false, change: Some((1, 8 * 60)) });
    }

    #[test]
    fn over_midnight() {
        let h = Hours::parse("Fr,Sa 18:00-02:00").unwrap();
        // sobota 1:00 – jeszcze otwarte od piątku
        assert_eq!(h.status(d(2026, 11, 14), 60), Status { open: true, change: Some((0, 120)) });
        assert_eq!(h.status(d(2026, 11, 13), 23 * 60).open, true);
    }

    #[test]
    fn date_ranges_with_days() {
        let h = Hours::parse("Nov 01-Jun 15 off; Jun 16-Oct 31 08:00-20:00").unwrap();
        assert_eq!(h.status(d(2026, 11, 11), 12 * 60).open, false);
        assert_eq!(h.status(d(2026, 6, 20), 12 * 60).open, true);
    }

    #[test]
    fn unknown_syntax() {
        assert!(Hours::parse("sunrise-sunset").is_none());
        assert!(Hours::parse("Mo-Fr 08:00-12:00 week 1-10").is_none());
    }

    #[test]
    fn ph_rules_skipped() {
        let h = Hours::parse("Mo-Su 08:00-20:00; PH off").unwrap();
        assert_eq!(h.status(d(2026, 11, 11), 12 * 60).open, true);
    }
}
