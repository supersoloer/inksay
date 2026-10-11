pub(crate) use crate::persistence::{read as read_json, write as write_json};
use crate::statistics::{StatisticsSnapshot, UsageRecord};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::Manager;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DictionaryEntry {
    pub id: String,
    pub term: String,
    pub source: String,
    pub created_at: Value,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub history: Vec<Value>,
    pub dictionary: Vec<DictionaryEntry>,
    pub replacement_rules: Vec<crate::lexicon::ReplacementRule>,
}

pub struct Store {
    directory: PathBuf,
    history: Vec<Value>,
    dictionary: Vec<DictionaryEntry>,
    term_memory: TermMemory,
    rules: crate::lexicon::RuleBook,
    usage: BTreeMap<String, UsageRecord>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct TermMemory {
    counts: BTreeMap<String, u32>,
    ignored: BTreeSet<String>,
}

impl Store {
    pub fn open(app: &tauri::AppHandle) -> Result<Self, String> {
        let directory = app
            .path()
            .app_data_dir()
            .map_err(|error| error.to_string())?;
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;

        let history: Vec<Value> = read_json(&directory.join("history.json"))?;
        let dictionary: Vec<DictionaryEntry> = read_json(&directory.join("dictionary.json"))?;
        let rules = read_json(&directory.join("replacement-rules.json"))?;
        let term_memory: TermMemory = read_json(&directory.join("term-memory.json"))?;
        let mut usage: BTreeMap<String, UsageRecord> =
            read_json(&directory.join("statistics.json"))?;
        for entry in &history {
            if let Some(record) = UsageRecord::from_history(entry) {
                usage.insert(record.id.clone(), record);
            }
        }
        write_json(&directory.join("statistics.json"), &usage)?;
        Ok(Self {
            usage,
            history,
            dictionary,
            term_memory,
            rules,
            directory,
        })
    }

    pub fn statistics(&self) -> StatisticsSnapshot {
        StatisticsSnapshot {
            records: self.usage.values().cloned().collect(),
            dictionary_terms: self
                .dictionary
                .iter()
                .filter(|entry| entry.source != "suggested")
                .count(),
            learned_terms: self
                .dictionary
                .iter()
                .filter(|entry| entry.source == "learned")
                .count(),
        }
    }

    fn record_usage(&mut self, entry: &Value) -> Result<(), String> {
        if let Some(record) = UsageRecord::from_history(entry) {
            self.usage.insert(record.id.clone(), record);
            write_json(&self.directory.join("statistics.json"), &self.usage)?;
        }
        Ok(())
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            history: self
                .history
                .iter()
                .map(|entry| {
                    let mut entry = entry.clone();
                    entry["hasAudio"] = Value::Bool(self.audio_path(&entry).is_some());
                    entry
                })
                .collect(),
            dictionary: self.dictionary.clone(),
            replacement_rules: self.rules.rules.clone(),
        }
    }

    pub fn audio_bytes(&self, id: &str) -> Result<(Vec<u8>, &'static str), String> {
        let entry = self
            .history
            .iter()
            .find(|entry| entry.get("id").and_then(Value::as_str) == Some(id))
            .ok_or("history entry was not found")?;
        let path = self
            .audio_path(entry)
            .ok_or("recording file was not found")?;
        let mime_type = match path.extension().and_then(|value| value.to_str()) {
            Some("wav") => "audio/wav",
            Some("m4a") => "audio/mp4",
            _ => return Err("unsupported recording format".into()),
        };
        Ok((
            fs::read(path).map_err(|error| error.to_string())?,
            mime_type,
        ))
    }

    pub fn recording_bytes(&self) -> Result<u64, String> {
        let directory = self.directory.join("recordings");
        if !directory.exists() {
            return Ok(0);
        }
        let mut total = 0;
        for item in fs::read_dir(directory).map_err(|error| error.to_string())? {
            total += item
                .map_err(|error| error.to_string())?
                .metadata()
                .map_err(|error| error.to_string())?
                .len();
        }
        Ok(total)
    }

