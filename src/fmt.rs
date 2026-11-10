//! Formatowanie liczb, odległości, czasu i współrzędnych po polsku (przecinek dziesiętny, spacja tysięcy).

/// Liczba całkowita z odstępem tysięcy: „12 450”.
pub fn int(v: f64) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    let n = v.round() as i64;
    let s = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 && s.len() > 4 {
            out.push(' ');
        }
        out.push(c);
    }
    if n < 0 {
        format!("−{out}")
    } else {
        out
    }
}

/// Liczba z `d` miejscami po przecinku.
pub fn dec(v: f64, d: usize) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    format!("{v:.d$}").replace('.', ",").replace('-', "−")
}

pub fn dist(m: f64) -> String {
    if !m.is_finite() {
        "—".into()
    } else if m < 1000.0 {
        format!("{} m", int(m))
    } else if m < 10_000.0 {
        tf!("{} km", dec(m / 1000.0, 2))
    } else {
        tf!("{} km", dec(m / 1000.0, 1))
    }
}

pub fn ele(m: f64) -> String {
    if m.is_finite() {
        format!("{} m", int(m))
    } else {
        "—".into()
    }
}

/// Czas w godzinach → „3 h 25 min” (zaokrąglenie do 5 min).
pub fn time(h: f64) -> String {
    if !h.is_finite() {
        return "—".into();
    }
    let total = ((h * 60.0 / 5.0).round() * 5.0) as i64;
    let (hh, mm) = (total / 60, total % 60);
    match (hh, mm) {
        (0, m) => tf!("{} min", m),
        (h, 0) => format!("{h} h"),
        (h, m) => tf!("{} h {:02} min", h, m),
    }
}

pub fn signed(m: f64, unit: &str) -> String {
    let s = if m > 0.5 {
        "+"
    } else if m < -0.5 {
        "−"
    } else {
        "±"
    };
    tf!("{}{} {}", s, int(m.abs()), unit)
}

/// Współrzędne w formacie wybranym w ustawieniach (stopnie, UTM, MGRS).
pub fn coords(lon: f64, lat: f64) -> String {
    crate::coords::format_pos(lon, lat)
}

/// Stopnie dziesiętne – zapis stały (nazwy zapisanych miejsc, linki).
pub fn decimal(lon: f64, lat: f64) -> String {
    tf!("{:.5}, {:.5}", lat, lon)
}

const DIRS: [&str; 16] = [
    "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW", "NW",
    "NNW",
];

pub fn compass(deg: f64) -> &'static str {
    DIRS[((deg.rem_euclid(360.0) / 22.5).round() as usize) % 16]
}

/// Godzina dziesiętna → „07:45”.
pub fn hour(h: f64) -> String {
    if !h.is_finite() {
        return "—".into();
    }
    let m = (h * 60.0).round() as i64;
    format!("{:02}:{:02}", (m / 60) % 24, m % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polish_formats() {
        assert_eq!(dist(850.0), "850 m");
        assert_eq!(dist(4321.0), "4,32 km");
        assert_eq!(time(3.42), "3 h 25 min");
        assert_eq!(time(0.5), "30 min");
        assert_eq!(int(12450.0), "12 450");
        assert_eq!(int(2499.0), "2499");
        assert_eq!(compass(350.0), "N");
        assert_eq!(hour(7.75), "07:45");
    }
}
