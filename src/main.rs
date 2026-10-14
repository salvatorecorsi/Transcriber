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
mod waveform;

fn main() -> eframe::Result {
    if app::run_command_line() {
        return Ok(());
    }
    let arguments: Vec<String> = std::env::args().collect();
    let preview_size = arguments
        .iter()
        .any(|argument| argument == "--screenshot")
        .then(|| {
            arguments
                .windows(2)
                .find(|pair| pair[0] == "--window-size")
                .and_then(|pair| pair[1].split_once('x'))
                .and_then(|(width, height)| {
                    Some([width.parse::<f32>().ok()?, height.parse::<f32>().ok()?])
                })
                .filter(|size| size[0].is_finite() && size[1].is_finite())
                .map(|size| [size[0].clamp(1000.0, 3840.0), size[1].clamp(700.0, 2160.0)])
        })
        .flatten()
        .unwrap_or([1440.0, 900.0]);
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size(preview_size)
            .with_min_inner_size([1000.0, 700.0])
            .with_title("Transcriber!"),
        ..Default::default()
    };
    eframe::run_native(
        "Transcriber!",
        options,
        Box::new(|creation| Ok(Box::new(app::TranscribeApp::new(creation)))),
    )
}