    pub fn prune_recordings(&mut self, retention: &str) -> Result<(), String> {
        let days = match retention {
            "off" => Some(0),
            "day7" => Some(7),
            "day30" => Some(30),
            "forever" => None,
            _ => return Err("invalid recording retention".into()),
        };
        let directory = self.directory.join("recordings");
        if !directory.exists() {
            return Ok(());
        }
        let now = chrono::Utc::now();
        let mut entries = self.history.clone();
        let mut changed = false;
        for entry in &mut entries {
            let Some(name) = entry.get("audioFileName").and_then(Value::as_str) else {
                continue;
            };
            if Path::new(name).file_name().and_then(|value| value.to_str()) != Some(name) {
                continue;
            }
            let expired = match days {
                Some(0) => true,
                Some(count) => entry
                    .get("createdAt")
                    .and_then(Value::as_str)
                    .and_then(|date| chrono::DateTime::parse_from_rfc3339(date).ok())
                    .is_some_and(|created| now.signed_duration_since(created).num_days() >= count),
                None => false,
            };
            if expired {
                let path = directory.join(name);
                if let Err(error) = fs::remove_file(path) {
                    if error.kind() != std::io::ErrorKind::NotFound {
                        return Err(error.to_string());
                    }
                }
                entry["audioFileName"] = Value::Null;
                changed = true;
            }
        }
        if changed {
            write_json(&self.directory.join("history.json"), &entries)?;
            self.history = entries;
        }
        for item in fs::read_dir(directory).map_err(|error| error.to_string())? {
            let item = item.map_err(|error| error.to_string())?;
            let name = item.file_name().to_string_lossy().into_owned();
            if self.history.iter().any(|entry| {
                entry.get("audioFileName").and_then(Value::as_str) == Some(name.as_str())
            }) {
                continue;
            }
            let modified = item
                .metadata()
                .map_err(|error| error.to_string())?
                .modified()
                .map_err(|error| error.to_string())?;
            if modified.elapsed().is_ok_and(|age| age.as_secs() >= 600) {
                fs::remove_file(item.path()).map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }

    fn audio_path(&self, entry: &Value) -> Option<PathBuf> {
        let name = entry.get("audioFileName")?.as_str()?;
        if Path::new(name).file_name()?.to_str()? != name {
            return None;
        }
        let path = self.directory.join("recordings").join(name);
        path.is_file().then_some(path)
    }

    pub fn apply_rules(&self, text: &str) -> (String, Vec<crate::lexicon::AppliedRule>) {
        self.rules.apply(text)
    }

    pub fn save_rule(
        &mut self,
        id: Option<&str>,
        from: &str,
        to: &str,
        enabled: bool,
    ) -> Result<(), String> {
        let mut rules = self.rules.clone();
        rules.save(id, from, to, enabled)?;
        write_json(&self.directory.join("replacement-rules.json"), &rules)?;
        self.rules = rules;
        Ok(())
    }

    pub fn delete_rule(&mut self, id: &str) -> Result<(), String> {
        let mut rules = self.rules.clone();
        rules.delete(id)?;
        write_json(&self.directory.join("replacement-rules.json"), &rules)?;
        self.rules = rules;
        Ok(())
    }

    pub fn suggest_correction(&mut self, original: &str, edited: &str) -> Result<bool, String> {
        let Some((from, to)) = crate::lexicon::correction_pair(original, edited) else {
            return Ok(false);
        };
        let mut rules = self.rules.clone();
        if !rules.suggest(from, to) {
            return Ok(false);
        }
        write_json(&self.directory.join("replacement-rules.json"), &rules)?;
        self.rules = rules;
        Ok(true)
    }

    pub fn correct_history(&mut self, id: &str, text: &str) -> Result<(), String> {
        let text = text.trim();
        if text.is_empty() || text.chars().count() > 20000 {
            return Err("纠正内容无效".into());
        }
        let index = self
            .history
            .iter()
            .position(|e| e["id"].as_str() == Some(id))
            .ok_or("记录不存在")?;
        let entry = &self.history[index];
        let original = entry["insertedText"]
            .as_str()
            .or(entry["formattedText"].as_str())
            .or(entry["refinedText"].as_str())
            .unwrap_or("")
            .to_owned();
        let dictate = entry["mode"].as_str().unwrap_or("dictate") == "dictate";
        let mut history = self.history.clone();
        history[index]["userCorrectedText"] = Value::String(text.into());
        history[index]["correctedAt"] = Value::String(chrono::Utc::now().to_rfc3339());
        write_json(&self.directory.join("history.json"), &history)?;
        self.history = history;
        if dictate {
            self.suggest_correction(&original, text)?;
        }
        Ok(())
    }

    pub fn dictionary_terms(&self) -> Vec<String> {
        self.dictionary
            .iter()
            .filter(|entry| entry.source != "suggested")
            .map(|entry| entry.term.clone())
            .collect()
    }

    pub fn learn_from_text(&mut self, text: &str) -> Result<bool, String> {
        let candidates = crate::learning::candidates(text);
        if candidates.is_empty() {
            return Ok(false);
        }
        let mut memory = self.term_memory.clone();
        let mut dictionary = self.dictionary.clone();
        let mut promoted = false;
        for term in candidates {
            let key = term.to_lowercase();
            if memory.ignored.contains(&key)
                || dictionary
                    .iter()
                    .any(|entry| entry.term.eq_ignore_ascii_case(&term))
            {
                continue;
            }
            if !memory.counts.contains_key(&key)
                && memory.counts.len() >= crate::learning::MAX_TRACKED_TERMS
            {
                let victim = memory
                    .counts
                    .iter()
                    .filter(|(_, count)| **count < crate::learning::PROMOTION_THRESHOLD)
                    .min_by_key(|(_, count)| *count)
                    .map(|(key, _)| key.clone());
                let Some(victim) = victim else { continue };
                memory.counts.remove(&victim);
            }
            let count = memory.counts.entry(key).or_default();
            *count = count.saturating_add(1);
            if *count < crate::learning::PROMOTION_THRESHOLD
                || dictionary
                    .iter()
                    .filter(|entry| entry.source == "suggested")
                    .count()
                    >= crate::learning::MAX_LEARNED_TERMS
            {
                continue;
            }
            dictionary.insert(
                0,
                DictionaryEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    term,
                    source: "suggested".to_owned(),
                    created_at: Value::String(
                        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    ),
                },
            );
            promoted = true;
        }
        write_json(&self.directory.join("term-memory.json"), &memory)?;
        self.term_memory = memory;
        if promoted {
            write_json(&self.directory.join("dictionary.json"), &dictionary)?;
            self.dictionary = dictionary;
        }
        Ok(promoted)
    }

    pub fn add_history(&mut self, entry: Value) -> Result<(), String> {
        let mut entries = self.history.clone();
        entries.insert(0, entry.clone());
        entries.truncate(500);
        write_json(&self.directory.join("history.json"), &entries)?;
        self.history = entries;
        self.record_usage(&entry)?;
        Ok(())
    }

    pub fn update_insertion(
        &mut self,
        history_id: &str,
        inserted_text: Option<&str>,
        error: Option<&str>,
        insertion_duration: Option<f64>,
    ) -> Result<(), String> {
        let mut entries = self.history.clone();
        let entry = entries
            .iter_mut()
            .find(|entry| entry["id"] == history_id)
            .ok_or("history entry was not found")?;
        entry["insertedText"] = inserted_text.map_or(Value::Null, Value::from);
        if let Some(error) = error {
            entry["error"] = Value::from(error);
        }
        if let Some(duration) = insertion_duration {
            entry["insertionDuration"] = Value::from(duration);
            if let Some(total) = entry["processingDuration"].as_f64() {
                entry["processingDuration"] = Value::from(total + duration);
            }
        }
        let updated = entry.clone();
        write_json(&self.directory.join("history.json"), &entries)?;
        self.history = entries;
        self.record_usage(&updated)?;
        Ok(())
    }

    pub fn add_dictionary(&mut self, raw: &str) -> Result<bool, String> {
        let term = raw.trim();
        if term.is_empty() {
            return Ok(false);
        }
        let mut entries = self.dictionary.clone();
        if let Some(existing) = entries
            .iter_mut()
            .find(|entry| entry.term.eq_ignore_ascii_case(term))
        {
            if existing.source != "suggested" {
                return Ok(false);
            }
            existing.source = "manual".to_owned();
            write_json(&self.directory.join("dictionary.json"), &entries)?;
            self.dictionary = entries;
            return Ok(true);
        }
        entries.insert(
            0,
            DictionaryEntry {
                id: uuid::Uuid::new_v4().to_string(),
                term: term.to_owned(),
                source: "manual".to_owned(),
                created_at: Value::String(
                    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                ),
            },
        );
        write_json(&self.directory.join("dictionary.json"), &entries)?;
        self.dictionary = entries;
        Ok(true)
    }

    pub fn import_dictionary(&mut self, content: &str) -> Result<usize, String> {
        let mut entries = self.dictionary.clone();
        let mut added = 0;
        for raw in content.split([',', '，', '\n', '\r']) {
            let term = raw.trim();
            if term.is_empty()
                || entries
                    .iter()
                    .any(|entry| entry.term.eq_ignore_ascii_case(term))
            {
                continue;
            }
            entries.push(DictionaryEntry {
                id: uuid::Uuid::new_v4().to_string(),
                term: term.to_owned(),
                source: "manual".to_owned(),
                created_at: Value::String(
                    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                ),
            });
            added += 1;
        }
        if added > 0 {
            write_json(&self.directory.join("dictionary.json"), &entries)?;
            self.dictionary = entries;
        }
        Ok(added)
    }

    pub fn accept_suggestion(&mut self, id: &str) -> Result<(), String> {
        let mut entries = self.dictionary.clone();
        let Some(entry) = entries
            .iter_mut()
            .find(|entry| entry.id == id && entry.source == "suggested")
        else {
            return Ok(());
        };
        entry.source = "learned".to_owned();
        write_json(&self.directory.join("dictionary.json"), &entries)?;
        self.dictionary = entries;
        Ok(())
    }

    pub fn update_dictionary(&mut self, id: &str, raw: &str) -> Result<bool, String> {
        let term = raw.trim();
        if term.is_empty()
            || self
                .dictionary
                .iter()
                .any(|entry| entry.id != id && entry.term.eq_ignore_ascii_case(term))
        {
            return Ok(false);
        }
        let mut entries = self.dictionary.clone();
        let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) else {
            return Ok(false);
        };
        entry.term = term.to_owned();
        write_json(&self.directory.join("dictionary.json"), &entries)?;
        self.dictionary = entries;
        Ok(true)
    }

