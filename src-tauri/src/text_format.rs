use serde_json::Value;
use std::sync::OnceLock;

pub fn apply(text: &str, config: &serde_json::Map<String, Value>) -> String {
    let english = config.get("chineseEnglishSpacing").and_then(Value::as_str) == Some("space");
    let numbers = config.get("chineseNumberSpacing").and_then(Value::as_str) == Some("space");
    let punctuation = config.get("chinesePunctuation").and_then(Value::as_str) == Some("standard");
    if !english && !numbers && !punctuation {
        return text.to_owned();
    }
    static PROTECTED: OnceLock<regex::Regex> = OnceLock::new();
    let protected = PROTECTED.get_or_init(|| regex::Regex::new(
        r"(?s)```.*?(?:```|\z)|`[^`\n]*(?:`|\z)|(?:https?://|www\.)[^\s<>，。！？；]+|[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}|(?:[A-Za-z]:[\\/]|[~/])[^\s，。！？；]+|[A-Za-z0-9_\-]+(?:\.[A-Za-z0-9_\-]+)+|[A-Za-z][A-Za-z0-9]*_[A-Za-z0-9_]+"
    ).unwrap());
    let mut output = String::with_capacity(text.len());
    let mut offset = 0;
    for span in protected.find_iter(text) {
        output.push_str(&format_prose(
            &text[offset..span.start()],
            english,
            numbers,
            punctuation,
        ));
        output.push_str(span.as_str());
        offset = span.end();
    }
    output.push_str(&format_prose(
        &text[offset..],
        english,
        numbers,
        punctuation,
    ));
    output
}

fn chinese(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0x20000..=0x323af)
}

fn format_prose(text: &str, english: bool, numbers: bool, punctuation: bool) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut output = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 {
            let previous = chars[i - 1];
            let other = if chinese(previous) {
                Some(c)
            } else if chinese(c) {
                Some(previous)
            } else {
                None
            };
            if other.is_some_and(|c| {
                (english && c.is_ascii_alphabetic()) || (numbers && c.is_ascii_digit())
            }) {
                output.push(' ');
            }
        }
        let left = chars[..i].iter().rev().find(|c| !c.is_whitespace());
        let right = chars[i + 1..].iter().find(|c| !c.is_whitespace());
        let chinese_context =
            left.is_some_and(|c| chinese(*c)) || right.is_some_and(|c| chinese(*c));
        output.push(if punctuation && chinese_context {
            match c {
                ',' => '，',
                '.' => '。',
                ':' => '：',
                ';' => '；',
                '!' => '！',
                '?' => '？',
                _ => c,
            }
        } else {
            c
        });
    }
    output
}

#[cfg(test)]
mod tests {
    fn apply(text: &str, config: &serde_json::Value) -> String {
        super::apply(text, config.as_object().unwrap())
    }
    use serde_json::json;
    #[test]
    fn defaults_preserve_every_byte() {
        let text = "使用Inksay, 花了3分钟.\n\tHello world!";
        assert_eq!(apply(text, &json!({})), text);
        assert_eq!(
            apply(
                text,
                &json!({"chineseEnglishSpacing":"preserve","chineseNumberSpacing":"preserve","chinesePunctuation":"preserve"})
            ),
            text
        );
    }
    #[test]
    fn preferences_are_independent_and_idempotent() {
        assert_eq!(
            apply(
                "使用Inksay花了3分钟",
                &json!({"chineseEnglishSpacing":"space"})
            ),
            "使用 Inksay 花了3分钟"
        );
        assert_eq!(
            apply(
                "使用Inksay花了3分钟",
                &json!({"chineseNumberSpacing":"space"})
            ),
            "使用Inksay花了 3 分钟"
        );
        assert_eq!(
            apply(
                "你好,世界! Hello, world!",
                &json!({"chinesePunctuation":"standard"})
            ),
            "你好，世界！ Hello, world!"
        );
        let config = json!({"chineseEnglishSpacing":"space","chineseNumberSpacing":"space","chinesePunctuation":"standard"});
        let result = apply("使用Inksay花了3分钟!", &config);
        assert_eq!(apply(&result, &config), result);
    }
    #[test]
    fn technical_spans_are_untouched() {
        let text = "https://example.com/a?q=3.1 test@example.com C:\\Users\\me\\a.txt /tmp/a.txt v1.2.3 3.14 `中文abc3,`\n```js\nconst 名称=3;\n```";
        let config = json!({"chineseEnglishSpacing":"space","chineseNumberSpacing":"space","chinesePunctuation":"standard"});
        assert_eq!(apply(text, &config), text);
    }
}
