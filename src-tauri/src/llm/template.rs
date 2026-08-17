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

/// 模板可用的全部变量名
pub fn supported_variables() -> &'static [&'static str] {
    SUPPORTED
}

/// 把模板支持、但调用方没提供的变量补成占位说明。
///
/// [`render`] 对缺失变量是直接报错的，这个设计本身没问题——它能挡住写错的模板名。
/// 出事的是调用方：变量按条件注入（查到岗位才填 job_description、开了开关才填 resume），
/// 于是配置的某些组合下渲染必然失败，而失败点在渲染阶段，日志上看起来却像
/// 「模型没生成内容」。猎聘自动回复曾因此在默认配置下一条消息都发不出去。
///
/// 调用方在渲染前统一过一遍这里，那类组合就再也构造不出来了。
pub fn fill_missing(params: &mut Value, placeholder: &str) {
    let Some(map) = params.as_object_mut() else {
        return;
    };
    for key in SUPPORTED {
        let missing = map
            .get(*key)
            .is_none_or(|value| value.as_str().is_some_and(|text| text.trim().is_empty()));
        if missing {
            map.insert((*key).to_string(), Value::String(placeholder.to_string()));
        }
    }
}

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

    /// 这是猎聘自动回复全平台失效的根因：模板引用了 job_description，
    /// 装参数时却只在查得到岗位时才填
    #[test]
    fn filling_missing_variables_makes_any_supported_template_renderable() {
        let mut params = json!({ "chat_history": "HR: 在吗" });
        super::fill_missing(&mut params, "（未提供）");

        for key in super::supported_variables() {
            let template = format!("{{{{{key}}}}}");
            assert!(
                render(&template, &params).is_ok(),
                "补全后 {key} 仍然渲染失败"
            );
        }
    }

    #[test]
    fn filling_never_overwrites_a_value_the_caller_supplied() {
        let mut params = json!({ "chat_history": "HR: 在吗" });
        super::fill_missing(&mut params, "（未提供）");

        assert_eq!(params["chat_history"], json!("HR: 在吗"));
        assert_eq!(params["job_description"], json!("（未提供）"));
    }

    /// 空串等同于没提供：留着会让提示词出现「【个人简历】」后面直接跟下一段标题，
    /// 模型会以为这一节被刻意留白
    #[test]
    fn blank_values_are_treated_as_missing() {
        let mut params = json!({ "resume": "   " });
        super::fill_missing(&mut params, "（未提供）");

        assert_eq!(params["resume"], json!("（未提供）"));
    }
}
