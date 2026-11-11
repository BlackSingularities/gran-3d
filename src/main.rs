//! GRAŃ — atlas szlaków górskich 3D (Windows, natywnie: Rust + wgpu + egui).
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

#[macro_use]
mod i18n;

mod analysis;
mod avalanche;
mod app;
mod bake;
mod coords;
mod download;
mod feedback;
mod fmt;
mod formats;
mod geo;
mod gpx;
mod graph;
mod fit;
mod heights;
mod hours;
mod landcover;
mod library;
mod limits;
mod metrics;
mod mvt;
mod national;
mod notices;
mod osm;
mod pack;
mod render;
mod settings;
mod store;
mod sun;
mod tz;
mod transit;
mod ui;
mod updater;
mod weather;
mod world;

/// Dziennik programu: `%LOCALAPPDATA%\GRAN\gran.log` (poprzedni przebieg w `gran.1.log`).
pub fn log_path() -> std::path::PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("GRAN")
        .join("gran.log")
}

fn init_log() {
    let mut b = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,wgpu_hal=warn,wgpu_core=warn,naga=warn"),
    );
    let path = log_path();
    let file = path.parent().and_then(|d| std::fs::create_dir_all(d).ok()).and_then(|_| {
        let _ = std::fs::rename(&path, path.with_file_name("gran.1.log"));
        std::fs::File::create(&path).ok()
    });
    if let Some(f) = file {
        b.target(env_logger::Target::Pipe(Box::new(f)));
        crate::feedback::set_log_file(path);
    }
    b.init();
    log::info!(
        "GRAŃ {} · {} {} · język {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        i18n::current()
    );
}

fn main() -> eframe::Result<()> {
    feedback::install("gran-3d", "GRAŃ", env!("CARGO_PKG_VERSION"), "GRAN");
    let s = settings::Settings::load();
    i18n::set(if s.lang.is_empty() { i18n::system() } else { &s.lang });
    init_log();
    let icon =
        eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).unwrap_or_default();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("GRAŃ")
            .with_app_id("gran")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([1024.0, 640.0])
            .with_drag_and_drop(true)
            .with_icon(icon),
        renderer: eframe::Renderer::Wgpu,
        vsync: s.vsync,
        persist_window: true,
        ..Default::default()
    };
    tz::warm_up();
    eframe::run_native(
        "GRAŃ",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
