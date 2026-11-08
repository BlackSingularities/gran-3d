//! Czasy przejścia (PTTK, DIN 33466), suma podejść z histerezą, ślad trasy i jego statystyki.

use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};

use crate::geo::haversine;

/// Model czasu przejścia: 0 – reguła PTTK, 1 – DIN 33466.
static MODEL: AtomicU8 = AtomicU8::new(0);
/// Tempo (mnożnik czasu × 1000): 800 szybkie … 1250 spokojne.
static PACE: AtomicU32 = AtomicU32::new(1000);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeModel {
    Pttk,
    Din,
}

impl TimeModel {
    pub fn label(self) -> &'static str {
        match self {
            TimeModel::Pttk => crate::i18n::tr("reguła PTTK"),
            TimeModel::Din => "DIN 33466",
        }
    }
}

pub fn set_model(m: TimeModel, pace: f64) {
    MODEL.store((m == TimeModel::Din) as u8, Ordering::Relaxed);
    PACE.store((pace * 1000.0).round() as u32, Ordering::Relaxed);
}

pub fn model() -> (TimeModel, f64) {
    (
        if MODEL.load(Ordering::Relaxed) == 1 {
            TimeModel::Din
        } else {
            TimeModel::Pttk
        },
        PACE.load(Ordering::Relaxed) as f64 / 1000.0,
    )
}

pub fn din_time(horiz: f64, up: f64, down: f64) -> f64 {
    let th = horiz / 4000.0;
    let tv = up / 300.0 + down / 500.0;
    th.max(tv) + th.min(tv) / 2.0
}

/// Czas przejścia (h). PTTK: 15 min/km + 1 min/10 m podejścia + 1 min/17 m zejścia.
pub fn walk_time(horiz: f64, up: f64, down: f64) -> f64 {
    let (m, pace) = model();
    let base = match m {
        TimeModel::Din => din_time(horiz, up, down),
        TimeModel::Pttk => horiz / 4000.0 + up / 600.0 + down / 1000.0,
    };
    base * pace
}

/// Mnożnik czasu dla skali trudności SAC (T1–T6).
pub fn sac_factor(sac: u8) -> f64 {
    [1.0, 1.0, 1.03, 1.12, 1.28, 1.5, 1.8]
        .get(sac as usize)
        .copied()
        .unwrap_or(1.0)
}

pub const SAC_LABEL: [&str; 7] = [
    "—",
    "T1 turystyczna",
    "T2 górska",
    "T3 wymagająca",
    "T4 wysokogórska",
    "T5 wymagająca wysokogórska",
    "T6 trudna wysokogórska",
];

pub const DIFFICULTY: [&str; 4] = ["łatwa", "umiarkowana", "trudna", "bardzo trudna"];

/// Suma podejść i zejść z histerezą (eliminuje szum modelu terenu).
pub fn climb(e: &[f32]) -> (f64, f64) {
    if e.is_empty() {
        return (0.0, 0.0);
    }
    let (mut up, mut down) = (0.0f64, 0.0f64);
    let mut r = e[0] as f64;
    for &v in &e[1..] {
        let d = v as f64 - r;
        if d >= 2.5 {
            up += d;
            r = v as f64;
        } else if d <= -2.5 {
            down -= d;
            r = v as f64;
        }
    }
    let d = *e.last().unwrap() as f64 - r;
    if d > 0.0 {
        up += d
    } else {
        down -= d
    }
    (up, down)
}

/// Kolory szlaków (PTTK i inne).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum TrailColor {
    Red,
    Blue,
    Green,
    Yellow,
    Black,
    Other,
    Offtrail,
    Orange,
    Purple,
    Brown,
}

impl TrailColor {
    pub fn parse(s: &str) -> TrailColor {
        match s {
            "red" => TrailColor::Red,
            "blue" => TrailColor::Blue,
            "green" => TrailColor::Green,
            "yellow" => TrailColor::Yellow,
            "black" => TrailColor::Black,
            "orange" => TrailColor::Orange,
            "purple" => TrailColor::Purple,
            "brown" => TrailColor::Brown,
            _ => TrailColor::Other,
        }
    }

