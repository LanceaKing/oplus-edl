#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use eframe::egui;
use oplus_edl_desktop::{app::DesktopApp, preferences::Preferences};

fn main() -> eframe::Result {
    let preferences = Preferences::load();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Oplus EDL")
            .with_inner_size(preferences.window_size)
            .with_min_inner_size([640.0, 480.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "oplus-edl-desktop",
        options,
        Box::new(move |context| Ok(Box::new(DesktopApp::native(&context.egui_ctx, preferences)))),
    )
}
