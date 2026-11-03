//! Wypiekanie kafli do postaci trzymanej na dysku – przy pobieraniu, w tle.
//!
//! Teren: wysokości jako `u16` z krokiem 1/8 m (−1000…7191 m), predykcja z sąsiada po lewej
//! (pierwsza kolumna – z wiersza wyżej) i kompresja LZ4. Wychodzi mniej niż PNG z Terrarium,
//! a odczyt to dekompresja LZ4 i suma prefiksowa – bez dekodowania PNG i bez przeliczania RGB.
//! Zdjęcia: JPEG bez zmian (już skompresowany), z jednobajtowym znacznikiem formatu.

use crate::store::Layer;

const DEM_TAG: u8 = b'D';
const JPEG_TAG: u8 = b'J';
const DEM_VERSION: u8 = 1;
const OFFSET_M: f32 = 1000.0;
const STEP: f32 = 8.0;

#[derive(Debug)]
pub struct BakeError(pub String);

impl std::fmt::Display for BakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Plik ze źródła (PNG Terrarium / JPEG) → zapis na dysk.
pub fn bake(layer: Layer, source: &[u8]) -> Result<Vec<u8>, BakeError> {
    match layer {
        Layer::Dem => {
            let img = image::load_from_memory(source)
                .map_err(|e| BakeError(format!("PNG terenu: {e}")))?
                .to_rgb8();
            let (w, h) = (img.width() as usize, img.height() as usize);
            let elev: Vec<f32> = img
                .pixels()
                .map(|p| p[0] as f32 * 256.0 + p[1] as f32 + p[2] as f32 / 256.0 - 32768.0)
                .collect();
            Ok(encode_dem(&elev, w, h))
        }
        Layer::Sat => {
            if source.len() < 4 || source[0] != 0xFF || source[1] != 0xD8 {
                return Err(BakeError("odpowiedź nie jest obrazem JPEG".into()));
            }
            let mut out = Vec::with_capacity(source.len() + 1);
            out.push(JPEG_TAG);
            out.extend_from_slice(source);
            Ok(out)
        }
    }
}

pub fn encode_dem(elev: &[f32], w: usize, h: usize) -> Vec<u8> {
    let q: Vec<u16> = elev
        .iter()
        .map(|e| ((e + OFFSET_M) * STEP).round().clamp(0.0, 65535.0) as u16)
        .collect();
    let mut delta = Vec::with_capacity(w * h * 2);
    for y in 0..h {
        for x in 0..w {
            let pred = if x > 0 {
                q[y * w + x - 1]
            } else if y > 0 {
                q[(y - 1) * w]
            } else {
                0
            };
            delta.extend_from_slice(&q[y * w + x].wrapping_sub(pred).to_le_bytes());
        }
    }
    let mut out = vec![DEM_TAG, DEM_VERSION];
    out.extend_from_slice(&(w as u16).to_le_bytes());
    out.extend_from_slice(&(h as u16).to_le_bytes());
    out.extend_from_slice(&lz4_flex::compress_prepend_size(&delta));
    out
}

/// Wysokości kafla terenu (m), wiersz po wierszu.
pub struct Dem {
    pub w: usize,
    pub h: usize,
    pub elev: Vec<f32>,
}

pub fn decode_dem(blob: &[u8]) -> Option<Dem> {
    if blob.len() < 6 || blob[0] != DEM_TAG || blob[1] != DEM_VERSION {
        return None;
    }
    let w = u16::from_le_bytes([blob[2], blob[3]]) as usize;
    let h = u16::from_le_bytes([blob[4], blob[5]]) as usize;
    let delta = lz4_flex::decompress_size_prepended(&blob[6..]).ok()?;
    if delta.len() != w * h * 2 {
        return None;
    }
    let mut elev = Vec::with_capacity(w * h);
    let mut row_start = 0u16;
    let mut prev = 0u16;
    for (i, c) in delta.chunks_exact(2).enumerate() {
        let d = u16::from_le_bytes([c[0], c[1]]);
        let v = if i % w == 0 {
            let v = row_start.wrapping_add(d);
            row_start = v;
            v
        } else {
            prev.wrapping_add(d)
        };
        prev = v;
        elev.push(v as f32 / STEP - OFFSET_M);
    }
    Some(Dem { w, h, elev })
}

/// JPEG zdjęcia z zapisu na dysku.
pub fn jpeg(blob: &[u8]) -> Option<&[u8]> {
    (blob.first() == Some(&JPEG_TAG)).then(|| &blob[1..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dem_roundtrip_precision() {
        let (w, h) = (64, 48);
        let elev: Vec<f32> = (0..w * h)
            .map(|i| {
                800.0 + (i % w) as f32 * 3.7 - (i / w) as f32 * 1.3 + ((i * 7919) % 13) as f32 * 0.2
            })
            .collect();
        let blob = encode_dem(&elev, w, h);
        let d = decode_dem(&blob).unwrap();
        assert_eq!((d.w, d.h), (w, h));
        for (a, b) in elev.iter().zip(&d.elev) {
            assert!((a - b).abs() <= 0.0625 + 1e-4, "{a} {b}");
        }
    }

    #[test]
    fn jpeg_tagging() {
        let src = [0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3];
        let b = bake(Layer::Sat, &src).unwrap();
        assert_eq!(jpeg(&b), Some(&src[..]));
        assert!(bake(Layer::Sat, b"<html>").is_err());
    }
}
