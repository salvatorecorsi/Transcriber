#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod design;
mod export;
mod harmony;
mod midi;
mod runtime;
mod score;
mod tablature;
mod transcription;

fn main() -> eframe::Result {
    if app::run_command_line() {
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 940.0])
            .with_min_inner_size([1240.0, 820.0])
            .with_title("Transcriber!"),
        ..Default::default()
    };
    eframe::run_native(
        "Transcriber!",
        options,
        Box::new(|creation| Ok(Box::new(app::TranscribeApp::new(creation)))),
    )
}
