use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

pub const RELEASE_URL: &str = "https://github.com/salvatorecorsi/Transcriber/releases/tag/v0.4.0";
const RELEASE_DOWNLOAD: &str =
    "https://github.com/salvatorecorsi/Transcriber/releases/download/v0.2.0";
const MODEL_URL: &str = "https://huggingface.co/DamRsn/muscriptor-gguf/resolve/d7045f94e8b19427f4ff9542975035e66596e51c/v1/muscriptor-small-f16.gguf";
const MODEL_HASH: &str = "925f55af65a20ebc4f8b45ceaf095a12b72493d436cb112623cd0041a1af23d4";
const CPU_HASH: &str = "e6adf6f7a62013b0fc7a499a5b10a57a0d76040767be4dd3d76094937f411d6c";
const VULKAN_HASH: &str = "51c607747cc551a1eae46b17a1c0fd878df21f97ddf0b33d67d3261c5801c6dc";

pub enum InstallEvent {
    Progress(f32, String),
    Finished,
    FinishedWithWarning(String),
    Failed(String),
}

pub fn application_root() -> PathBuf {
    let executable = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("Transcriber.exe"));
    let directory = executable.parent().unwrap_or(Path::new("."));
    directory
        .ancestors()
        .take(4)
        .find(|root| root.join("Cargo.toml").is_file() && root.join("src/main.rs").is_file())
        .unwrap_or(directory)
        .to_path_buf()
}

pub fn data_directory() -> PathBuf {
    if let Some(directory) = std::env::var_os("TRANSCRIBER_DATA_DIR") {
        return PathBuf::from(directory);
    }
    let root = application_root();
    if root.join("Cargo.toml").is_file() || root.join("portable.flag").is_file() {
        return root;
    }
    std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("APPDATA"))
        .map(|directory| PathBuf::from(directory).join("Transcriber"))
        .unwrap_or_else(|| root.join("data"))
}

pub fn resource_path(relative: &str) -> Option<PathBuf> {
    [application_root(), data_directory()]
        .into_iter()
        .map(|root| root.join(relative))
        .find(|path| path.is_file())
}

pub fn library_directory() -> PathBuf {
    application_root().join("audio")
}

pub fn vulkan_available() -> bool {
    cfg!(windows)
        && std::env::var_os("SystemRoot")
            .is_some_and(|root| PathBuf::from(root).join("System32/vulkan-1.dll").is_file())
}

fn verified_file(path: &Path, expected: &str) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let Ok(count) = file.read(&mut buffer) else {
            return false;
        };
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    format!("{:x}", digest.finalize()) == expected
}

fn download(
    url: &str,
    destination: &Path,
    expected_hash: &str,
    title: &str,
    segment: (f32, f32),
    sender: &mpsc::Sender<InstallEvent>,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        return Err("Installazione annullata".to_owned());
    }
    if verified_file(destination, expected_hash) {
        let _ = sender.send(InstallEvent::Progress(
            segment.0 + segment.1,
            format!("{title} già pronto"),
        ));
        return Ok(());
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let partial = destination.with_extension("download");
    let result = (|| {
        let configuration = ureq::Agent::config_builder()
            .timeout_connect(Some(std::time::Duration::from_secs(15)))
            .timeout_recv_response(Some(std::time::Duration::from_secs(30)))
            .build();
        let agent: ureq::Agent = configuration.into();
        let response = agent
            .get(url)
            .header(
                "User-Agent",
                concat!("Transcriber/", env!("CARGO_PKG_VERSION")),
            )
            .call()
            .map_err(|error| format!("Download {title} fallito: {error}"))?;
        let length = response
            .headers()
            .get("content-length")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        let mut reader = response.into_body().into_reader();
        let mut file = File::create(&partial).map_err(|error| error.to_string())?;
        let mut digest = Sha256::new();
        let mut count = 0u64;
        let mut last_report = std::time::Instant::now() - std::time::Duration::from_secs(1);
        let mut buffer = [0u8; 65536];
        loop {
            if cancelled.load(Ordering::Relaxed) {
                return Err("Installazione annullata".to_owned());
            }
            let received = reader
                .read(&mut buffer)
                .map_err(|error| error.to_string())?;
            if received == 0 {
                break;
            }
            file.write_all(&buffer[..received])
                .map_err(|error| error.to_string())?;
            digest.update(&buffer[..received]);
            count += received as u64;
            if last_report.elapsed().as_millis() >= 150 {
                let fraction =
                    length.map_or(0.0, |length| (count as f32 / length.max(1) as f32).min(1.0));
                let _ = sender.send(InstallEvent::Progress(
                    segment.0 + segment.1 * fraction,
                    format!("{title} · {} MB", count / 1_048_576),
                ));
                last_report = std::time::Instant::now();
            }
        }
        file.flush().map_err(|error| error.to_string())?;
        drop(file);
        if format!("{:x}", digest.finalize()) != expected_hash {
            return Err(format!(
                "Download {title} incompleto: verifica SHA-256 fallita"
            ));
        }
        if destination.exists() {
            fs::remove_file(destination).map_err(|error| error.to_string())?;
        }
        fs::rename(&partial, destination).map_err(|error| error.to_string())?;
        let _ = sender.send(InstallEvent::Progress(
            segment.0 + segment.1,
            format!("{title} pronto"),
        ));
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}

pub fn spawn_installation(sender: mpsc::Sender<InstallEvent>, cancelled: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let root = data_directory();
        let result = (|| {
            download(
                &format!("{RELEASE_DOWNLOAD}/transcriber-ai-cpu.exe"),
                &root.join("engine/bin/transcriber-ai.exe"),
                CPU_HASH,
                "Motore AI",
                (0.0, 0.05),
                &sender,
                &cancelled,
            )?;
            download(
                MODEL_URL,
                &root.join("models/muscriptor-small-f16.gguf"),
                MODEL_HASH,
                "Modello MuScriptor",
                (0.05, 0.80),
                &sender,
                &cancelled,
            )?;
            if vulkan_available()
                && let Err(error) = download(
                    &format!("{RELEASE_DOWNLOAD}/transcriber-ai-vulkan.exe"),
                    &root.join("engine/bin-vulkan/transcriber-ai.exe"),
                    VULKAN_HASH,
                    "Accelerazione GPU",
                    (0.85, 0.15),
                    &sender,
                    &cancelled,
                )
            {
                if cancelled.load(Ordering::Relaxed) {
                    return Err(error);
                }
                return Ok(Some(format!(
                    "AI pronta sulla CPU. Accelerazione GPU non installata: {error}"
                )));
            }
            Ok::<Option<String>, String>(None)
        })();
        let _ = sender.send(match result {
            Ok(None) => InstallEvent::Finished,
            Ok(Some(warning)) => InstallEvent::FinishedWithWarning(warning),
            Err(error) => InstallEvent::Failed(error),
        });
    });
}

#[cfg(test)]
mod tests {
    use super::verified_file;
    use sha2::{Digest, Sha256};

    #[test]
    fn installer_rejects_corrupted_or_missing_assets() {
        let path =
            std::env::temp_dir().join(format!("transcriber-integrity-{}", std::process::id()));
        std::fs::write(&path, b"verified runtime").unwrap();
        let expected = format!("{:x}", Sha256::digest(b"verified runtime"));
        assert!(verified_file(&path, &expected));
        std::fs::write(&path, b"corrupted runtime").unwrap();
        assert!(!verified_file(&path, &expected));
        std::fs::remove_file(&path).unwrap();
        assert!(!verified_file(&path, &expected));
    }
}
