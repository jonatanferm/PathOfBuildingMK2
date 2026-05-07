use pob_ui::PobApp;

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 600.0])
            .with_title("Path of Building MK2"),
        ..Default::default()
    };

    eframe::run_native(
        "Path of Building MK2",
        native_options,
        Box::new(|cc| Ok(Box::new(PobApp::new(cc)))),
    )
}
