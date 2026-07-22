use crate::error::AppError;
use serde_json::Value;

const SUPPORTED: &[&str] = &[
    "job_content",
    "job_description",
    "chat_history",
    "chat_context",
    "message_content",
    "resume",
    "resume_context",
    "background_context",
];

pub fn render(template: &str, params: &Value) -> Result<String, AppError> {
    let values = params
        .as_object()
        .ok_or_else(|| AppError::validation("提示词参数必须是对象"))?;
    let regex = regex::Regex::new(r"\{\{\s*([^{}]+?)\s*\}\}").expect("valid regex");
    let mut output = String::with_capacity(template.len());
    let mut cursor = 0;
    for captures in regex.captures_iter(template) {
        let whole = captures.get(0).expect("whole capture");
        let key = captures.get(1).expect("key capture").as_str().trim();
        if !SUPPORTED.contains(&key) {
            return Err(AppError::validation(format!("不支持的提示词变量: {key}")));
        }
        let value = values
            .get(key)
            .ok_or_else(|| AppError::validation(format!("缺少提示词变量: {key}")))?;
        let value = value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string());
        output.push_str(&template[cursor..whole.start()]);
        output.push_str(&value);
        cursor = whole.end();
    }
    output.push_str(&template[cursor..]);
    if output.contains("{{") || output.contains("}}") {
        return Err(AppError::validation("提示词包含无效占位符"));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::render;
    use serde_json::json;

    #[test]
    fn renders_repeated_unicode_variables() {
        assert_eq!(
            render("你好 {{resume}} / {{resume}}", &json!({"resume":"世界"})).unwrap(),
            "你好 世界 / 世界"
        );
    }

    #[test]
    fn rejects_missing_and_unknown_placeholders() {
        for template in ["{{job_content}} {{resume}}", "{{not_supported}}"] {
            let error = render(template, &json!({"job_content":"JD"})).unwrap_err();
            assert_eq!(error.code, crate::error::AppErrorCode::Validation);
        }
    }
}
