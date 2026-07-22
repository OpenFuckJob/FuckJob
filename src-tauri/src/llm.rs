use crate::config::AppRuntimeConfig;
use crate::dao::job_detail_dao;
use crate::llm::service::LlmService;
use crate::logger;
use crate::rpa::common::{ChatMessage, RpaJob};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub mod openai_compatible;
pub mod service;
pub mod sse;
pub mod template;
pub mod types;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmGenerateResult {
    pub success: bool,
    pub data: String,
}

/// 生成岗位打招呼文本
pub async fn generate_greet_text(
    config: AppRuntimeConfig,
    job: &RpaJob,
) -> Result<LlmGenerateResult, anyhow::Error> {
    let template = config.greet_config.reply_prompt.clone();
    if template.is_none() {
        logger::warning("打招呼提示词未配置，默认生成空内容")?;
        return Ok(LlmGenerateResult {
            success: true,
            data: String::new(),
        });
    }
    let template = template.unwrap();
    let job_content = serde_json::to_string(job).unwrap_or_else(|_| String::new());

    let mut params = json!({
        "job_content": job_content
    });

    if config.resume_config.inject_llm_context {
        let resume_context = config
            .resume_config
            .resume_content
            .clone()
            .unwrap_or_default();
        // params 加 resume_context
        if let Value::Object(ref mut map) = params {
            map.insert("resume_context".to_string(), json!(resume_context));
        }
    }

    let credential = crate::credential::resolve()?;
    let service = LlmService::from_runtime(&config, &credential)?;
    let vo = service.generate_template(&template, &params).await?;
    Ok(LlmGenerateResult {
        success: true,
        data: vo.content,
    })
}

/// 生成回复文本
pub async fn generate_replay_text(
    job_id: String,
    config: &AppRuntimeConfig,
    messages: &[ChatMessage],
) -> Result<LlmGenerateResult, anyhow::Error> {
    let template = config.replay_config.reply_prompt.clone();
    if template.is_none() {
        logger::warning("回复提示词未配置，默认生成空内容")?;
        return Ok(LlmGenerateResult {
            success: true,
            data: String::new(),
        });
    }
    let template = template.unwrap();

    let chat_history = format_chat_history(messages);

    let mut params = json!({
        "chat_history": chat_history
    });

    // 查询岗位详情，注入 job_description
    if let Ok(Some(job_detail)) = job_detail_dao::get_by_id(&job_id) {
        if !job_detail.detail.is_empty() {
            if let Value::Object(ref mut map) = params {
                map.insert("job_description".to_string(), json!(job_detail.detail));
            }
        }
    }

    if config.resume_config.inject_llm_context {
        let resume_content = config
            .resume_config
            .resume_content
            .clone()
            .unwrap_or_default();
        if let Value::Object(ref mut map) = params {
            map.insert("resume".to_string(), json!(resume_content));
            map.insert("resume_context".to_string(), json!(resume_content));
        }
    }

    if let Some(ref background) = config.replay_config.background_context {
        if !background.is_empty() {
            if let Value::Object(ref mut map) = params {
                map.insert("background_context".to_string(), json!(background));
            }
        }
    }

    let credential = crate::credential::resolve()?;
    let service = LlmService::from_runtime(config, &credential)?;
    let vo = service.generate_template(&template, &params).await?;
    Ok(LlmGenerateResult {
        success: true,
        data: vo.content,
    })
}

fn format_chat_history(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .map(|m| {
            let role = if m.received { "HR" } else { "我" };
            format!("{}({}): {}", m.from_name, role, m.text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}
