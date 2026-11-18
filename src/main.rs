//! GRAŃ — atlas szlaków górskich 3D (Windows, natywnie: Rust + wgpu + egui). Cały program jest w
//! bibliotece `gran` (`lib.rs`), wspólnej z wersją na Androida.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

#[cfg(not(target_os = "android"))]
fn main() -> eframe::Result<()> {
    gran::run_desktop()
}

#[cfg(target_os = "android")]
fn main() {}
