//! Języki interfejsu.
//!
//! Językiem źródłowym jest polski: napisy w kodzie są po polsku, a [`tr`] zwraca tłumaczenie
//! w bieżącym języku (albo oryginał, gdy tłumaczenia brak). Napisy z wartościami – makro
//! [`tf!`] w miejsce `format!` (ta sama składnia, tłumaczony jest wzorzec).
//!
//! **Nowy język**: dodaj plik `lang/<kod>.tsv` i wpis w [`LANGS`]. Plik ma w każdym wierszu
//! polski oryginał, tabulator i tłumaczenie; `\n` oznacza nową linię, `{}` – wstawianą wartość
//! (kolejność jak w oryginale; `{0}`, `{1}`… pozwala ją zmienić). Wiersze zaczynające się od `#`
//! są komentarzami. Brakujące wiersze zostają po polsku.

use std::collections::HashMap;
use std::fmt::{Display, Write as _};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

pub struct Lang {
    /// Kod ISO 639-1, zapisywany w ustawieniach.
    pub code: &'static str,
    /// Nazwa języka w tym języku.
    pub name: &'static str,
    table: &'static str,
}

pub const LANGS: &[Lang] = &[
    Lang { code: "pl", name: "Polski", table: "" },
    Lang { code: "en", name: "English", table: include_str!("../lang/en.tsv") },
];

static CURRENT: AtomicUsize = AtomicUsize::new(0);

struct Table {
    fwd: HashMap<String, &'static str>,
    back: HashMap<&'static str, &'static str>,
}

fn unescape(s: &str) -> String {
    s.replace("\\n", "\n").replace("\\t", "\t")
}

fn parse(text: &'static str) -> Table {
    let mut fwd = HashMap::new();
    let mut back = HashMap::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((src, dst)) = line.split_once('\t') else { continue };
        if dst.is_empty() {
            continue;
        }
        let src: &'static str = Box::leak(unescape(src).into_boxed_str());
        let dst: &'static str = Box::leak(unescape(dst).into_boxed_str());
        fwd.insert(src.to_string(), dst);
        back.insert(dst, src);
    }
    Table { fwd, back }
}

fn tables() -> &'static [Table] {
    static T: OnceLock<Vec<Table>> = OnceLock::new();
    T.get_or_init(|| LANGS.iter().map(|l| parse(l.table)).collect())
}

/// Ustawia język po kodzie (nieznany – polski).
pub fn set(code: &str) {
    let i = LANGS.iter().position(|l| l.code == code).unwrap_or(0);
    CURRENT.store(i, Ordering::Relaxed);
}

/// Kod bieżącego języka.
pub fn current() -> &'static str {
    LANGS[CURRENT.load(Ordering::Relaxed).min(LANGS.len() - 1)].code
}

/// Język systemu Windows, jeśli GRAŃ go zna; w innym razie angielski (polski tylko dla polskiego systemu).
pub fn system() -> &'static str {
    // LANGID: najniższe 10 bitów to język podstawowy (0x15 – polski)
    #[cfg(windows)]
    let primary = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() } & 0x3ff;
    #[cfg(not(windows))]
    let primary: u16 = if std::env::var("LANG").unwrap_or_default().starts_with("pl") { 0x15 } else { 0 };
    if primary == 0x15 {
        "pl"
    } else {
        "en"
    }
}

/// Tłumaczenie napisu (polskiego oryginału) na bieżący język.
pub fn tr(s: &str) -> &str {
    let i = CURRENT.load(Ordering::Relaxed);
    if i == 0 {
        return s;
    }
    tables().get(i).and_then(|t| t.fwd.get(s)).copied().unwrap_or(s)
}

/// Polski oryginał przetłumaczonego napisu (do porównań z kluczami w kodzie).
pub fn source(s: &str) -> &str {
    let i = CURRENT.load(Ordering::Relaxed);
    if i == 0 {
        return s;
    }
    tables().get(i).and_then(|t| t.back.get(s)).copied().unwrap_or(s)
}

/// Miejsca na wartości we wzorcu: (indeks jawny, specyfikacja po dwukropku).
fn holes(t: &str) -> Vec<(Option<usize>, String)> {
    let mut out = Vec::new();
    let mut it = t.chars().peekable();
    while let Some(c) = it.next() {
        if c == '{' {
            if it.peek() == Some(&'{') {
                it.next();
                continue;
            }
            let mut inner = String::new();
            for d in it.by_ref() {
                if d == '}' {
                    break;
                }
                inner.push(d);
            }
            let (arg, spec) = inner.split_once(':').unwrap_or((inner.as_str(), ""));
            out.push((arg.trim().parse().ok(), spec.to_string()));
        } else if c == '}' && it.peek() == Some(&'}') {
            it.next();
        }
    }
    out
}

