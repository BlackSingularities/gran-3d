//! Podgląd kamer internetowych w panelu punktu.

use super::*;

/// Podgląd kamery internetowej w panelu punktu.
pub enum CamState {
    Loading,
    Image(egui::TextureHandle),
    /// Adres prowadzi do strony, nie do obrazu.
    NotImage,
    Failed(String),
}

pub(super) type CamResult = Arc<std::sync::Mutex<Option<Result<Option<egui::ColorImage>, String>>>>;

pub(super) struct CamSlot {
    url: String,
    result: CamResult,
    tex: Option<egui::TextureHandle>,
    at: Instant,
}

/// Obraz z adresu kamery: `None`, gdy to nie obraz (strona z odtwarzaczem itp.).
pub(super) fn fetch_cam(url: &str) -> Result<Option<egui::ColorImage>, String> {
    let r = crate::download::agent().get(url).timeout(Duration::from_secs(20)).call().map_err(|e| e.to_string())?;
    if !r.content_type().starts_with("image/") {
        return Ok(None);
    }
    let mut buf = Vec::new();
    use std::io::Read;
    r.into_reader().take(12 << 20).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let img = image::load_from_memory(&buf).map_err(|e| e.to_string())?;
    let img = if img.width() > 1280 { img.resize(1280, 1280, image::imageops::FilterType::Triangle) } else { img };
    let rgba = img.to_rgba8();
    Ok(Some(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw())))
}

impl Atlas {
    /// Obraz kamery (pobierany w tle, odświeżany co 5 minut).
    pub fn webcam_image(&mut self, ctx: &egui::Context, url: &str) -> CamState {
        let stale = self.webcam.as_ref().is_none_or(|c| c.url != url || c.at.elapsed() > Duration::from_secs(300));
        if stale {
            let result: CamResult = Default::default();
            let (slot, u, c) = (result.clone(), url.to_string(), ctx.clone());
            thread::spawn(move || {
                let r = fetch_cam(&u);
                *slot.lock().unwrap() = Some(r);
                c.request_repaint();
            });
            let tex = self.webcam.take().filter(|c| c.url == url).and_then(|c| c.tex);
            self.webcam = Some(CamSlot { url: url.to_string(), result, tex, at: Instant::now() });
        }
        let c = self.webcam.as_mut().unwrap();
        let done = c.result.lock().unwrap().take();
        match done {
            Some(Ok(Some(img))) => c.tex = Some(ctx.load_texture("kamera", img, egui::TextureOptions::LINEAR)),
            Some(Ok(None)) => {
                c.tex = None;
                *c.result.lock().unwrap() = Some(Ok(None));
                return CamState::NotImage;
            }
            Some(Err(e)) => {
                *c.result.lock().unwrap() = Some(Err(e.clone()));
                return match &c.tex {
                    Some(t) => CamState::Image(t.clone()),
                    None => CamState::Failed(e),
                };
            }
            None => {}
        }
        match &c.tex {
            Some(t) => CamState::Image(t.clone()),
            None => CamState::Loading,
        }
    }
}
