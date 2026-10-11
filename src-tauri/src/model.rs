use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{Emitter, Manager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const MODEL_REVISION: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";
pub const DEFAULT_MODEL: &str = "large-v3-v20240930_626MB";
pub type PreparationLock = tokio::sync::Mutex<()>;

#[derive(Clone, Copy)]
pub struct ModelSpec {
    file_name: &'static str,
    size: u64,
    sha256: &'static str,
}

pub fn spec(id: &str) -> Result<ModelSpec, String> {
    match id {
        "tiny" => Ok(ModelSpec {
            file_name: "ggml-tiny.bin",
            size: 77_691_713,
            sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
        }),
        "base" => Ok(ModelSpec {
            file_name: "ggml-base.bin",
            size: 147_951_465,
            sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
        }),
        "small" => Ok(ModelSpec {
            file_name: "ggml-small.bin",
            size: 487_601_967,
            sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
        }),
        DEFAULT_MODEL => Ok(ModelSpec {
            file_name: "ggml-large-v3-turbo-q5_0.bin",
            size: 574_041_195,
            sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        }),
        _ => Err(format!("unknown Whisper model: {id}")),
    }
}

pub fn path(app: &tauri::AppHandle, id: &str) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("models")
        .join(spec(id)?.file_name))
}

pub fn has_cached_candidate(app: &tauri::AppHandle, id: &str) -> Result<bool, String> {
    Ok(path(app, id)?.is_file())
}

pub fn total_bytes(app: &tauri::AppHandle) -> Result<u64, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("models");
    total_bytes_in(&directory)
}

fn total_bytes_in(directory: &Path) -> Result<u64, String> {
    let mut total = 0;
    for id in ["tiny", "base", "small", DEFAULT_MODEL] {
        let model = directory.join(spec(id)?.file_name);
        for file in [model.clone(), model.with_extension("bin.part")] {
            match file.metadata() {
                Ok(metadata) => total += metadata.len(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
        }
    }
    Ok(total)
}

pub async fn prepare(app: &tauri::AppHandle, id: &str) -> Result<PathBuf, String> {
    let preparation = app.state::<PreparationLock>();
    let _guard = preparation.lock().await;
    let model = spec(id)?;
    let target = path(app, id)?;
    let existing = target.clone();
    if target.is_file()
        && tokio::task::spawn_blocking(move || verify_model(&existing, model))
            .await
            .map_err(|error| error.to_string())??
    {
        return Ok(target);
    }
    let url = format!(
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/{MODEL_REVISION}/{}",
        model.file_name
    );
    download_model(&url, &target, model, |downloaded, total| {
        let _ = app.emit("model-download-progress", (downloaded, Some(total)));
    })
    .await?;
    let _ = app.emit("model-ready", id);
    Ok(target)
}

async fn download_model(
    url: &str,
    target: &PathBuf,
    model: ModelSpec,
    progress: impl Fn(u64, u64),
) -> Result<(), String> {
    let parent = target.parent().ok_or("model path has no parent")?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|error| error.to_string())?;
    let temporary = target.with_extension("bin.part");
    let mut downloaded = match tokio::fs::metadata(&temporary).await {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error.to_string()),
    };
    if downloaded >= model.size {
        if downloaded == model.size {
            let complete = temporary.clone();
            if tokio::task::spawn_blocking(move || verify_model(&complete, model))
                .await
                .map_err(|error| error.to_string())??
            {
                install_model(&temporary, &target).await?;
                return Ok(());
            }
        }
        tokio::fs::remove_file(&temporary)
            .await
            .map_err(|error| error.to_string())?;
        downloaded = 0;
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| error.to_string())?;
    let mut request = client.get(url);
    if downloaded > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={downloaded}-"));
    }
    let mut response = request
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?;
    let resumed = downloaded > 0 && response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    if resumed {
        let range = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|value| value.to_str().ok());
        if !range.is_some_and(|value| valid_content_range(value, downloaded, model.size)) {
            tokio::fs::remove_file(&temporary)
                .await
                .map_err(|error| error.to_string())?;
            return Err("model server returned an invalid byte range".into());
        }
    } else {
        downloaded = 0;
    }
    let mut hash = Sha256::new();
    if resumed {
        let mut previous = tokio::fs::File::open(&temporary)
            .await
            .map_err(|error| error.to_string())?;
        let mut buffer = vec![0_u8; 1024 * 1024];
        loop {
            let length = previous
                .read(&mut buffer)
                .await
                .map_err(|error| error.to_string())?;
            if length == 0 {
                break;
            }
            hash.update(&buffer[..length]);
        }
    }
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(resumed)
        .truncate(!resumed)
        .open(&temporary)
        .await
        .map_err(|error| error.to_string())?;
    while let Some(chunk) = tokio::time::timeout(Duration::from_secs(60), response.chunk())
        .await
        .map_err(|_| "model download stalled")?
        .map_err(|error| error.to_string())?
    {
        file.write_all(&chunk)
            .await
            .map_err(|error| error.to_string())?;
        downloaded += chunk.len() as u64;
        hash.update(&chunk);
        progress(downloaded, model.size);
    }
    file.flush().await.map_err(|error| error.to_string())?;
    drop(file);
    if downloaded != model.size || format!("{:x}", hash.finalize()) != model.sha256 {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err("model download is incomplete".into());
    }
    install_model(&temporary, &target).await?;
    Ok(())
}

