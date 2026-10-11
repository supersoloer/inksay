use jieba_rs::Jieba;
use std::collections::HashSet;
use std::sync::OnceLock;

pub const PROMOTION_THRESHOLD: u32 = 4;
pub const MAX_TRACKED_TERMS: usize = 2048;
pub const MAX_LEARNED_TERMS: usize = 64;

pub fn candidates(text: &str) -> Vec<String> {
    static SEGMENTER: OnceLock<Jieba> = OnceLock::new();
    let segmenter = SEGMENTER.get_or_init(Jieba::new);
    let tagged = segmenter.tag(text, false);
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for token in &tagged {
        let word = token.word.trim();
        let length = word.chars().count();
        let han = is_han(word);
        let latin = word
            .chars()
            .all(|character| character.is_ascii_alphanumeric());
        let proper_name = matches!(token.tag, "nr" | "ns" | "nt" | "nz");
        let useful = (han
            && (proper_name && (2..=12).contains(&length)
                || token.tag.starts_with('n') && (3..=12).contains(&length)))
            || (latin
                && (3..=32).contains(&length)
                && word.chars().any(|character| character.is_ascii_uppercase()));
        if useful && seen.insert(word.to_lowercase()) {
            result.push(word.to_owned());
        }
    }
    for pair in tagged.windows(2) {
        let first = &pair[0];
        let second = &pair[1];
        if first.byte_end != second.byte_start
            || !first.tag.starts_with('n')
            || !(second.tag.starts_with('n') || second.tag.starts_with('v'))
            || !is_han(first.word)
            || !is_han(second.word)
        {
            continue;
        }
        let term = format!("{}{}", first.word, second.word);
        if (3..=12).contains(&term.chars().count()) && seen.insert(term.clone()) {
            result.push(term);
        }
    }
    result.truncate(32);
    result
}

fn is_han(word: &str) -> bool {
    word.chars()
        .all(|character| ('\u{4e00}'..='\u{9fff}').contains(&character))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_repeated_names_once_per_utterance() {
        let terms = candidates("Inksay 和 Inksay 使用 DeepSeek。今天开会。 ");
        assert_eq!(terms.iter().filter(|term| *term == "Inksay").count(), 1);
        assert!(terms.contains(&"DeepSeek".to_owned()));
        assert!(!terms.contains(&"今天".to_owned()));
    }

    #[test]
    fn keeps_chinese_topic_terms_without_function_words() {
        let terms = candidates("我们经常讨论机器学习和语音识别。");
        assert!(terms.contains(&"机器学习".to_owned()) || terms.contains(&"语音识别".to_owned()));
        assert!(!terms.contains(&"我们".to_owned()));
    }
}
