//! GRAŃ — atlas szlaków górskich 3D (Rust + wgpu + egui). Jeden program dla Windows i Androida:
//! `main.rs` uruchamia go na komputerze, a pakiet `android/rust` – na telefonie i tablecie.

#[macro_use]
mod i18n;

mod account;
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
mod gps;
mod gpx;
mod graph;
mod heights;
mod hours;
mod landcover;
mod library;
mod lidar;
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
mod recording;
mod render;
mod roads;
mod settings;
mod store;
mod studio;
mod sun;
mod transit;
mod tz;
mod ui;
mod updater;
mod weather;
mod world;

#[cfg(target_os = "android")]
pub use android_activity::AndroidApp;

/// Wersja programu: Windows – z `Cargo.toml`, Android – własna numeracja z `android/VERSION`
/// (wydania `android-v<wersja>` niezależne od wydań Windows `v<wersja>`).
pub fn app_version() -> &'static str {
    if platform::IOS {
        include_str!("../ios/VERSION").trim_ascii()
    } else if platform::MOBILE {
        include_str!("../android/VERSION").trim_ascii()
    } else {
        env!("CARGO_PKG_VERSION")
    }
}

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
    #[cfg(target_os = "ios")]
    {
        let logger = b.build();
        let level = logger.filter();
        let _ = log::set_boxed_logger(Box::new(platform::TeeLogger(logger)));
        log::set_max_level(level);
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    b.init();
    log::info!(
        "GRAŃ {} · {} {} · {} · język {}",
        app_version(),
        std::env::consts::OS,
        std::env::consts::ARCH,
        platform::device(),
        i18n::current()
    );
}

/// Wspólny start: zgłoszenia awarii, ustawienia, język, dziennik i strefy czasowe.
fn prepare() -> settings::Settings {
    feedback::install("gran-3d", "GRAŃ", app_version(), "GRAN");
    let s = settings::Settings::load();
    i18n::set(if s.lang.is_empty() { i18n::system() } else { &s.lang });
    // zrzuty do dokumentacji w wybranym języku niezależnie od systemu
    if let Ok(l) = std::env::var("GRAN_LANG") {
        i18n::set(&l);
    }
    init_log();
    tz::warm_up();
    s
}

fn creator() -> eframe::AppCreator<'static> {
    Box::new(|cc| Ok(Box::new(app::App::new(cc))))
}

/// iPhone i iPad: foldery aplikacji zamiast `%APPDATA%`/`%LOCALAPPDATA%`, potem ten sam program.
#[cfg(target_os = "ios")]
pub fn run_ios() {
    platform::init();
    let s = prepare();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        vsync: s.vsync,
        persistence_path: Some(settings::config_dir().join("widok.ron")),
        ..Default::default()
    };
    if let Err(e) = eframe::run_native("GRAŃ", options, creator()) {
        log::error!("eframe: {e}");
    }
}

/// Windows (i inne systemy biurkowe).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn run_desktop() -> eframe::Result<()> {
    let s = prepare();
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).unwrap_or_default();
    // zrzuty do dokumentacji: GRAN_WINDOW=szer×wys (np. 824x1830 – układ telefonu przy GRAN_ZOOM=2)
    let window = std::env::var("GRAN_WINDOW").ok().and_then(|w| {
        let (a, b) = w.split_once(['x', '×'])?;
        Some([a.trim().parse::<f32>().ok()?, b.trim().parse::<f32>().ok()?])
    });
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("GRAŃ")
            .with_app_id("gran")
            .with_inner_size(window.unwrap_or([1440.0, 900.0]))
            .with_min_inner_size(window.map(|_| [320.0, 480.0]).unwrap_or([1024.0, 640.0]))
            .with_drag_and_drop(true)
            .with_icon(icon),
        renderer: eframe::Renderer::Wgpu,
        vsync: s.vsync,
        persist_window: window.is_none(),
        wgpu_options: desktop_wgpu(),
        ..Default::default()
    };
    eframe::run_native("GRAŃ", options, creator())
}

/// Grafika na Windows: najpierw DirectX 12 (sterowniki kart Intel bywają z Vulkanem zawodne –
/// „Parent device is lost” w trakcie pracy), Vulkan jako zapas; karta oddzielna przed zintegrowaną.
/// Zgubiona albo nieaktualna powierzchnia (zmiana monitora, uśpienie) – odtwarzana zamiast awarii.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn desktop_wgpu() -> eframe::egui_wgpu::WgpuConfiguration {
    use eframe::egui_wgpu::{self, SurfaceErrorAction, WgpuSetup, WgpuSetupCreateNew};
    use eframe::wgpu;
    let mut setup = WgpuSetupCreateNew::default();
    setup.native_adapter_selector = Some(std::sync::Arc::new(|adapters: &[wgpu::Adapter], surface: Option<&wgpu::Surface<'_>>| {
        let score = |a: &wgpu::Adapter| {
            let info = a.get_info();
            let kind = match info.device_type {
                wgpu::DeviceType::DiscreteGpu => 3,
                wgpu::DeviceType::IntegratedGpu => 2,
                wgpu::DeviceType::VirtualGpu => 1,
                _ => 0,
            };
            let backend = match info.backend {
                wgpu::Backend::Dx12 => 2,
                wgpu::Backend::Vulkan => 1,
                _ => 0,
            };
            (kind, backend)
        };
        let best = adapters
            .iter()
            .filter(|a| surface.is_none_or(|s| a.is_surface_supported(s)))
            .max_by_key(|a| score(a))
            .cloned()
            .ok_or_else(|| "brak karty graficznej zgodnej z oknem".to_string())?;
        let info = best.get_info();
        log::info!("GPU: {} ({:?}, {})", info.name, info.backend, info.driver_info);
        Ok(best)
    }));
    egui_wgpu::WgpuConfiguration {
        wgpu_setup: WgpuSetup::CreateNew(setup),
        on_surface_error: std::sync::Arc::new(|e| match e {
            wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated => SurfaceErrorAction::RecreateSurface,
            e => {
                log::warn!("powierzchnia: {e}");
                SurfaceErrorAction::SkipFrame
            }
        }),
        ..Default::default()
    }
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
    let forced = platform::forced_backend();
    let gles = match forced.as_str() {
        "vulkan" => false,
        "gl" => true,
        _ => s.gles || platform::is_emulator(),
    };
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
