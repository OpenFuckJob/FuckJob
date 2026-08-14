//! 求职链路上的具体 Agent 任务。
//!
//! 每个任务只描述「给什么上下文、要什么结果、什么算合格」，
//! 模型调用、重试、净化统一由 [`crate::agent::run`] 负责。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agent::output;
use crate::agent::prompts;
use crate::agent::run::AgentTask;
use crate::config::{AppRuntimeConfig, RegexRule};
use crate::error::AppError;
use crate::llm::template;
use crate::llm::JobSemanticMatch;
use crate::rpa::common::RpaJob;
use crate::rpa::conversation::{ConversationContext, ReplyDecision, ResumeState};

/// 上下文缺失时的统一占位。写成人话而不是空串，模型才分得清
/// 「这一项没有」和「这一项被刻意留白」
const MISSING: &str = "（未提供）";

/// 岗位描述、简历这类长文本的截断上限。
/// 超过这个量对判断几乎没有增量帮助，却会明显推高延迟与额度消耗
const LONG_TEXT_LIMIT: usize = 8_000;

fn clip(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

/// 简历注入是用户可关的开关，关掉时要显式告诉模型「没有」，
/// 而不是留下一个空变量让它自由发挥
fn resume_text(config: &AppRuntimeConfig) -> String {
    if !config.resume_config.inject_llm_context {
        return MISSING.to_string();
    }
    let content = config
        .resume_config
        .resume_content
        .as_deref()
        .unwrap_or_default()
        .trim();
    if content.is_empty() {
        MISSING.to_string()
    } else {
        clip(content, LONG_TEXT_LIMIT)
    }
}

// ================================
// 自动回复决策
// ================================

/// 判断这一轮该怎么处置，并写出要发送的内容。
///
/// 决策和写作合在一次调用里，而不是先判断再生成：分两次会让「决定投简历」
/// 和「写出的话」互相脱节，模型也会因为看不到自己写了什么而给出前后不一的理由。
pub struct ReplyDecisionTask<'a> {
    config: &'a AppRuntimeConfig,
    context: &'a ConversationContext,
}

impl<'a> ReplyDecisionTask<'a> {
    pub fn new(config: &'a AppRuntimeConfig, context: &'a ConversationContext) -> Self {
        Self { config, context }
    }
}

fn resume_state_hint(state: ResumeState) -> &'static str {
    match state {
        ResumeState::Sendable => "简历入口可用，允许主动投递。",
        ResumeState::RequestedByPeer => "对方正在索要简历，系统会自动同意，你不需要再选投递动作。",
        ResumeState::Unavailable => "简历已投递、或平台要求先等对方回复，不要选投递动作。",
        ResumeState::Unknown => "无法确认简历入口状态，不要选投递动作。",
    }
}

