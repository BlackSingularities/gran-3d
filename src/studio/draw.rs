//! Rysunek wektorowy Studio (mapy, przekroje) i jego zapis do SVG, PDF i DXF.
//!
//! Współrzędne w milimetrach strony, początek w lewym górnym rogu, y w dół (jak w SVG). Każdy
//! element należy do warstwy – w SVG to warstwy Inkscape, w DXF warstwy CAD, w PDF kolejność.

use std::fmt::Write;

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

#[derive(Clone, Debug)]
pub struct Layer {
    pub id: &'static str,
    pub name: String,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, Default)]
pub struct Drawing {
    pub w: f64,
    pub h: f64,
    pub title: String,
    pub layers: Vec<Layer>,
}

impl Drawing {
    pub fn new(w: f64, h: f64, title: &str) -> Self {
        Self { w, h, title: title.to_string(), layers: Vec::new() }
    }

    /// Warstwa o identyfikatorze `id` (tworzona przy pierwszym użyciu, w kolejności rysowania).
    pub fn layer(&mut self, id: &'static str, name: &str) -> &mut Vec<Item> {
        if let Some(k) = self.layers.iter().position(|l| l.id == id) {
            return &mut self.layers[k].items;
        }
        self.layers.push(Layer { id, name: name.to_string(), items: Vec::new() });
        &mut self.layers.last_mut().unwrap().items
    }