async fn install_model(temporary: &PathBuf, target: &PathBuf) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    if target.is_file() {
        tokio::fs::remove_file(target)
            .await
            .map_err(|error| error.to_string())?;
    }
    tokio::fs::rename(temporary, target)
        .await
        .map_err(|error| error.to_string())
}

fn valid_content_range(value: &str, start: u64, total: u64) -> bool {
    let Some((bounds, size)) = value
        .strip_prefix("bytes ")
        .and_then(|value| value.split_once('/'))
    else {
        return false;
    };
    let Some((first, last)) = bounds.split_once('-') else {
        return false;
    };
    matches!((first.parse::<u64>(), last.parse::<u64>(), size.parse::<u64>()),
        (Ok(first), Ok(last), Ok(size)) if first == start && last >= first && last < total && size == total)
}

fn verify_model(path: &PathBuf, model: ModelSpec) -> Result<bool, String> {
    use std::io::Read;
    if path.metadata().map_err(|error| error.to_string())?.len() != model.size {
        return Ok(false);
    }
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let length = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if length == 0 {
            break;
        }
        hash.update(&buffer[..length]);
    }
    Ok(format!("{:x}", hash.finalize()) == model.sha256)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};

    #[test]
    fn model_ids_map_to_distinct_files() {
        let names: Vec<_> = ["tiny", "base", "small", DEFAULT_MODEL]
            .into_iter()
            .map(|id| spec(id).unwrap().file_name)
            .collect();
        assert_eq!(names.len(), 4);
        assert!(names.iter().all(|name| name.starts_with("ggml-")));
        for id in ["tiny", "base", "small", DEFAULT_MODEL] {
            let digest = spec(id).unwrap().sha256;
            assert_eq!(digest.len(), 64, "invalid SHA-256 length for {id}");
            assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
        }
        assert!(spec("unknown").is_err());
    }

    #[test]
    fn model_usage_includes_incomplete_downloads() {
        let directory =
            std::env::temp_dir().join(format!("inksay-model-size-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("ggml-tiny.bin"), [1, 2, 3]).unwrap();
        std::fs::write(directory.join("ggml-base.bin.part"), [4, 5, 6, 7, 8]).unwrap();
        assert_eq!(total_bytes_in(&directory).unwrap(), 8);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn resumed_response_must_match_requested_model_range() {
        assert!(valid_content_range("bytes 1024-2047/2048", 1024, 2048));
        assert!(!valid_content_range("bytes 0-2047/2048", 1024, 2048));
        assert!(!valid_content_range("bytes 1024-2047/4096", 1024, 2048));
        assert!(!valid_content_range("bytes 1024-3000/2048", 1024, 2048));
    }

    #[test]
    fn partial_download_resumes_and_installs_verified_model() {
        let directory =
            std::env::temp_dir().join(format!("inksay-resume-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let target = directory.join("toy.bin");
        std::fs::write(target.with_extension("bin.part"), b"hello ").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            while !request.ends_with(b"\r\n\r\n") {
                let length = stream.read(&mut buffer).unwrap();
                assert!(length > 0 && request.len() < 8192);
                request.extend_from_slice(&buffer[..length]);
            }
            let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
            assert!(request.contains("range: bytes=6-"));
            stream.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 6-10/11\r\nContent-Length: 5\r\nConnection: close\r\n\r\nworld").unwrap();
        });
        let spec = ModelSpec {
            file_name: "toy.bin",
            size: 11,
            sha256: "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9",
        };
        let progress = Arc::new(Mutex::new(Vec::new()));
        let observed = progress.clone();
        tauri::async_runtime::block_on(download_model(
            &format!("http://{address}/toy.bin"),
            &target,
            spec,
            move |done, total| observed.lock().unwrap().push((done, total)),
        ))
        .unwrap();
        server.join().unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"hello world");
        assert_eq!(progress.lock().unwrap().last(), Some(&(11, 11)));
        assert!(!target.with_extension("bin.part").exists());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn interrupted_download_keeps_partial_bytes_for_next_attempt() {
        let directory =
            std::env::temp_dir().join(format!("inksay-interrupted-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let target = directory.join("toy.bin");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (partial_written, may_disconnect) = mpsc::channel();
        let server = std::thread::spawn(move || {
            for attempt in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                while !request.ends_with(b"\r\n\r\n") {
                    let length = stream.read(&mut buffer).unwrap();
                    assert!(length > 0 && request.len() < 8192);
                    request.extend_from_slice(&buffer[..length]);
                }
                let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
                if attempt == 0 {
                    assert!(!request.contains("range:"));
                    stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\nhello ").unwrap();
                    may_disconnect.recv_timeout(Duration::from_secs(5)).unwrap();
                } else {
                    assert!(request.contains("range: bytes=6-"));
                    stream.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 6-10/11\r\nContent-Length: 5\r\nConnection: close\r\n\r\nworld").unwrap();
                }
            }
        });
        let spec = ModelSpec {
            file_name: "toy.bin",
            size: 11,
            sha256: "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9",
        };
        let url = format!("http://{address}/toy.bin");
        assert!(
            tauri::async_runtime::block_on(download_model(&url, &target, spec, |done, _| {
                if done == 6 {
                    let _ = partial_written.send(());
                }
            }))
            .is_err()
        );
        assert_eq!(
            std::fs::read(target.with_extension("bin.part")).unwrap(),
            b"hello "
        );
        tauri::async_runtime::block_on(download_model(&url, &target, spec, |_, _| {})).unwrap();
        server.join().unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"hello world");
        std::fs::remove_dir_all(directory).unwrap();
    }
}