impl AgentTask for ReplyDecisionTask<'_> {
    type Output = ReplyDecision;

    fn name(&self) -> &'static str {
        "自动回复决策"
    }

    fn prompt_template(&self) -> Result<String, AppError> {
        self.config
            .replay_config
            .reply_prompt
            .as_deref()
            .map(str::trim)
            .filter(|prompt| !prompt.is_empty())
            .map(str::to_string)
            .ok_or_else(|| AppError::configuration("尚未配置自动回复提示词"))
    }

    fn params(&self) -> Result<Value, AppError> {
        let job = self.context.job.as_ref();
        let mut params = json!({
            "chat_history": self.context.transcript(),
            "chat_context": self.context.transcript(),
            "message_content": self
                .context
                .last_received()
                .map(|message| message.text.clone())
                .unwrap_or_default(),
            "job_description": job
                .map(|job| clip(&job.detail, LONG_TEXT_LIMIT))
                .unwrap_or_default(),
            "job_content": job
                .map(|job| format!("{}｜{}｜{}", job.title, job.company_name, job.salary))
                .unwrap_or_default(),
            "resume": resume_text(self.config),
            "resume_context": resume_text(self.config),
            "background_context": self
                .config
                .replay_config
                .background_context
                .clone()
                .unwrap_or_default(),
        });

        // 用户模板引用了哪些变量无法预知，一律补全。漏一个就是整条链路静默失效
        template::fill_missing(&mut params, MISSING);
        Ok(params)
    }

    fn build_prompt(&self) -> Result<String, AppError> {
        // 用户模板必须先单独渲染完再拼外壳：它的渲染结果里可能含花括号，
        // 交给外层渲染会被当成占位符而报错
        let body = template::render(&self.prompt_template()?, &self.params()?)?;
        // 共享片段与骨架记号先替换掉；业务变量在上一行已经渲染完，两层互不干扰
        let frame = prompts::compose(
            &prompts::with_shared(prompts::REPLY_DECISION_FRAME),
            &[("RESUME_STATE", resume_state_hint(self.context.resume_state))],
        );
        Ok(format!("{body}{frame}"))
    }

    fn parse(&self, raw: &str) -> Result<Self::Output, String> {
        let json = output::extract_json(raw).ok_or("输出里没有找到 JSON 对象")?;
        serde_json::from_str::<ReplyDecision>(json)
            .map_err(|error| format!("JSON 结构不符合要求：{error}"))
    }

    fn validate(&self, decision: &Self::Output) -> Result<(), String> {
        if decision.confidence > 100 {
            return Err("confidence 必须在 0 到 100 之间".to_string());
        }
        if !decision.action.needs_text() {
            return Ok(());
        }

        let reply = decision.reply.trim();
        if reply.is_empty() {
            return Err("action 需要发送内容，但 reply 是空的".to_string());
        }
        if output::looks_like_refusal(reply) {
            return Err("reply 是拒答话术，不能发给招聘方，请直接写出可发送的正文".to_string());
        }
        if output::has_placeholder(reply) {
            return Err(
                "reply 里残留了未填充的占位符，请用真实信息或模糊但自然的表述替换".to_string(),
            );
        }
        Ok(())
    }
}

// ================================
// 打招呼
// ================================

/// 生成岗位打招呼开场白。纯文本输出，但同样要过占位符与拒答体检
pub struct GreetTask<'a> {
    config: &'a AppRuntimeConfig,
    job: &'a RpaJob,
}

impl<'a> GreetTask<'a> {
    pub fn new(config: &'a AppRuntimeConfig, job: &'a RpaJob) -> Self {
        Self { config, job }
    }
}

impl AgentTask for GreetTask<'_> {
    type Output = String;

    fn name(&self) -> &'static str {
        "打招呼生成"
    }

    fn prompt_template(&self) -> Result<String, AppError> {
        self.config
            .greet_config
            .reply_prompt
            .as_deref()
            .map(str::trim)
            .filter(|prompt| !prompt.is_empty())
            .map(str::to_string)
            .ok_or_else(|| AppError::configuration("尚未配置打招呼提示词"))
    }

    fn params(&self) -> Result<Value, AppError> {
        let mut params = json!({
            "job_content": serde_json::to_string(self.job).unwrap_or_default(),
            "job_description": clip(&self.job.detail, LONG_TEXT_LIMIT),
            "resume": resume_text(self.config),
            "resume_context": resume_text(self.config),
        });

        template::fill_missing(&mut params, MISSING);
        Ok(params)
    }

    fn parse(&self, raw: &str) -> Result<Self::Output, String> {
        Ok(raw.trim().to_string())
    }

    fn validate(&self, text: &Self::Output) -> Result<(), String> {
        if text.is_empty() {
            return Err("没有生成任何内容".to_string());
        }
        if output::looks_like_refusal(text) {
            return Err("生成的是拒答话术，请直接写出可发送的打招呼正文".to_string());
        }
        if output::has_placeholder(text) {
            return Err(
                "内容里残留了未填充的占位符。简历信息不足时请用「有相关经验」这类模糊但自然的说法，不要写 X 年、XX 公司".to_string(),
            );
        }
        Ok(())
    }
}

// ================================
// 岗位语义复核
// ================================

/// 在关键词和正则规则通过后，复核岗位是否真的符合投递意图。
///
/// 提示词是内置的、不开放配置：这道关卡的作用是防误投，
/// 让用户改判定标准等于把关卡本身关掉。
pub struct JobMatchTask<'a> {
    config: &'a AppRuntimeConfig,
    job: &'a RpaJob,
}

impl<'a> JobMatchTask<'a> {
    pub fn new(config: &'a AppRuntimeConfig, job: &'a RpaJob) -> Self {
        Self { config, job }
    }
}