    pub fn rgb(self) -> [u8; 3] {
        match self {
            TrailColor::Red => [232, 58, 48],
            TrailColor::Blue => [44, 116, 232],
            TrailColor::Green => [38, 170, 76],
            TrailColor::Yellow => [246, 205, 40],
            TrailColor::Black => [24, 24, 28],
            TrailColor::Other => [214, 196, 160],
            TrailColor::Offtrail => [255, 140, 60],
            TrailColor::Orange => [245, 130, 32],
            TrailColor::Purple => [176, 96, 214],
            TrailColor::Brown => [140, 90, 50],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            TrailColor::Red => crate::i18n::tr("czerwony"),
            TrailColor::Blue => crate::i18n::tr("niebieski"),
            TrailColor::Green => crate::i18n::tr("zielony"),
            TrailColor::Yellow => crate::i18n::tr("żółty"),
            TrailColor::Black => crate::i18n::tr("czarny"),
            TrailColor::Other => crate::i18n::tr("inny"),
            TrailColor::Offtrail => crate::i18n::tr("poza szlakiem"),
            TrailColor::Orange => crate::i18n::tr("pomarańczowy"),
            TrailColor::Purple => crate::i18n::tr("fioletowy"),
            TrailColor::Brown => crate::i18n::tr("brązowy"),
        }
    }
}

/// Ślad trasy z wartościami skumulowanymi.
#[derive(Clone, Debug, Default)]
pub struct Track {
    pub lon: Vec<f64>,
    pub lat: Vec<f64>,
    pub e: Vec<f32>,
    /// skumulowany dystans poziomy (m)
    pub d2: Vec<f32>,
    /// skumulowany dystans po powierzchni (m)
    pub d3: Vec<f32>,
    /// skumulowany czas (h)
    pub t: Vec<f32>,
    pub color: Vec<TrailColor>,
    pub sac: Vec<u8>,
}

impl Track {
    pub fn len(&self) -> usize {
        self.lon.len()
    }

    pub fn push(&mut self, lon: f64, lat: f64, e: f32, color: TrailColor, sac: u8) {
        self.lon.push(lon);
        self.lat.push(lat);
        self.e.push(e);
        self.color.push(color);
        self.sac.push(sac);
    }

    /// Skumulowane dystanse i czas; `time_factor` np. 1,35 dla marszu poza szlakiem.
    pub fn finalize(&mut self, time_factor: f64) {
        let n = self.len();
        self.d2 = vec![0.0; n];
        self.d3 = vec![0.0; n];
        self.t = vec![0.0; n];
        if n < 2 {
            return;
        }
        for i in 1..n {
            let h = haversine(self.lon[i - 1], self.lat[i - 1], self.lon[i], self.lat[i]);
            let dh = (self.e[i] - self.e[i - 1]) as f64;
            self.d2[i] = self.d2[i - 1] + h as f32;
            self.d3[i] = self.d3[i - 1] + h.hypot(dh) as f32;
            let dt =
                walk_time(h, dh.max(0.0), (-dh).max(0.0)) * sac_factor(self.sac[i]) * time_factor;
            self.t[i] = self.t[i - 1] + dt as f32;
        }
        // skalowanie, żeby suma zgadzała się z modelem liczonym dla całości (bez szumu terenu)
        let (up, down) = climb(&self.e);
        let total = self.d2[n - 1] as f64;
        let din = walk_time(total, up, down) * time_factor;
        let sac_w: f64 = (1..n)
            .map(|i| sac_factor(self.sac[i]) * (self.d2[i] - self.d2[i - 1]) as f64)
            .sum();
        let target = din * if total > 0.0 { sac_w / total } else { 1.0 };
        let k = if self.t[n - 1] > 0.0 {
            target / self.t[n - 1] as f64
        } else {
            1.0
        };
        for v in &mut self.t[1..] {
            *v *= k as f32;
        }
    }

    /// Punkt śladu na dystansie poziomym `d` (interpolacja): indeks i ułamek.
    pub fn at_dist(&self, d: f32) -> (usize, f32) {
        let n = self.len();
        if n < 2 {
            return (0, 0.0);
        }
        let i = self.d2.partition_point(|&v| v <= d).clamp(1, n - 1) - 1;
        let seg = self.d2[i + 1] - self.d2[i];
        (
            i,
            if seg > 0.0 {
                ((d - self.d2[i]) / seg).clamp(0.0, 1.0)
            } else {
                0.0
            },
        )
    }

