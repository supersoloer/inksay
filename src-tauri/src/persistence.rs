use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Document<T> {
    schema_version: u32,
    data: T,
}

enum ReadError {
    Missing,
    Io(String),
    Future(u32),
    Invalid(String),
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, ReadError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| ReadError::Invalid(e.to_string()))?;
    let version = value
        .get("schemaVersion")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| ReadError::Invalid("missing schemaVersion".into()))?;
    if version > u64::from(SCHEMA_VERSION) {
        return Err(ReadError::Future(version.min(u64::from(u32::MAX)) as u32));
    }
    if version != u64::from(SCHEMA_VERSION) {
        return Err(ReadError::Invalid(format!(
            "unsupported schema version {version}"
        )));
    }
    // New application starts at v1. Future upgrades must add an explicit migration here.
    let document: Document<T> =
        serde_json::from_value(value).map_err(|e| ReadError::Invalid(e.to_string()))?;
    Ok(document.data)
}

fn load<T: DeserializeOwned>(path: &Path) -> Result<T, ReadError> {
    let bytes = fs::read(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ReadError::Missing
        } else {
            ReadError::Io(e.to_string())
        }
    })?;
    let value: serde_json::Value = decode(&bytes)?;
    if path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n == "history.json" || n == "history.json.bak")
    {
        if !value.as_array().is_some_and(|entries| {
            entries
                .iter()
                .all(|entry| entry.is_object() && entry.get("id").is_some_and(|id| id.is_string()))
        }) {
            return Err(ReadError::Invalid(
                "invalid history record structure".into(),
            ));
        }
    }
    serde_json::from_value(value).map_err(|e| ReadError::Invalid(e.to_string()))
}

fn backup(path: &Path) -> PathBuf {
    path.with_extension("json.bak")
}

pub fn read<T: DeserializeOwned + Default>(path: &Path) -> Result<T, String> {
    match load(path) {
        Ok(data) => Ok(data),
        Err(ReadError::Future(version)) => Err(format!(
            "{} 使用较新的数据格式 v{version}，请升级 InkSay",
            path.display()
        )),
        Err(ReadError::Io(error)) => Err(format!("{}: {error}", path.display())),
        Err(error) => {
            let saved = backup(path);
            if matches!(error, ReadError::Missing) && !saved.exists() {
                return Ok(T::default());
            }
            let data = load::<T>(&saved).map_err(|_| {
                let detail = match error {
                    ReadError::Invalid(ref detail) => detail.as_str(),
                    _ => "file missing",
                };
                format!(
                    "{} 无法读取，备份不可用；原文件未被覆盖。{detail}",
                    path.display()
                )
            })?;
            if path.exists() {
                let quarantine =
                    path.with_extension(format!("json.corrupt-{}", uuid::Uuid::new_v4()));
                fs::rename(path, quarantine).map_err(|e| e.to_string())?;
            }
            atomic_write(path, &fs::read(&saved).map_err(|e| e.to_string())?)?;
            // Metadata only: never copy transcription or credentials into recovery logs.
            let notice = path
                .parent()
                .ok_or("data file has no parent")?
                .join("recovery.log");
            let mut options = fs::OpenOptions::new();
            options.create(true).append(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(notice).map_err(|e| e.to_string())?;
            writeln!(
                file,
                "{} restored {} from backup",
                chrono::Utc::now().to_rfc3339(),
                path.file_name().unwrap().to_string_lossy()
            )
            .map_err(|e| e.to_string())?;
            Ok(data)
        }
    }
}

pub fn write<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(&Document {
        schema_version: SCHEMA_VERSION,
        data: value,
    })
    .map_err(|e| e.to_string())?;
    match fs::read(path) {
        Ok(previous) => {
            decode::<serde_json::Value>(&previous)
                .map_err(|_| format!("拒绝覆盖未经恢复或版本不兼容的数据：{}", path.display()))?;
            atomic_write(&backup(path), &previous)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            atomic_write(&backup(path), &bytes)?;
        }
        Err(e) => return Err(e.to_string()),
    }
    atomic_write(path, &bytes)
}

// Device mute recovery is a short-lived lock, not backed up after successful removal.
pub fn write_transient<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    atomic_write(path, &serde_json::to_vec(value).map_err(|e| e.to_string())?)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temporary = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        #[cfg(unix)]
        fs::File::open(path.parent().unwrap())?.sync_all()?;
        Ok::<(), std::io::Error>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn valid_json_with_invalid_history_shape_recovers_from_backup() {
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir(&dir).unwrap();
        let path = dir.join("history.json");
        write(&path, &vec![serde_json::json!({"id":"record"})]).unwrap();
        fs::write(&path, br#"{"schemaVersion":1,"data":[null]}"#).unwrap();
        let records: Vec<serde_json::Value> = read(&path).unwrap();
        assert_eq!(records[0]["id"], "record");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn backup_recovery_preserves_corrupt_file_and_future_version() {
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir(&dir).unwrap();
        let path = dir.join("test.json");
        assert!(read::<Vec<String>>(&path).unwrap().is_empty());
        write(&path, &vec!["first"]).unwrap();
        write(&path, &vec!["second"]).unwrap();
        fs::write(&path, b"broken").unwrap();
        assert_eq!(read::<Vec<String>>(&path).unwrap(), vec!["first"]);
        assert!(fs::read_dir(&dir).unwrap().any(|f| f
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("corrupt")));
        fs::write(&path, br#"{"schemaVersion":99,"data":[]}"#).unwrap();
        assert!(read::<Vec<String>>(&path).is_err());
        assert!(write(&path, &vec!["no"]).is_err());
        fs::remove_file(&path).unwrap();
        assert_eq!(read::<Vec<String>>(&path).unwrap(), vec!["first"]);
        fs::write(&path, b"broken").unwrap();
        fs::write(backup(&path), b"broken too").unwrap();
        assert!(read::<Vec<String>>(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"broken");
        fs::remove_dir_all(dir).unwrap();
    }
}
