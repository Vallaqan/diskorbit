#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod scanner;

fn main() -> eframe::Result<()> {
    // Embed the icon bytes at compile time — no external file needed at runtime
    let icon_bytes = include_bytes!("../icon.ico");
    let icon = load_icon(icon_bytes);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("DiskOrbit")
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([600.0, 400.0])
            .with_icon(icon),
        ..Default::default()
    };

    eframe::run_native(
        "DiskOrbit",
        options,
        Box::new(|cc| Box::new(app::DiskOrbitApp::new(cc))),
    )
}

fn load_icon(bytes: &[u8]) -> egui::viewport::IconData {
    let image = image::load_from_memory(bytes)
        .expect("Failed to load icon")
        .into_rgba8();
    let (w, h) = image.dimensions();
    egui::viewport::IconData {
        rgba: image.into_raw(),
        width: w,
        height: h,
    }
}
