//! Punkt wejścia aplikacji na Androidzie. Cały program to biblioteka `gran` – ta sama co w Windows.

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: gran::AndroidApp) {
    gran::run_android(app);
}
