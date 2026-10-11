pub fn simplified(text: &str, language: Option<&str>) -> String {
    if language.is_some_and(|value| value.to_ascii_lowercase().starts_with("zh")) {
        zhconv::zhconv(text, zhconv::Variant::ZhHans)
    } else {
        text.to_owned()
    }
}

pub fn reject_short_prompt_echo(text: &str, vocabulary: &[String], duration: f64) -> bool {
    if duration < 0.35 {
        return true;
    }
    if duration >= 1.5 || vocabulary.is_empty() {
        return false;
    }
    let mut remainder = alphanumeric(text);
    if remainder.is_empty() {
        return true;
    }
    let mut terms: Vec<String> = vocabulary.iter().map(|term| alphanumeric(term)).collect();
    terms.retain(|term| !term.is_empty());
    terms.sort_by_key(|term| std::cmp::Reverse(term.chars().count()));
    for term in terms {
        remainder = remainder.replace(&term, "");
    }
    remainder.is_empty()
}

fn alphanumeric(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .filter(|ch| ch.is_alphanumeric())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_transcript_is_simplified_before_refinement() {
        assert_eq!(
            simplified("今天測試語音識別。", Some("zh-CN")),
            "今天测试语音识别。"
        );
        assert_eq!(
            simplified("今天測試語音識別。", Some("en-US")),
            "今天測試語音識別。"
        );
        assert_eq!(simplified("今天測試語音識別。", None), "今天測試語音識別。");
    }

    #[test]
    fn short_recording_rejects_dictionary_echo_only() {
        let terms = vec!["WhisperKit".to_owned(), "语音识别".to_owned()];
        assert!(reject_short_prompt_echo("WhisperKit。", &terms, 0.8));
        assert!(!reject_short_prompt_echo(
            "我在用 WhisperKit。",
            &terms,
            0.8
        ));
        assert!(!reject_short_prompt_echo("WhisperKit。", &terms, 1.5));
        assert!(reject_short_prompt_echo("随便说什么", &terms, 0.3));
    }
}