    pub fn is_empty(&self) -> bool {
        self.layers.iter().all(|l| l.items.is_empty())
    }
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

fn path_d(pts: &[(f64, f64)], closed: bool) -> String {
    let mut d = String::with_capacity(pts.len() * 16);
    for (k, p) in pts.iter().enumerate() {
        let _ = write!(d, "{}{:.2} {:.2}", if k == 0 { "M" } else { "L" }, p.0, p.1);
    }
    if closed {
        d.push('Z');
    }
    d
}

/* ── SVG ───────────────────────────────────────────────────────────────────────── */

pub fn svg(d: &Drawing) -> String {
    use base64::Engine;
    let mut o = String::new();
    let _ = writeln!(
        o,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape" width="{w}mm" height="{h}mm" viewBox="0 0 {w} {h}" font-family="Helvetica, Arial, sans-serif">
<title>{t}</title>"#,
        w = d.w,
        h = d.h,
        t = esc(&d.title)
    );
    for l in &d.layers {
        if l.items.is_empty() {
            continue;
        }
        let _ = writeln!(o, r#"<g id="{}" inkscape:groupmode="layer" inkscape:label="{}">"#, l.id, esc(&l.name));
        for it in &l.items {
            match it {
                Item::Path { pts, closed, stroke, fill, dash } => {
                    let _ = write!(o, r#"<path d="{}" fill="{}""#, path_d(pts, *closed), fill.map(hex).unwrap_or("none".into()));
                    if let Some((c, w)) = stroke {
                        let _ = write!(o, r#" stroke="{}" stroke-width="{:.3}" stroke-linejoin="round" stroke-linecap="round""#, hex(*c), w);
                    }
                    if let Some(ds) = dash {
                        let _ = write!(o, r#" stroke-dasharray="{:.2} {:.2}""#, ds, ds * 0.7);
                    }
                    o.push_str("/>\n");
                }
                Item::Rings { rings, fill, stroke } => {
                    let dd: String = rings.iter().map(|r| path_d(r, true)).collect();
                    let _ = write!(o, r#"<path d="{}" fill="{}" fill-rule="evenodd""#, dd, hex(*fill));
                    if let Some((c, w)) = stroke {
                        let _ = write!(o, r#" stroke="{}" stroke-width="{:.3}""#, hex(*c), w);
                    }
                    o.push_str("/>\n");
                }
                Item::Text { x, y, text, size, color, anchor, angle, halo, bold } => {
                    let a = match anchor {
                        Anchor::Start => "start",
                        Anchor::Middle => "middle",
                        Anchor::End => "end",
                    };
                    let tr = if *angle != 0.0 { format!(r#" transform="rotate({:.2} {:.2} {:.2})""#, angle, x, y) } else { String::new() };
                    let weight = if *bold { r#" font-weight="bold""# } else { "" };
                    let halo_attr = if *halo { format!(r##" stroke="#ffffff" stroke-width="{:.2}" stroke-linejoin="round" paint-order="stroke""##, size * 0.22) } else { String::new() };
                    let _ = writeln!(o, r#"<text x="{:.2}" y="{:.2}" font-size="{:.2}" fill="{}" text-anchor="{}"{}{}{}>{}</text>"#, x, y, size, hex(*color), a, weight, halo_attr, tr, esc(text));
                }
                Item::Circle { x, y, r, fill, stroke } => {
                    let _ = write!(o, r#"<circle cx="{:.2}" cy="{:.2}" r="{:.2}" fill="{}""#, x, y, r, fill.map(hex).unwrap_or("none".into()));
                    if let Some((c, w)) = stroke {
                        let _ = write!(o, r#" stroke="{}" stroke-width="{:.3}""#, hex(*c), w);
                    }
                    o.push_str("/>\n");
                }
                Item::Image { x, y, w, h, img } => {
                    let mut png = Vec::new();
                    let _ = img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
                    let _ = writeln!(o, r#"<image x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" preserveAspectRatio="none" href="data:image/png;base64,{}"/>"#, x, y, w, h, b64);
                }
            }
        }
        o.push_str("</g>\n");
    }
    o.push_str("</svg>\n");
    o
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
        let mut out = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
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
    let enc = pdf.add(format!("<< /Type /Encoding /BaseEncoding /WinAnsiEncoding /Differences {diffs} >>").into_bytes());
    let font = pdf.add(format!("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding {enc} 0 R >>").into_bytes());
    let bold = pdf.add(format!("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding {enc} 0 R >>").into_bytes());
    let gs = pdf.add(b"<< /Type /ExtGState /LJ 1 /LC 1 >>".to_vec());
    let mut images = Vec::new();
    let mut c: Vec<u8> = Vec::new();
    let y = |v: f64| (d.h - v) * PT;
    let x = |v: f64| v * PT;
    let rgb = |c: Rgb| format!("{:.3} {:.3} {:.3}", c[0] as f64 / 255.0, c[1] as f64 / 255.0, c[2] as f64 / 255.0);
    let _ = writeln!(Out(&mut c), "/GS0 gs");
    for l in &d.layers {
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
                    let wdt = text_width(text, *size);
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
                    // tryb 1 – sam biały obrys (otoczka napisu), tryb 0 – wypełnienie
                    for mode in [1u8, 0] {
                        if mode == 1 && !*halo {
                            continue;
                        }
                        let _ = write!(
                            Out(&mut c),
                            "BT {f} {:.2} Tf {} rg {} RG {:.3} w {mode} Tr {ca:.4} {sa:.4} {:.4} {ca:.4} {:.2} {:.2} Tm ",
                            size * PT,
                            rgb(*color),
                            rgb([255, 255, 255]),
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
    }
    let content = pdf.stream("", &c);
    let xobj: String = images.iter().map(|(n, id)| format!("/{n} {id} 0 R ")).collect();
    let pages = pdf.reserve();
    let page = pdf.add(
        format!(
            "<< /Type /Page /Parent {pages} 0 R /MediaBox [0 0 {:.2} {:.2}] /Resources << /Font << /F1 {font} 0 R /F2 {bold} 0 R >> /ExtGState << /GS0 {gs} 0 R >> /XObject << {xobj}>> >> /Contents {content} 0 R >>",
            d.w * PT,
            d.h * PT
        )
        .into_bytes(),
    );
    pdf.set(pages, format!("<< /Type /Pages /Kids [{page} 0 R] /Count 1 >>").into_bytes());
    let root = pdf.add(format!("<< /Type /Catalog /Pages {pages} 0 R >>").into_bytes());
    let mut title = b"<< /Producer (GRAN Studio) /Title ".to_vec();
    title.extend_from_slice(&pdf_string(&d.title, &codes));
    title.extend_from_slice(b" >>");
    let info = pdf.add(title);
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

/* ── DXF (R12, ASCII) ──────────────────────────────────────────────────────────── */

/// Najbliższy kolor palety AutoCAD (ACI) – podstawowe kolory.
fn aci(c: Rgb) -> u8 {
    const P: [(u8, Rgb); 9] = [(1, [255, 0, 0]), (2, [255, 255, 0]), (3, [0, 255, 0]), (4, [0, 255, 255]), (5, [0, 0, 255]), (6, [255, 0, 255]), (7, [0, 0, 0]), (8, [128, 128, 128]), (30, [255, 127, 0])];
    let d = |a: Rgb, b: Rgb| (0..3).map(|i| (a[i] as i32 - b[i] as i32).pow(2)).sum::<i32>();
    P.iter().min_by_key(|(_, p)| d(*p, c)).map(|(k, _)| *k).unwrap_or(7)
}

fn dxf_text(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii() { c.to_string() } else { format!("\\U+{:04X}", c as u32) }).collect()
}

/// DXF: milimetry, y w górę (odbicie względem strony), warstwy jak w rysunku; obrazy pominięte.
pub fn dxf(d: &Drawing) -> String {
    let mut o = String::new();
    let y = |v: f64| d.h - v;
    o.push_str("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n9\n$INSUNITS\n70\n4\n9\n$EXTMIN\n10\n0\n20\n0\n9\n$EXTMAX\n10\n");
    let _ = write!(o, "{:.2}\n20\n{:.2}\n0\nENDSEC\n", d.w, d.h);
    o.push_str("0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n70\n");
    let _ = writeln!(o, "{}", d.layers.len());
    for l in &d.layers {
        let col = l
            .items
            .iter()
            .find_map(|it| match it {
                Item::Path { stroke: Some((c, _)), .. } => Some(*c),
                Item::Path { fill: Some(c), .. } | Item::Rings { fill: c, .. } => Some(*c),
                Item::Text { color, .. } => Some(*color),
                _ => None,
            })
            .unwrap_or([0, 0, 0]);
        let _ = write!(o, "0\nLAYER\n2\n{}\n70\n0\n62\n{}\n6\nCONTINUOUS\n", l.id.to_uppercase(), aci(col));
    }
    o.push_str("0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n");
    let poly = |o: &mut String, layer: &str, pts: &[(f64, f64)], closed: bool| {
        let _ = write!(o, "0\nPOLYLINE\n8\n{layer}\n66\n1\n70\n{}\n", if closed { 1 } else { 0 });
        for p in pts {
            let _ = write!(o, "0\nVERTEX\n8\n{layer}\n10\n{:.3}\n20\n{:.3}\n", p.0, y(p.1));
        }
        o.push_str("0\nSEQEND\n");
    };
    for l in &d.layers {
        let layer = l.id.to_uppercase();
        for it in &l.items {
            match it {
                Item::Path { pts, closed, .. } if pts.len() >= 2 => poly(&mut o, &layer, pts, *closed),
                Item::Rings { rings, .. } => {
                    for r in rings {
                        poly(&mut o, &layer, r, true);
                    }
                }
                Item::Circle { x, y: cy, r, .. } => {
                    let _ = write!(o, "0\nCIRCLE\n8\n{layer}\n10\n{:.3}\n20\n{:.3}\n40\n{:.3}\n", x, y(*cy), r);
                }
                Item::Text { x, y: ty, text, size, anchor, angle, .. } => {
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
        d.layer("teren", "Teren").push(Item::Path { pts: vec![(0.0, 40.0), (50.0, 10.0), (100.0, 40.0)], closed: false, stroke: Some(([10, 10, 10], 0.3)), fill: None, dash: None });
        d.layer("opisy", "Opisy").push(Item::Text { x: 50.0, y: 8.0, text: "Łomnica 2634 m".into(), size: 3.0, color: [0, 0, 0], anchor: Anchor::Middle, angle: 0.0, halo: true, bold: true });
        d.layer("podklad", "Podkład").push(Item::Image { x: 0.0, y: 0.0, w: 10.0, h: 10.0, img: image::RgbImage::new(4, 4) });
        d
    }

    #[test]
    fn writers() {
        let d = sample();
        let s = svg(&d);
        assert!(s.contains("inkscape:groupmode=\"layer\"") && s.contains("Łomnica"));
        let p = pdf(&d);
        assert!(p.starts_with(b"%PDF-1.4") && p.ends_with(b"%%EOF\n"));
        assert!(String::from_utf8_lossy(&p).contains("/Lslash"));
        let x = dxf(&d);
        assert!(x.contains("POLYLINE") && x.contains("\\U+0141"));
    }
}