    pub fn delete_dictionary(&mut self, id: &str) -> Result<(), String> {
        let mut entries = self.dictionary.clone();
        let Some(entry) = entries.iter().find(|entry| entry.id == id) else {
            return Ok(());
        };
        let mut memory = self.term_memory.clone();
        memory.ignored.insert(entry.term.to_lowercase());
        entries.retain(|entry| entry.id != id);
        write_json(&self.directory.join("term-memory.json"), &memory)?;
        write_json(&self.directory.join("dictionary.json"), &entries)?;
        self.term_memory = memory;
        self.dictionary = entries;
        Ok(())
    }

    pub fn delete_history(&mut self, id: &str) -> Result<(), String> {
        if let Some(entry) = self
            .history
            .iter()
            .find(|entry| entry.get("id").and_then(Value::as_str) == Some(id))
        {
            if let Some(path) = self.audio_path(entry) {
                fs::remove_file(path).map_err(|error| error.to_string())?;
            }
        }
        let mut entries = self.history.clone();
        entries.retain(|entry| entry.get("id").and_then(Value::as_str) != Some(id));
        write_json(&self.directory.join("history.json"), &entries)?;
        self.history = entries;
        Ok(())
    }

    pub fn clear_history(&mut self) -> Result<(), String> {
        for entry in &self.history {
            if let Some(path) = self.audio_path(entry) {
                fs::remove_file(path).map_err(|error| error.to_string())?;
            }
        }
        write_json(&self.directory.join("history.json"), &Vec::<Value>::new())?;
        self.history.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statistics_survive_history_deletion_and_update_without_double_counting() {
        let directory =
            std::env::temp_dir().join(format!("inksay-statistics-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let mut store = Store {
            directory: directory.clone(),
            history: Vec::new(),
            dictionary: Vec::new(),
            term_memory: TermMemory::default(),
            rules: crate::lexicon::RuleBook::default(),
            usage: BTreeMap::new(),
        };
        let entry = serde_json::json!({"id":"one", "createdAt":"2026-09-28T01:00:00Z", "transcript":"你好", "refinedText":"你好！", "duration":2.0, "processingDuration":1.0});
        store.add_history(entry.clone()).unwrap();
        store.record_usage(&entry).unwrap();
        assert_eq!(store.statistics().records.len(), 1);
        store
            .update_insertion("one", Some("你好！"), None, Some(0.2))
            .unwrap();
        assert_eq!(store.statistics().records[0].inserted_characters, Some(3));
        assert_eq!(store.statistics().records[0].processing_duration, Some(1.2));
        store.delete_history("one").unwrap();
        assert!(store.history.is_empty());
        let saved: BTreeMap<String, UsageRecord> =
            read_json(&directory.join("statistics.json")).unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved["one"].inserted_characters, Some(3));
        store.clear_history().unwrap();
        assert_eq!(store.statistics().records.len(), 1);
        // A new entry can evict old history without evicting lifetime metrics.
        for index in 0..500 {
            let mut old = entry.clone();
            old["id"] = Value::from(format!("old-{index}"));
            let record = UsageRecord::from_history(&old).unwrap();
            store.usage.insert(record.id.clone(), record);
            store.history.push(old);
        }
        store.add_history(entry).unwrap();
        assert_eq!(store.history.len(), 500);
        assert_eq!(store.statistics().records.len(), 501);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn repeated_dictation_learns_and_deleted_term_stays_forgotten() {
        let directory =
            std::env::temp_dir().join(format!("inksay-learning-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let mut store = Store {
            directory: directory.clone(),
            history: Vec::new(),
            dictionary: Vec::new(),
            term_memory: TermMemory::default(),
            rules: crate::lexicon::RuleBook::default(),
            usage: BTreeMap::new(),
        };
        for _ in 0..crate::learning::PROMOTION_THRESHOLD - 1 {
            assert!(!store.learn_from_text("Inksay Inksay").unwrap());
        }
        assert!(store.dictionary_terms().is_empty());
        assert!(store.learn_from_text("Inksay").unwrap());
        assert!(store.dictionary_terms().is_empty());
        assert_eq!(store.dictionary[0].source, "suggested");
        assert!(!store.learn_from_text("Inksay").unwrap());

        let id = store.dictionary[0].id.clone();
        store.delete_dictionary(&id).unwrap();
        let memory: TermMemory = read_json(&directory.join("term-memory.json")).unwrap();
        assert!(memory.ignored.contains("inksay"));
        for _ in 0..crate::learning::PROMOTION_THRESHOLD {
            assert!(!store.learn_from_text("Inksay").unwrap());
        }
        assert!(store.dictionary_terms().is_empty());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn dictionary_mutations_persist_without_touching_history() {
        let directory = std::env::temp_dir().join(format!("inksay-store-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let history = vec![serde_json::json!({"id": "existing", "transcript": "原文"})];
        write_json(&directory.join("history.json"), &history).unwrap();
        let mut store = Store {
            directory: directory.clone(),
            history: history.clone(),
            dictionary: Vec::new(),
            term_memory: TermMemory::default(),
            rules: crate::lexicon::RuleBook::default(),
            usage: BTreeMap::new(),
        };

        assert!(store.add_dictionary("Inksay").unwrap());
        assert!(!store.add_dictionary("inksay").unwrap());
        let id = store.dictionary[0].id.clone();
        assert!(store.update_dictionary(&id, "VoiceSyllo").unwrap());
        let saved: Vec<DictionaryEntry> = read_json(&directory.join("dictionary.json")).unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].term, "VoiceSyllo");
        assert_eq!(
            read_json::<Vec<Value>>(&directory.join("history.json")).unwrap(),
            history
        );

        store.delete_dictionary(&id).unwrap();
        assert!(
            read_json::<Vec<DictionaryEntry>>(&directory.join("dictionary.json"))
                .unwrap()
                .is_empty()
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn importing_vocabulary_is_idempotent() {
        let directory =
            std::env::temp_dir().join(format!("inksay-vocabulary-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let mut store = Store {
            directory: directory.clone(),
            history: Vec::new(),
            dictionary: Vec::new(),
            term_memory: TermMemory::default(),
            rules: crate::lexicon::RuleBook::default(),
            usage: BTreeMap::new(),
        };
        assert_eq!(
            store
                .import_dictionary("WhisperKit, whisperkit，Inksay\n")
                .unwrap(),
            2
        );
        assert_eq!(
            store
                .import_dictionary("WhisperKit, whisperkit，Inksay\n")
                .unwrap(),
            0
        );
        assert_eq!(store.dictionary_terms(), vec!["WhisperKit", "Inksay"]);
        assert_eq!(
            read_json::<Vec<DictionaryEntry>>(&directory.join("dictionary.json"))
                .unwrap()
                .len(),
            2
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn manually_added_suggestion_becomes_an_accepted_term() {
        let directory =
            std::env::temp_dir().join(format!("inksay-suggest-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let entry = DictionaryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            term: "WhisperKit".into(),
            source: "suggested".into(),
            created_at: Value::String("2026-09-26T00:00:00Z".into()),
        };
        let mut store = Store {
            directory: directory.clone(),
            history: Vec::new(),
            dictionary: vec![entry.clone()],
            term_memory: TermMemory::default(),
            rules: crate::lexicon::RuleBook::default(),
            usage: BTreeMap::new(),
        };
        assert!(store.add_dictionary("whisperkit").unwrap());
        assert_eq!(store.dictionary[0].id, entry.id);
        assert_eq!(store.dictionary[0].source, "manual");
        assert!(!store.add_dictionary("WhisperKit").unwrap());
        let saved: Vec<DictionaryEntry> = read_json(&directory.join("dictionary.json")).unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].source, "manual");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn correction_terms_remain_suggestions_until_accepted() {
        let directory =
            std::env::temp_dir().join(format!("inksay-correction-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let mut store = Store {
            directory: directory.clone(),
            history: Vec::new(),
            dictionary: Vec::new(),
            term_memory: TermMemory::default(),
            rules: crate::lexicon::RuleBook::default(),
            usage: BTreeMap::new(),
        };
        for _ in 0..crate::learning::PROMOTION_THRESHOLD {
            store.learn_from_text("WhisperKit WhisperKit").unwrap();
        }
        assert_eq!(store.dictionary.len(), 1);
        assert!(store.dictionary_terms().is_empty());
        let id = store.dictionary[0].id.clone();
        store.accept_suggestion(&id).unwrap();
        assert_eq!(store.dictionary_terms(), vec!["WhisperKit"]);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn history_correction_preserves_original_and_rules_require_confirmation() {
        let directory = std::env::temp_dir().join(format!("inksay-rules-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let mut store = Store {
            directory: directory.clone(),
            history: vec![serde_json::json!({
                "id": "one", "mode": "dictate", "transcript": "WhisperKit", "refinedText": "我用 WhisperKit 写稿",
                "insertedText": "我用 WhisperKit 写稿", "duration": 3.0
            })],
            dictionary: Vec::new(),
            term_memory: TermMemory::default(),
            rules: read_json(&directory.join("replacement-rules.json")).unwrap(),
            usage: BTreeMap::new(),
        };
        store.correct_history("one", "我用 Inksay 写稿").unwrap();
        assert_eq!(store.history[0]["transcript"], "WhisperKit");
        assert_eq!(store.history[0]["refinedText"], "我用 WhisperKit 写稿");
        assert_eq!(store.history[0]["insertedText"], "我用 WhisperKit 写稿");
        assert_eq!(store.history[0]["userCorrectedText"], "我用 Inksay 写稿");
        assert!(store.usage.is_empty());
        assert_eq!(store.rules.rules.len(), 1);
        assert_eq!(store.apply_rules("WhisperKit").0, "WhisperKit");
        let id = store.rules.rules[0].id.clone();
        store
            .save_rule(Some(&id), "WhisperKit", "Inksay", true)
            .unwrap();
        assert_eq!(store.apply_rules("WhisperKit").0, "Inksay");
        store.rules = read_json(&directory.join("replacement-rules.json")).unwrap();
        assert_eq!(store.apply_rules("WhisperKit").0, "Inksay");
        store
            .save_rule(Some(&id), "WhisperKit", "Inksay", false)
            .unwrap();
        assert_eq!(store.apply_rules("WhisperKit").0, "WhisperKit");
        store.delete_rule(&id).unwrap();
        assert!(!store
            .suggest_correction("我用 WhisperKit 写稿", "我用 Inksay 写稿")
            .unwrap());
        store.history[0]["mode"] = Value::String("translate".into());
        store.correct_history("one", "我用 DeepSeek 写稿").unwrap();
        assert!(store.rules.rules.is_empty());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn insertion_updates_total_and_stage_duration() {
        let directory =
            std::env::temp_dir().join(format!("inksay-timing-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let mut store = Store {
            directory: directory.clone(),
            history: vec![
                serde_json::json!({"id":"first","audioFileName":null,"processingDuration":1.2}),
            ],
            dictionary: Vec::new(),
            term_memory: TermMemory::default(),
            rules: crate::lexicon::RuleBook::default(),
            usage: BTreeMap::new(),
        };
        store
            .update_insertion("first", Some("内容"), None, Some(0.3))
            .unwrap();
        let saved: Vec<Value> = read_json(&directory.join("history.json")).unwrap();
        assert_eq!(saved[0]["insertedText"], "内容");
        assert_eq!(saved[0]["insertionDuration"], 0.3);
        assert!((saved[0]["processingDuration"].as_f64().unwrap() - 1.5).abs() < 1e-9);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn history_audio_only_reads_recordings_inside_app_data() {
        let directory =
            std::env::temp_dir().join(format!("inksay-audio-store-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(directory.join("recordings")).unwrap();
        fs::write(directory.join("recordings/voice.wav"), [1, 2, 3]).unwrap();
        let store = Store {
            directory: directory.clone(),
            history: vec![
                serde_json::json!({"id":"valid","audioFileName":"voice.wav"}),
                serde_json::json!({"id":"outside","audioFileName":"../voice.wav"}),
            ],
            dictionary: Vec::new(),
            term_memory: TermMemory::default(),
            rules: crate::lexicon::RuleBook::default(),
            usage: BTreeMap::new(),
        };
        assert_eq!(store.audio_bytes("valid").unwrap().0, vec![1, 2, 3]);
        assert!(store.audio_bytes("outside").is_err());
        assert_eq!(store.snapshot().history[0]["hasAudio"], true);
        assert_eq!(store.snapshot().history[1]["hasAudio"], false);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn deleting_history_removes_only_its_recordings() {
        let directory =
            std::env::temp_dir().join(format!("inksay-delete-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(directory.join("recordings")).unwrap();
        fs::write(directory.join("recordings/first.wav"), [1]).unwrap();
        fs::write(directory.join("recordings/second.wav"), [2]).unwrap();
        fs::write(directory.join("outside.wav"), [3]).unwrap();
        let mut store = Store {
            directory: directory.clone(),
            history: vec![
                serde_json::json!({"id":"first","audioFileName":"first.wav"}),
                serde_json::json!({"id":"second","audioFileName":"second.wav"}),
                serde_json::json!({"id":"outside","audioFileName":"../outside.wav"}),
            ],
            dictionary: Vec::new(),
            term_memory: TermMemory::default(),
            rules: crate::lexicon::RuleBook::default(),
            usage: BTreeMap::new(),
        };
        store.delete_history("first").unwrap();
        assert!(!directory.join("recordings/first.wav").exists());
        assert!(directory.join("recordings/second.wav").exists());
        assert_eq!(store.snapshot().history.len(), 2);
        store.clear_history().unwrap();
        assert!(!directory.join("recordings/second.wav").exists());
        assert!(directory.join("outside.wav").exists());
        assert!(read_json::<Vec<Value>>(&directory.join("history.json"))
            .unwrap()
            .is_empty());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn recording_retention_removes_expired_audio_without_deleting_history() {
        let directory =
            std::env::temp_dir().join(format!("inksay-retention-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(directory.join("recordings")).unwrap();
        fs::write(directory.join("recordings/old.wav"), [1, 2]).unwrap();
        fs::write(directory.join("recordings/new.wav"), [3, 4, 5]).unwrap();
        let old = (chrono::Utc::now() - chrono::Duration::days(40)).to_rfc3339();
        let recent = chrono::Utc::now().to_rfc3339();
        let mut store = Store {
            directory: directory.clone(),
            history: vec![
                serde_json::json!({"id":"old","createdAt":old,"audioFileName":"old.wav"}),
                serde_json::json!({"id":"new","createdAt":recent,"audioFileName":"new.wav"}),
            ],
            dictionary: Vec::new(),
            term_memory: TermMemory::default(),
            rules: crate::lexicon::RuleBook::default(),
            usage: BTreeMap::new(),
        };
        assert_eq!(store.recording_bytes().unwrap(), 5);
        store.prune_recordings("day30").unwrap();
        assert_eq!(store.recording_bytes().unwrap(), 3);
        assert_eq!(store.snapshot().history.len(), 2);
        assert!(store.snapshot().history[0]["audioFileName"].is_null());
        assert_eq!(store.snapshot().history[1]["audioFileName"], "new.wav");
        store.prune_recordings("off").unwrap();
        assert_eq!(store.recording_bytes().unwrap(), 0);
        let saved: Vec<Value> = read_json(&directory.join("history.json")).unwrap();
        assert!(saved.iter().all(|entry| entry["audioFileName"].is_null()));
        store
            .update_insertion("new", Some("保留文字"), None, Some(0.2))
            .unwrap();
        let saved: Vec<Value> = read_json(&directory.join("history.json")).unwrap();
        assert_eq!(saved[1]["insertedText"], "保留文字");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn future_dated_orphan_does_not_abort_startup_cleanup() {
        let directory =
            std::env::temp_dir().join(format!("inksay-future-audio-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(directory.join("recordings")).unwrap();
        let orphan = directory.join("recordings/orphan.wav");
        fs::write(&orphan, [1, 2, 3]).unwrap();
        let file = fs::OpenOptions::new().write(true).open(&orphan).unwrap();
        let future = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
        file.set_times(fs::FileTimes::new().set_modified(future))
            .unwrap();
        let mut store = Store {
            directory: directory.clone(),
            history: Vec::new(),
            dictionary: Vec::new(),
            term_memory: TermMemory::default(),
            rules: crate::lexicon::RuleBook::default(),
            usage: BTreeMap::new(),
        };
        store.prune_recordings("forever").unwrap();
        assert!(orphan.exists());
        fs::remove_dir_all(directory).unwrap();
    }
}
