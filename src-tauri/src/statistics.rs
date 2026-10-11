use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRecord {
    pub id: String,
    pub created_at: String,
    pub mode: Option<String>,
    pub app_name: Option<String>,
    pub language: Option<String>,
    pub refine_style: Option<String>,
    pub recognition_model: Option<String>,
    pub text_model: Option<String>,
    pub backend: Option<String>,
    pub asr_provider: Option<String>,
    pub asr_input_tokens: Option<u64>,
    pub asr_output_tokens: Option<u64>,
    pub asr_billed_seconds: Option<f64>,
    pub transcript_characters: usize,
    pub generated_characters: usize,
    pub inserted_characters: Option<usize>,
    pub duration: f64,
    pub processing_duration: Option<f64>,
    pub transcription_duration: Option<f64>,
    pub model_duration: Option<f64>,
    pub insertion_duration: Option<f64>,
    pub has_error: bool,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
}

impl UsageRecord {
    pub fn from_history(entry: &Value) -> Option<Self> {
        let id = entry["id"].as_str()?.to_owned();
        let created_at = entry["createdAt"].as_str()?.to_owned();
        chrono::DateTime::parse_from_rfc3339(&created_at).ok()?;
        let characters = |key: &str| {
            entry[key]
                .as_str()
                .map(|s| s.chars().filter(|c| !c.is_whitespace()).count())
        };
        let seconds = |key: &str| entry[key].as_f64().filter(|v| v.is_finite() && *v >= 0.0);
        let text = |key: &str| {
            entry[key]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        };
        Some(Self {
            id,
            created_at,
            mode: text("mode"),
            app_name: text("appName"),
            language: text("language"),
            refine_style: text("refineStyle"),
            recognition_model: text("recognitionModel"),
            text_model: text("textModel"),
            backend: text("backend"),
            asr_provider: text("asrProvider"),
            asr_input_tokens: entry["asrInputTokens"].as_u64(),
            asr_output_tokens: entry["asrOutputTokens"].as_u64(),
            asr_billed_seconds: seconds("asrBilledSeconds"),
            transcript_characters: characters("transcript").unwrap_or(0),
            generated_characters: characters("refinedText").unwrap_or(0),
            inserted_characters: characters("insertedText"),
            duration: seconds("duration").unwrap_or(0.0),
            processing_duration: seconds("processingDuration"),
            transcription_duration: seconds("transcriptionDuration"),
            model_duration: seconds("modelDuration"),
            insertion_duration: seconds("insertionDuration"),
            has_error: text("error").is_some(),
            prompt_tokens: entry["promptTokens"].as_u64(),
            completion_tokens: entry["completionTokens"].as_u64(),
        })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatisticsSnapshot {
    pub records: Vec<UsageRecord>,
    pub dictionary_terms: usize,
    pub learned_terms: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn metrics_exclude_content_and_preserve_unknown_timings() {
        let entry = json!({"id":"one", "createdAt":"2026-09-28T01:00:00Z", "transcript":"你好 世界👋", "refinedText":"你好世界！", "duration":2.5, "windowTitle":"private", "audioFileName":"private.wav"});
        let metrics = UsageRecord::from_history(&entry).unwrap();
        assert_eq!(metrics.transcript_characters, 5);
        assert_eq!(metrics.generated_characters, 5);
        assert_eq!(metrics.processing_duration, None);
        assert_eq!(metrics.inserted_characters, None);
        let saved = serde_json::to_string(&metrics).unwrap();
        assert!(!saved.contains("你好"));
        assert!(!saved.contains("private"));
        assert!(UsageRecord::from_history(&json!({"id":"invalid", "createdAt":"bad"})).is_none());
    }
}
