//! Calibro — precision 3D CAD in Rust.
//!
//! Desktop: `cargo run --release -p calibro`
//! Browser: `trunk serve` inside `app/` (see docs/BEGINNER_GUIDE.md)

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod camera;
mod files;
mod panels;
mod picking;
mod renderer;
mod style;
mod viewport;

fn wgpu_options() -> egui_wgpu::WgpuConfiguration {
    egui_wgpu::WgpuConfiguration::default()
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Calibro")
            .with_inner_size([1400.0, 880.0])
            .with_min_inner_size([900.0, 600.0]),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: wgpu_options(),
        depth_buffer: 0,
        ..Default::default()
    };
    eframe::run_native(
        "Calibro",
        options,
        Box::new(|cc| Ok(Box::new(app::CalibroApp::new(cc)))),
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen::JsCast;
    eframe::WebLogger::init(log::LevelFilter::Warn).ok();
    let web_options = eframe::WebOptions {
        wgpu_options: wgpu_options(),
        ..Default::default()
    };
    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("no window")
            .document()
            .expect("no document");
        let canvas = document
            .get_element_by_id("calibro_canvas")
            .expect("missing #calibro_canvas")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("#calibro_canvas is not a canvas");
        let result = eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|cc| Ok(Box::new(app::CalibroApp::new(cc)))),
            )
            .await;
        if let Some(loading) = document.get_element_by_id("loading") {
            match result {
                Ok(()) => loading.remove(),
                Err(e) => loading.set_inner_html(&format!(
                    "<p>Calibro could not start: {e:?}</p><p>Your browser may not support WebGPU/WebGL2.</p>"
                )),
            }
        }
    });
}
