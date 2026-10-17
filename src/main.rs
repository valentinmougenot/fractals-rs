#![recursion_limit = "256"]

mod app;
mod fractal;
mod gpu;

use eframe::{egui, egui_wgpu};

fn main() -> eframe::Result {
    let mut wgpu_options = egui_wgpu::WgpuConfiguration::default();
    if let egui_wgpu::WgpuSetup::CreateNew(setup) = &mut wgpu_options.wgpu_setup {
        // On hybrid laptops, prefer the discrete GPU.
        setup.power_preference = egui_wgpu::wgpu::PowerPreference::HighPerformance;
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Fractals")
            .with_app_id("fractals")
            .with_inner_size([1360.0, 840.0])
            .with_min_inner_size([640.0, 420.0]),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options,
        ..Default::default()
    };

    eframe::run_native("Fractals", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}
