use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Note {
    pub onset: f64,
    pub offset: f64,
    pub pitch: u8,
    pub program: u8,
    pub is_drum: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transcription {
    pub source: String,
    pub duration: f64,
    pub notes: Vec<Note>,
    pub engine: String,
}

pub enum TranscriptionEvent {
    Progress(f32, String),
    Finished(Transcription),
    Failed(String),
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum EngineEvent {
    Progress { progress: f32, message: String },
    Finished { engine: String, notes: Vec<Note> },
    Error { message: String },
}

fn engine_paths() -> Vec<PathBuf> {
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let cpu_selected = std::env::var("TRANSCRIBER_TRANSCRIPTION_BACKEND")
        .or_else(|_| std::env::var("CORSIVO_TRANSCRIPTION_BACKEND"))
        .is_ok_and(|backend| backend.eq_ignore_ascii_case("cpu"));
    let mut executables = Vec::new();
    let directories = if !cpu_selected && crate::runtime::vulkan_available() {
        vec!["engine/bin-vulkan", "engine/bin"]
    } else {
        vec!["engine/bin"]
    };
    for directory in directories {
        if let Some(executable) = ["transcriber-ai", "corsivo-transcription"]
            .into_iter()
            .find_map(|name| crate::runtime::resource_path(&format!("{directory}/{name}{suffix}")))
        {
            executables.push(executable);
        }
    }
    executables
}

pub fn engine_ready() -> bool {
    !engine_paths().is_empty()
        && crate::runtime::resource_path("models/muscriptor-small-f16.gguf").is_some()
}

fn storage_directory() -> PathBuf {
    crate::runtime::data_directory().join("transcriptions")
}

pub fn cache_path(path: &Path) -> PathBuf {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let source = source_name(path);
    let fingerprint = source.bytes().fold(0xcbf29ce484222325u64, |digest, byte| {
        (digest ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    let directory = storage_directory();
    let unique = directory.join(format!("{stem}-{fingerprint:016x}.json"));
    if unique.exists() {
        return unique;
    }
    let preferred = directory.join(format!("{stem}.json"));
    if let Ok(file) = File::open(&preferred) {
        #[derive(Deserialize)]
        struct StoredSource {
            source: String,
        }
        if !serde_json::from_reader::<_, StoredSource>(file)
            .is_ok_and(|cached| cached.source == source)
        {
            return unique;
        }
    }
    preferred
}

fn source_name(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

pub fn load_cached(path: &Path) -> Option<Transcription> {
    let cached_path = cache_path(path);
    let transcription: Transcription =
        serde_json::from_reader(File::open(&cached_path).ok()?).ok()?;
    if transcription.source != source_name(path)
        || !transcription.duration.is_finite()
        || transcription.duration <= 0.0
        || transcription.notes.iter().any(|note| {
            !note.onset.is_finite()
                || !note.offset.is_finite()
                || note.onset < 0.0
                || note.offset <= note.onset
                || note.pitch > 127
                || note.program > 128
        })
    {
        return None;
    }
    let source_changed = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .zip(
            fs::metadata(cached_path)
                .and_then(|metadata| metadata.modified())
                .ok(),
        )
        .is_some_and(|(source_modified, cache_modified)| source_modified > cache_modified);
    (!source_changed).then_some(transcription)
}

fn write_wave(path: &Path, samples: &[f32]) -> Result<(), String> {
    let sample_bytes = samples
        .len()
        .checked_mul(4)
        .and_then(|length| u32::try_from(length).ok())
        .filter(|length| *length <= u32::MAX - 36)
        .ok_or_else(|| "Il file audio supera la dimensione supportata".to_owned())?;
    let mut file = std::io::BufWriter::new(File::create(path).map_err(|error| error.to_string())?);
    file.write_all(b"RIFF").map_err(|error| error.to_string())?;
    file.write_all(&(sample_bytes + 36).to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(b"WAVEfmt ")
        .map_err(|error| error.to_string())?;
    file.write_all(&16u32.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(&3u16.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(&1u16.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(&16000u32.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(&64000u32.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(&4u16.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(&32u16.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(b"data").map_err(|error| error.to_string())?;
    file.write_all(&sample_bytes.to_le_bytes())
        .map_err(|error| error.to_string())?;
    for &sample in samples {
        let sample = if sample.is_finite() { sample } else { 0.0 };
        file.write_all(&sample.to_le_bytes())
            .map_err(|error| error.to_string())?;
    }
    file.flush().map_err(|error| error.to_string())
}

fn save_cache(path: &Path, transcription: &Transcription) -> Result<(), String> {
    let destination = cache_path(path);
    let partial = destination.with_extension("json.part");
    let mut file = File::create(&partial).map_err(|error| error.to_string())?;
    serde_json::to_writer(&mut file, transcription).map_err(|error| error.to_string())?;
    file.flush().map_err(|error| error.to_string())?;
    drop(file);
    if destination.exists() {
        fs::remove_file(&destination).map_err(|error| error.to_string())?;
    }
    fs::rename(partial, destination).map_err(|error| error.to_string())
}

fn run_engine(
    audio_path: &Path,
    duration: f64,
    instruments: &[String],
    sender: &mpsc::Sender<TranscriptionEvent>,
    cancelled: &AtomicBool,
    scratch_path: &Path,
    engine: (&Path, &Path),
) -> Result<Transcription, String> {
    let (executable, model) = engine;
    let mut command = Command::new(executable);
    command
        .arg(model)
        .arg(scratch_path)
        .args(instruments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("Avvio del motore fallito: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Il motore non ha aperto l'output".to_owned())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Il motore non ha aperto il log".to_owned())?;
    let (line_sender, line_receiver) = mpsc::channel();
    let output_reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if line_sender.send(line).is_err() {
                break;
            }
        }
    });
    let error_reader = thread::spawn(move || {
        let mut diagnostics = String::new();
        BufReader::new(stderr)
            .take(65536)
            .read_to_string(&mut diagnostics)
            .ok();
        diagnostics
    });
    let mut result = None;
    let mut engine_error = None;
    loop {
        if cancelled.load(Ordering::Relaxed) {
            child.kill().ok();
            child.wait().ok();
            output_reader.join().ok();
            error_reader.join().ok();
            return Err("Trascrizione annullata".to_owned());
        }
        match line_receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(line)) => match serde_json::from_str::<EngineEvent>(&line) {
                Ok(EngineEvent::Progress { progress, message }) => {
                    sender
                        .send(TranscriptionEvent::Progress(
                            progress.clamp(0.0, 1.0),
                            message,
                        ))
                        .ok();
                }
                Ok(EngineEvent::Finished { engine, notes }) => {
                    result = Some(Transcription {
                        source: source_name(audio_path),
                        duration,
                        notes,
                        engine,
                    });
                }
                Ok(EngineEvent::Error { message }) => engine_error = Some(message),
                Err(error) => {
                    engine_error = Some(format!("Risposta del motore non valida: {error}"))
                }
            },
            Ok(Err(error)) => engine_error = Some(error.to_string()),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            while let Ok(line) = line_receiver.try_recv() {
                if let Ok(line) = line {
                    match serde_json::from_str::<EngineEvent>(&line) {
                        Ok(EngineEvent::Finished { engine, notes }) => {
                            result = Some(Transcription {
                                source: source_name(audio_path),
                                duration,
                                notes,
                                engine,
                            });
                        }
                        Ok(EngineEvent::Error { message }) => engine_error = Some(message),
                        _ => {}
                    }
                }
            }
            if result.is_some() || engine_error.is_some() {
                break;
            }
        }
    }
    let status = child.wait().map_err(|error| error.to_string())?;
    output_reader.join().ok();
    let diagnostics = error_reader.join().unwrap_or_default();
    if !status.success() {
        return Err(engine_error
            .unwrap_or_else(|| format!("Il motore si è fermato: {}", diagnostics.trim())));
    }
    let mut transcription = result.ok_or_else(|| {
        engine_error.unwrap_or_else(|| "Il motore non ha restituito una trascrizione".to_owned())
    })?;
    for note in &mut transcription.notes {
        note.offset = note.offset.min(duration);
    }
    transcription.notes.retain(|note| {
        note.onset.is_finite()
            && note.offset.is_finite()
            && note.onset >= 0.0
            && note.onset < note.offset
            && note.pitch <= 127
            && note.program <= 128
    });
    Ok(transcription)
}

fn transcribe(
    audio_path: &Path,
    samples: &[f32],
    duration: f64,
    instruments: &[String],
    sender: &mpsc::Sender<TranscriptionEvent>,
    cancelled: &AtomicBool,
    scratch_path: &Path,
) -> Result<Transcription, String> {
    if cancelled.load(Ordering::Relaxed) {
        return Err("Trascrizione annullata".to_owned());
    }
    if samples.is_empty() || !duration.is_finite() || duration <= 0.0 {
        return Err("Il file audio non contiene campioni validi".to_owned());
    }
    let executables = engine_paths();
    if executables.is_empty() {
        return Err("Motore assente: premi Installa AI nell'app".to_owned());
    }
    let model = crate::runtime::resource_path("models/muscriptor-small-f16.gguf")
        .ok_or_else(|| "Modello assente: premi Installa AI nell'app".to_owned())?;
    sender
        .send(TranscriptionEvent::Progress(
            0.0,
            "Preparazione audio".to_owned(),
        ))
        .ok();
    write_wave(scratch_path, samples)?;
    let mut failure = String::new();
    for (index, executable) in executables.iter().enumerate() {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Trascrizione annullata".to_owned());
        }
        if index > 0 {
            sender
                .send(TranscriptionEvent::Progress(
                    0.0,
                    "GPU non disponibile, proseguo con la CPU".to_owned(),
                ))
                .ok();
        }
        match run_engine(
            audio_path,
            duration,
            instruments,
            sender,
            cancelled,
            scratch_path,
            (executable, &model),
        ) {
            Ok(transcription) => {
                save_cache(audio_path, &transcription)?;
                return Ok(transcription);
            }
            Err(error) => failure = error,
        }
    }
    Err(failure)
}

pub fn spawn_transcription(
    audio_path: PathBuf,
    samples: Vec<f32>,
    duration: f64,
    instruments: Vec<String>,
    sender: mpsc::Sender<TranscriptionEvent>,
    cancelled: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let scratch_directory = storage_directory().join(".scratch");
        let scratch_path =
            scratch_directory.join(format!("{}-{timestamp}.wav", std::process::id()));
        let outcome = fs::create_dir_all(&scratch_directory)
            .map_err(|error| error.to_string())
            .and_then(|_| {
                transcribe(
                    &audio_path,
                    &samples,
                    duration,
                    &instruments,
                    &sender,
                    &cancelled,
                    &scratch_path,
                )
            });
        fs::remove_file(&scratch_path).ok();
        let event = match outcome {
            Ok(transcription) => TranscriptionEvent::Finished(transcription),
            Err(error) => TranscriptionEvent::Failed(error),
        };
        sender.send(event).ok();
    });
}