impl AgentTask for JobMatchTask<'_> {
    type Output = JobSemanticMatch;

    fn name(&self) -> &'static str {
        "岗位语义复核"
    }

    fn prompt_template(&self) -> Result<String, AppError> {
        Ok(prompts::with_shared(prompts::JOB_MATCH))
    }

    fn params(&self) -> Result<Value, AppError> {
        Ok(json!({}))
    }

    fn build_prompt(&self) -> Result<String, AppError> {
        let intent = self
            .config
            .job_filter_config
            .semantic_filter_intent
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| AppError::configuration("已启用 AI 岗位复核，但未填写目标岗位要求"))?;

        let payload = json!({
            "target_intent": intent,
            "job": {
                "title": self.job.title,
                "company": self.job.company_name,
                "description": clip(&self.job.detail, LONG_TEXT_LIMIT),
            },
            "resume": resume_text(self.config),
        });

        Ok(prompts::compose(
            &self.prompt_template()?,
            &[(
                "PAYLOAD",
                &serde_json::to_string(&payload).map_err(|error| {
                    AppError::internal("岗位复核数据序列化失败").with_detail(error.to_string())
                })?,
            )],
        ))
    }

    fn parse(&self, raw: &str) -> Result<Self::Output, String> {
        let json = output::extract_json(raw).ok_or("输出里没有找到 JSON 对象")?;
        let mut result: JobSemanticMatch =
            serde_json::from_str(json).map_err(|error| format!("JSON 结构不符合要求：{error}"))?;

        // 阈值在本地兜底判定，不信任模型自己声明的 matched：
        // 实测存在给出 60 分却仍然 matched=true 的情况
        result.matched = result.matched && result.score >= 75;
        if result.reason.trim().is_empty() {
            result.reason = "AI 未提供判断理由".to_string();
        }
        Ok(result)
    }
}

// ================================
// 现场拼装提示词的通用任务
// ================================

/// 模板与变量由调用方现场给出的通用任务。
///
/// 给那些提示词随输入拼装、且不需要结构化校验的用途用（例如岗位面试分析）。
/// 它们照样能拿到统一的流式采集、重试降级和输出净化——此前这些用途各自
/// 直连服务，模型吐的 `<think>` 会原样进结果。
pub struct TemplateTask<'a> {
    name: &'static str,
    template: &'a str,
    params: Value,
}

impl<'a> TemplateTask<'a> {
    pub fn new(name: &'static str, template: &'a str, params: Value) -> Self {
        Self {
            name,
            template,
            params,
        }
    }
}

impl AgentTask for TemplateTask<'_> {
    type Output = String;

    fn name(&self) -> &'static str {
        self.name
    }

    fn prompt_template(&self) -> Result<String, AppError> {
        Ok(self.template.to_string())
    }

    fn params(&self) -> Result<Value, AppError> {
        let mut params = self.params.clone();
        template::fill_missing(&mut params, MISSING);
        Ok(params)
    }

    fn parse(&self, raw: &str) -> Result<Self::Output, String> {
        let text = raw.trim();
        if text.is_empty() {
            return Err("没有生成任何内容".to_string());
        }
        Ok(text.to_string())
    }
}

// ================================
// 岗位筛选规则生成
// ================================

/// 把自然语言需求转成正则筛选规则。
///
/// 校验放在这里而不是调用方：正则编译不过、条数超限这些都能作为返工理由
/// 发回给模型再来一轮，比直接把错误抛给用户有用得多。
pub struct JobFilterRulesTask<'a> {
    requirement: &'a str,
}

impl<'a> JobFilterRulesTask<'a> {
    pub fn new(requirement: &'a str) -> Self {
        Self { requirement }
    }
}

const MAX_GENERATED_RULES: usize = 12;

