//! GRAŃ — atlas szlaków górskich 3D (Rust + wgpu + egui). Cały program jest w bibliotece `gran`
//! (`lib.rs`), wspólnej z wersją na Androida; ten plik uruchamia go na Windows i na iOS.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn main() -> eframe::Result<()> {
    gran::run_desktop()
}

#[cfg(target_os = "ios")]
fn main() {
    gran::run_ios();
}

#[cfg(target_os = "android")]
fn main() {}
