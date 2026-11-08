//! Położenie Słońca (algorytm NOAA, dokładność ~0,01°), czas polski, wschody i zachody, pora roku.

/// Dzień kalendarzowy (rok, miesiąc 1–12, dzień 1–31).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Day {
    pub y: i32,
    pub m: u32,
    pub d: u32,
}

/// Dni od 1970-01-01 (algorytm H. Hinnanta).
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn civil_from_days(z: i64) -> Day {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    Day {
        y: (if m <= 2 { y + 1 } else { y }) as i32,
        m,
        d,
    }
}

impl Day {
    pub fn today() -> Day {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let local = secs + warsaw_offset_min(secs as f64) as i64 * 60;
        civil_from_days(local.div_euclid(86_400))
    }

    pub fn ordinal(self) -> u32 {
        (days_from_civil(self.y, self.m, self.d) - days_from_civil(self.y, 1, 1) + 1) as u32
    }

    pub fn add_days(self, n: i64) -> Day {
        civil_from_days(days_from_civil(self.y, self.m, self.d) + n)
    }

    pub fn label(self) -> String {
        const M: [&str; 12] = [
            "stycznia",
            "lutego",
            "marca",
            "kwietnia",
            "maja",
            "czerwca",
            "lipca",
            "sierpnia",
            "września",
            "października",
            "listopada",
            "grudnia",
        ];
        format!("{} {} {}", self.d, crate::i18n::tr(M[(self.m - 1) as usize]), self.y)
    }
}

/// Ostatnia niedziela miesiąca (dzień od 1970-01-01).
fn last_sunday(y: i32, m: u32) -> i64 {
    let next = if m == 12 {
        days_from_civil(y + 1, 1, 1)
    } else {
        days_from_civil(y, m + 1, 1)
    };
    let last = next - 1;
    // 1970-01-01 był czwartkiem: (dni + 4) % 7 = 0 → niedziela
    last - (last + 4).rem_euclid(7)
}

/// Przesunięcie czasu Europe/Warsaw (min) w chwili `unix` (sekundy UTC): CET/CEST.
pub fn warsaw_offset_min(unix: f64) -> i32 {
    let day = (unix / 86_400.0).floor() as i64;
    let y = civil_from_days(day).y;
    let start = last_sunday(y, 3) as f64 * 86_400.0 + 3600.0;
    let end = last_sunday(y, 10) as f64 * 86_400.0 + 3600.0;
    if unix >= start && unix < end {
        120
    } else {
        60
    }
}

/// Dzień i godzina dziesiętna czasu polskiego → sekundy UTC.
pub fn warsaw_unix(day: Day, hour: f64) -> f64 {
    let base = days_from_civil(day.y, day.m, day.d) as f64 * 86_400.0;
    let off = warsaw_offset_min(base + 12.0 * 3600.0) as f64;
    base + (hour * 60.0 - off) * 60.0
}

/// Azymut (0 = N, zgodnie z zegarem) i wysokość nad horyzontem (stopnie).
pub fn position(unix: f64, lat: f64, lon: f64) -> (f64, f64) {
    let deg = std::f64::consts::PI / 180.0;
    let jd = unix / 86_400.0 + 2_440_587.5;
    let t = (jd - 2_451_545.0) / 36_525.0;
    let l0 = (280.46646 + t * (36000.76983 + t * 0.0003032)).rem_euclid(360.0);
    let m = 357.52911 + t * (35999.05029 - 0.0001537 * t);
    let e = 0.016708634 - t * (0.000042037 + 0.0000001267 * t);
    let c = (m * deg).sin() * (1.914602 - t * (0.004817 + 0.000014 * t))
        + (2.0 * m * deg).sin() * (0.019993 - 0.000101 * t)
        + (3.0 * m * deg).sin() * 0.000289;
    let omega = 125.04 - 1934.136 * t;
    let lambda = l0 + c - 0.00569 - 0.00478 * (omega * deg).sin();
    let eps0 = 23.0 + (26.0 + (21.448 - t * (46.815 + t * (0.00059 - t * 0.001813))) / 60.0) / 60.0;
    let eps = eps0 + 0.00256 * (omega * deg).cos();
    let decl = ((eps * deg).sin() * (lambda * deg).sin()).asin();
    let y = ((eps / 2.0) * deg).tan().powi(2);
    let eq_time = 4.0 / deg
        * (y * (2.0 * l0 * deg).sin() - 2.0 * e * (m * deg).sin()
            + 4.0 * e * y * (m * deg).sin() * (2.0 * l0 * deg).cos()
            - 0.5 * y * y * (4.0 * l0 * deg).sin()
            - 1.25 * e * e * (2.0 * m * deg).sin());
    let minutes = (unix.rem_euclid(86_400.0)) / 60.0;
    let tst = (minutes + eq_time + 4.0 * lon + 1440.0).rem_euclid(1440.0);
    let mut ha = tst / 4.0 - 180.0;
    if ha < -180.0 {
        ha += 360.0;
    }
    let lr = lat * deg;
    let cos_zen = lr.sin() * decl.sin() + lr.cos() * decl.cos() * (ha * deg).cos();
    let zen = cos_zen.clamp(-1.0, 1.0).acos();
    let mut az = ((lr.sin() * zen.cos() - decl.sin()) / (lr.cos() * zen.sin()))
        .clamp(-1.0, 1.0)
        .acos()
        / deg;
    az = if ha > 0.0 {
        (az + 180.0).rem_euclid(360.0)
    } else {
        (540.0 - az).rem_euclid(360.0)
    };
    let mut alt = 90.0 - zen / deg;
    if alt > -0.575 {
        alt += 1.02 / ((alt + 10.3 / (alt + 5.11)) * deg).tan() / 60.0;
    }
    (az, alt)
}