impl AgentTask for JobFilterRulesTask<'_> {
    type Output = Vec<RegexRule>;

    fn name(&self) -> &'static str {
        "岗位筛选规则生成"
    }

    fn prompt_template(&self) -> Result<String, AppError> {
        Ok(build_job_filter_rules_prompt(self.requirement))
    }

    fn params(&self) -> Result<Value, AppError> {
        Ok(json!({}))
    }

    fn build_prompt(&self) -> Result<String, AppError> {
        // 需求原文由用户输入，可能带花括号，不能过渲染
        self.prompt_template()
    }

    fn parse(&self, raw: &str) -> Result<Self::Output, String> {
        let json = output::extract_json_array(raw).ok_or("输出里没有找到 JSON 数组")?;
        serde_json::from_str(json).map_err(|error| format!("JSON 结构不符合要求：{error}"))
    }

    fn validate(&self, rules: &Self::Output) -> Result<(), String> {
        if rules.is_empty() {
            return Err("没有生成任何规则".to_string());
        }
        if rules.len() > MAX_GENERATED_RULES {
            return Err(format!(
                "生成了 {} 条规则，超过上限 {MAX_GENERATED_RULES} 条，请合并同类规则",
                rules.len()
            ));
        }

        for (index, rule) in rules.iter().enumerate() {
            let position = index + 1;
            if rule.name.trim().is_empty() || rule.pattern.trim().is_empty() {
                return Err(format!("第 {position} 条规则缺少名称或正则表达式"));
            }
            // Rust 的 regex 不支持前瞻后顾，模型很容易写出 PCRE 语法
            if let Err(error) = regex::Regex::new(rule.pattern.trim()) {
                return Err(format!(
                    "第 {position} 条规则的正则表达式无法编译（{error}），请改用 Rust regex 支持的语法，不要使用前瞻、后顾和反向引用"
                ));
            }
        }
        Ok(())
    }
}

fn build_job_filter_rules_prompt(requirement: &str) -> String {
    prompts::compose(
        &prompts::with_shared(prompts::JOB_FILTER_RULES),
        &[
            ("REQUIREMENT", requirement),
            ("MAX_RULES", &MAX_GENERATED_RULES.to_string()),
        ],
    )
}

// ================================
// 简历追问预测
// ================================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PredictedQuestion {
    pub id: i64,
    pub question: String,
    pub intent: String,
    pub target_section: String,
}

/// 从简历里找薄弱点并预测面试追问
pub struct ResumeQuestionsTask<'a> {
    resume_content: &'a str,
}

impl<'a> ResumeQuestionsTask<'a> {
    pub fn new(resume_content: &'a str) -> Self {
        Self { resume_content }
    }
}

impl AgentTask for ResumeQuestionsTask<'_> {
    type Output = Vec<PredictedQuestion>;

    fn name(&self) -> &'static str {
        "简历追问预测"
    }

    fn prompt_template(&self) -> Result<String, AppError> {
        Ok(prompts::compose(
            &prompts::with_shared(prompts::RESUME_QUESTIONS),
            &[("RESUME", self.resume_content)],
        ))
    }

    fn params(&self) -> Result<Value, AppError> {
        Ok(json!({}))
    }

    fn build_prompt(&self) -> Result<String, AppError> {
        // 简历原文里的花括号不能被当成占位符
        self.prompt_template()
    }

    fn parse(&self, raw: &str) -> Result<Self::Output, String> {
        let json = output::extract_json_array(raw).ok_or("输出里没有找到 JSON 数组")?;
        serde_json::from_str(json).map_err(|error| format!("JSON 结构不符合要求：{error}"))
    }

    fn validate(&self, questions: &Self::Output) -> Result<(), String> {
        if questions.is_empty() {
            return Err("没有预测出任何问题".to_string());
        }
        Ok(())
    }
}

// ================================
// 简历定向优化
// ================================

#[derive(Debug, Deserialize)]
pub struct OptimizeWithAnswerRequest {
    pub resume_content: String,
    pub question: String,
    pub user_answer: String,
    pub section_title: String,
}

/// 把候选人对追问的口头回答重构进简历对应章节
pub struct ResumeOptimizeTask<'a> {
    request: &'a OptimizeWithAnswerRequest,
}

impl<'a> ResumeOptimizeTask<'a> {
    pub fn new(request: &'a OptimizeWithAnswerRequest) -> Self {
        Self { request }
    }
}

