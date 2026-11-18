//! GRAŃ — atlas szlaków górskich 3D (Rust + wgpu + egui). Jeden program dla Windows i Androida:
//! `main.rs` uruchamia go na komputerze, a pakiet `android/rust` – na telefonie i tablecie.

#[macro_use]
mod i18n;

mod analysis;
mod avalanche;
mod app;
mod bake;
mod coords;
mod download;
mod feedback;
mod fit;
mod fmt;
mod formats;
mod geo;
mod gpx;
mod graph;
mod heights;
mod hours;
mod landcover;
mod library;
mod limits;
mod metrics;
mod mvt;
mod national;
mod notices;
mod offline;
mod osm;
mod pack;
mod platform;
mod points;
mod render;
mod roads;
mod settings;
mod store;
mod sun;
mod transit;
mod tz;
mod ui;
mod updater;
mod weather;
mod world;

#[cfg(target_os = "android")]
pub use android_activity::AndroidApp;

/// Dziennik programu: `%LOCALAPPDATA%\GRAN\gran.log` (poprzedni przebieg w `gran.1.log`).
/// Na Androidzie `LOCALAPPDATA` wskazuje folder aplikacji (ustawiany w [`run_android`]).
pub fn log_path() -> std::path::PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("GRAN")
        .join("gran.log")
}

fn init_log() {
    let filter = "info,wgpu_hal=warn,wgpu_core=warn,naga=warn";
    let mut b = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(filter));
    let path = log_path();
    let file = path.parent().and_then(|d| std::fs::create_dir_all(d).ok()).and_then(|_| {
        let _ = std::fs::rename(&path, path.with_file_name("gran.1.log"));
        std::fs::File::create(&path).ok()
    });
    if let Some(f) = file {
        b.target(env_logger::Target::Pipe(Box::new(f)));
        crate::feedback::set_log_file(path);
    }
    #[cfg(target_os = "android")]
    {
        // dziennik idzie do pliku (zgłoszenia błędów) i do logcat
        let logger = b.build();
        let level = logger.filter();
        let cat = android_logger::AndroidLogger::new(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("GRAN"));
        let _ = log::set_boxed_logger(Box::new(platform::TeeLogger(logger, cat)));
        log::set_max_level(level);
    }
    #[cfg(not(target_os = "android"))]
    b.init();
    log::info!(
        "GRAŃ {} · {} {} · {} · język {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        platform::device(),
        i18n::current()
    );
}

/// Wspólny start: zgłoszenia awarii, ustawienia, język, dziennik i strefy czasowe.
fn prepare() -> settings::Settings {
    feedback::install("gran-3d", "GRAŃ", env!("CARGO_PKG_VERSION"), "GRAN");
    let s = settings::Settings::load();
    i18n::set(if s.lang.is_empty() { i18n::system() } else { &s.lang });
    init_log();
    tz::warm_up();
    s
}

fn creator() -> eframe::AppCreator<'static> {
    Box::new(|cc| Ok(Box::new(app::App::new(cc))))
}

/// Windows (i inne systemy biurkowe).
#[cfg(not(target_os = "android"))]
pub fn run_desktop() -> eframe::Result<()> {
    let s = prepare();
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).unwrap_or_default();
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
    eframe::run_native("GRAŃ", options, creator())
}

/// Android: foldery aplikacji zamiast `%APPDATA%`/`%LOCALAPPDATA%`, potem ten sam program co w Windows.
#[cfg(target_os = "android")]
pub fn run_android(app: AndroidApp) {
    use eframe::egui_wgpu::{self, SurfaceErrorAction, WgpuSetup, WgpuSetupCreateNew};
    use eframe::wgpu;
    platform::init(&app);
    let mut s = prepare();
    // Vulkan, a w trybie zgodności OpenGL ES. Tryb zgodności włącza się sam w emulatorze i po awarii
    // grafiki w poprzednim uruchomieniu (sterowniki Vulkana bywają zawodne).
    let gpu_crash = feedback::pending_crash().is_some_and(|c| c.stack.contains("wgpu"));
    if !s.gles && gpu_crash {
        log::warn!("poprzednio awaria grafiki – włączam tryb zgodności (OpenGL ES)");
        s.gles = true;
        let _ = s.save();
    }
    let gles = s.gles || platform::is_emulator();
    log::info!("grafika: {}", if gles { "OpenGL ES" } else { "Vulkan" });
    let mut setup = WgpuSetupCreateNew::default();
    setup.instance_descriptor.backends = if gles { wgpu::Backends::GL } else { wgpu::Backends::VULKAN | wgpu::Backends::GL };
    // limity takie, jakie GPU naprawdę ma (domyślne dla OpenGL ES to limity WebGL2 – bez obliczeń
    // i buforów storage, z których korzysta odczyt punktu pod palcem)
    setup.device_descriptor = std::sync::Arc::new(|adapter: &wgpu::Adapter| {
        let info = adapter.get_info();
        log::info!("GPU: {} ({:?}, {})", info.name, info.backend, info.driver_info);
        wgpu::DeviceDescriptor { label: Some("GRAŃ"), required_limits: adapter.limits(), ..Default::default() }
    });
    let wgpu_options = egui_wgpu::WgpuConfiguration {
        wgpu_setup: WgpuSetup::CreateNew(setup),
        on_surface_error: std::sync::Arc::new(|e| match e {
            wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated => SurfaceErrorAction::RecreateSurface,
            e => {
                log::warn!("powierzchnia: {e}");
                SurfaceErrorAction::SkipFrame
            }
        }),
        ..Default::default()
    };
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        vsync: s.vsync,
        android_app: Some(app),
        persistence_path: Some(settings::config_dir().join("widok.ron")),
        wgpu_options,
        ..Default::default()
    };
    if let Err(e) = eframe::run_native("GRAŃ", options, creator()) {
        log::error!("eframe: {e}");
    }
}
