use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplacementRule {
    pub id: String,
    pub from: String,
    pub to: String,
    pub enabled: bool,
    pub source: String,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RuleBook {
    pub rules: Vec<ReplacementRule>,
    pub ignored: BTreeSet<(String, String)>,
}

#[derive(Clone, Serialize)]
pub struct AppliedRule {
    pub id: String,
    pub from: String,
    pub to: String,
    pub count: usize,
}

impl RuleBook {
    pub fn save(
        &mut self,
        id: Option<&str>,
        from: &str,
        to: &str,
        enabled: bool,
    ) -> Result<(), String> {
        let (from, to) = (from.trim(), to.trim());
        if from.is_empty()
            || to.is_empty()
            || from == to
            || from.chars().count() > 80
            || to.chars().count() > 80
            || from.chars().chain(to.chars()).any(char::is_control)
        {
            return Err("替换内容无效".into());
        }
        if self
            .rules
            .iter()
            .any(|r| Some(r.id.as_str()) != id && r.source != "suggested" && r.from == from)
        {
            return Err("该原词已有替换规则".into());
        }
        if let Some(id) = id {
            let rule = self
                .rules
                .iter_mut()
                .find(|r| r.id == id)
                .ok_or("规则不存在")?;
            rule.from = from.into();
            rule.to = to.into();
            rule.enabled = enabled;
            if rule.source == "suggested" {
                rule.source = "confirmed".into();
            }
        } else {
            self.rules.push(ReplacementRule {
                id: uuid::Uuid::new_v4().to_string(),
                from: from.into(),
                to: to.into(),
                enabled,
                source: "manual".into(),
            });
        }
        self.rules
            .retain(|r| r.source != "suggested" || r.from != from);
        self.ignored.remove(&(from.into(), to.into()));
        Ok(())
    }

    pub fn suggest(&mut self, from: String, to: String) -> bool {
        if self.ignored.contains(&(from.clone(), to.clone()))
            || self.rules.iter().any(|r| r.from == from)
            || self
                .rules
                .iter()
                .filter(|r| r.source == "suggested")
                .count()
                >= 64
        {
            return false;
        }
        self.rules.push(ReplacementRule {
            id: uuid::Uuid::new_v4().to_string(),
            from,
            to,
            enabled: false,
            source: "suggested".into(),
        });
        true
    }

    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        let index = self
            .rules
            .iter()
            .position(|r| r.id == id)
            .ok_or("规则不存在")?;
        let rule = self.rules.remove(index);
        self.ignored.insert((rule.from, rule.to));
        Ok(())
    }

    pub fn apply(&self, text: &str) -> (String, Vec<AppliedRule>) {
        let mut rules: Vec<_> = self
            .rules
            .iter()
            .filter(|r| r.enabled && r.source != "suggested")
            .collect();
        rules.sort_by_key(|r| std::cmp::Reverse(r.from.len()));
        let mut output = String::with_capacity(text.len());
        let mut applied: Vec<AppliedRule> = Vec::new();
        let mut offset = 0;
        while offset < text.len() {
            let found = rules
                .iter()
                .find(|r| text[offset..].starts_with(&r.from) && boundary(text, offset, &r.from));
            if let Some(rule) = found {
                output.push_str(&rule.to);
                offset += rule.from.len();
                if let Some(hit) = applied.iter_mut().find(|hit| hit.id == rule.id) {
                    hit.count += 1;
                } else {
                    applied.push(AppliedRule {
                        id: rule.id.clone(),
                        from: rule.from.clone(),
                        to: rule.to.clone(),
                        count: 1,
                    });
                }
            } else {
                let ch = text[offset..].chars().next().unwrap();
                output.push(ch);
                offset += ch.len_utf8();
            }
        }
        (output, applied)
    }
}

fn boundary(text: &str, offset: usize, from: &str) -> bool {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let left = from.chars().next().is_some_and(word)
        && text[..offset].chars().next_back().is_some_and(word);
    let right = from.chars().next_back().is_some_and(word)
        && text[offset + from.len()..].chars().next().is_some_and(word);
    !left && !right
}

// Whole token differences prevent learning a dangerous single-character replacement.
pub fn correction_pair(original: &str, edited: &str) -> Option<(String, String)> {
    use std::sync::OnceLock;
    static JIEBA: OnceLock<jieba_rs::Jieba> = OnceLock::new();
    if original.chars().count() > 2000 || edited.chars().count() > 2000 {
        return None;
    }
    let jieba = JIEBA.get_or_init(jieba_rs::Jieba::new);
    let a = jieba.cut(original, true);
    let b = jieba.cut(edited, true);
    let prefix = a
        .iter()
        .zip(&b)
        .take_while(|(a, b)| a.word == b.word)
        .count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(a, b)| a.word == b.word)
        .count();
    let from: String = a[prefix..a.len() - suffix].iter().map(|t| t.word).collect();
    let to: String = b[prefix..b.len() - suffix].iter().map(|t| t.word).collect();
    let valid = |s: &str| {
        (2..=24).contains(&s.chars().count())
            && s.chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
            && s.chars().any(char::is_alphabetic)
    };
    if from == to
        || !valid(&from)
        || !valid(&to)
        || from.chars().any(char::is_numeric)
        || to.chars().any(char::is_numeric)
    {
        return None;
    }
    // Chinese rephrasing is ambiguous; only closely related spellings become suggestions.
    if !from.is_ascii() || !to.is_ascii() {
        let common = from.chars().filter(|c| to.contains(*c)).count();
        if common * 2 < from.chars().count().max(to.chars().count())
            || from.contains(['不', '没'])
            || to.contains(['不', '没'])
        {
            return None;
        }
    }
    Some((from, to))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn precise_non_cascading_rules() {
        let mut book = RuleBook::default();
        book.save(None, "Notion", "Inksay", true).unwrap();
        book.save(None, "Inksay", "Other", true).unwrap();
        book.save(None, "唐小西", "唐小羲", true).unwrap();
        book.save(None, "唐小西同学", "小羲", true).unwrap();
        book.save(None, "disabled", "wrong", false).unwrap();
        book.suggest("pending".into(), "wrong".into());
        let (text, hits) = book.apply("Notional Notion，唐小西和唐小西同学 disabled pending");
        assert_eq!(text, "Notional Inksay，唐小羲和小羲 disabled pending");
        assert_eq!(hits.len(), 3);
        assert!(book.save(None, "Notion", "conflict", true).is_err());
    }
    #[test]
    fn suggestions_require_real_replacement_and_can_be_forgotten() {
        assert_eq!(
            correction_pair("我用 WhisperKit 写稿", "我用 Inksay 写稿"),
            Some(("WhisperKit".into(), "Inksay".into()))
        );
        assert_eq!(
            correction_pair("联系唐小西", "联系唐小羲"),
            Some(("唐小西".into(), "唐小羲".into()))
        );
        assert!(correction_pair("今天开会", "今天开会然后吃饭").is_none());
        assert!(correction_pair("今天开会", "今天不开会").is_none());
        assert!(correction_pair("会议在15点", "会议在16点").is_none());
        let mut book = RuleBook::default();
        book.suggest("wrong".into(), "right".into());
        let id = book.rules[0].id.clone();
        book.delete(&id).unwrap();
        assert!(!book.suggest("wrong".into(), "right".into()));
    }
}
