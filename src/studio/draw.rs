//! Rysunek wektorowy Studio (mapy, przekroje) i jego zapis do SVG, PDF i DXF.
//!
//! Współrzędne w milimetrach strony, początek w lewym górnym rogu, y w dół (jak w SVG). Każdy
//! element należy do warstwy, a warstwy do grup (np. „Wody” → rzeki, jeziora):
//! - SVG: zagnieżdżone warstwy Inkscape (i nazwy warstw Illustratora), wspólny styl warstwy na
//!   grupie, przycięcie do obszaru (`clipPath`), metadane Dublin Core, ukryte i zablokowane warstwy;
//! - PDF: warstwy PDF (Optional Content) w tych samych grupach, przycięcie i krycie;
//! - DXF: warstwy CAD, linie przycięte do obszaru geometrycznie.

use std::collections::BTreeMap;
use std::fmt::Write;

use serde::{Deserialize, Serialize};

pub type Rgb = [u8; 3];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

#[derive(Clone, Debug)]
pub enum Item {
    /// Łamana albo wielokąt.
    Path { pts: Vec<(f64, f64)>, closed: bool, stroke: Option<(Rgb, f64)>, fill: Option<Rgb>, dash: Option<f64> },
    /// Wielokąt z dziurami (reguła parzystości).
    Rings { rings: Vec<Vec<(f64, f64)>>, fill: Rgb, stroke: Option<(Rgb, f64)> },
    Text { x: f64, y: f64, text: String, size: f64, color: Rgb, anchor: Anchor, angle: f64, halo: bool, bold: bool },
    Circle { x: f64, y: f64, r: f64, fill: Option<Rgb>, stroke: Option<(Rgb, f64)> },
    /// Obraz RGB (podkład z cieniowaniem) w prostokącie.
    Image { x: f64, y: f64, w: f64, h: f64, img: image::RgbImage },
}

/// Ustawienia warstwy wybrane przez użytkownika (zapisywane z ustawieniami mapy).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayerCfg {
    /// Widoczna w pliku (ukryta zostaje w SVG/PDF, ale wyłączona).
    pub visible: bool,
    /// Zapisywana w ogóle.
    pub export: bool,
    /// Zablokowana do edycji (Inkscape).
    pub locked: bool,
    /// Krycie 0..1.
    pub opacity: f32,
    /// Własny kolor wszystkich elementów warstwy.
    pub color: Option<Rgb>,
    /// Mnożnik grubości linii.
    pub width: f32,
}

impl Default for LayerCfg {
    fn default() -> Self {
        Self { visible: true, export: true, locked: false, opacity: 1.0, color: None, width: 1.0 }
    }
}

impl LayerCfg {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Clone, Debug)]
pub struct Layer {
    pub id: &'static str,
    pub name: String,
    /// Grupa warstw (pusta – warstwa najwyższego poziomu).
    pub group: &'static str,
    pub group_name: String,
    pub items: Vec<Item>,
    /// Przycinana do obszaru rysunku ([`Drawing::clip`]).
    pub clipped: bool,
    pub hidden: bool,
    pub locked: bool,
    pub opacity: f32,
}

/// Opis warstwy do listy w oknie Studio.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerInfo {
    pub id: &'static str,
    pub name: String,
    pub group_name: String,
    pub count: usize,
    /// Kolor przewodni warstwy (do próbki na liście).
    pub color: Rgb,
}

/// Krój pisma rysunku.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Font {
    #[default]
    Sans,
    Serif,
    Condensed,
}

