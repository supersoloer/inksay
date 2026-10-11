use crate::storage::write_json;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
#[cfg(test)]
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub values: Map<String, Value>,
    #[serde(rename = "refineAPIKeySet")]
    pub refine_api_key_set: bool,
    #[serde(rename = "cloudASRAPIKeySet")]
    pub cloud_asr_api_key_set: bool,
    pub platform: &'static str,
}

pub struct Settings {
    path: PathBuf,
    values: Map<String, Value>,
    refine_api_key: String,
    secret_store: Box<dyn SecretStore>,
    cloud_api_key: String,
    cloud_key_endpoint: String,
    cloud_secret_store: Box<dyn SecretStore>,
}

#[derive(Default, Serialize, Deserialize)]
struct CloudSecret {
    key: String,
    endpoint: String,
}

trait SecretStore: Send {
    fn load(&self) -> Result<Option<String>, String>;
    fn save(&self, value: &str) -> Result<(), String>;
    fn clear(&self) -> Result<(), String>;
}

struct SystemSecret(keyring::Entry);

impl SecretStore for SystemSecret {
    fn load(&self) -> Result<Option<String>, String> {
        match self.0.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    fn save(&self, value: &str) -> Result<(), String> {
        self.0
            .set_password(value)
            .map_err(|error| error.to_string())
    }

    fn clear(&self) -> Result<(), String> {
        match self.0.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

impl Settings {
    pub fn open(directory: PathBuf) -> Result<Self, String> {
        let identifier = directory
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("settings directory has no application identifier")?;
        let entry = keyring::Entry::new(&format!("{identifier}.refinement"), "refineAPIKey")
            .map_err(|error| error.to_string())?;
        let cloud = keyring::Entry::new(&format!("{identifier}.transcription"), "cloudASRAPIKey")
            .map_err(|error| error.to_string())?;
        Self::open_with_store(
            directory,
            Box::new(SystemSecret(entry)),
            Box::new(SystemSecret(cloud)),
        )
    }

    fn open_with_store(
        directory: PathBuf,
        secret_store: Box<dyn SecretStore>,
        cloud_secret_store: Box<dyn SecretStore>,
    ) -> Result<Self, String> {
        let path = directory.join("config.json");
        let mut values: Map<String, Value> = crate::persistence::read(&path)?;
        let mut defaults_changed = false;
        let defaults = [
            ("transcriptionBackend", Value::from("cloudASR")),
            ("cloudASRProvider", Value::from("openai")),
            ("cloudASRProtocol", Value::from("transcription")),
            ("cloudASRFullURL", Value::from(false)),
            ("cloudASRBaseURL", Value::from("https://api.openai.com/v1")),
            ("cloudASRModel", Value::from("whisper-1")),
            (
                "localWhisperModel",
                Value::from(crate::model::DEFAULT_MODEL),
            ),
            ("refineStyle", Value::from("clean")),
            ("chineseEnglishSpacing", Value::from("preserve")),
            ("chineseNumberSpacing", Value::from("preserve")),
            ("chinesePunctuation", Value::from("preserve")),
            ("refineBaseURL", Value::from("https://api.openai.com/v1")),
            ("refineModel", Value::from("gpt-4o-mini")),
            ("customRefineInstruction", Value::from("")),
            ("translationTargetLanguage", Value::from("en-US")),
            (
                "hotkeyTrigger",
                Value::from(if cfg!(target_os = "windows") {
                    "rightControl"
                } else {
                    "rightOption"
                }),
            ),
            ("hotkeyMode", Value::from("hold")),
            ("muteSystemOutput", Value::from(true)),
            ("interactionSounds", Value::from(true)),
            ("preferredMicrophoneUID", Value::from("")),
            ("recordingRetention", Value::from("day30")),
            ("language", Value::from("zh-CN")),
            ("autoDetectLanguage", Value::from(false)),
            ("includeContext", Value::from(true)),
            ("silenceAutoStopSeconds", Value::from(0.0)),
            ("injectionStrategy", Value::from("automatic")),
            ("appearance", Value::from("system")),
            ("launchAtLogin", Value::from(false)),
            ("showInDock", Value::from(true)),
            ("hasShownInitialSetup", Value::from(false)),
        ];
        for (key, value) in defaults {
            if !values.contains_key(key) {
                values.insert(key.to_owned(), value);
                defaults_changed = true;
            }
        }
        #[cfg(target_os = "windows")]
        if !matches!(
            values.get("hotkeyTrigger").and_then(Value::as_str),
            Some("rightControl" | "leftControl")
        ) {
            values.insert("hotkeyTrigger".into(), Value::from("rightControl"));
            defaults_changed = true;
        }
        if values.contains_key("refineAPIKey") || values.contains_key("cloudASRAPIKey") {
            return Err("API keys must be stored in the system credential store".into());
        }
        let refine_api_key = secret_store.load()?.unwrap_or_default();
        let cloud_secret = match cloud_secret_store.load()? {
            Some(raw) => serde_json::from_str::<CloudSecret>(&raw)
                .map_err(|_| "invalid cloud credential record")?,
            None => CloudSecret::default(),
        };
        if values.remove("cloudASRAPIKey").is_some() {
            return Err("Cloud API keys must be stored in the system credential store".into());
        }
        if defaults_changed {
            write_json(&path, &values)?;
        }
        Ok(Self {
            path,
            values,
            refine_api_key,
            secret_store,
            cloud_api_key: cloud_secret.key,
            cloud_key_endpoint: cloud_secret.endpoint,
            cloud_secret_store,
        })
    }

    fn cloud_key_available(&self) -> bool {
        if self.cloud_api_key.is_empty() {
            return false;
        }
        let Ok(endpoint) = crate::cloud_asr::endpoint(&self.values) else {
            return false;
        };
        if endpoint == self.cloud_key_endpoint {
            return true;
        }
        if self.values.get("cloudASRProvider").and_then(Value::as_str) != Some("bailian") {
            return false;
        }
        let (Ok(current), Ok(bound)) = (
            reqwest::Url::parse(&endpoint),
            reqwest::Url::parse(&self.cloud_key_endpoint),
        ) else {
            return false;
        };
        // A Bailian key can serve the same API on its public endpoint or official MaaS gateway.
        // Arbitrary compatible hosts and different API paths still require an explicit key save.
        let official = |url: &reqwest::Url| {
            url.scheme() == "https"
                && url.port().is_none()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && url.host_str().is_some_and(|host| {
                    host == "dashscope.aliyuncs.com" || host.ends_with(".maas.aliyuncs.com")
                })
        };
        official(&current) && official(&bound) && current.path() == bound.path()
    }

    pub fn snapshot(&self) -> SettingsSnapshot {
        let mut values = self.values.clone();
        values.remove("refineAPIKey");
        values.remove("cloudASRAPIKey");
        SettingsSnapshot {
            values,
            refine_api_key_set: !self.refine_api_key.is_empty(),
            cloud_asr_api_key_set: self.cloud_key_available(),
            platform: std::env::consts::OS,
        }
    }

    pub fn backend_values(&self) -> Map<String, Value> {
        let mut values = self.values.clone();
        values.insert(
            "refineAPIKey".into(),
            Value::from(self.refine_api_key.clone()),
        );
        values.insert(
            "cloudASRAPIKey".into(),
            Value::from(if self.cloud_key_available() {
                self.cloud_api_key.clone()
            } else {
                String::new()
            }),
        );
        values
    }

    pub fn set(&mut self, key: &str, value: Value) -> Result<SettingsSnapshot, String> {
        let valid = match key {
            "transcriptionBackend" => matches!(value.as_str(), Some("localWhisper" | "cloudASR")),
            "cloudASRProvider" => {
                matches!(value.as_str(), Some("openai" | "compatible" | "bailian"))
            }
            "cloudASRProtocol" => matches!(
                value.as_str(),
                Some("transcription" | "audioMessages" | "dashscope")
            ),
            "cloudASRFullURL" => value.is_boolean(),
            "cloudASRBaseURL" | "cloudASRModel" | "cloudASRAPIKey" => value.is_string(),
            "chineseEnglishSpacing" | "chineseNumberSpacing" => {
                matches!(value.as_str(), Some("preserve" | "space"))
            }
            "chinesePunctuation" => matches!(value.as_str(), Some("preserve" | "standard")),
            "refineStyle" => matches!(
                value.as_str(),
                Some("off" | "clean" | "polish" | "formal" | "concise" | "custom")
            ),
            "injectionStrategy" => {
                matches!(value.as_str(), Some("automatic" | "pasteboardOnly"))
            }
            "recordingRetention" => {
                matches!(value.as_str(), Some("off" | "day7" | "day30" | "forever"))
            }
            "localWhisperModel" => matches!(
                value.as_str(),
                Some("tiny" | "base" | "small" | "large-v3-v20240930_626MB")
            ),
            "refineBaseURL"
            | "refineModel"
            | "refineAPIKey"
            | "customRefineInstruction"
            | "translationTargetLanguage"
            | "preferredMicrophoneUID"
            | "language" => value.is_string(),
            "appearance" => matches!(value.as_str(), Some("system" | "light" | "dark")),
            "hotkeyTrigger" => {
                #[cfg(target_os = "windows")]
                {
                    matches!(value.as_str(), Some("rightControl" | "leftControl"))
                }
                #[cfg(not(target_os = "windows"))]
                {
                    matches!(
                        value.as_str(),
                        Some("rightOption" | "leftOption" | "function")
                    )
                }
            }
            "hotkeyMode" => matches!(value.as_str(), Some("hold" | "toggle")),
            "muteSystemOutput"
            | "interactionSounds"
            | "autoDetectLanguage"
            | "includeContext"
            | "launchAtLogin"
            | "showInDock"
            | "hasShownInitialSetup" => value.is_boolean(),
            "silenceAutoStopSeconds" => value.as_f64().is_some_and(|number| number >= 0.0),
            _ => false,
        };
        if !valid {
            return Err(format!("invalid setting: {key}"));
        }
        if key == "cloudASRBaseURL" {
            let mut candidate = self.values.clone();
            candidate.insert("cloudASRProvider".into(), Value::from("compatible"));
            candidate.insert(key.to_owned(), value.clone());
            crate::cloud_asr::endpoint(&candidate)?;
        }
        if key == "cloudASRModel"
            && value
                .as_str()
                .is_some_and(|model| model.trim().is_empty() || model.chars().any(char::is_control))
        {
            return Err("请填写有效的识别模型".into());
        }
        if key == "cloudASRAPIKey" {
            let password = value.as_str().ok_or("API key must be text")?.trim();
            if password.is_empty() {
                self.cloud_secret_store.clear()?;
                self.cloud_api_key.clear();
                self.cloud_key_endpoint.clear();
                return Ok(self.snapshot());
            }
            let endpoint = crate::cloud_asr::endpoint(&self.values)?;
            self.cloud_secret_store.save(
                &serde_json::to_string(&CloudSecret {
                    key: password.to_owned(),
                    endpoint: endpoint.clone(),
                })
                .map_err(|error| error.to_string())?,
            )?;
            self.cloud_api_key = password.to_owned();
            self.cloud_key_endpoint = endpoint;
            return Ok(self.snapshot());
        }
        if key == "refineAPIKey" {
            let password = value.as_str().ok_or("API key must be text")?;
            if password.is_empty() {
                self.secret_store.clear()?;
            } else {
                self.secret_store.save(password)?;
            }
            self.refine_api_key = password.to_owned();
            return Ok(self.snapshot());
        }
        let value = if key == "cloudASRModel" {
            Value::from(value.as_str().unwrap().trim())
        } else {
            value
        };
        let mut next = self.values.clone();
        if key == "cloudASRProvider" {
            let preset = match value.as_str() {
                Some("openai") => Some(("https://api.openai.com/v1", "transcription", "whisper-1")),
                Some("bailian") => Some((
                    "https://dashscope.aliyuncs.com/api/v1",
                    "dashscope",
                    "qwen-audio-3.1-asr-flash",
                )),
                _ => None,
            };
            if let Some((base, protocol, model)) = preset {
                next.insert("cloudASRBaseURL".into(), Value::from(base));
                next.insert("cloudASRProtocol".into(), Value::from(protocol));
                next.insert("cloudASRModel".into(), Value::from(model));
                next.insert("cloudASRFullURL".into(), Value::from(false));
            }
        }
        if key == "cloudASRProtocol" && next["cloudASRProvider"] == "bailian" {
            let audio_messages = match value.as_str() {
                Some("audioMessages") => true,
                Some("dashscope") => false,
                _ => return Err("百炼不支持此接入方式".into()),
            };
            let mut url = reqwest::Url::parse(next["cloudASRBaseURL"].as_str().unwrap())
                .map_err(|_| "识别服务地址无效")?;
            let full = next["cloudASRFullURL"].as_bool().unwrap_or(false);
            url.set_path(match (audio_messages, full) {
                (true, false) => "/compatible-mode/v1",
                (true, true) => "/compatible-mode/v1/chat/completions",
                (false, false) => "/api/v1",
                (false, true) => "/api/v1/services/aigc/multimodal-generation/generation",
            });
            next.insert("cloudASRBaseURL".into(), Value::from(url.to_string()));
            if matches!(
                next["cloudASRModel"].as_str(),
                Some("qwen3-asr-flash" | "qwen-audio-3.1-asr-flash")
            ) {
                next.insert(
                    "cloudASRModel".into(),
                    Value::from(if audio_messages {
                        "qwen3-asr-flash"
                    } else {
                        "qwen-audio-3.1-asr-flash"
                    }),
                );
            }
        }
        next.insert(key.to_owned(), value);
        write_json(&self.path, &next)?;
        self.values = next;
        Ok(self.snapshot())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct MemorySecret {
        value: Arc<Mutex<Option<String>>>,
        fail_save: bool,
    }

    impl SecretStore for MemorySecret {
        fn load(&self) -> Result<Option<String>, String> {
            Ok(self.value.lock().unwrap().clone())
        }

        fn save(&self, value: &str) -> Result<(), String> {
            if self.fail_save {
                return Err("credential store unavailable".into());
            }
            *self.value.lock().unwrap() = Some(value.to_owned());
            Ok(())
        }

        fn clear(&self) -> Result<(), String> {
            *self.value.lock().unwrap() = None;
            Ok(())
        }
    }

    fn open_test(directory: PathBuf) -> (Settings, MemorySecret) {
        let secret = MemorySecret::default();
        let settings = Settings::open_with_store(
            directory,
            Box::new(secret.clone()),
            Box::new(MemorySecret::default()),
        )
        .unwrap();
        (settings, secret)
    }

    #[test]
    fn bailian_modes_preserve_host_and_switch_paths_models_and_credentials() {
        let directory =
            std::env::temp_dir().join(format!("inksay-bailian-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let (mut settings, _) = open_test(directory.clone());
        settings
            .set("cloudASRProvider", Value::from("bailian"))
            .unwrap();
        settings
            .set(
                "cloudASRBaseURL",
                Value::from("https://workspace.example/api/v1"),
            )
            .unwrap();
        settings
            .set("cloudASRAPIKey", Value::from("test-secret"))
            .unwrap();
        settings
            .set("cloudASRProtocol", Value::from("audioMessages"))
            .unwrap();
        assert_eq!(
            settings.snapshot().values["cloudASRBaseURL"],
            "https://workspace.example/compatible-mode/v1"
        );
        assert_eq!(
            settings.snapshot().values["cloudASRModel"],
            "qwen3-asr-flash"
        );
        assert!(!settings.snapshot().cloud_asr_api_key_set);
        assert_eq!(
            crate::cloud_asr::endpoint(&settings.values).unwrap(),
            "https://workspace.example/compatible-mode/v1/chat/completions"
        );
        settings.set("cloudASRFullURL", Value::from(true)).unwrap();
        settings
            .set("cloudASRProtocol", Value::from("dashscope"))
            .unwrap();
        assert_eq!(
            crate::cloud_asr::endpoint(&settings.values).unwrap(),
            "https://workspace.example/api/v1/services/aigc/multimodal-generation/generation"
        );
        assert_eq!(
            settings.snapshot().values["cloudASRModel"],
            "qwen-audio-3.1-asr-flash"
        );
        assert!(settings
            .set("cloudASRProtocol", Value::from("transcription"))
            .is_err());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn fresh_install_requires_cloud_configuration_and_preserves_local_choice() {
        let directory =
            std::env::temp_dir().join(format!("inksay-default-cloud-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let (mut settings, _) = open_test(directory.clone());
        assert_eq!(
            settings.backend_values()["transcriptionBackend"],
            "cloudASR"
        );
        assert!(crate::cloud_asr::CloudConfig::from_values(&settings.backend_values()).is_err());
        settings
            .set("transcriptionBackend", Value::from("localWhisper"))
            .unwrap();
        drop(settings);
        let (settings, _) = open_test(directory.clone());
        assert_eq!(
            settings.backend_values()["transcriptionBackend"],
            "localWhisper"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn bailian_key_remains_available_on_official_gateway_only() {
        let directory =
            std::env::temp_dir().join(format!("inksay-bailian-key-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let (mut settings, _) = open_test(directory.clone());
        settings
            .set("cloudASRProvider", Value::from("bailian"))
            .unwrap();
        settings
            .set("cloudASRAPIKey", Value::from("test-key"))
            .unwrap();
        settings
            .set(
                "cloudASRBaseURL",
                Value::from("https://llm-example.cn-beijing.maas.aliyuncs.com/api/v1"),
            )
            .unwrap();
        assert!(settings.snapshot().cloud_asr_api_key_set);
        assert_eq!(settings.backend_values()["cloudASRAPIKey"], "test-key");
        for url in [
            "https://other.example/api/v1",
            "https://x.maas.aliyuncs.com.evil.example/api/v1",
            "https://llm-example.cn-beijing.maas.aliyuncs.com/other",
        ] {
            settings.set("cloudASRBaseURL", Value::from(url)).unwrap();
            assert!(!settings.snapshot().cloud_asr_api_key_set);
            assert_eq!(settings.backend_values()["cloudASRAPIKey"], "");
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn cloud_presets_preserve_custom_protocol_and_isolate_credentials() {
        let directory =
            std::env::temp_dir().join(format!("inksay-presets-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let (mut settings, _) = open_test(directory.clone());
        assert_eq!(
            settings.snapshot().values["cloudASRProtocol"],
            "transcription"
        );
        assert_eq!(settings.snapshot().values["cloudASRFullURL"], false);
        settings
            .set("cloudASRAPIKey", Value::from("old-key"))
            .unwrap();
        settings
            .set("cloudASRProvider", Value::from("bailian"))
            .unwrap();
        assert_eq!(settings.snapshot().values["cloudASRProtocol"], "dashscope");
        assert!(!settings.snapshot().cloud_asr_api_key_set);
        settings
            .set("cloudASRAPIKey", Value::from("new-key"))
            .unwrap();
        settings
            .set("cloudASRProvider", Value::from("compatible"))
            .unwrap();
        assert!(settings.snapshot().cloud_asr_api_key_set);
        assert_eq!(settings.snapshot().values["cloudASRProtocol"], "dashscope");
        settings.set("cloudASRFullURL", Value::from(true)).unwrap();
        assert!(!settings.snapshot().cloud_asr_api_key_set);
        settings.set("cloudASRFullURL", Value::from(false)).unwrap();
        assert!(settings.snapshot().cloud_asr_api_key_set);
        settings
            .set("cloudASRProtocol", Value::from("audioMessages"))
            .unwrap();
        assert!(!settings.snapshot().cloud_asr_api_key_set);
        assert!(settings
            .set("cloudASRProtocol", Value::from("unknown"))
            .is_err());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn cloud_key_is_independent_bound_to_endpoint_and_survives_reopening() {
        let directory =
            std::env::temp_dir().join(format!("inksay-cloud-settings-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let refine = MemorySecret::default();
        let cloud = MemorySecret::default();
        let mut settings = Settings::open_with_store(
            directory.clone(),
            Box::new(refine.clone()),
            Box::new(cloud.clone()),
        )
        .unwrap();
        settings
            .set("transcriptionBackend", Value::from("cloudASR"))
            .unwrap();
        assert_eq!(
            settings.snapshot().values["transcriptionBackend"],
            "cloudASR"
        );
        assert!(crate::cloud_asr::CloudConfig::from_values(&settings.backend_values()).is_err());
        settings
            .set("refineAPIKey", Value::from("refine-secret"))
            .unwrap();
        settings
            .set("cloudASRAPIKey", Value::from("cloud-secret"))
            .unwrap();
        settings
            .set("transcriptionBackend", Value::from("cloudASR"))
            .unwrap();
        let reopened =
            Settings::open_with_store(directory.clone(), Box::new(refine), Box::new(cloud))
                .unwrap();
        assert_eq!(
            reopened.snapshot().values["transcriptionBackend"],
            "cloudASR"
        );
        assert_eq!(reopened.backend_values()["refineAPIKey"], "refine-secret");
        assert_eq!(reopened.backend_values()["cloudASRAPIKey"], "cloud-secret");
        assert!(!serde_json::to_string(&reopened.snapshot())
            .unwrap()
            .contains("secret"));
        assert!(!fs::read_to_string(directory.join("config.json"))
            .unwrap()
            .contains("secret"));
        settings
            .set("cloudASRProvider", Value::from("compatible"))
            .unwrap();
        settings
            .set("cloudASRBaseURL", Value::from("https://other.example/v1"))
            .unwrap();
        assert!(!settings.snapshot().cloud_asr_api_key_set);
        assert_eq!(settings.backend_values()["cloudASRAPIKey"], "");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn snapshot_reports_key_presence_without_exposing_key() {
        let settings = Settings {
            path: PathBuf::new(),
            values: Map::new(),
            refine_api_key: "secret".into(),
            secret_store: Box::new(MemorySecret::default()),
            cloud_api_key: String::new(),
            cloud_key_endpoint: String::new(),
            cloud_secret_store: Box::new(MemorySecret::default()),
        };
        let serialized = serde_json::to_value(settings.snapshot()).unwrap();
        assert_eq!(serialized["refineAPIKeySet"], true);
        assert!(serialized["values"].get("refineAPIKey").is_none());
        assert!(!serialized.to_string().contains("secret"));
        assert_eq!(settings.backend_values()["refineAPIKey"], "secret");
    }

    #[test]
    fn updates_preserve_extra_fields_without_exposing_key() {
        let directory =
            std::env::temp_dir().join(format!("inksay-settings-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        write_json(
            &directory.join("config.json"),
            &serde_json::json!({"language":"zh-CN", "extraField":7}),
        )
        .unwrap();
        let (mut settings, secret) = open_test(directory.clone());
        settings.set("refineAPIKey", Value::from("secret")).unwrap();
        let snapshot = settings.set("language", Value::from("en-US")).unwrap();
        assert!(snapshot.refine_api_key_set);
        assert!(!snapshot.values.contains_key("refineAPIKey"));
        assert_eq!(snapshot.values["extraField"], 7);
        assert_eq!(snapshot.values["language"], "en-US");
        assert!(settings.set("extraField", Value::from(8)).is_err());
        let persisted: Value = crate::persistence::read(&directory.join("config.json")).unwrap();
        assert!(persisted.get("refineAPIKey").is_none());
        assert_eq!(secret.load().unwrap().as_deref(), Some("secret"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn missing_fields_receive_current_defaults() {
        let directory =
            std::env::temp_dir().join(format!("inksay-defaults-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        write_json(&directory.join("config.json"), &serde_json::json!({})).unwrap();
        let (settings, _) = open_test(directory.clone());
        assert_eq!(
            settings.snapshot().values["refineBaseURL"],
            "https://api.openai.com/v1"
        );
        assert_eq!(settings.snapshot().values["refineModel"], "gpt-4o-mini");
        assert_eq!(settings.snapshot().values["includeContext"], true);
        let saved: Value = crate::persistence::read(&directory.join("config.json")).unwrap();
        assert_eq!(saved["refineModel"], "gpt-4o-mini");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn pasteboard_injection_choice_survives_reopening() {
        let directory =
            std::env::temp_dir().join(format!("inksay-paste-setting-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        write_json(
            &directory.join("config.json"),
            &serde_json::json!({"injectionStrategy":"pasteboardOnly"}),
        )
        .unwrap();
        let (settings, _) = open_test(directory.clone());
        assert_eq!(
            settings.snapshot().values["injectionStrategy"],
            "pasteboardOnly"
        );
        let saved: Value = crate::persistence::read(&directory.join("config.json")).unwrap();
        assert_eq!(saved["injectionStrategy"], "pasteboardOnly");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn changed_api_key_only_updates_credential_store() {
        let directory =
            std::env::temp_dir().join(format!("inksay-key-change-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let (mut settings, secret) = open_test(directory.clone());
        settings
            .set("refineAPIKey", Value::from("new-secret"))
            .unwrap();
        assert_eq!(secret.load().unwrap().as_deref(), Some("new-secret"));
        assert!(settings.snapshot().refine_api_key_set);
        assert!(!fs::read_to_string(directory.join("config.json"))
            .unwrap()
            .contains("new-secret"));
        settings.set("refineAPIKey", Value::from("")).unwrap();
        assert_eq!(secret.load().unwrap(), None);
        assert!(!settings.snapshot().refine_api_key_set);
        fs::remove_dir_all(directory).unwrap();
    }
}
