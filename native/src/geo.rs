//! Geometria: Web Mercator (siatka kafli XYZ), prostokąty geograficzne i powierzchnia.

use serde::{Deserialize, Serialize};

pub const TILE: f64 = 256.0;
const EARTH_R: f64 = 6_371_008.8;

/// Kafel XYZ (Web Mercator).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileId {
    pub z: u8,
    pub x: u32,
    pub y: u32,
}

impl TileId {
    pub const fn new(z: u8, x: u32, y: u32) -> Self {
        Self { z, x, y }
    }

    /// Rodzic o `dz` poziomów wyżej i położenie tego kafla w nim (0..1).
    pub fn ancestor(self, dz: u8) -> (TileId, [f32; 2], f32) {
        let k = 1u32 << dz;
        let p = TileId::new(self.z - dz, self.x / k, self.y / k);
        let off = [
            (self.x % k) as f32 / k as f32,
            (self.y % k) as f32 / k as f32,
        ];
        (p, off, 1.0 / k as f32)
    }

    pub fn bbox(self) -> BBox {
        let n = (1u64 << self.z) as f64;
        BBox {
            w: x_to_lon(self.x as f64 / n),
            e: x_to_lon((self.x + 1) as f64 / n),
            n: y_to_lat(self.y as f64 / n),
            s: y_to_lat((self.y + 1) as f64 / n),
        }
    }
}

/// Długość geograficzna → współrzędna świata 0..1.
pub fn lon_to_x(lon: f64) -> f64 {
    (lon + 180.0) / 360.0
}

/// Szerokość geograficzna → współrzędna świata 0..1 (północ = 0).
pub fn lat_to_y(lat: f64) -> f64 {
    let s = lat.clamp(-85.05112878, 85.05112878).to_radians().sin();
    0.5 - ((1.0 + s) / (1.0 - s)).ln() / (4.0 * std::f64::consts::PI)
}

pub fn x_to_lon(x: f64) -> f64 {
    x * 360.0 - 180.0
}

pub fn y_to_lat(y: f64) -> f64 {
    let n = std::f64::consts::PI * (1.0 - 2.0 * y);
    n.sinh().atan().to_degrees()
}

/// Rozmiar piksela kafla (m) na danej szerokości i poziomie.
pub fn meters_per_pixel(lat: f64, z: u8) -> f64 {
    2.0 * std::f64::consts::PI * EARTH_R * lat.to_radians().cos() / (TILE * (1u64 << z) as f64)
}

/// Prostokąt geograficzny w stopniach.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BBox {
    pub w: f64,
    pub s: f64,
    pub e: f64,
    pub n: f64,
}

impl BBox {
    pub const fn new(w: f64, s: f64, e: f64, n: f64) -> Self {
        Self { w, s, e, n }
    }

    pub fn from_corners(a: [f64; 2], b: [f64; 2]) -> Self {
        Self {
            w: a[0].min(b[0]),
            e: a[0].max(b[0]),
            s: a[1].min(b[1]),
            n: a[1].max(b[1]),
        }
    }

    pub fn center(&self) -> [f64; 2] {
        [(self.w + self.e) / 2.0, (self.s + self.n) / 2.0]
    }

    /// Powierzchnia na kuli (km²).
    pub fn km2(&self) -> f64 {
        let r = EARTH_R / 1000.0;
        r * r
            * (self.e - self.w).to_radians().abs()
            * (self.n.to_radians().sin() - self.s.to_radians().sin()).abs()
    }

    /// Zakres kafli poziomu `z` pokrywających prostokąt (włącznie).
    pub fn tile_range(&self, z: u8) -> (u32, u32, u32, u32) {
        let n = 1u32 << z;
        let f = |v: f64| ((v * n as f64).floor() as i64).clamp(0, n as i64 - 1) as u32;
        (
            f(lon_to_x(self.w)),
            f(lat_to_y(self.n)),
            f(lon_to_x(self.e)),
            f(lat_to_y(self.s)),
        )
    }

    pub fn tile_count(&self, z: u8) -> u64 {
        let (x0, y0, x1, y1) = self.tile_range(z);
        (x1 - x0 + 1) as u64 * (y1 - y0 + 1) as u64
    }

    pub fn tiles(&self, z: u8) -> impl Iterator<Item = TileId> {
        let (x0, y0, x1, y1) = self.tile_range(z);
        (y0..=y1).flat_map(move |y| (x0..=x1).map(move |x| TileId::new(z, x, y)))
    }

    /// Czytelny opis współrzędnych: „49,15–49,30° N · 19,75–20,25° E”.
    pub fn label(&self) -> String {
        let f = |v: f64| format!("{v:.2}").replace('.', ",");
        format!(
            "{}–{}° N · {}–{}° E",
            f(self.s),
            f(self.n),
            f(self.w),
            f(self.e)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mercator_roundtrip() {
        for lat in [-60.0, 0.0, 49.2, 70.0] {
            assert!((y_to_lat(lat_to_y(lat)) - lat).abs() < 1e-9);
        }
        assert!((x_to_lon(lon_to_x(20.0)) - 20.0).abs() < 1e-12);
    }

    #[test]
    fn tatry_area() {
        let b = BBox::new(19.7, 49.1, 20.3, 49.35);
        let km2 = b.km2();
        assert!(km2 > 1200.0 && km2 < 1400.0, "{km2}");
        assert_eq!(b.tiles(10).count() as u64, b.tile_count(10));
    }
}