impl Font {
    pub const ALL: [Font; 3] = [Font::Sans, Font::Serif, Font::Condensed];
    pub fn label(self) -> &'static str {
        match self {
            Font::Sans => crate::i18n::tr("Bezszeryfowy (Helvetica, Arial)"),
            Font::Serif => crate::i18n::tr("Szeryfowy (Georgia, Times)"),
            Font::Condensed => crate::i18n::tr("Wąski (Arial Narrow)"),
        }
    }
    fn css(self) -> &'static str {
        match self {
            Font::Sans => "Helvetica, Arial, 'Segoe UI', sans-serif",
            Font::Serif => "Georgia, 'Times New Roman', Times, serif",
            Font::Condensed => "'Arial Narrow', 'Roboto Condensed', 'Helvetica Neue Condensed', sans-serif",
        }
    }
    /// Względna szerokość znaków (do szacowania miejsca na napis).
    pub fn width_k(self) -> f64 {
        match self {
            Font::Sans => 1.0,
            Font::Serif => 0.96,
            Font::Condensed => 0.82,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Drawing {
    pub w: f64,
    pub h: f64,
    pub title: String,
    pub layers: Vec<Layer>,
    /// Obrys obszaru (mm strony) – przycina warstwy z `clipped`.
    pub clip: Option<Vec<(f64, f64)>>,
    pub font: Font,
    /// Kolor papieru: tło arkusza i otoczki napisów.
    pub paper: Rgb,
    /// Rysować tło arkusza (prostokąt w kolorze papieru).
    pub background: bool,
    /// Źródła danych do metadanych pliku.
    pub credits: Vec<String>,
    /// Opis do metadanych (skala, obszar).
    pub desc: String,
}

impl Default for Drawing {
    fn default() -> Self {
        Self { w: 0.0, h: 0.0, title: String::new(), layers: Vec::new(), clip: None, font: Font::Sans, paper: [255, 255, 255], background: false, credits: Vec::new(), desc: String::new() }
    }
}

impl Drawing {
    pub fn new(w: f64, h: f64, title: &str) -> Self {
        Self { w, h, title: title.to_string(), ..Default::default() }
    }

    /// Zapowiada warstwę w grupie – kolejność zapowiedzi to kolejność rysowania.
    pub fn declare(&mut self, group: &'static str, group_name: &str, id: &'static str, name: &str, clipped: bool) {
        if self.layers.iter().any(|l| l.id == id) {
            return;
        }
        self.layers.push(Layer { id, name: name.to_string(), group, group_name: group_name.to_string(), items: Vec::new(), clipped, hidden: false, locked: false, opacity: 1.0 });
    }

    /// Warstwa o identyfikatorze `id` (tworzona przy pierwszym użyciu, w kolejności rysowania).
    pub fn layer(&mut self, id: &'static str, name: &str) -> &mut Vec<Item> {
        if let Some(k) = self.layers.iter().position(|l| l.id == id) {
            return &mut self.layers[k].items;
        }
        self.layers.push(Layer { id, name: name.to_string(), group: "", group_name: String::new(), items: Vec::new(), clipped: false, hidden: false, locked: false, opacity: 1.0 });
        &mut self.layers.last_mut().unwrap().items
    }

    pub fn is_empty(&self) -> bool {
        self.layers.iter().all(|l| l.items.is_empty())
    }

    /// Warstwy z elementami (do listy w oknie).
    pub fn info(&self) -> Vec<LayerInfo> {
        self.layers
            .iter()
            .filter(|l| !l.items.is_empty())
            .map(|l| LayerInfo { id: l.id, name: l.name.clone(), group_name: l.group_name.clone(), count: l.items.len(), color: lead_color(&l.items) })
            .collect()
    }

    /// Ustawienia warstw: pominięcie, ukrycie, blokada, krycie, kolor i grubość linii.
    pub fn apply(&mut self, cfg: &BTreeMap<String, LayerCfg>) {
        self.layers.retain(|l| cfg.get(l.id).map(|c| c.export).unwrap_or(true));
        let paper = self.paper;
        for l in &mut self.layers {
            let Some(c) = cfg.get(l.id) else { continue };
            l.hidden = !c.visible;
            l.locked = c.locked;
            l.opacity = c.opacity.clamp(0.0, 1.0);
            let wk = c.width.clamp(0.1, 10.0) as f64;
            let keep = |v: Rgb| v == [255, 255, 255] || v == paper;
            for it in &mut l.items {
                match it {
                    Item::Path { stroke, fill, .. } => {
                        if let Some((sc, w)) = stroke {
                            *w *= wk;
                            if let Some(col) = c.color {
                                *sc = col;
                            }
                        }
                        if let (Some(col), Some(f)) = (c.color, fill.as_mut()) {
                            if !keep(*f) {
                                *f = col;
                            }
                        }
                    }
                    Item::Rings { fill, stroke, .. } => {
                        if let Some(col) = c.color {
                            if !keep(*fill) {
                                *fill = col;
                            }
                        }
                        if let Some((sc, w)) = stroke {
                            *w *= wk;
                            if let Some(col) = c.color {
                                *sc = col;
                            }
                        }
                    }
                    Item::Text { color, .. } => {
                        if let Some(col) = c.color {
                            *color = col;
                        }
                    }
                    Item::Circle { fill, stroke, .. } => {
                        if let (Some(col), Some(f)) = (c.color, fill.as_mut()) {
                            *f = col;
                        }
                        if let Some((sc, w)) = stroke {
                            *w *= wk;
                            if let Some(col) = c.color {
                                *sc = col;
                            }
                        }
                    }
                    Item::Image { .. } => {}
                }
            }
        }
    }

    /// Rysunek z jedną warstwą (eksport warstw do osobnych plików).
    pub fn only(&self, id: &str) -> Drawing {
        Drawing { layers: self.layers.iter().filter(|l| l.id == id).cloned().map(|mut l| {
            l.hidden = false;
            l
        }).collect(), background: false, ..self.clone() }
    }
}

fn lead_color(items: &[Item]) -> Rgb {
    for it in items {
        match it {
            Item::Path { stroke: Some((c, _)), .. } => return *c,
            Item::Path { fill: Some(c), .. } | Item::Rings { fill: c, .. } => return *c,
            Item::Text { color, .. } => return *color,
            Item::Circle { fill: Some(c), .. } => return *c,
            Item::Image { .. } => return [150, 150, 150],
            _ => {}
        }
    }
    [0, 0, 0]
}

/// Szacunkowa szerokość napisu (mm) w Helvetica/Arial.
pub fn text_width(s: &str, size: f64) -> f64 {
    s.chars()
        .map(|c| match c {
            'i' | 'j' | 'l' | 'ł' | '.' | ',' | ':' | ';' | '\'' | '!' | '|' | 'I' => 0.24,
            'f' | 't' | 'r' | ' ' | '(' | ')' | '-' => 0.33,
            'm' | 'w' | 'M' | 'W' => 0.82,
            c if c.is_uppercase() => 0.67,
            c if c.is_ascii_digit() => 0.556,
            _ => 0.53,
        })
        .sum::<f64>()
        * size
}

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Liczba z dokładnością 0,01 mm bez zbędnych zer („12.5”, „3”, „-0.04”).
fn num(v: f64) -> String {
    let mut s = format!("{:.2}", v);
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" {
        s = "0".into();
    }
    s
}

/// Dane ścieżki: bezwzględne M i niejawne L (krótko, czytelnie dla programów graficznych).
fn path_d(pts: &[(f64, f64)], closed: bool) -> String {
    let mut d = String::with_capacity(pts.len() * 12);
    for (k, p) in pts.iter().enumerate() {
        if k == 0 {
            let _ = write!(d, "M{} {}", num(p.0), num(p.1));
        } else if k == 1 {
            let _ = write!(d, "L{} {}", num(p.0), num(p.1));
        } else {
            let _ = write!(d, " {} {}", num(p.0), num(p.1));
        }
    }
    if closed {
        d.push('Z');
    }
    d
}

/// Dzisiejsza data (RRRR-MM-DD) bez zależności od strefy czasowej.
fn today() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

/* ── SVG ───────────────────────────────────────────────────────────────────────── */

/// Wspólny styl elementu (atrybuty), gdy da się go przenieść na grupę warstwy.
fn shape_style(it: &Item) -> Option<String> {
    let stroke_attr = |s: &Option<(Rgb, f64)>| match s {
        Some((c, w)) => format!(r#" stroke="{}" stroke-width="{}" stroke-linejoin="round" stroke-linecap="round""#, hex(*c), num3(*w)),
        None => String::new(),
    };
    match it {
        Item::Path { stroke, fill, dash, .. } => {
            let mut s = format!(r#"fill="{}""#, fill.map(hex).unwrap_or_else(|| "none".into()));
            s.push_str(&stroke_attr(stroke));
            if let (Some(ds), Some(_)) = (dash, stroke) {
                let _ = write!(s, r#" stroke-dasharray="{} {}""#, num(*ds), num(ds * 0.7));
            }
            Some(s)
        }
        Item::Rings { fill, stroke, .. } => Some(format!(r#"fill="{}" fill-rule="evenodd"{}"#, hex(*fill), stroke_attr(stroke))),
        _ => None,
    }
}

fn num3(v: f64) -> String {
    let mut s = format!("{:.3}", v);
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    s
}

fn svg_item(o: &mut String, it: &Item, shared: bool, paper: Rgb) {
    use base64::Engine;
    match it {
        Item::Path { pts, closed, .. } => {
            if pts.len() < 2 {
                return;
            }
            let st = if shared { String::new() } else { format!(" {}", shape_style(it).unwrap_or_default()) };
            let _ = writeln!(o, r#"<path d="{}"{}/>"#, path_d(pts, *closed), st);
        }
        Item::Rings { rings, .. } => {
            let dd: String = rings.iter().map(|r| path_d(r, true)).collect();
            let st = if shared { String::new() } else { format!(" {}", shape_style(it).unwrap_or_default()) };
            let _ = writeln!(o, r#"<path d="{}"{}/>"#, dd, st);
        }
        Item::Text { x, y, text, size, color, anchor, angle, halo, bold } => {
            let a = match anchor {
                Anchor::Start => "",
                Anchor::Middle => r#" text-anchor="middle""#,
                Anchor::End => r#" text-anchor="end""#,
            };
            let tr = if *angle != 0.0 { format!(r#" transform="rotate({} {} {})""#, num(*angle), num(*x), num(*y)) } else { String::new() };
            let weight = if *bold { r#" font-weight="bold""# } else { "" };
            let halo_attr = if *halo {
                let c = if paper == [255, 255, 255] { String::new() } else { format!(r#" stroke="{}""#, hex(paper)) };
                format!(r#" class="halo" stroke-width="{}"{}"#, num3(size * 0.22), c)
            } else {
                String::new()
            };
            let _ = writeln!(o, r#"<text x="{}" y="{}" font-size="{}" fill="{}"{}{}{}{}>{}</text>"#, num(*x), num(*y), num3(*size), hex(*color), a, weight, halo_attr, tr, esc(text));
        }
        Item::Circle { x, y, r, fill, stroke } => {
            let _ = write!(o, r#"<circle cx="{}" cy="{}" r="{}" fill="{}""#, num(*x), num(*y), num3(*r), fill.map(hex).unwrap_or_else(|| "none".into()));
            if let Some((c, w)) = stroke {
                let _ = write!(o, r#" stroke="{}" stroke-width="{}""#, hex(*c), num3(*w));
            }
            o.push_str("/>\n");
        }
        Item::Image { x, y, w, h, img } => {
            let mut png = Vec::new();
            let _ = img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
            let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
            let _ = writeln!(o, r#"<image x="{}" y="{}" width="{}" height="{}" preserveAspectRatio="none" image-rendering="optimizeQuality" xlink:href="data:image/png;base64,{}"/>"#, num(*x), num(*y), num(*w), num(*h), b64);
        }
    }
}

pub fn svg(d: &Drawing) -> String {
    let mut o = String::new();
    let credits = d.credits.join(" · ");
    let _ = writeln!(
        o,
        r##"<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<!-- GRAŃ Studio · redzikowski.com · jednostki: milimetry (1 jednostka = 1 mm) -->
<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape" xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd" xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:cc="http://creativecommons.org/ns#" xmlns:dc="http://purl.org/dc/elements/1.1/" version="1.1" width="{w}mm" height="{h}mm" viewBox="0 0 {w} {h}">
<title>{t}</title>
<desc>{desc}</desc>
<metadata><rdf:RDF><cc:Work rdf:about=""><dc:format>image/svg+xml</dc:format><dc:type rdf:resource="http://purl.org/dc/dcmitype/StillImage"/><dc:title>{t}</dc:title><dc:date>{date}</dc:date><dc:creator><cc:Agent><dc:title>GRAŃ Studio</dc:title></cc:Agent></dc:creator><dc:source>{cr}</dc:source><dc:rights><cc:Agent><dc:title>{cr}</dc:title></cc:Agent></dc:rights><dc:description>{desc}</dc:description></cc:Work></rdf:RDF></metadata>
<sodipodi:namedview id="widok" pagecolor="{paper}" bordercolor="#666666" inkscape:document-units="mm"/>"##,
        w = num(d.w),
        h = num(d.h),
        t = esc(&d.title),
        desc = esc(&d.desc),
        date = today(),
        cr = esc(&credits),
        paper = hex(d.paper),
    );
    let _ = write!(o, "<defs>\n<style>text{{font-family:{}}}.halo{{paint-order:stroke;stroke:#ffffff;stroke-linejoin:round;stroke-linecap:round}}</style>\n", d.font.css().replace('\'', "&apos;"));
    if let Some(c) = &d.clip {
        let _ = writeln!(o, r#"<clipPath id="obszar" clipPathUnits="userSpaceOnUse"><path d="{}"/></clipPath>"#, path_d(c, true));
    }
    o.push_str("</defs>\n");
    if d.background {
        let _ = writeln!(o, r#"<rect id="arkusz" x="0" y="0" width="{}" height="{}" fill="{}" sodipodi:insensitive="true"/>"#, num(d.w), num(d.h), hex(d.paper));
    }
    let mut group: Option<&'static str> = None;
    let mut used_groups: BTreeMap<&'static str, usize> = BTreeMap::new();
    for l in d.layers.iter().filter(|l| !l.items.is_empty()) {
        if group != Some(l.group) {
            if group.is_some_and(|g| !g.is_empty()) {
                o.push_str("</g>\n");
            }
            if !l.group.is_empty() {
                let n = used_groups.entry(l.group).or_default();
                *n += 1;
                let id = if *n == 1 { l.group.to_string() } else { format!("{}_{}", l.group, n) };
                let _ = writeln!(o, r#"<g id="{id}" inkscape:groupmode="layer" inkscape:label="{n}" data-name="{n}">"#, n = esc(&l.group_name));
            }
            group = Some(l.group);
        }
        // wspólny styl warstwy na grupie (zmiana koloru w jednym miejscu)
        let styles: Vec<Option<String>> = l.items.iter().map(shape_style).collect();
        let shared = styles.first().cloned().flatten().filter(|s| styles.iter().all(|x| x.as_deref() == Some(s.as_str())));
        let mut attrs = String::new();
        if let Some(s) = &shared {
            let _ = write!(attrs, " {s}");
        }
        if l.clipped && d.clip.is_some() {
            attrs.push_str(r#" clip-path="url(#obszar)""#);
        }
        if l.opacity < 0.999 {
            let _ = write!(attrs, r#" opacity="{}""#, num3(l.opacity as f64));
        }
        if l.hidden {
            attrs.push_str(r#" style="display:none""#);
        }
        if l.locked {
            attrs.push_str(r#" sodipodi:insensitive="true""#);
        }
        let _ = writeln!(o, r#"<g id="{id}" inkscape:groupmode="layer" inkscape:label="{n}" data-name="{n}"{attrs}>"#, id = l.id, n = esc(&l.name));
        for it in &l.items {
            svg_item(&mut o, it, shared.is_some(), d.paper);
        }
        o.push_str("</g>\n");
    }
    if group.is_some_and(|g| !g.is_empty()) {
        o.push_str("</g>\n");
    }
    o.push_str("</svg>\n");
    o
}

/// Obraz podglądu SVG (RGBA, mnożone przez alfę) o dłuższym boku `px` – tym samym silnikiem,
/// którym przeglądarki i programy graficzne pokażą plik.
#[cfg(windows)]
pub fn raster(svg: &str, px: u32) -> Option<image::RgbaImage> {
    use resvg::{tiny_skia, usvg};
    use std::sync::{Arc, OnceLock};
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    let db = FONTS.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        db.set_sans_serif_family("Arial");
        db.set_serif_family("Georgia");
        Arc::new(db)
    });
    let opt = usvg::Options { fontdb: db.clone(), ..Default::default() };
    let tree = usvg::Tree::from_str(svg, &opt).ok()?;
    let size = tree.size();
    let k = px as f32 / size.width().max(size.height()).max(1.0);
    let (w, h) = (((size.width() * k).round() as u32).max(1), ((size.height() * k).round() as u32).max(1));
    let mut pm = tiny_skia::Pixmap::new(w, h)?;
    pm.fill(tiny_skia::Color::WHITE);
    resvg::render(&tree, tiny_skia::Transform::from_scale(k, k), &mut pm.as_mut());
    image::RgbaImage::from_raw(w, h, pm.take())
}

#[cfg(not(windows))]
pub fn raster(_svg: &str, _px: u32) -> Option<image::RgbaImage> {
    None
}

/* ── PDF ───────────────────────────────────────────────────────────────────────── */

/// Nazwy glifów Helvetiki dla znaków spoza ASCII (polskie, czeskie, słowackie, niemieckie…).
fn glyph(c: char) -> Option<&'static str> {
    Some(match c {
        'ą' => "aogonek", 'Ą' => "Aogonek", 'ć' => "cacute", 'Ć' => "Cacute", 'ę' => "eogonek", 'Ę' => "Eogonek",
        'ł' => "lslash", 'Ł' => "Lslash", 'ń' => "nacute", 'Ń' => "Nacute", 'ó' => "oacute", 'Ó' => "Oacute",
        'ś' => "sacute", 'Ś' => "Sacute", 'ź' => "zacute", 'Ź' => "Zacute", 'ż' => "zdotaccent", 'Ż' => "Zdotaccent",
        'á' => "aacute", 'Á' => "Aacute", 'č' => "ccaron", 'Č' => "Ccaron", 'ď' => "dcaron", 'Ď' => "Dcaron",
        'é' => "eacute", 'É' => "Eacute", 'ě' => "ecaron", 'Ě' => "Ecaron", 'í' => "iacute", 'Í' => "Iacute",
        'ň' => "ncaron", 'Ň' => "Ncaron", 'ř' => "rcaron", 'Ř' => "Rcaron", 'š' => "scaron", 'Š' => "Scaron",
        'ť' => "tcaron", 'Ť' => "Tcaron", 'ú' => "uacute", 'Ú' => "Uacute", 'ů' => "uring", 'Ů' => "Uring",
        'ý' => "yacute", 'Ý' => "Yacute", 'ž' => "zcaron", 'Ž' => "Zcaron", 'ä' => "adieresis", 'Ä' => "Adieresis",
        'ö' => "odieresis", 'Ö' => "Odieresis", 'ü' => "udieresis", 'Ü' => "Udieresis", 'ß' => "germandbls",
        'ô' => "ocircumflex", 'ľ' => "lcaron", 'Ľ' => "Lcaron", 'ĺ' => "lacute", 'Ĺ' => "Lacute", 'ŕ' => "racute",
        '–' => "endash", '—' => "emdash", '°' => "degree", '©' => "copyright", '×' => "multiply", '·' => "periodcentered",
        '„' => "quotedblbase", '”' => "quotedblright", '“' => "quotedblleft", '’' => "quoteright", '²' => "twosuperior",
        '′' => "minute", '″' => "second",
        _ => return None,
    })
}

struct Pdf {
    objs: Vec<Vec<u8>>,
}

impl Pdf {
    fn add(&mut self, body: Vec<u8>) -> usize {
        self.objs.push(body);
        self.objs.len()
    }
    fn set(&mut self, id: usize, body: Vec<u8>) {
        self.objs[id - 1] = body;
    }
    fn reserve(&mut self) -> usize {
        self.add(Vec::new())
    }
    fn stream(&mut self, dict: &str, data: &[u8]) -> usize {
        let z = deflate(data);
        let mut b = format!("<< {dict} /Length {} /Filter /FlateDecode >>\nstream\n", z.len()).into_bytes();
        b.extend_from_slice(&z);
        b.extend_from_slice(b"\nendstream");
        self.add(b)
    }
    fn finish(self, root: usize, info: usize) -> Vec<u8> {
        let mut out = b"%PDF-1.5\n%\xe2\xe3\xcf\xd3\n".to_vec();
        let mut offs = Vec::new();
        for (k, o) in self.objs.iter().enumerate() {
            offs.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", k + 1).as_bytes());
            out.extend_from_slice(o);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", self.objs.len() + 1).as_bytes());
        for o in offs {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size {} /Root {root} 0 R /Info {info} 0 R >>\nstartxref\n{xref}\n%%EOF\n", self.objs.len() + 1).as_bytes());
        out
    }
}

pub fn deflate(data: &[u8]) -> Vec<u8> {
    use std::io::Write as _;
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = e.write_all(data);
    e.finish().unwrap_or_default()
}

/// Napis PDF w kodowaniu z tabelą `/Differences` (znaki spoza ASCII → kody 128…).
fn pdf_string(s: &str, codes: &std::collections::HashMap<char, u8>) -> Vec<u8> {
    let mut out = vec![b'('];
    for c in s.chars() {
        let b = if (' '..='~').contains(&c) { c as u8 } else { *codes.get(&c).unwrap_or(&b'?') };
        if matches!(b, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(b);
    }
    out.push(b')');
    out
}

/// Napis tekstowy PDF (nazwy warstw, metadane) w UTF-16BE.
fn pdf_text(s: &str) -> String {
    let mut o = String::from("<FEFF");
    for u in s.encode_utf16() {
        let _ = write!(o, "{u:04X}");
    }
    o.push('>');
    o
}

pub fn pdf(d: &Drawing) -> Vec<u8> {
    const PT: f64 = 72.0 / 25.4;
    let mut pdf = Pdf { objs: Vec::new() };
    // znaki spoza ASCII użyte w rysunku → kody 128..255
    let mut codes: std::collections::HashMap<char, u8> = Default::default();
    let mut diffs = String::from("[128");
    for l in &d.layers {
        for it in &l.items {
            if let Item::Text { text, .. } = it {
                for c in text.chars() {
                    if !(' '..='~').contains(&c) && !codes.contains_key(&c) && codes.len() < 120 {
                        if let Some(g) = glyph(c) {
                            codes.insert(c, 128 + codes.len() as u8);
                            let _ = write!(diffs, " /{g}");
                        }
                    }
                }
            }
        }
    }
    diffs.push(']');
    let (regular, heavy) = match d.font {
        Font::Serif => ("Times-Roman", "Times-Bold"),
        _ => ("Helvetica", "Helvetica-Bold"),
    };
    let enc = pdf.add(format!("<< /Type /Encoding /BaseEncoding /WinAnsiEncoding /Differences {diffs} >>").into_bytes());
    let font = pdf.add(format!("<< /Type /Font /Subtype /Type1 /BaseFont /{regular} /Encoding {enc} 0 R >>").into_bytes());
    let bold = pdf.add(format!("<< /Type /Font /Subtype /Type1 /BaseFont /{heavy} /Encoding {enc} 0 R >>").into_bytes());
    let gs = pdf.add(b"<< /Type /ExtGState /LJ 1 /LC 1 >>".to_vec());
    let mut images = Vec::new();
    let mut opac: Vec<(String, usize)> = Vec::new();
    let mut ocgs: Vec<(usize, &Layer)> = Vec::new();
    let mut c: Vec<u8> = Vec::new();
    let y = |v: f64| (d.h - v) * PT;
    let x = |v: f64| v * PT;
    let rgb = |c: Rgb| format!("{:.3} {:.3} {:.3}", c[0] as f64 / 255.0, c[1] as f64 / 255.0, c[2] as f64 / 255.0);
    let halo_c = d.paper;
    let _ = writeln!(Out(&mut c), "/GS0 gs");
    if d.background {
        let _ = writeln!(Out(&mut c), "{} rg 0 0 {:.2} {:.2} re f", rgb(d.paper), d.w * PT, d.h * PT);
    }
    for l in d.layers.iter().filter(|l| !l.items.is_empty()) {
        let ocg = pdf.add(format!("<< /Type /OCG /Name {} >>", pdf_text(&l.name)).into_bytes());
        let tag = format!("L{}", ocgs.len());
        ocgs.push((ocg, l));
        let _ = writeln!(Out(&mut c), "/OC /{tag} BDC q");
        if l.opacity < 0.999 {
            let key = format!("{:.3}", l.opacity);
            let name = match opac.iter().position(|(k, _)| *k == key) {
                Some(i) => format!("GA{i}"),
                None => {
                    let id = pdf.add(format!("<< /Type /ExtGState /ca {key} /CA {key} >>").into_bytes());
                    opac.push((key, id));
                    format!("GA{}", opac.len() - 1)
                }
            };
            let _ = writeln!(Out(&mut c), "/{name} gs");
        }
        if let (true, Some(clip)) = (l.clipped, &d.clip) {
            let mut s = String::new();
            for (k, p) in clip.iter().enumerate() {
                let _ = write!(s, "{:.2} {:.2} {} ", x(p.0), y(p.1), if k == 0 { "m" } else { "l" });
            }
            s.push_str("h W n");
            let _ = writeln!(Out(&mut c), "{s}");
        }
        for it in &l.items {
            match it {
                Item::Path { pts, closed, stroke, fill, dash } => {
                    if pts.len() < 2 {
                        continue;
                    }
                    let mut s = String::new();
                    for (k, p) in pts.iter().enumerate() {
                        let _ = write!(s, "{:.2} {:.2} {} ", x(p.0), y(p.1), if k == 0 { "m" } else { "l" });
                    }
                    if *closed {
                        s.push_str("h ");
                    }
                    if let Some(f) = fill {
                        let _ = write!(s, "{} rg ", rgb(*f));
                    }
                    if let Some((sc, w)) = stroke {
                        let _ = write!(s, "{} RG {:.3} w ", rgb(*sc), w * PT);
                        match dash {
                            Some(ds) => {
                                let _ = write!(s, "[{:.2} {:.2}] 0 d ", ds * PT, ds * 0.7 * PT);
                            }
                            None => s.push_str("[] 0 d "),
                        }
                    }
                    s.push_str(match (fill.is_some(), stroke.is_some()) {
                        (true, true) => "B",
                        (true, false) => "f",
                        (false, true) => "S",
                        _ => "n",
                    });
                    let _ = writeln!(Out(&mut c), "{s}");
                }
                Item::Rings { rings, fill, stroke } => {
                    let mut s = String::new();
                    for r in rings {
                        for (k, p) in r.iter().enumerate() {
                            let _ = write!(s, "{:.2} {:.2} {} ", x(p.0), y(p.1), if k == 0 { "m" } else { "l" });
                        }
                        s.push_str("h ");
                    }
                    let _ = write!(s, "{} rg ", rgb(*fill));
                    if let Some((sc, w)) = stroke {
                        let _ = write!(s, "{} RG {:.3} w [] 0 d B*", rgb(*sc), w * PT);
                    } else {
                        s.push_str("f*");
                    }
                    let _ = writeln!(Out(&mut c), "{s}");
                }
                Item::Circle { x: cx, y: cy, r, fill, stroke } => {
                    // cztery łuki Béziera
                    let k = 0.5523 * r;
                    let (px, py, pr, pk) = (x(*cx), y(*cy), r * PT, k * PT);
                    let mut s = format!("{:.2} {:.2} m ", px + pr, py);
                    let _ = write!(s, "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c ", px + pr, py + pk, px + pk, py + pr, px, py + pr);
                    let _ = write!(s, "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c ", px - pk, py + pr, px - pr, py + pk, px - pr, py);
                    let _ = write!(s, "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c ", px - pr, py - pk, px - pk, py - pr, px, py - pr);
                    let _ = write!(s, "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c h ", px + pk, py - pr, px + pr, py - pk, px + pr, py);
                    if let Some(f) = fill {
                        let _ = write!(s, "{} rg ", rgb(*f));
                    }
                    if let Some((sc, w)) = stroke {
                        let _ = write!(s, "{} RG {:.3} w [] 0 d ", rgb(*sc), w * PT);
                    }
                    s.push_str(match (fill.is_some(), stroke.is_some()) {
                        (true, true) => "B",
                        (true, false) => "f",
                        (false, true) => "S",
                        _ => "n",
                    });
                    let _ = writeln!(Out(&mut c), "{s}");
                }
                Item::Text { x: tx, y: ty, text, size, color, anchor, angle, halo, bold: b } => {
                    let wdt = text_width(text, *size) * d.font.width_k();
                    let dx = match anchor {
                        Anchor::Start => 0.0,
                        Anchor::Middle => -wdt / 2.0,
                        Anchor::End => -wdt,
                    };
                    let a = -angle.to_radians();
                    let (ca, sa) = (a.cos(), a.sin());
                    let (ox, oy) = (x(*tx), y(*ty));
                    let f = if *b { "/F2" } else { "/F1" };
                    let s = pdf_string(text, &codes);
                    // tryb 1 – sam obrys w kolorze papieru (otoczka napisu), tryb 0 – wypełnienie
                    for mode in [1u8, 0] {
                        if mode == 1 && !*halo {
                            continue;
                        }
                        let _ = write!(
                            Out(&mut c),
                            "BT {f} {:.2} Tf {} rg {} RG {:.3} w {mode} Tr {ca:.4} {sa:.4} {:.4} {ca:.4} {:.2} {:.2} Tm ",
                            size * PT,
                            rgb(*color),
                            rgb(halo_c),
                            size * 0.22 * PT,
                            -sa,
                            ox + dx * PT * ca,
                            oy + dx * PT * sa
                        );
                        c.extend_from_slice(&s);
                        c.extend_from_slice(b" Tj ET\n");
                    }
                }
                Item::Image { x: ix, y: iy, w, h, img } => {
                    let id = pdf.stream(&format!("/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8", img.width(), img.height()), img.as_raw());
                    let name = format!("Im{}", images.len());
                    images.push((name.clone(), id));
                    let _ = writeln!(Out(&mut c), "q {:.3} 0 0 {:.3} {:.2} {:.2} cm /{name} Do Q", w * PT, h * PT, x(*ix), y(iy + h));
                }
            }
        }
        let _ = writeln!(Out(&mut c), "Q EMC");
    }
    let content = pdf.stream("", &c);
    let xobj: String = images.iter().map(|(n, id)| format!("/{n} {id} 0 R ")).collect();
    let gsx: String = opac.iter().enumerate().map(|(i, (_, id))| format!("/GA{i} {id} 0 R ")).collect();
    let props: String = ocgs.iter().enumerate().map(|(i, (id, _))| format!("/L{i} {id} 0 R ")).collect();
    let pages = pdf.reserve();
    let page = pdf.add(
        format!(
            "<< /Type /Page /Parent {pages} 0 R /MediaBox [0 0 {:.2} {:.2}] /Resources << /Font << /F1 {font} 0 R /F2 {bold} 0 R >> /ExtGState << /GS0 {gs} 0 R {gsx}>> /XObject << {xobj}>> /Properties << {props}>> >> /Contents {content} 0 R >>",
            d.w * PT,
            d.h * PT
        )
        .into_bytes(),
    );
    pdf.set(pages, format!("<< /Type /Pages /Kids [{page} 0 R] /Count 1 >>").into_bytes());
    // warstwy PDF: grupy jak w SVG, ukryte wyłączone
    let all: String = ocgs.iter().map(|(id, _)| format!("{id} 0 R ")).collect();
    let off: String = ocgs.iter().filter(|(_, l)| l.hidden).map(|(id, _)| format!("{id} 0 R ")).collect();
    let mut order = String::new();
    let mut k = 0;
    while k < ocgs.len() {
        let g = ocgs[k].1.group;
        if g.is_empty() {
            let _ = write!(order, "{} 0 R ", ocgs[k].0);
            k += 1;
            continue;
        }
        let _ = write!(order, "[{} ", pdf_text(&ocgs[k].1.group_name));
        while k < ocgs.len() && ocgs[k].1.group == g {
            let _ = write!(order, "{} 0 R ", ocgs[k].0);
            k += 1;
        }
        order.push_str("] ");
    }
    let root = pdf.add(format!("<< /Type /Catalog /Pages {pages} 0 R /PageMode /UseOC /OCProperties << /OCGs [{all}] /D << /Name (GRAN Studio) /Order [{order}] /OFF [{off}] >> >> >>").into_bytes());
    let info = pdf.add(
        format!(
            "<< /Producer (GRAN Studio) /Creator (GRAN Studio - redzikowski.com) /Title {} /Subject {} /Keywords {} >>",
            pdf_text(&d.title),
            pdf_text(&d.desc),
            pdf_text(&d.credits.join("; "))
        )
        .into_bytes(),
    );
    pdf.finish(root, info)
}

/// `write!` do wektora bajtów.
struct Out<'a>(&'a mut Vec<u8>);

impl std::fmt::Write for Out<'_> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0.extend_from_slice(s.as_bytes());
        Ok(())
    }
}

/* ── przycinanie łamanych do wielokąta ─────────────────────────────────────────── */

fn inside(p: (f64, f64), poly: &[(f64, f64)]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Części łamanej leżące wewnątrz wielokąta `poly` (dowolnego, także wklęsłego).
pub fn clip_to_polygon(pts: &[(f64, f64)], closed: bool, poly: &[(f64, f64)]) -> Vec<Vec<(f64, f64)>> {
    let mut line = pts.to_vec();
    if closed && line.len() > 2 && line.first() != line.last() {
        line.push(line[0]);
    }
    let mut out: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut cur: Vec<(f64, f64)> = Vec::new();
    let n = poly.len();
    for w in line.windows(2) {
        let (a, b) = (w[0], w[1]);
        // parametry przecięć odcinka z krawędziami wielokąta
        let mut ts = vec![0.0, 1.0];
        for i in 0..n {
            let (c, e) = (poly[i], poly[(i + 1) % n]);
            let r = (b.0 - a.0, b.1 - a.1);
            let s = (e.0 - c.0, e.1 - c.1);
            let den = r.0 * s.1 - r.1 * s.0;
            if den.abs() < 1e-12 {
                continue;
            }
            let t = ((c.0 - a.0) * s.1 - (c.1 - a.1) * s.0) / den;
            let u = ((c.0 - a.0) * r.1 - (c.1 - a.1) * r.0) / den;
            if t > 0.0 && t < 1.0 && (0.0..=1.0).contains(&u) {
                ts.push(t);
            }
        }
        ts.sort_by(|p, q| p.total_cmp(q));
        let at = |t: f64| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        for k in 0..ts.len() - 1 {
            let (t0, t1) = (ts[k], ts[k + 1]);
            if t1 - t0 < 1e-9 {
                continue;
            }
            if inside(at((t0 + t1) / 2.0), poly) {
                let p0 = at(t0);
                if cur.last().map(|q| (q.0 - p0.0).abs() > 1e-9 || (q.1 - p0.1).abs() > 1e-9).unwrap_or(true) {
                    if cur.len() >= 2 {
                        out.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                    cur.push(p0);
                }
                cur.push(at(t1));
            } else if cur.len() >= 2 {
                out.push(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out
}

/* ── DXF (R12, ASCII) ──────────────────────────────────────────────────────────── */

/// Najbliższy kolor palety AutoCAD (ACI) – podstawowe kolory.
fn aci(c: Rgb) -> i32 {
    const P: [(i32, Rgb); 9] = [(1, [255, 0, 0]), (2, [255, 255, 0]), (3, [0, 255, 0]), (4, [0, 255, 255]), (5, [0, 0, 255]), (6, [255, 0, 255]), (7, [0, 0, 0]), (8, [128, 128, 128]), (30, [255, 127, 0])];
    let d = |a: Rgb, b: Rgb| (0..3).map(|i| (a[i] as i32 - b[i] as i32).pow(2)).sum::<i32>();
    P.iter().min_by_key(|(_, p)| d(*p, c)).map(|(k, _)| *k).unwrap_or(7)
}

fn dxf_text(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii() { c.to_string() } else { format!("\\U+{:04X}", c as u32) }).collect()
}

/// DXF: milimetry, y w górę (odbicie względem strony), warstwy jak w rysunku (ukryte – wyłączone),
/// warstwy obszaru przycięte geometrycznie do jego obrysu; obrazy pominięte.
pub fn dxf(d: &Drawing) -> String {
    let mut o = String::new();
    let y = |v: f64| d.h - v;
    let layers: Vec<&Layer> = d.layers.iter().filter(|l| !l.items.is_empty()).collect();
    o.push_str("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n9\n$INSUNITS\n70\n4\n9\n$EXTMIN\n10\n0\n20\n0\n9\n$EXTMAX\n10\n");
    let _ = write!(o, "{:.2}\n20\n{:.2}\n0\nENDSEC\n", d.w, d.h);
    o.push_str("0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n70\n");
    let _ = writeln!(o, "{}", layers.len());
    for l in &layers {
        let col = aci(lead_color(&l.items));
        let _ = write!(o, "0\nLAYER\n2\n{}\n70\n{}\n62\n{}\n6\nCONTINUOUS\n", l.id.to_uppercase(), if l.locked { 4 } else { 0 }, if l.hidden { -col } else { col });
    }
    o.push_str("0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n");
    let poly = |o: &mut String, layer: &str, pts: &[(f64, f64)], closed: bool| {
        let _ = write!(o, "0\nPOLYLINE\n8\n{layer}\n66\n1\n70\n{}\n", if closed { 1 } else { 0 });
        for p in pts {
            let _ = write!(o, "0\nVERTEX\n8\n{layer}\n10\n{:.3}\n20\n{:.3}\n", p.0, y(p.1));
        }
        o.push_str("0\nSEQEND\n");
    };
    for l in &layers {
        let layer = l.id.to_uppercase();
        let clip = if l.clipped { d.clip.as_deref() } else { None };
        let line = |o: &mut String, pts: &[(f64, f64)], closed: bool| match clip {
            Some(c) => {
                let parts = clip_to_polygon(pts, closed, c);
                // nieprzycięty zamknięty wielokąt zostaje zamknięty
                if parts.len() == 1 && closed && parts[0].len() == pts.len() + 1 {
                    poly(o, &layer, pts, true);
                } else {
                    for p in parts {
                        poly(o, &layer, &p, false);
                    }
                }
            }
            None => poly(o, &layer, pts, closed),
        };
        let keep = |p: (f64, f64)| clip.map(|c| inside(p, c)).unwrap_or(true);
        for it in &l.items {
            match it {
                Item::Path { pts, closed, .. } if pts.len() >= 2 => line(&mut o, pts, *closed),
                Item::Rings { rings, .. } => {
                    for r in rings {
                        line(&mut o, r, true);
                    }
                }
                Item::Circle { x, y: cy, r, .. } if keep((*x, *cy)) => {
                    let _ = write!(o, "0\nCIRCLE\n8\n{layer}\n10\n{:.3}\n20\n{:.3}\n40\n{:.3}\n", x, y(*cy), r);
                }
                Item::Text { x, y: ty, text, size, anchor, angle, .. } if keep((*x, *ty)) => {
                    let j = match anchor {
                        Anchor::Start => 0,
                        Anchor::Middle => 1,
                        Anchor::End => 2,
                    };
                    let _ = write!(o, "0\nTEXT\n8\n{layer}\n10\n{x:.3}\n20\n{:.3}\n40\n{:.3}\n1\n{}\n50\n{:.2}\n", y(*ty), size * 0.72, dxf_text(text), -angle);
                    if j != 0 {
                        let _ = write!(o, "72\n{j}\n11\n{x:.3}\n21\n{:.3}\n", y(*ty));
                    }
                }
                _ => {}
            }
        }
    }
    o.push_str("0\nENDSEC\n0\nEOF\n");
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Drawing {
        let mut d = Drawing::new(100.0, 50.0, "Przekrój – Łomnica");
        d.declare("rzezba", "Rzeźba terenu", "teren", "Teren", true);
        d.declare("rzezba", "Rzeźba terenu", "teren2", "Teren 2", true);
        d.layer("teren", "Teren").push(Item::Path { pts: vec![(0.0, 40.0), (50.0, 10.0), (100.0, 40.0)], closed: false, stroke: Some(([10, 10, 10], 0.3)), fill: None, dash: None });
        d.layer("teren2", "Teren 2").push(Item::Path { pts: vec![(0.0, 30.0), (100.0, 30.0)], closed: false, stroke: Some(([10, 10, 10], 0.3)), fill: None, dash: None });
        d.layer("opisy", "Opisy").push(Item::Text { x: 50.0, y: 8.0, text: "Łomnica 2634 m".into(), size: 3.0, color: [0, 0, 0], anchor: Anchor::Middle, angle: 0.0, halo: true, bold: true });
        d.layer("podklad", "Podkład").push(Item::Image { x: 0.0, y: 0.0, w: 10.0, h: 10.0, img: image::RgbImage::new(4, 4) });
        d.clip = Some(vec![(10.0, 5.0), (90.0, 5.0), (90.0, 45.0), (10.0, 45.0)]);
        d.background = true;
        d.credits = vec!["© Autorzy OpenStreetMap".into()];
        d
    }

    #[test]
    fn writers() {
        let mut d = sample();
        let mut cfg = BTreeMap::new();
        cfg.insert("teren2".to_string(), LayerCfg { visible: false, color: Some([200, 0, 0]), ..Default::default() });
        d.apply(&cfg);
        let s = svg(&d);
        assert!(s.contains("inkscape:groupmode=\"layer\"") && s.contains("Łomnica"));
        assert!(s.contains("clip-path=\"url(#obszar)\"") && s.contains("display:none") && s.contains("#c80000"));
        assert!(s.contains("<dc:title>"));
        assert!(roxmltree::Document::parse(&s).is_ok(), "SVG nie jest poprawnym XML");
        let p = pdf(&d);
        assert!(p.starts_with(b"%PDF-1.5") && p.ends_with(b"%%EOF\n"));
        let text = String::from_utf8_lossy(&p);
        assert!(text.contains("/Lslash") && text.contains("/OCProperties") && text.contains(" W n"));
        let x = dxf(&d);
        assert!(x.contains("POLYLINE") && x.contains("\\U+0141"));
        assert_eq!(d.info().len(), 4);
    }

    #[test]
    fn polygon_clipping() {
        // wklęsły wielokąt w kształcie litery U
        let u = vec![(0.0, 0.0), (30.0, 0.0), (30.0, 30.0), (20.0, 30.0), (20.0, 10.0), (10.0, 10.0), (10.0, 30.0), (0.0, 30.0)];
        let parts = clip_to_polygon(&[(-5.0, 20.0), (35.0, 20.0)], false, &u);
        assert_eq!(parts.len(), 2);
        assert!((parts[0][0].0 - 0.0).abs() < 1e-6 && (parts[0][1].0 - 10.0).abs() < 1e-6);
        let inner = clip_to_polygon(&[(1.0, 1.0), (5.0, 5.0)], false, &u);
        assert_eq!(inner.len(), 1);
        assert_eq!(num(1.50), "1.5");
        assert_eq!(num(-0.001), "0");
        assert_eq!(today().len(), 10);
    }
}
