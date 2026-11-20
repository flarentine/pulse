// GUI subsystem in every build, so no console window opens alongside Pulse.
#![cfg_attr(not(test), windows_subsystem = "windows")]

mod app;
mod fmt;
mod worker;

fn main() -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1180.0, 720.0])
        .with_min_inner_size([760.0, 420.0])
        .with_title("Pulse");
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/pulse.png"))
    {
        viewport = viewport.with_icon(icon);
    }

    let native_options = eframe::NativeOptions {
        viewport,
        // glow sits at noticeably less memory than the default wgpu backend.
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        "Pulse",
        native_options,
        Box::new(|cc| Ok(Box::new(app::PulseApp::new(cc)))),
    )
}
