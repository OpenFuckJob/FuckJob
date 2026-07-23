use crate::command::base::CommandResult;
use crate::config::RegexRule;
use crate::error::AppError;
use crate::llm::service::LlmService;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
pub struct DebugReplayRequest {
    pub job_title: String,
    pub company_name: String,
    pub job_detail: String,
    pub salary: String,
    pub location: String,
    pub messages: Vec<DebugChatMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugChatMessage {
    pub text: String,
    pub from_name: String,
    pub received: bool,
}

#[derive(Debug, Deserialize)]
pub struct DebugGreetRequest {
    pub job_title: String,
    pub company_name: String,
    pub job_detail: String,
    pub salary: String,
    pub location: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PredictedQuestion {
    pub id: i64,
    pub question: String,
    pub intent: String,
    pub target_section: String,
}

#[derive(Debug, Deserialize)]
pub struct OptimizeWithAnswerRequest {
    pub resume_content: String,
    pub question: String,
    pub user_answer: String,
    pub section_title: String,
}

#[derive(Debug, Serialize)]
pub struct ResumeLlmResult {
    pub success: bool,
    pub data: String,
}

#[tauri::command]
pub async fn generate_job_filter_rules(
    app_handle: tauri::AppHandle,
    requirement: String,
) -> CommandResult<Vec<RegexRule>> {
    let requirement = requirement.trim();
    if requirement.is_empty() {
        return CommandResult::err(AppError::validation("请输入岗位筛选需求"));
    }

    let config = match crate::config::load_app_config_inner(app_handle) {
        Ok(value) => value,
        Err(error) => return CommandResult::err(error),
    };
    let service = match service(&config) {
        Ok(value) => value,
        Err(error) => return CommandResult::err(error),
    };

    match service
        .generate(build_job_filter_rules_prompt(requirement))
        .await
    {
        Ok(response) => match parse_generated_job_filter_rules(&response.content) {
            Ok(rules) => CommandResult::ok(rules),
            Err(error) => CommandResult::err(error),
        },
        Err(error) => CommandResult::err(error),
    }
}

fn service(config: &crate::config::AppRuntimeConfig) -> Result<LlmService, crate::error::AppError> {
    let credential = crate::credential::resolve()?;
    LlmService::from_runtime(config, &credential)
}

#[tauri::command]
pub async fn debug_generate_replay(
    app_handle: tauri::AppHandle,
    req: DebugReplayRequest,
) -> CommandResult<String> {
    let config = match crate::config::load_app_config_inner(app_handle) {
        Ok(cfg) => cfg,
        Err(err) => return CommandResult::err(err),
    };

    let template = match &config.replay_config.reply_prompt {
        Some(t) if !t.is_empty() => t.clone(),
        _ => return CommandResult::err("回复提示词未配置，请在配置中心设置回复提示词"),
    };

    let messages_json = serde_json::to_string(&req.messages).unwrap_or_default();
    let mut params = json!({
        "message_content": messages_json,
        "job_description": req.job_detail,
    });

    if config.resume_config.inject_llm_context {
        if let Some(ref resume) = config.resume_config.resume_content {
            if !resume.is_empty() {
                if let Value::Object(ref mut map) = params {
                    map.insert("resume_context".to_string(), json!(resume));
                }
            }
        }
    }

    if let Some(ref bg) = config.replay_config.background_context {
        if !bg.is_empty() {
            if let Value::Object(ref mut map) = params {
                map.insert("background_context".to_string(), json!(bg));
            }
        }
    }

    let service = match service(&config) {
        Ok(v) => v,
        Err(e) => return CommandResult::err(e),
    };
    match service.generate_template(&template, &params).await {
        Ok(vo) => CommandResult::ok(vo.content),
        Err(err) => CommandResult::err(err),
    }
}

#[tauri::command]
pub async fn debug_generate_greet(
    app_handle: tauri::AppHandle,
    req: DebugGreetRequest,
) -> CommandResult<String> {
    let config = match crate::config::load_app_config_inner(app_handle) {
        Ok(cfg) => cfg,
        Err(err) => return CommandResult::err(err),
    };

    let template = match &config.greet_config.reply_prompt {
        Some(t) if !t.is_empty() => t.clone(),
        _ => return CommandResult::err("打招呼提示词未配置，请在配置中心设置打招呼提示词"),
    };

    let job = json!({
        "title": req.job_title,
        "company_name": req.company_name,
        "detail": req.job_detail,
        "salary": req.salary,
        "location": req.location,
    });

    let mut params = json!({
        "job_content": job.to_string(),
    });

    if config.resume_config.inject_llm_context {
        if let Some(ref resume) = config.resume_config.resume_content {
            if !resume.is_empty() {
                if let Value::Object(ref mut map) = params {
                    map.insert("resume_context".to_string(), json!(resume));
                }
            }
        }
    }

    let service = match service(&config) {
        Ok(v) => v,
        Err(e) => return CommandResult::err(e),
    };
    match service.generate_template(&template, &params).await {
        Ok(vo) => CommandResult::ok(vo.content),
        Err(err) => CommandResult::err(err),
    }
}

#[tauri::command]
pub async fn predict_resume_questions(
    app_handle: tauri::AppHandle,
    resume_content: String,
) -> CommandResult<Vec<PredictedQuestion>> {
    if resume_content.trim().is_empty() {
        return CommandResult::err("请先输入/导入简历内容");
    }

    let config = match crate::config::load_app_config_inner(app_handle) {
        Ok(v) => v,
        Err(e) => return CommandResult::err(e),
    };
    let service = match service(&config) {
        Ok(v) => v,
        Err(e) => return CommandResult::err(e),
    };
    match service
        .generate(build_predict_resume_questions_prompt(&resume_content))
        .await
    {
        Ok(vo) => match parse_predicted_questions(&vo.content) {
            Ok(questions) => CommandResult::ok(questions),
            Err(err) => CommandResult::err(err),
        },
        Err(err) => CommandResult::err(err),
    }
}

#[tauri::command]
pub async fn optimize_resume_with_answer(
    app_handle: tauri::AppHandle,
    request: OptimizeWithAnswerRequest,
) -> CommandResult<ResumeLlmResult> {
    if request.resume_content.trim().is_empty() {
        return CommandResult::err("请先输入/导入简历内容");
    }
    if request.question.trim().is_empty() {
        return CommandResult::err("请选择要回答的问题");
    }
    if request.user_answer.trim().is_empty() {
        return CommandResult::err("请输入您的真实回答");
    }
    if request.section_title.trim().is_empty() {
        return CommandResult::err("缺少关联优化章节");
    }

    let config = match crate::config::load_app_config_inner(app_handle) {
        Ok(v) => v,
        Err(e) => return CommandResult::err(e),
    };
    let service = match service(&config) {
        Ok(v) => v,
        Err(e) => return CommandResult::err(e),
    };
    match service
        .generate(build_optimize_resume_with_answer_prompt(&request))
        .await
    {
        Ok(vo) => CommandResult::ok(ResumeLlmResult {
            success: true,
            data: vo.content.trim().to_string(),
        }),
        Err(err) => CommandResult::err(err),
    }
}

fn build_predict_resume_questions_prompt(resume_content: &str) -> String {
    format!(
        r#"你是一位挑剔且经验丰富的技术面试官。请仔细阅读以下候选人的 Markdown 简历：
---
{resume_content}
---
请找出简历中不详实、缺乏量化指标（如QPS、性能提升百分比、业务成效）、或者技术方案可能存在漏洞的 5 个薄弱点。
针对这 5 个薄弱点，提出 5 个在真实面试中面试官最可能追问的深度专业问题，并说明你的提问意图（想考察候选人什么底层能力）。

输出约束（极其重要）：
只输出一个合法的 JSON 数组，不要包含任何 Markdown 代码块标记（如 ```json），不要有任何前言、后记或解释。

JSON 数组格式如下：
[
  {{
    "id": 1,
    "question": "具体追问的问题，如：你提到在网关层做限流，能详细说说令牌桶算法和漏桶算法的区别，以及你们为什么选择前者吗？",
    "intent": "考察对高并发限流方案的底层掌握程度及技术选型思考",
    "target_section": "项目经历"
  }}
]"#
    )
}

fn build_job_filter_rules_prompt(requirement: &str) -> String {
    format!(
        r#"你是岗位筛选规则生成器。请把用户的自然语言需求转换为 Rust regex crate 兼容的正则规则。

用户需求：
<requirement>
{requirement}
</requirement>

规则字段说明：
- name：简短中文名称。
- pattern：Rust regex 兼容的正则表达式，禁止使用前瞻、后顾和反向引用。
- target：只能是 Title、Company、Description、All。Title 匹配岗位标题，Company 匹配公司名，Description 和 All 匹配岗位描述。
- mode：只能是 ACCEPT 或 REJECT。ACCEPT 表示只接受命中的岗位，REJECT 表示拒绝命中的岗位。

输出要求：
1. 只输出合法 JSON 数组，不得输出 Markdown、解释或其他文字。
2. 每条规则只表达一个清晰意图，最多输出 12 条。
3. 使用非捕获分组和 | 表达同类关键词，例如“Java|Golang”。
4. 不要臆造用户未提出的筛选条件。

输出格式：
[
  {{"name":"排除外包","pattern":"外包|驻场","target":"Description","mode":"REJECT"}}
]"#
    )
}

fn parse_generated_job_filter_rules(raw: &str) -> Result<Vec<RegexRule>, AppError> {
    let cleaned = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let mut rules: Vec<RegexRule> = serde_json::from_str(cleaned).map_err(|error| {
        AppError::validation("大模型返回的规则格式无效").with_detail(error.to_string())
    })?;

    if rules.is_empty() {
        return Err(AppError::validation("大模型未生成任何规则"));
    }
    if rules.len() > 12 {
        return Err(AppError::validation("大模型生成的规则过多，请缩小需求范围"));
    }

    for (index, rule) in rules.iter_mut().enumerate() {
        rule.name = rule.name.trim().to_string();
        rule.pattern = rule.pattern.trim().to_string();
        if rule.name.is_empty() || rule.pattern.is_empty() {
            return Err(AppError::validation(format!(
                "第 {} 条规则缺少名称或正则表达式",
                index + 1
            )));
        }
        regex::Regex::new(&rule.pattern).map_err(|error| {
            AppError::validation(format!("第 {} 条规则的正则表达式无效", index + 1))
                .with_detail(error.to_string())
        })?;
    }

    Ok(rules)
}

fn build_optimize_resume_with_answer_prompt(request: &OptimizeWithAnswerRequest) -> String {
    format!(
        r#"你是一位资深简历精修专家。候选人针对简历中的某项缺陷回答了面试官的追问。请将他回答中包含的有效信息（技术细节、行动步骤、可量化的数据结果）重构融进简历对应章节中。

原简历内容：
{resume_content}

面试提问：
{question}

候选人的回答：
{user_answer}

关联优化章节：
{section_title}

优化及重构要求：
1. 提取回答中的闪光点，将其提炼为符合“STAR原则”（情境-任务-行动-结果）的描述。
2. 保持简历专业、简洁的学术风格，用词要精确（如“负责、主导、重构、优化”）。
3. 只输出优化重构后的【整个章节】（必须包含原标题，如 ## {section_title}）的 Markdown 文本，原简历其他章节无需输出。
4. 禁止输出任何解释、引导语、注释或包裹 Markdown 块标记。

输出格式示例：
## {section_title}
- 优化后的内容1...
- 优化后的内容2..."#,
        resume_content = request.resume_content,
        question = request.question,
        user_answer = request.user_answer,
        section_title = request.section_title
    )
}

fn parse_predicted_questions(raw: &str) -> Result<Vec<PredictedQuestion>, String> {
    let cleaned = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let questions: Vec<PredictedQuestion> =
        serde_json::from_str(cleaned).map_err(|error| format!("解析预测问题失败：{error}"))?;

    if questions.is_empty() {
        return Err("预测问题为空".to_string());
    }

    Ok(questions)
}

#[cfg(test)]
mod tests {
    use super::{
        build_job_filter_rules_prompt, build_optimize_resume_with_answer_prompt,
        build_predict_resume_questions_prompt, parse_generated_job_filter_rules,
        OptimizeWithAnswerRequest,
    };

    #[test]
    fn predict_resume_questions_prompt_requires_json_array() {
        let prompt = build_predict_resume_questions_prompt("## 项目经历\n- 做过网关限流");

        assert!(prompt.contains("挑剔且经验丰富的技术面试官"));
        assert!(prompt.contains("只输出一个合法的 JSON 数组"));
        assert!(prompt.contains("\"target_section\""));
        assert!(prompt.contains("## 项目经历"));
    }

    #[test]
    fn optimize_resume_with_answer_prompt_includes_answer_and_section() {
        let request = OptimizeWithAnswerRequest {
            resume_content: "## 项目经历\n- 负责网关".to_string(),
            question: "你们为什么选择令牌桶？".to_string(),
            user_answer: "峰值 QPS 3000，令牌桶允许突发流量。".to_string(),
            section_title: "项目经历".to_string(),
        };

        let prompt = build_optimize_resume_with_answer_prompt(&request);

        assert!(prompt.contains("资深简历精修专家"));
        assert!(prompt.contains("你们为什么选择令牌桶？"));
        assert!(prompt.contains("峰值 QPS 3000"));
        assert!(prompt.contains("## 项目经历"));
        assert!(prompt.contains("只输出优化重构后的【整个章节】"));
    }

    #[test]
    fn job_filter_prompt_requires_structured_rust_regex_rules() {
        let prompt = build_job_filter_rules_prompt("只看 Java，排除外包和驻场");

        assert!(prompt.contains("Rust regex crate"));
        assert!(prompt.contains("只看 Java，排除外包和驻场"));
        assert!(prompt.contains("\"target\""));
        assert!(prompt.contains("\"mode\""));
    }

    #[test]
    fn generated_job_filter_rules_are_parsed_and_validated() {
        let rules = parse_generated_job_filter_rules(
            r#"```json
[{"name":"排除外包","pattern":"外包|驻场","target":"Description","mode":"REJECT"}]
```"#,
        )
        .unwrap();

        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].name, "排除外包");
        assert_eq!(rules[0].pattern, "外包|驻场");
    }

    #[test]
    fn generated_job_filter_rules_reject_invalid_regex() {
        let error = parse_generated_job_filter_rules(
            r#"[{"name":"错误规则","pattern":"(?=Java)","target":"Title","mode":"ACCEPT"}]"#,
        )
        .unwrap_err();

        assert!(error.message.contains("正则表达式无效"));
    }
}
