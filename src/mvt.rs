//! Punkty z kafla wektorowego (Mapbox Vector Tile): tylko to, czego potrzebują etykiety.

use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Text(String),
    Number(f64),
}

/// Punkt kafla: warstwa, położenie w kaflu (0..1) i atrybuty.
#[derive(Clone, Debug)]
pub struct Point {
    pub layer: String,
    pub x: f64,
    pub y: f64,
    pub tags: HashMap<String, Value>,
}

impl Point {
    pub fn text(&self, key: &str) -> Option<&str> {
        match self.tags.get(key) {
            Some(Value::Text(s)) => Some(s),
            _ => None,
        }
    }

    pub fn number(&self, key: &str) -> Option<f64> {
        match self.tags.get(key) {
            Some(Value::Number(n)) => Some(*n),
            _ => None,
        }
    }
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

/// Pole wiadomości protobuf.
enum Field<'a> {
    Varint(u64),
    Fixed(u64),
    Bytes(&'a [u8]),
}

impl<'a> Reader<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self { b, i: 0 }
    }

    fn varint(&mut self) -> Option<u64> {
        let (mut v, mut shift) = (0u64, 0u32);
        loop {
            let c = *self.b.get(self.i)?;
            self.i += 1;
            v |= ((c & 0x7f) as u64) << shift;
            if c & 0x80 == 0 {
                return Some(v);
            }
            shift += 7;
            if shift > 63 {
                return None;
            }
        }
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.i..self.i.checked_add(n)?)?;
        self.i += n;
        Some(s)
    }

    fn field(&mut self) -> Option<(u64, Field<'a>)> {
        if self.i >= self.b.len() {
            return None;
        }
        let key = self.varint()?;
        let f = match key & 7 {
            0 => Field::Varint(self.varint()?),
            1 => Field::Fixed(u64::from_le_bytes(self.take(8)?.try_into().ok()?)),
            2 => {
                let n = self.varint()? as usize;
                Field::Bytes(self.take(n)?)
            }
            5 => Field::Fixed(u32::from_le_bytes(self.take(4)?.try_into().ok()?) as u64),
            _ => return None,
        };
        Some((key >> 3, f))
    }
}

fn varints(b: &[u8]) -> Vec<u64> {
    let mut r = Reader::new(b);
    let mut out = Vec::new();
    while r.i < b.len() {
        match r.varint() {
            Some(v) => out.push(v),
            None => break,
        }
    }
    out
}

fn zigzag(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
}

fn value(b: &[u8]) -> Option<Value> {
    let mut r = Reader::new(b);
    let (tag, f) = r.field()?;
    Some(match (tag, f) {
        (1, Field::Bytes(s)) => Value::Text(String::from_utf8_lossy(s).into_owned()),
        (2, Field::Fixed(v)) => Value::Number(f32::from_bits(v as u32) as f64),
        (3, Field::Fixed(v)) => Value::Number(f64::from_bits(v)),
        (4 | 5, Field::Varint(v)) => Value::Number(v as i64 as f64),
        (6, Field::Varint(v)) => Value::Number(zigzag(v) as f64),
        (7, Field::Varint(v)) => Value::Number(v as f64),
        _ => return None,
    })
}

/// Linia kafla: warstwa, atrybuty i części (łamane) w układzie kafla – 0..1, z zapasem poza krawędzią.
#[derive(Clone, Debug)]
pub struct Line {
    pub layer: String,
    pub tags: HashMap<String, Value>,
    pub parts: Vec<Vec<(f64, f64)>>,
}

impl Line {
    pub fn text(&self, key: &str) -> Option<&str> {
        match self.tags.get(key) {
            Some(Value::Text(s)) => Some(s),
            _ => None,
        }
    }
}

/// Warstwa kafla rozłożona na części: nazwa, rozdzielczość, słowniki atrybutów i surowe obiekty.
struct RawLayer<'a> {
    name: String,
    extent: f64,
    keys: Vec<String>,
    values: Vec<Option<Value>>,
    features: Vec<&'a [u8]>,
}