    pub fn lerp(&self, i: usize, f: f32) -> (f64, f64, f32) {
        let j = (i + 1).min(self.len() - 1);
        let ff = f as f64;
        (
            self.lon[i] + (self.lon[j] - self.lon[i]) * ff,
            self.lat[i] + (self.lat[j] - self.lat[i]) * ff,
            self.e[i] + (self.e[j] - self.e[i]) * f,
        )
    }

    pub fn stats(&self) -> TrackStats {
        let n = self.len();
        let (up, down) = climb(&self.e);
        let mut s = TrackStats {
            up,
            down,
            min_ele: f64::INFINITY,
            max_ele: f64::NEG_INFINITY,
            ..Default::default()
        };
        if n == 0 {
            return s;
        }
        for i in 0..n {
            let e = self.e[i] as f64;
            if e > s.max_ele {
                s.max_ele = e;
                s.max_ele_idx = i;
            }
            s.min_ele = s.min_ele.min(e);
            s.sac_max = s.sac_max.max(self.sac[i]);
        }
        // największe nachylenie w oknie ~100 m (odporne na szum modelu)
        let mut j = 0;
        for i in 0..n {
            while j < n - 1 && self.d2[j] - self.d2[i] < 100.0 {
                j += 1;
            }
            let dd = (self.d2[j] - self.d2[i]) as f64;
            if dd >= 70.0 {
                s.max_grade = s.max_grade.max((self.e[j] - self.e[i]).abs() as f64 / dd);
            }
        }
        s.len2 = self.d2[n - 1] as f64;
        s.len3 = self.d3[n - 1] as f64;
        s.time = self.t[n - 1] as f64;
        s.got = (s.len2 / 1000.0).round() + (up / 100.0).round();
        s.straight = haversine(self.lon[0], self.lat[0], self.lon[n - 1], self.lat[n - 1]);
        s.avg_grade = if s.len2 > 0.0 {
            (up + down) / s.len2
        } else {
            0.0
        };
        s.max_slope = s.max_grade.atan().to_degrees();
        let score =
            s.sac_max as f64 * 1.2 + s.max_slope / 10.0 + up / 700.0 + s.len2 / 12_000.0;
        s.difficulty = if score > 9.0 {
            3
        } else if score > 6.5 {
            2
        } else if score > 4.0 {
            1
        } else {
            0
        };
        s
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TrackStats {
    pub len2: f64,
    pub len3: f64,
    pub up: f64,
    pub down: f64,
    pub time: f64,
    pub got: f64,
    pub max_ele: f64,
    pub min_ele: f64,
    pub max_ele_idx: usize,
    pub max_slope: f64,
    pub max_grade: f64,
    pub avg_grade: f64,
    pub sac_max: u8,
    pub straight: f64,
    pub difficulty: usize,
}

/// Nachylenie odcinka → klasa koloru (0..5) w profilu.
pub fn grade_class(g: f64) -> usize {
    let a = g.abs();
    if a < 0.05 {
        0
    } else if a < 0.12 {
        1
    } else if a < 0.2 {
        2
    } else if a < 0.3 {
        3
    } else if a < 0.45 {
        4
    } else {
        5
    }
}

pub const GRADE_RGB: [[u8; 3]; 6] = [
    [134, 200, 120],
    [196, 214, 98],
    [240, 200, 72],
    [244, 146, 58],
    [226, 82, 52],
    [150, 40, 70],
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pttk_time_matches_rule() {
        set_model(TimeModel::Pttk, 1.0);
        // 4 km i 600 m podejścia: 1 h + 1 h
        assert!((walk_time(4000.0, 600.0, 0.0) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn climb_hysteresis() {
        let e: Vec<f32> = (0..100).map(|i| 1000.0 + (i % 2) as f32).collect();
        let (up, down) = climb(&e);
        assert!(up < 2.0 && down < 2.0);
    }
}
