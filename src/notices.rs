//! Komunikat turystyczny Tatrzańskiego Parku Narodowego (tpn.gov.pl/komunikat-turystyczny):
//! warunki na szlakach, zagrożenia, zamknięte szlaki i informacje dodatkowe (m.in. o zamknięciach
//! w TANAP). Strona jest zwykłym HTML-em; pobierana w tle i trzymana w pamięci przez 30 minut.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const TPN_URL: &str = "https://tpn.gov.pl/komunikat-turystyczny";
pub const TANAP_URL: &str = "https://www.tanap.sk/";
const TTL: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Notice {
    /// Krótkie podsumowanie warunków z nagłówka strony.
    pub headline: String,
    /// Data komunikatu (tak jak na stronie, np. „11 listopada 2026”).
    pub date: String,
    pub hazards: Vec<String>,
    pub closures: Vec<String>,
    pub extra: Vec<String>,
}

#[derive(Clone)]
pub enum State {
    Loading,
    Ready(Arc<Notice>),
    Failed(String),
}

type Slot = Arc<Mutex<Option<(Instant, Option<Result<Arc<Notice>, String>>)>>>;

#[derive(Clone, Default)]
pub struct Notices {
    slot: Slot,
}

impl Notices {
    /// Komunikat TPN (pobierany w tle; wołaj co klatkę).
    pub fn get(&self, ctx: &eframe::egui::Context) -> State {
        let mut g = self.slot.lock().unwrap();
        match &*g {
            Some((_, None)) => return State::Loading,
            Some((at, Some(r))) if at.elapsed() < TTL => {
                return match r {
                    Ok(n) => State::Ready(n.clone()),
                    Err(e) => State::Failed(e.clone()),
                }
            }
            _ => {}
        }
        *g = Some((Instant::now(), None));
        drop(g);
        let (slot, ctx) = (self.slot.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = fetch().map(Arc::new);
            if let Err(e) = &r {
                log::warn!("komunikat TPN: {e}");
            }
            *slot.lock().unwrap() = Some((Instant::now(), Some(r)));
            ctx.request_repaint();
        });
        State::Loading
    }
}

fn fetch() -> Result<Notice, String> {
    let html = crate::download::agent()
        .get(TPN_URL)
        .timeout(Duration::from_secs(30))
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(c, _) => format!("HTTP {c}"),
            e => e.to_string(),
        })?
        .into_string()
        .map_err(|e| e.to_string())?;
    parse(&html)
}

/// Tekst pierwszego elementu z klasą `class` (bez znaczników).
fn class_text(html: &str, class: &str) -> String {
    let Some(at) = html.find(&format!("class=\"{class}\"")) else { return String::new() };
    let Some(open) = html[at..].find('>') else { return String::new() };
    let start = at + open + 1;
    let end = html[start..].find("</").map(|e| start + e).unwrap_or(html.len());
    text(&html[start..end])
}

/// Punkty listy z sekcji o danym `id` (`<li>` albo akapity).
fn section_items(html: &str, id: &str) -> Vec<String> {
    let Some(at) = html.find(&format!("id=\"{id}\"")) else { return Vec::new() };
    let rest = &html[at..];
    let Some(c) = rest.find("information__content") else { return Vec::new() };
    let body = &rest[c..];
    // sekcja kończy się na następnej sekcji
    let end = body[1..].find("information__item").map(|e| e + 1).unwrap_or(body.len());
    let body = &body[..end];
    let mut out = Vec::new();
    let tag = if body.contains("<li") { "<li" } else { "<p" };
    for part in body.split(tag).skip(1) {
        let Some(gt) = part.find('>') else { continue };
        let close = if tag == "<li" { "</li>" } else { "</p>" };
        let inner = &part[gt + 1..part.find(close).unwrap_or(part.len())];
        let t = text(inner);
        if t.chars().count() > 3 {
            out.push(t);
        }
    }
    out
}

/// HTML → zwykły tekst: bez znaczników, z rozwiniętymi encjami i pojedynczymi spacjami.
pub fn text(html: &str) -> String {
    let mut s = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                s.push(' ');
            }
            c if !in_tag => s.push(c),
            _ => {}
        }
    }
    let s = s
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&ndash;", "–")
        .replace("&mdash;", "—")
        .replace("&oacute;", "ó")
        .replace('\u{a0}', " ");
    let mut out = String::with_capacity(s.len());
    for w in s.split_whitespace() {
        // spacja przed znakiem interpunkcyjnym po zamkniętym znaczniku
        if !out.is_empty() && !matches!(w.chars().next(), Some('.' | ',' | ';' | ':' | ')')) {
            out.push(' ');
        }
        out.push_str(w);
    }
    out
}

pub fn parse(html: &str) -> Result<Notice, String> {
    let n = Notice {
        headline: class_text(html, "banner__content__text"),
        date: class_text(html, "banner__content__date"),
        hazards: section_items(html, "zagrozenia"),
        closures: section_items(html, "zamkniete-szlaki"),
        extra: section_items(html, "informacje-dodatkowe"),
    };
    if n.closures.is_empty() && n.hazards.is_empty() && n.headline.is_empty() {
        return Err(crate::i18n::tr("strona nie zawiera komunikatu (zmieniony układ?)").into());
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<div class="banner__content center">
<h1 class="banner__content__title">Komunikat turystyczny</h1>
<div class="banner__content__text">Na szlakach świeża warstwa śniegu i oblodzenia - w wielu miejscach ślisko.</div>
<div class="banner__content__date">11 listopada 2026</div></div>
<div class="information__item" id="zagrozenia"><h4 class="information__text">Zagrożenia</h4>
<div class="information__content block__text"><ul>
<li>W Tatrach obowiązuje zagrożenie lawinowe. Szczegóły na&nbsp;stronie <a href="https://lawiny.topr.pl/">https://lawiny.topr.pl/</a>.</li>
</ul></div></div>
<div class="information__item" id="zamkniete-szlaki"><h4 class="information__text">Zamknięte szlaki</h4>
<div class="information__content block__text"><ul>
<li><strong>Od 29&nbsp;października do&nbsp;odwołania zamknięty jest szlak w&nbsp;Dolinie Olczyskiej.</strong> Powód zamknięcia: prace budowlane.</li>
<li>Na terenie TANAP-u (Tatry Słowackie) od&nbsp;1 listopada do&nbsp;31 maja szlaki powyżej schronisk są&nbsp;zamknięte.</li>
</ul></div></div>
<div class="information__item" id="informacje-dodatkowe"><div class="information__content"><ul><li>Schronisko na&nbsp;Hali Ornak jest nieczynne.</li></ul></div></div>"#;

    #[test]
    fn parses_sections() {
        let n = parse(SAMPLE).unwrap();
        assert_eq!(n.date, "11 listopada 2026");
        assert!(n.headline.starts_with("Na szlakach świeża warstwa"));
        assert_eq!(n.hazards.len(), 1);
        assert!(n.hazards[0].ends_with("https://lawiny.topr.pl/."), "{}", n.hazards[0]);
        assert_eq!(n.closures.len(), 2);
        assert_eq!(n.closures[0], "Od 29 października do odwołania zamknięty jest szlak w Dolinie Olczyskiej. Powód zamknięcia: prace budowlane.");
        assert!(n.closures[1].contains("TANAP"));
        assert_eq!(n.extra, vec!["Schronisko na Hali Ornak jest nieczynne.".to_string()]);
    }

    #[test]
    fn empty_page_is_error() {
        assert!(parse("<html></html>").is_err());
    }
}