fn raw_layer(b: &[u8]) -> RawLayer<'_> {
    let mut l = RawLayer { name: String::new(), extent: 4096.0, keys: Vec::new(), values: Vec::new(), features: Vec::new() };
    let mut r = Reader::new(b);
    while let Some((tag, f)) = r.field() {
        match (tag, f) {
            (1, Field::Bytes(s)) => l.name = String::from_utf8_lossy(s).into_owned(),
            (2, Field::Bytes(s)) => l.features.push(s),
            (3, Field::Bytes(s)) => l.keys.push(String::from_utf8_lossy(s).into_owned()),
            (4, Field::Bytes(s)) => l.values.push(value(s)),
            (5, Field::Varint(v)) => l.extent = (v as f64).max(1.0),
            _ => {}
        }
    }
    l
}

/// Obiekt warstwy: indeksy atrybutów, rodzaj geometrii (1 punkt, 2 linia, 3 wielokąt) i polecenia geometrii.
fn raw_feature(feature: &[u8]) -> (Vec<u64>, u64, Vec<u64>) {
    let (mut tags, mut kind, mut geometry) = (Vec::new(), 0, Vec::new());
    let mut r = Reader::new(feature);
    while let Some((tag, f)) = r.field() {
        match (tag, f) {
            (2, Field::Bytes(s)) => tags = varints(s),
            (3, Field::Varint(v)) => kind = v,
            (4, Field::Bytes(s)) => geometry = varints(s),
            _ => {}
        }
    }
    (tags, kind, geometry)
}

fn tag_map(l: &RawLayer<'_>, tags: &[u64]) -> HashMap<String, Value> {
    let mut map = HashMap::new();
    for kv in tags.chunks_exact(2) {
        if let (Some(k), Some(Some(v))) = (l.keys.get(kv[0] as usize), l.values.get(kv[1] as usize)) {
            map.insert(k.clone(), v.clone());
        }
    }
    map
}

const POINT: u64 = 1;
const LINE: u64 = 2;
const MOVE_TO: u64 = 1;
const LINE_TO: u64 = 2;

/// Warstwy kafla (pole 3 wiadomości).
fn layers(tile: &[u8]) -> Vec<RawLayer<'_>> {
    const LAYER: u64 = 3;
    let mut out = Vec::new();
    let mut r = Reader::new(tile);
    while let Some((tag, f)) = r.field() {
        if let (LAYER, Field::Bytes(layer)) = (tag, f) {
            out.push(raw_layer(layer));
        }
    }
    out
}

/// Punkty z wybranych warstw. Punkty leżące w zapasie poza kaflem są pomijane: należą do sąsiada.
pub fn points(tile: &[u8], wanted: &[&str]) -> Vec<Point> {
    let mut out = Vec::new();
    for l in layers(tile).iter().filter(|l| wanted.contains(&l.name.as_str())) {
        for feature in &l.features {
            let (tags, kind, geometry) = raw_feature(feature);
            if kind != POINT || geometry.len() < 3 || geometry[0] & 7 != MOVE_TO {
                continue;
            }
            let (x, y) = (zigzag(geometry[1]) as f64 / l.extent, zigzag(geometry[2]) as f64 / l.extent);
            if !(0.0..1.0).contains(&x) || !(0.0..1.0).contains(&y) {
                continue;
            }
            out.push(Point { layer: l.name.clone(), x, y, tags: tag_map(l, &tags) });
        }
    }
    out
}

