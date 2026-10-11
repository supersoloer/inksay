pub struct FocusedContext {
    pub app_name: Option<String>,
    pub bundle_id: Option<String>,
    pub window_title: Option<String>,
    pub page_url: Option<String>,
    pub selected_text: Option<String>,
    pub text_before_cursor: Option<String>,
    pub text_after_cursor: Option<String>,
    pub content_format: &'static str,
}

impl Default for FocusedContext {
    fn default() -> Self {
        Self {
            app_name: None,
            bundle_id: None,
            window_title: None,
            page_url: None,
            selected_text: None,
            text_before_cursor: None,
            text_after_cursor: None,
            content_format: "纯文本",
        }
    }
}

impl FocusedContext {
    pub fn prompt_description(&self) -> Option<String> {
        if self.app_name.is_none()
            && self.window_title.is_none()
            && self.page_url.is_none()
            && self.selected_text.is_none()
            && self.text_before_cursor.is_none()
            && self.text_after_cursor.is_none()
        {
            return None;
        }
        let mut lines = Vec::new();
        if let Some(name) = &self.app_name {
            let mut app = format!("应用：{name}");
            if let Some(title) = &self.window_title {
                app.push_str(&format!("｜窗口：{title}"));
            }
            lines.push(app);
        } else if let Some(title) = &self.window_title {
            lines.push(format!("窗口：{title}"));
        }
        if let Some(url) = &self.page_url {
            lines.push(format!("网页：{url}"));
        }
        if let Some(selected) = &self.selected_text {
            lines.push(format!("选中的内容：{}", truncate_tail(selected, 400)));
        }
        lines.push(format!("输入格式：{}", self.content_format));
        if let Some(before) = &self.text_before_cursor {
            lines.push(format!("光标前文：{}", truncate_tail(before, 400)));
        }
        if let Some(after) = &self.text_after_cursor {
            lines.push(format!("光标后文：{}", truncate_tail(after, 400)));
        }
        Some(lines.join("\n"))
    }
}

fn truncate_tail(text: &str, limit: usize) -> String {
    let length = text.chars().count();
    if length <= limit {
        text.to_owned()
    } else {
        format!("…{}", text.chars().skip(length - limit).collect::<String>())
    }
}

#[cfg(any(target_os = "macos", test))]
pub fn sanitized_page_url(raw: &str) -> Option<String> {
    let url = reqwest::Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?;
    let mut safe = format!("{}://{host}", url.scheme());
    if let Some(port) = url.port() {
        safe.push_str(&format!(":{port}"));
    }
    safe.push_str(url.path());
    if safe.chars().count() > 300 {
        Some(format!("{}…", safe.chars().take(300).collect::<String>()))
    } else {
        Some(safe)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_url_never_exposes_query_or_fragment() {
        assert_eq!(
            sanitized_page_url("https://example.com/a/b?token=secret#private"),
            Some("https://example.com/a/b".into())
        );
        assert_eq!(sanitized_page_url("file:///Users/person/private"), None);
    }

    #[test]
    fn prompt_limits_surrounding_text_to_near_cursor() {
        let context = FocusedContext {
            app_name: Some("Editor".into()),
            selected_text: Some("a".repeat(420)),
            text_before_cursor: Some(format!("{}near", "z".repeat(420))),
            content_format: "Markdown",
            ..Default::default()
        };
        let prompt = context.prompt_description().unwrap();
        assert!(prompt.contains("光标前文：…"));
        assert!(prompt.ends_with("near"));
        assert!(!prompt.contains(&"z".repeat(420)));
    }
}