/// Formatowanie wartości według specyfikacji `[[wypełnienie]wyrównanie][+][0][szerokość][.precyzja]`.
fn apply(v: &dyn Display, spec: &str) -> String {
    let ch: Vec<char> = spec.chars().collect();
    let mut i = 0;
    let (mut fill, mut align) = (' ', None);
    if ch.len() >= 2 && matches!(ch[1], '<' | '>' | '^') {
        fill = ch[0];
        align = Some(ch[1]);
        i = 2;
    } else if !ch.is_empty() && matches!(ch[0], '<' | '>' | '^') {
        align = Some(ch[0]);
        i = 1;
    }
    let plus = ch.get(i) == Some(&'+');
    if plus {
        i += 1;
    }
    let zero = ch.get(i) == Some(&'0');
    if zero {
        i += 1;
    }
    let mut width = 0usize;
    while let Some(d) = ch.get(i).and_then(|c| c.to_digit(10)) {
        width = width * 10 + d as usize;
        i += 1;
    }
    let mut prec = None;
    if ch.get(i) == Some(&'.') {
        i += 1;
        let mut p = 0usize;
        while let Some(d) = ch.get(i).and_then(|c| c.to_digit(10)) {
            p = p * 10 + d as usize;
            i += 1;
        }
        prec = Some(p);
    }
    let mut s = match (plus, prec) {
        (true, Some(p)) => format!("{:+.*}", p, v),
        (true, None) => format!("{:+}", v),
        (false, Some(p)) => format!("{:.*}", p, v),
        (false, None) => v.to_string(),
    };
    let n = s.chars().count();
    if n < width {
        let pad = width - n;
        if zero && align.is_none() {
            let at = usize::from(s.starts_with(['+', '-']));
            s.insert_str(at, &"0".repeat(pad));
        } else {
            let f = |k: usize| fill.to_string().repeat(k);
            s = match align {
                Some('<') | None if !zero => format!("{s}{}", f(pad)),
                Some('^') => format!("{}{s}{}", f(pad / 2), f(pad - pad / 2)),
                _ => format!("{}{s}", f(pad)),
            };
        }
    }
    s
}

/// Napis z wartościami: wzorzec `orig` (jak w `format!`) tłumaczony, wartości formatowane
/// według specyfikacji z oryginału. Używaj przez makro [`tf!`].
pub fn fill(orig: &str, args: &[&dyn Display]) -> String {
    let specs = holes(orig);
    let mut next = 0;
    let vals: Vec<String> = specs
        .iter()
        .map(|(idx, spec)| {
            let k = idx.unwrap_or_else(|| {
                next += 1;
                next - 1
            });
            args.get(k).map(|a| apply(*a, spec)).unwrap_or_default()
        })
        .collect();
    // wartości według pozycji w oryginale; tłumaczenie może je przestawić przez {0}, {1}…
    let by_arg: Vec<String> = (0..args.len())
        .map(|k| {
            let mut n = 0;
            specs
                .iter()
                .position(|(idx, _)| {
                    let j = idx.unwrap_or_else(|| {
                        n += 1;
                        n - 1
                    });
                    j == k
                })
                .map(|p| vals[p].clone())
                .unwrap_or_else(|| args[k].to_string())
        })
        .collect();
    let target = tr(orig);
    let mut out = String::with_capacity(target.len() + 16);
    let mut it = target.chars().peekable();
    let mut seq = 0;
    while let Some(c) = it.next() {
        match c {
            '{' if it.peek() == Some(&'{') => {
                it.next();
                out.push('{');
            }
            '}' if it.peek() == Some(&'}') => {
                it.next();
                out.push('}');
            }
            '{' => {
                let mut inner = String::new();
                for d in it.by_ref() {
                    if d == '}' {
                        break;
                    }
                    inner.push(d);
                }
                let arg = inner.split(':').next().unwrap_or("").trim();
                let k = arg.parse::<usize>().unwrap_or_else(|_| {
                    seq += 1;
                    seq - 1
                });
                let _ = write!(out, "{}", by_arg.get(k).map(String::as_str).unwrap_or(""));
            }
            c => out.push(c),
        }
    }
    out
}

/// Jak `format!`, ale wzorzec jest tłumaczony na bieżący język.
macro_rules! tf {
    ($lit:literal $(,)?) => {
        $crate::i18n::fill($lit, &[])
    };
    ($lit:literal, $($arg:expr),+ $(,)?) => {
        $crate::i18n::fill($lit, &[$(&$arg as &dyn ::std::fmt::Display),+])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_formats_like_format() {
        // napisy spoza tabel: wynik nie zależy od języka ustawionego przez inne testy
        let x = 3.14159;
        assert_eq!(fill("a {} b {:.1} c {:+.0}", &[&"x", &x, &x]), format!("a {} b {:.1} c {:+.0}", "x", x, x));
        assert_eq!(fill("{:>5}|{:<4}|{:05.1}", &[&"ab", &"c", &x]), format!("{:>5}|{:<4}|{:05.1}", "ab", "c", x));
        assert_eq!(fill("{{dosł.}} {}", &[&7]), "{dosł.} 7");
        assert_eq!(tf!("próba-xyz · {} pkt", 3), "próba-xyz · 3 pkt");
    }

    #[test]
    fn every_language_file_is_well_formed() {
        for (l, t) in LANGS.iter().zip(tables()) {
            for line in l.table.lines().filter(|s| !s.is_empty() && !s.starts_with('#')) {
                assert!(line.contains('\t'), "{}: wiersz bez tabulatora: {line}", l.code);
            }
            // tłumaczenie ma tyle samo miejsc na wartości co oryginał
            for (src, dst) in &t.fwd {
                assert_eq!(holes(src).len(), holes(dst).len(), "{}: {src} → {dst}", l.code);
            }
        }
    }

    #[test]
    fn english_and_reorder() {
        set("en");
        let k = tables()[1].fwd.keys().next().cloned();
        if let Some(k) = k {
            assert_eq!(tr(&k), tables()[1].fwd[&k]);
        }
        set("pl");
    }
}