/// Linie z wybranych warstw (drogi, tory). Współrzędne mogą wychodzić w zapas poza kafel.
pub fn lines(tile: &[u8], wanted: &[&str]) -> Vec<Line> {
    let mut out = Vec::new();
    for l in layers(tile).iter().filter(|l| wanted.contains(&l.name.as_str())) {
        for feature in &l.features {
            let (tags, kind, geometry) = raw_feature(feature);
            if kind != LINE {
                continue;
            }
            // polecenia: (rodzaj | liczba << 3), potem pary przesunięć zapisane zygzakiem
            let mut parts: Vec<Vec<(f64, f64)>> = Vec::new();
            let (mut x, mut y) = (0i64, 0i64);
            let mut i = 0;
            while i < geometry.len() {
                let (cmd, count) = (geometry[i] & 7, (geometry[i] >> 3) as usize);
                i += 1;
                if cmd != MOVE_TO && cmd != LINE_TO {
                    // zamknięcie ścieżki nie ma parametrów; nieznane polecenie kończy obiekt
                    if cmd == 7 {
                        continue;
                    }
                    break;
                }
                if count > geometry.len() || i + count * 2 > geometry.len() {
                    break;
                }
                for _ in 0..count {
                    x += zigzag(geometry[i]);
                    y += zigzag(geometry[i + 1]);
                    i += 2;
                    let p = (x as f64 / l.extent, y as f64 / l.extent);
                    if cmd == MOVE_TO {
                        parts.push(vec![p]);
                    } else if let Some(last) = parts.last_mut() {
                        last.push(p);
                    }
                }
            }
            parts.retain(|p| p.len() >= 2);
            if !parts.is_empty() {
                out.push(Line { layer: l.name.clone(), tags: tag_map(l, &tags), parts });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var(mut v: u64) -> Vec<u8> {
        let mut o = Vec::new();
        loop {
            let c = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                o.push(c);
                return o;
            }
            o.push(c | 0x80);
        }
    }

    fn bytes(tag: u64, b: &[u8]) -> Vec<u8> {
        let mut o = var(tag << 3 | 2);
        o.extend(var(b.len() as u64));
        o.extend_from_slice(b);
        o
    }

    fn vint(tag: u64, v: u64) -> Vec<u8> {
        let mut o = var(tag << 3);
        o.extend(var(v));
        o
    }

    fn zz(v: i64) -> u64 {
        ((v << 1) ^ (v >> 63)) as u64
    }

    fn layer(name: &str, x: i64, y: i64) -> Vec<u8> {
        let mut f = bytes(2, &[0, 0, 1, 1]);
        f.extend(vint(3, 1));
        let g: Vec<u8> = [9, zz(x), zz(y)].iter().flat_map(|v| var(*v)).collect();
        f.extend(bytes(4, &g));
        let mut l = bytes(1, name.as_bytes());
        l.extend(bytes(2, &f));
        l.extend(bytes(3, b"name"));
        l.extend(bytes(3, b"ele"));
        l.extend(bytes(4, &bytes(1, "Świnica".as_bytes())));
        l.extend(bytes(4, &vint(4, 2301)));
        l.extend(vint(5, 4096));
        l
    }

    #[test]
    fn reads_points_of_wanted_layers() {
        let mut tile = bytes(3, &layer("mountain_peak", 1024, 2048));
        tile.extend(bytes(3, &layer("building", 10, 10)));
        tile.extend(bytes(3, &layer("place", -5, 10)));
        let p = points(&tile, &["mountain_peak", "place"]);
        assert_eq!(p.len(), 1, "budynek odrzucony, punkt spoza kafla też");
        assert_eq!(p[0].layer, "mountain_peak");
        assert_eq!((p[0].x, p[0].y), (0.25, 0.5));
        assert_eq!(p[0].text("name"), Some("Świnica"));
        assert_eq!(p[0].number("ele"), Some(2301.0));
    }

    #[test]
    fn reads_lines_with_tags() {
        // łamana (0,0) → (2048,0) → (2048,4096) i druga część od (100,100) o jeden odcinek
        let geom: Vec<u8> = [9, zz(0), zz(0), 2 << 3 | 2, zz(2048), zz(0), zz(0), zz(4096), 9, zz(-1948), zz(-3996), 1 << 3 | 2, zz(50), zz(0)]
            .iter()
            .flat_map(|v| var(*v))
            .collect();
        let mut f = bytes(2, &[0, 0]);
        f.extend(vint(3, 2));
        f.extend(bytes(4, &geom));
        let mut l = bytes(1, b"transportation");
        l.extend(bytes(2, &f));
        l.extend(bytes(3, b"class"));
        l.extend(bytes(4, &bytes(1, b"primary")));
        l.extend(vint(5, 4096));
        let mut tile = bytes(3, &l);
        tile.extend(bytes(3, &layer("place", 10, 10)));
        let got = lines(&tile, &["transportation"]);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].text("class"), Some("primary"));
        assert_eq!(got[0].parts.len(), 2);
        assert_eq!(got[0].parts[0], vec![(0.0, 0.0), (0.5, 0.0), (0.5, 1.0)]);
        assert_eq!(got[0].parts[1].len(), 2);
        assert!(lines(&tile, &["place"]).is_empty(), "punkt nie jest linią");
    }

    #[test]
    fn broken_tile_gives_nothing() {
        assert!(points(&[0xff, 0xff, 0xff], &["place"]).is_empty());
        assert!(points(&bytes(3, &[0x0a, 0xff]), &["place"]).is_empty());
    }
}