impl AgentTask for ResumeOptimizeTask<'_> {
    type Output = String;

    fn name(&self) -> &'static str {
        "简历定向优化"
    }

    fn prompt_template(&self) -> Result<String, AppError> {
        let request = self.request;
        Ok(prompts::compose(
            &prompts::with_shared(prompts::RESUME_OPTIMIZE),
            &[
                ("RESUME", &request.resume_content),
                ("QUESTION", &request.question),
                ("ANSWER", &request.user_answer),
                ("SECTION", &request.section_title),
            ],
        ))
    }

    fn params(&self) -> Result<Value, AppError> {
        Ok(json!({}))
    }

    fn build_prompt(&self) -> Result<String, AppError> {
        // 简历与回答都是用户原文，花括号不能被当成占位符
        self.prompt_template()
    }

    fn parse(&self, raw: &str) -> Result<Self::Output, String> {
        let text = raw.trim();
        if text.is_empty() {
            return Err("没有生成任何内容".to_string());
        }
        Ok(text.to_string())
    }

    fn validate(&self, text: &Self::Output) -> Result<(), String> {
        if !text.contains(self.request.section_title.trim()) {
            return Err(format!(
                "输出必须包含章节标题「{}」，请连标题一起输出整个章节",
                self.request.section_title.trim()
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_app_config;
    use crate::rpa::common::ChatMessage;
    use crate::rpa::conversation::ReplyAction;
    use crate::rpa::run_flow::PlatformKind;

    fn context(resume_state: ResumeState) -> ConversationContext {
        ConversationContext {
            platform: PlatformKind::Boss,
            conversation_id: "c1".to_string(),
            job: None,
            messages: vec![ChatMessage {
                mid: 1,
                received: true,
                text: "方便发一份简历吗".to_string(),
                time: 1,
                from_name: "招聘者".to_string(),
            }],
            resume_state,
        }
    }

    /// `default_app_config` 出厂时不带提示词（要走 app_config.yaml 才有），
    /// 这里补上一份引用了全部变量的模板，正是为了验证补全逻辑
    fn config_with_reply_prompt() -> AppRuntimeConfig {
        let mut config = default_app_config();
        config.replay_config.reply_prompt = Some(
            "【岗位】{{job_description}}\n【简历】{{resume}}\n【补充】{{background_context}}\n【对话】{{chat_history}}"
                .to_string(),
        );
        config
    }

    fn decision(action: ReplyAction, reply: &str, confidence: u8) -> ReplyDecision {
        ReplyDecision {
            action,
            reply: reply.to_string(),
            reason: "测试".to_string(),
            confidence,
        }
    }

    /// 没有岗位归属、没开简历注入、背景为空——猎聘的典型情况。
    /// 这个组合此前会让渲染直接报错，进而整条链路静默失效
    #[test]
    fn prompt_builds_even_with_no_job_no_resume_and_no_background() {
        let mut config = config_with_reply_prompt();
        config.resume_config.inject_llm_context = false;
        config.replay_config.background_context = None;
        let context = context(ResumeState::Sendable);

        let prompt = ReplyDecisionTask::new(&config, &context)
            .build_prompt()
            .expect("上下文全缺时也必须能组装出提示词");

        assert!(prompt.contains("（未提供）"));
        assert!(prompt.contains("简历入口可用"));
        assert!(!prompt.contains("{{"));
    }

    #[test]
    fn prompt_tells_the_model_when_the_resume_entry_is_unusable() {
        let config = config_with_reply_prompt();
        let context = context(ResumeState::Unavailable);

        let prompt = ReplyDecisionTask::new(&config, &context)
            .build_prompt()
            .unwrap();

        assert!(prompt.contains("不要选投递动作"));
    }

    #[test]
    fn decision_json_survives_surrounding_prose() {
        let config = config_with_reply_prompt();
        let context = context(ResumeState::Sendable);
        let task = ReplyDecisionTask::new(&config, &context);

        let parsed = task
            .parse(r#"好的，我的判断是：{"action":"reply","reply":"您好","reason":"常规询问","confidence":80} 以上。"#)
            .unwrap();

        assert_eq!(parsed.action, ReplyAction::Reply);
        assert_eq!(parsed.reply, "您好");
    }

    #[test]
    fn refusal_and_placeholder_replies_are_rejected() {
        let config = config_with_reply_prompt();
        let context = context(ResumeState::Sendable);
        let task = ReplyDecisionTask::new(&config, &context);

        assert!(task
            .validate(&decision(
                ReplyAction::Reply,
                "作为一个AI，我无法代替你回复",
                90
            ))
            .is_err());
        assert!(task
            .validate(&decision(ReplyAction::Reply, "我有X年相关经验", 90))
            .is_err());
        assert!(task
            .validate(&decision(ReplyAction::Reply, "您好，我有相关项目经验", 90))
            .is_ok());
    }

    /// skip / escalate 本来就不发内容，不该因为 reply 为空被判不合格
    #[test]
    fn skip_and_escalate_do_not_require_reply_text() {
        let config = config_with_reply_prompt();
        let context = context(ResumeState::Sendable);
        let task = ReplyDecisionTask::new(&config, &context);

        assert!(task.validate(&decision(ReplyAction::Skip, "", 70)).is_ok());
        assert!(task
            .validate(&decision(ReplyAction::Escalate, "", 60))
            .is_ok());
    }

    // ---- 以下覆盖原先在 command/llm.rs 里的提示词与解析测试 ----

    #[test]
    fn resume_questions_prompt_carries_its_key_constraints() {
        let prompt = ResumeQuestionsTask::new(
            "## 项目经历
- 做过网关限流",
        )
        .prompt_template()
        .unwrap();

        assert!(prompt.contains("挑剔且经验丰富的技术面试官"));
        assert!(prompt.contains("仅输出合法 JSON"));
        assert!(prompt.contains("\"target_section\""));
        assert!(prompt.contains("## 项目经历"));
        assert!(!prompt.contains("{{__"), "记号必须已被替换：{prompt}");
    }

    #[test]
    fn resume_optimize_prompt_includes_answer_and_section() {
        let request = OptimizeWithAnswerRequest {
            resume_content: "## 项目经历
- 负责网关"
                .to_string(),
            question: "你们为什么选择令牌桶？".to_string(),
            user_answer: "峰值 QPS 3000，令牌桶允许突发流量。".to_string(),
            section_title: "项目经历".to_string(),
        };

        let prompt = ResumeOptimizeTask::new(&request).prompt_template().unwrap();

        assert!(prompt.contains("峰值 QPS 3000"));
        assert!(prompt.contains("## 项目经历"));
        assert!(prompt.contains("STAR"));
        assert!(!prompt.contains("{{__"));
    }

    #[test]
    fn job_filter_rules_prompt_states_the_output_contract() {
        let prompt = JobFilterRulesTask::new("只要上海的 Rust 岗位，排除外包")
            .build_prompt()
            .unwrap();

        assert!(prompt.contains("只要上海的 Rust 岗位，排除外包"));
        assert!(prompt.contains("禁止使用前瞻、后顾和反向引用"));
        assert!(prompt.contains("最多输出 12 条"));
        assert!(!prompt.contains("{{__"));
    }

    fn rule(name: &str, pattern: &str) -> RegexRule {
        RegexRule {
            name: name.to_string(),
            pattern: pattern.to_string(),
            target: crate::config::MatchTarget::All,
            mode: crate::config::RuleMode::REJECT,
        }
    }

    #[test]
    fn generated_rules_must_be_parseable_non_empty_and_bounded() {
        let task = JobFilterRulesTask::new("需求");

        assert!(task.parse("这不是 JSON").is_err());
        assert!(task.validate(&Vec::new()).is_err());
        assert!(task
            .validate(&(0..13).map(|i| rule(&format!("r{i}"), "x")).collect())
            .is_err());
        assert!(task.validate(&vec![rule("排除外包", "外包|驻场")]).is_ok());
    }

    /// Rust 的 regex 不支持前瞻，模型很爱写 PCRE 语法。
    /// 返工理由必须点明这一点，否则重试还是同样的错
    #[test]
    fn an_uncompilable_pattern_explains_the_rust_regex_limitation() {
        let error = JobFilterRulesTask::new("需求")
            .validate(&vec![rule("前瞻", "(?=外包)")])
            .unwrap_err();

        assert!(error.contains("前瞻"), "实际：{error}");
    }

    #[test]
    fn job_match_enforces_the_score_threshold_locally() {
        let config = default_app_config();
        let job = RpaJob {
            platform: PlatformKind::Boss,
            platform_job_id: "1".to_string(),
            title: "Rust 工程师".to_string(),
            company_name: "示例".to_string(),
            detail: "JD".to_string(),
            salary: "20-30k".to_string(),
            location: None,
            detail_url: String::new(),
        };
        let task = JobMatchTask::new(&config, &job);

        let result = task
            .parse(r#"{"matched":true,"score":60,"reason":"方向不一致"}"#)
            .unwrap();

        assert!(!result.matched, "模型自称匹配但分数不够时必须本地否决");
    }
}