/// Wschód, zachód, górowanie (godziny czasu polskiego) i najwyższa wysokość Słońca.
pub struct SunTimes {
    pub rise: f64,
    pub set: f64,
    pub noon: f64,
    pub max_alt: f64,
}

pub fn times(day: Day, lat: f64, lon: f64) -> SunTimes {
    let alt = |h: f64| position(warsaw_unix(day, h), lat, lon).1 + 0.833;
    let find = |a: f64, b: f64| {
        let mut fa = alt(a);
        let mut h = a + 0.25;
        while h <= b {
            let fb = alt(h);
            if fa.signum() != fb.signum() {
                let (mut lo, mut hi) = (h - 0.25, h);
                for _ in 0..20 {
                    let m = (lo + hi) / 2.0;
                    if alt(m).signum() == alt(lo).signum() {
                        lo = m
                    } else {
                        hi = m
                    }
                }
                return (lo + hi) / 2.0;
            }
            fa = fb;
            h += 0.25;
        }
        f64::NAN
    };
    let (mut noon, mut best) = (12.0, -90.0);
    let mut h = 9.0;
    while h <= 15.0 {
        let a = alt(h);
        if a > best {
            best = a;
            noon = h;
        }
        h += 0.05;
    }
    SunTimes {
        rise: find(0.0, 12.5),
        set: find(12.5, 24.0),
        noon,
        max_alt: best - 0.833,
    }
}

/// Wektor do Słońca w układzie świata (x – wschód, y – góra, z – południe).
pub fn vector(az: f64, alt: f64) -> [f32; 3] {
    let (a, h) = (az.to_radians(), alt.to_radians());
    [
        (a.sin() * h.cos()) as f32,
        h.sin() as f32,
        (-a.cos() * h.cos()) as f32,
    ]
}

/// Sezonowa granica śniegu (m) dla Karpat i Sudetów – przybliżenie klimatologiczne.
pub fn snowline(day: Day) -> f64 {
    const K: [(f64, f64); 12] = [
        (0.0, 700.0),
        (45.0, 650.0),
        (80.0, 1000.0),
        (110.0, 1500.0),
        (140.0, 1950.0),
        (170.0, 2350.0),
        (200.0, 2900.0),
        (260.0, 2900.0),
        (290.0, 2150.0),
        (315.0, 1600.0),
        (335.0, 1100.0),
        (366.0, 700.0),
    ];
    let doy = day.ordinal() as f64;
    for w in K.windows(2) {
        if doy <= w[1].0 {
            return w[0].1 + (w[1].1 - w[0].1) * (doy - w[0].0) / (w[1].0 - w[0].0);
        }
    }
    700.0
}

/// Natężenie barw jesieni (0..1): wrzesień–październik.
pub fn autumn(day: Day) -> f32 {
    let d = day.ordinal() as f32;
    ((d - 250.0) / 25.0).clamp(0.0, 1.0) * (1.0 - ((d - 305.0) / 15.0).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_roundtrip() {
        for z in [-1000i64, 0, 19_000, 20_400] {
            let d = civil_from_days(z);
            assert_eq!(days_from_civil(d.y, d.m, d.d), z);
        }
    }

    #[test]
    fn summer_noon_in_tatras() {
        let day = Day { y: 2026, m: 6, d: 21 };
        let t = times(day, 49.23, 19.98);
        assert!((t.noon - 13.0).abs() < 0.4, "{}", t.noon);
        assert!(t.rise > 4.0 && t.rise < 5.0, "{}", t.rise);
        assert!(t.max_alt > 63.0 && t.max_alt < 65.5, "{}", t.max_alt);
        assert_eq!(warsaw_offset_min(warsaw_unix(day, 12.0)), 120);
    }
}
