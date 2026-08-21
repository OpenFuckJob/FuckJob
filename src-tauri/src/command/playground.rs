//! 测试模式：把整条求职链路拆成可以逐个观察的环节。
//!
//! 这条链路上 LLM 环节和确定性环节交替出现，出问题时用户只看得到「没投出去」
//! 这一个结果，却分不清是正则把岗位挡了、模型判断不该投、还是内容没过发送前体检。
//! 于是每个环节在这里都单独执行、单独出一条 [`StepResult`]，链路在哪一步终止一目了然。
//!
//! **这些命令不复制任何判断逻辑**：筛选走 [`crate::verify::filter_decision`]，
//! 打招呼走 [`GreetTask`] 与 [`compose_greet_resources`]，回复走
//! [`conversation`] 里的闸门、路由、校正与体检——和真实运行调的是同一批函数。
//! 此前的调试入口自己拼了一套参数（还漏了 `chat_history`），调试看到的效果
//! 和真跑出来的根本不是一回事，那个坑不能再踩第二次。
//!
//! 提示词覆盖只作用在配置的内存副本上。`AgentTask::prompt_template()` 本来就从
//! 配置里读，改副本等于换了提示词，既不需要给任何 AgentTask 加参数，
//! 也不会污染磁盘：调试期的试错不该有任何一次意外落盘。

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agent::tasks::{GreetTask, JobMatchTask, ReplyDecisionTask};
use crate::agent::trace::{self, AgentTrace};
use crate::agent::AgentRunner;
use crate::command::base::CommandResult;
use crate::config::AppRuntimeConfig;
use crate::dao::model::JobDetail;
use crate::error::AppError;
use crate::rpa::common::{ChatMessage, RpaJob};
use crate::rpa::conversation::{
    self, ConversationContext, GateVerdict, GreetAction, GreetDecision, OutboundKind, ReplyAction,
    ReplyDecision, ReplyLimits, ReplyRoute, ResumeState, SendVerdict,
};
use crate::rpa::greet::compose_greet_resources;
use crate::rpa::run_flow::PlatformKind;
use crate::verify::FilterDecision;

/// 链路上的一个环节
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// ① 正则与关键词筛选
    RegexFilter,
    /// ② 岗位语义复核
    SemanticMatch,
    /// ③ 打招呼决策
    GreetDecide,
    /// ④ 打招呼发送序列组装
    GreetCompose,
    /// ⑤ 打招呼发送前体检
    GreetVet,
    /// ⑥ 回复闸门
    Gate,
    /// ⑦ 回复路由（模板 vs 模型）
    Route,
    /// ⑧ 自动回复决策
    ReplyDecide,
    /// ⑨ 投递意图校正
    Reconcile,
    /// ⑩ 回复发送前体检
    ReplyVet,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outcome {
    /// 环节通过，链路继续
    Pass,
    /// 环节拦截，链路终止。reason 要写成用户看得懂的人话
    Block { reason: String },
    /// 环节不适用或被跳过，链路可能继续也可能终止
    Skip { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub stage: Stage,
    pub outcome: Outcome,
    /// 该环节的结构化产物，前端按 stage 决定怎么渲染
    pub detail: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepReport {
    pub steps: Vec<StepResult>,
    /// 本次链路上产生的 Agent 轨迹 id，按发生顺序
    pub trace_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlaygroundJob {
    pub title: String,
    pub company_name: String,
    pub detail: String,
    pub salary: String,
    pub location: String,
}

/// 只在本次调用内生效的提示词覆盖，不落盘。
/// 这是测试模式的核心能力：改提示词立刻重跑，满意了再由前端走正常保存流程写回方案
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PromptOverrides {
    pub greet_prompt: Option<String>,
    pub reply_prompt: Option<String>,
    pub semantic_filter_intent: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlaygroundMessage {
    pub text: String,
    /// true = HR 发来的，false = 我发出去的
    pub received: bool,
}

/// 手输岗位和手造会话在库里都没有身份，统一给一个固定标识。
///
/// 用固定值而不是随机 id：这些数据永远不会落库，随机 id 只会让日志更难读
const PLAYGROUND_ID: &str = "playground";

/// 允许模型自主投递简历所需的最低把握。
///
/// 与 [`conversation::reconcile`] 里的阈值一致。这里复制的只是**用于解释**的数字，
/// 降级判定本身仍然只有 `reconcile` 一处——见 [`downgrade_reason`]
const MIN_RESUME_CONFIDENCE: u8 = 70;

impl StepReport {
    fn new() -> Self {
        Self {
            steps: Vec::new(),
            trace_ids: Vec::new(),
        }
    }

    fn push(&mut self, stage: Stage, outcome: Outcome, detail: Value) {
        self.steps.push(StepResult {
            stage,
            outcome,
            detail,
        });
    }

}

// ================================
// 公共前置
// ================================

/// 取配置 → 解析方案 → 把本次调用的提示词覆盖写进内存副本。
///
/// 走 `resolve_job_profile` 而不是直接用顶层配置：真实运行是按方案卡跑的，
/// 调试要是读了顶层镜像，用户在方案里改的提示词根本不会生效，
/// 「调试通过了但真跑还是老样子」比没有调试页更误导人
fn prepare_config(
    app_handle: tauri::AppHandle,
    profile_id: Option<String>,
    overrides: &PromptOverrides,
) -> Result<AppRuntimeConfig, AppError> {
    let config = crate::config::load_app_config_inner(app_handle)?;
    let resolved = crate::config::resolve_job_profile(&config, profile_id.as_deref())
        .map_err(AppError::configuration)?;

    let mut config = resolved.config;
    apply_overrides(&mut config, overrides);
    Ok(config)
}

/// 覆盖只改这三处提示词，其余配置原样保留。
///
/// 给 `None` 表示「用方案里存着的那份」，而不是「清空」——测试模式的常见用法
/// 是只调一个环节的提示词、其它环节保持现状对照，清空会让对照失去意义
fn apply_overrides(config: &mut AppRuntimeConfig, overrides: &PromptOverrides) {
    if let Some(prompt) = overrides.greet_prompt.clone() {
        config.greet_config.reply_prompt = Some(prompt);
    }
    if let Some(prompt) = overrides.reply_prompt.clone() {
        config.replay_config.reply_prompt = Some(prompt);
    }
    if let Some(intent) = overrides.semantic_filter_intent.clone() {
        config.job_filter_config.semantic_filter_intent = Some(intent);
    }
}

fn to_rpa_job(job: &PlaygroundJob) -> RpaJob {
    RpaJob {
        platform: PlatformKind::Boss,
        platform_job_id: PLAYGROUND_ID.to_string(),
        title: job.title.clone(),
        company_name: job.company_name.clone(),
        detail: job.detail.clone(),
        salary: job.salary.clone(),
        location: Some(job.location.clone()),
        // 测试模式里的岗位由用户手工构造，没有来源页面可提供招聘者活跃时间。
        recruiter_active_time: None,
        // 手输岗位没有来源页面。留空而不是编一个 URL：任何一处误把它当真实链接
        // 点开都是错的
        detail_url: String::new(),
    }
}

fn to_job_detail(job: &PlaygroundJob) -> JobDetail {
    JobDetail {
        id: PLAYGROUND_ID.to_string(),
        platform: "boss".to_string(),
        source_task_id: None,
        profile_id: None,
        profile_name: None,
        profile_snapshot_id: None,
        title: job.title.clone(),
        company_name: job.company_name.clone(),
        detail: job.detail.clone(),
        salary: job.salary.clone(),
        location: Some(job.location.clone()),
        is_reply: false,
        is_send_resume: false,
        created_at: String::new(),
        resume_sent_at: None,
        updated_at: String::new(),
    }
}

/// 用数组下标当 mid 和时间戳：手造的对话本来就只有先后顺序，没有真实时间。
/// 下标保证了 [`conversation::merge_messages`] 那套按 (time, mid) 排序的口径依然成立
fn to_chat_messages(messages: &[PlaygroundMessage]) -> Vec<ChatMessage> {
    messages
        .iter()
        .enumerate()
        .map(|(index, message)| ChatMessage {
            mid: index as i64,
            received: message.received,
            text: message.text.clone(),
            time: index as i64,
            from_name: if message.received { "HR" } else { "我" }.to_string(),
        })
        .collect()
}

// ================================
// ①② 岗位筛选
// ================================

/// 手输岗位过一遍筛选：正则关键词 + 可选的 AI 语义复核
#[tauri::command]
pub async fn playground_screen(
    app_handle: tauri::AppHandle,
    profile_id: Option<String>,
    job: PlaygroundJob,
    overrides: PromptOverrides,
) -> CommandResult<StepReport> {
    let config = match prepare_config(app_handle, profile_id, &overrides) {
        Ok(config) => config,
        Err(error) => return CommandResult::err(error),
    };

    let rpa_job = to_rpa_job(&job);
    let mut report = StepReport::new();

    let decision = crate::verify::filter_decision(&rpa_job, &config);
    let matched = decision.matched;
    report.steps.push(regex_filter_step(&decision));
    if !matched {
        return CommandResult::ok(report);
    }

    if !config.job_filter_config.enable_semantic_filter {
        report.push(
            Stage::SemanticMatch,
            Outcome::Skip {
                reason: "方案未启用 AI 岗位复核".to_string(),
            },
            json!({}),
        );
        return CommandResult::ok(report);
    }

    // 先拿号再跑：失败时 `AgentOutcome` 根本不会构造出来，
    // 而模型报错恰恰是最需要翻提示词原文的时候
    let trace_id = trace::next_id();
    report.trace_ids.push(trace_id.clone());
    match AgentRunner::new(&config)
        .with_trace_id(trace_id)
        .run(&JobMatchTask::new(&config, &rpa_job))
        .await
    {
        Ok(outcome) => {
            let result = outcome.output;
            let outcome = if result.matched {
                Outcome::Pass
            } else {
                Outcome::Block {
                    reason: result.reason.clone(),
                }
            };
            report.push(
                Stage::SemanticMatch,
                outcome,
                json!({
                    "matched": result.matched,
                    "score": result.score,
                    "reason": result.reason,
                }),
            );
        }
        // 复核失败按拦截处理：真实运行里判不出来就不该投，
        // 把它渲染成「通过」会让用户以为提示词没问题
        Err(error) => {
            report.push(
                Stage::SemanticMatch,
                Outcome::Block {
                    reason: error.message,
                },
                json!({}),
            );
        }
    }

    CommandResult::ok(report)
}

fn regex_filter_step(decision: &FilterDecision) -> StepResult {
    StepResult {
        stage: Stage::RegexFilter,
        outcome: if decision.matched {
            Outcome::Pass
        } else {
            Outcome::Block {
                reason: decision.reason.clone(),
            }
        },
        detail: json!({ "matched": decision.matched, "reason": decision.reason }),
    }
}

// ================================
// ③④⑤ 打招呼
// ================================

/// 手输岗位预演一次打招呼：决策 → 序列组装 → 发送前体检。
///
/// 不直接调 [`crate::rpa::greet::build_greet_resources`]，因为它把三步揉成一个结论返回，
/// 而测试模式要看的正是「模型写了什么」「拼成了什么序列」「体检拦没拦」这三件分开的事。
/// 但内部调的是它用的同一批函数，判断逻辑没有第二份
#[tauri::command]
pub async fn playground_greet(
    app_handle: tauri::AppHandle,
    profile_id: Option<String>,
    job: PlaygroundJob,
    overrides: PromptOverrides,
) -> CommandResult<StepReport> {
    let config = match prepare_config(app_handle, profile_id, &overrides) {
        Ok(config) => config,
        Err(error) => return CommandResult::err(error),
    };

    let rpa_job = to_rpa_job(&job);
    let mut report = StepReport::new();

    let generated = if config.greet_config.llm_resource_ready() {
        let trace_id = trace::next_id();
        report.trace_ids.push(trace_id.clone());
        match AgentRunner::new(&config)
            .with_trace_id(trace_id)
            .run(&GreetTask::new(&config, &rpa_job))
            .await
        {
            Ok(outcome) => {
                let decision = outcome.output;
                if decision.action == GreetAction::Skip {
                    // 模型有正规渠道说「不该投」。真实运行里这个结论会取消整轮发送，
                    // 包括后面的固定文本和图片，所以调试页也必须在这里断链——
                    // 让用户看到「后面还会发」是彻头彻尾的误导
                    report.push(
                        Stage::GreetDecide,
                        Outcome::Block {
                            reason: format!(
                                "模型判断该岗位不适合投递（把握 {} 分）：{}",
                                decision.confidence, decision.reason
                            ),
                        },
                        greet_detail(&decision),
                    );
                    return CommandResult::ok(report);
                }
                report.push(Stage::GreetDecide, Outcome::Pass, greet_detail(&decision));
                Some(decision.greeting)
            }
            // 生成失败属于服务不可用，不代表这个岗位不该投，固定内容照发。
            // 这个「失败不终止」的语义是 rpa::greet 里明确设计过的，两边必须一致
            Err(error) => {
                report.push(
                    Stage::GreetDecide,
                    Outcome::Skip {
                        reason: format!(
                            "模型生成失败，实际运行时会跳过 AI 那条、照发固定内容：{}",
                            error.message
                        ),
                    },
                    json!({}),
                );
                None
            }
        }
    } else {
        report.push(
            Stage::GreetDecide,
            Outcome::Skip {
                reason: "方案的打招呼未启用 AI 生成，或提示词/资源未配置".to_string(),
            },
            json!({}),
        );
        None
    };

    let resources = compose_greet_resources(&config.greet_config, generated);
    if resources.is_empty() {
        report.push(
            Stage::GreetCompose,
            Outcome::Block {
                reason: "打招呼发送序列没有可发送内容".to_string(),
            },
            json!({ "resources": [] }),
        );
        return CommandResult::ok(report);
    }
    report.push(
        Stage::GreetCompose,
        Outcome::Pass,
        json!({ "resources": &resources }),
    );

    report.steps.push(outbound_vet_step(
        Stage::GreetVet,
        conversation::vet_outbound(
            resources,
            config.replay_config.max_reply_chars,
            OutboundKind::Greeting,
        ),
    ));

    CommandResult::ok(report)
}

fn greet_detail(decision: &GreetDecision) -> Value {
    json!({
        "greeting": decision.greeting,
        "reason": decision.reason,
        "confidence": decision.confidence,
    })
}

/// 整轮发送的最后一道门。通过时 detail 放的是**最终真正会发出去**的内容——
/// 体检会按句截断，它和模型原文可能并不相同，用户要核对的是前者
fn outbound_vet_step(stage: Stage, verdict: SendVerdict) -> StepResult {
    match verdict {
        SendVerdict::Send(resources) => StepResult {
            stage,
            outcome: Outcome::Pass,
            detail: json!({ "resources": resources }),
        },
        SendVerdict::Hold(reason) => StepResult {
            stage,
            outcome: Outcome::Block { reason },
            detail: json!({}),
        },
    }
}

// ================================
// ⑥⑦⑧⑨⑩ 自动回复
// ================================

/// 手造一段和 HR 的对话，预演一次自动回复的完整链路。
///
/// 不看 `limits.dry_run`：测试模式本来就不发送，演练开关在这里没有意义
#[tauri::command]
pub async fn playground_reply(
    app_handle: tauri::AppHandle,
    profile_id: Option<String>,
    job: PlaygroundJob,
    messages: Vec<PlaygroundMessage>,
    resume_state: ResumeState,
    replies_in_window: usize,
    overrides: PromptOverrides,
) -> CommandResult<StepReport> {
    let config = match prepare_config(app_handle, profile_id, &overrides) {
        Ok(config) => config,
        Err(error) => return CommandResult::err(error),
    };

    let context = ConversationContext {
        platform: PlatformKind::Boss,
        conversation_id: PLAYGROUND_ID.to_string(),
        job: Some(to_job_detail(&job)),
        messages: to_chat_messages(&messages),
        resume_state,
        auto_replies_in_window: replies_in_window,
    };
    let limits = ReplyLimits::from_config(&config.replay_config);
    let mut report = StepReport::new();

    // 闸门在模型之前：这些情况根本不该消耗额度，更不该给模型自由发挥的机会
    let verdict = conversation::gate(&context, &limits);
    let proceed = verdict == GateVerdict::Proceed;
    report.steps.push(gate_step(verdict));
    if !proceed {
        return CommandResult::ok(report);
    }

    match conversation::choose_route(&config.replay_config, &context) {
        ReplyRoute::Skip(reason) => {
            report.push(
                Stage::Route,
                Outcome::Skip { reason },
                json!({ "route": "none" }),
            );
            return CommandResult::ok(report);
        }
        ReplyRoute::Template(hit) => {
            report.push(
                Stage::Route,
                Outcome::Pass,
                json!({
                    "route": "template",
                    "rule_name": hit.display_name(),
                    "resources": &hit.resources,
                }),
            );
            // 模板路径整条绕开模型。⑧⑨ 记成 Skip 而不是干脆不输出：
            // 环节列表的长度固定下来，前端才能把「还没走到」和「走到了但跳过」
            // 画成同一条链上的两种状态，而不是让链条突然少两截
            let reason = "命中确定性模板，未经过模型".to_string();
            report.push(
                Stage::ReplyDecide,
                Outcome::Skip {
                    reason: reason.clone(),
                },
                json!({}),
            );
            report.push(Stage::Reconcile, Outcome::Skip { reason }, json!({}));
            report.steps.push(outbound_vet_step(
                Stage::ReplyVet,
                conversation::vet_outbound(
                    hit.resources,
                    limits.max_reply_chars,
                    OutboundKind::Reply,
                ),
            ));
            return CommandResult::ok(report);
        }
        ReplyRoute::Decide => {
            report.push(Stage::Route, Outcome::Pass, json!({ "route": "model" }));
        }
    }

    let trace_id = trace::next_id();
    report.trace_ids.push(trace_id.clone());
    let decision = match AgentRunner::new(&config)
        .with_trace_id(trace_id)
        .run(&ReplyDecisionTask::new(&config, &context))
        .await
    {
        Ok(outcome) => {
            let decision = outcome.output;
            report.push(
                Stage::ReplyDecide,
                Outcome::Pass,
                reply_decision_detail(&decision),
            );
            decision
        }
        Err(error) => {
            report.push(
                Stage::ReplyDecide,
                Outcome::Block {
                    reason: error.message,
                },
                json!({}),
            );
            return CommandResult::ok(report);
        }
    };

    let effective = conversation::reconcile(&decision, resume_state, &limits);
    report
        .steps
        .push(reconcile_step(&decision, effective, resume_state, &limits));

    report.steps.push(reply_vet_step(
        effective,
        &decision.reply,
        limits.max_reply_chars,
    ));

    CommandResult::ok(report)
}

fn gate_step(verdict: GateVerdict) -> StepResult {
    match verdict {
        GateVerdict::Proceed => StepResult {
            stage: Stage::Gate,
            outcome: Outcome::Pass,
            detail: json!({}),
        },
        GateVerdict::Skip(reason) => StepResult {
            stage: Stage::Gate,
            outcome: Outcome::Skip { reason },
            detail: json!({}),
        },
        // kind 要带出去：前端凭它判断真实运行会把这条会话挂进哪一类待办，
        // 从文案里反猜是哪种情况正是当初把 kind 加进 GateVerdict 要消灭的事
        GateVerdict::Escalate { reason, kind } => StepResult {
            stage: Stage::Gate,
            outcome: Outcome::Block { reason },
            detail: json!({ "kind": kind, "kind_label": kind.label() }),
        },
    }
}

fn reply_decision_detail(decision: &ReplyDecision) -> Value {
    json!({
        "action": decision.action,
        "reply": decision.reply,
        "reason": decision.reason,
        "confidence": decision.confidence,
    })
}

/// 校正环节永远是 Pass：它不拦截链路，只把模型的意图对齐到现实能力。
///
/// detail 必须同时给出「模型想做什么」和「实际会做什么」。只给结论的话，
/// 用户看到的是「模型说要投简历，结果没投」，而看不到究竟是哪个开关拦住了
fn reconcile_step(
    decision: &ReplyDecision,
    effective: ReplyAction,
    resume_state: ResumeState,
    limits: &ReplyLimits,
) -> StepResult {
    let changed = effective != decision.action;
    let mut detail = json!({
        "requested": decision.action,
        "effective": effective,
        "changed": changed,
    });
    if changed {
        detail["downgrade_reason"] = json!(downgrade_reason(decision, resume_state, limits));
    }

    StepResult {
        stage: Stage::Reconcile,
        outcome: Outcome::Pass,
        detail,
    }
}

/// 找出是哪个条件把投递降级成了只回复。
///
/// 这里**不重新判断要不要降级**——那是 [`conversation::reconcile`] 唯一的职责，
/// 再写一份迟早会和它分叉。本函数只在降级已经发生之后，按 `reconcile` 里同样的
/// 优先级顺序找出第一个不满足的条件，用来告诉用户「改哪里才能让它真的投出去」
fn downgrade_reason(
    decision: &ReplyDecision,
    resume_state: ResumeState,
    limits: &ReplyLimits,
) -> String {
    if !limits.allow_auto_send_resume {
        return "方案里关闭了「允许模型自动投递简历」".to_string();
    }
    if decision.confidence < MIN_RESUME_CONFIDENCE {
        return format!(
            "模型自评把握只有 {} 分，未达到投递所需的 {MIN_RESUME_CONFIDENCE} 分",
            decision.confidence
        );
    }
    if resume_state != ResumeState::Sendable {
        return format!(
            "简历入口状态是「{}」，不允许主动投递",
            resume_state_label(resume_state)
        );
    }
    // 三个条件都满足却仍然被改写，说明 reconcile 的规则变了而这里没跟上。
    // 与其编一个像模像样的理由，不如让它显眼
    "动作被校正，但未能定位到具体原因，请检查投递校正规则".to_string()
}

fn resume_state_label(state: ResumeState) -> &'static str {
    match state {
        ResumeState::Sendable => "可主动投递",
        ResumeState::RequestedByPeer => "对方正在索要简历",
        ResumeState::Unavailable => "已投递或平台要求先等对方回复",
        ResumeState::Unknown => "页面上找不到可判定的入口",
    }
}

fn reply_vet_step(effective: ReplyAction, reply: &str, max_chars: usize) -> StepResult {
    if !effective.needs_text() {
        return StepResult {
            stage: Stage::ReplyVet,
            outcome: Outcome::Skip {
                reason: "该动作不发送正文".to_string(),
            },
            detail: json!({}),
        };
    }

    match conversation::vet_reply(reply, max_chars) {
        // 体检会按句截断，这里放的是最终真正会发出去的那段，可能与模型原文不同
        Ok(text) => StepResult {
            stage: Stage::ReplyVet,
            outcome: Outcome::Pass,
            detail: json!({ "text": text }),
        },
        Err(reason) => StepResult {
            stage: Stage::ReplyVet,
            outcome: Outcome::Block { reason },
            detail: json!({}),
        },
    }
}

// ================================
// 轨迹
// ================================

#[tauri::command]
pub fn playground_traces(ids: Vec<String>) -> CommandResult<Vec<AgentTrace>> {
    CommandResult::ok(trace::recent(&ids))
}

#[tauri::command]
pub fn playground_clear_traces() -> CommandResult<()> {
    trace::clear();
    CommandResult::ok(())
}

#[tauri::command]
pub async fn playground_export_traces(path: String) -> CommandResult<usize> {
    match trace::export(Path::new(&path)) {
        Ok(count) => CommandResult::ok(count),
        Err(error) => CommandResult::err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        default_app_config, ReplayResourceType, ReplyRegexRule, ReplyResource, ReplyTemplate,
    };
    use crate::dao::model::ManualReviewReason;

    fn job() -> PlaygroundJob {
        PlaygroundJob {
            title: "Rust 后端工程师".to_string(),
            company_name: "示例科技".to_string(),
            detail: "负责网关与限流".to_string(),
            salary: "25-40K".to_string(),
            location: "南京".to_string(),
        }
    }

    fn limits() -> ReplyLimits {
        ReplyLimits {
            max_auto_replies: 5,
            auto_reply_window_hours: 24,
            max_reply_chars: 200,
            allow_auto_send_resume: true,
            dry_run: false,
        }
    }

    fn decision(action: ReplyAction, reply: &str, confidence: u8) -> ReplyDecision {
        ReplyDecision {
            action,
            reply: reply.to_string(),
            reason: "测试".to_string(),
            confidence,
        }
    }

    fn context(messages: Vec<ChatMessage>) -> ConversationContext {
        ConversationContext {
            platform: PlatformKind::Boss,
            conversation_id: PLAYGROUND_ID.to_string(),
            job: None,
            messages,
            resume_state: ResumeState::Sendable,
            auto_replies_in_window: 0,
        }
    }

    /// 手输岗位必须变成真实链路认得的同一种形状，否则筛选和打招呼看到的
    /// 根本不是用户填进去的那个岗位
    #[test]
    fn a_typed_in_job_becomes_the_same_shape_the_real_pipeline_sees() {
        let rpa_job = to_rpa_job(&job());
        let detail = to_job_detail(&job());

        assert_eq!(rpa_job.platform, PlatformKind::Boss);
        assert_eq!(rpa_job.platform_job_id, PLAYGROUND_ID);
        assert_eq!(rpa_job.location.as_deref(), Some("南京"));
        assert!(rpa_job.detail_url.is_empty(), "手输岗位没有来源页面");
        assert_eq!(detail.id, PLAYGROUND_ID);
        assert_eq!(detail.platform, "boss");
        assert_eq!(detail.title, rpa_job.title);
        assert!(!detail.is_send_resume);
    }

    /// 角色前缀决定了模型分不分得清谁说的话，方向标错等于让它读一段错乱的对话
    #[test]
    fn handcrafted_messages_are_labelled_by_direction_and_keep_their_order() {
        let messages = to_chat_messages(&[
            PlaygroundMessage {
                text: "在吗".to_string(),
                received: true,
            },
            PlaygroundMessage {
                text: "在的".to_string(),
                received: false,
            },
        ]);

        assert_eq!(messages[0].from_name, "HR");
        assert_eq!(messages[1].from_name, "我");
        assert!(messages[0].time < messages[1].time);
        assert_eq!(context(messages).transcript(), "HR: 在吗\n我: 在的");
    }

    /// 给了值就覆盖：这是测试模式的核心能力，改一句提示词立刻重跑
    #[test]
    fn a_supplied_override_replaces_the_saved_prompt() {
        let mut config = default_app_config();
        config.greet_config.reply_prompt = Some("旧的打招呼".to_string());
        config.replay_config.reply_prompt = Some("旧的回复".to_string());
        config.job_filter_config.semantic_filter_intent = Some("旧的意图".to_string());

        apply_overrides(
            &mut config,
            &PromptOverrides {
                greet_prompt: Some("新的打招呼".to_string()),
                reply_prompt: Some("新的回复".to_string()),
                semantic_filter_intent: Some("新的意图".to_string()),
            },
        );

        assert_eq!(
            config.greet_config.reply_prompt.as_deref(),
            Some("新的打招呼")
        );
        assert_eq!(
            config.replay_config.reply_prompt.as_deref(),
            Some("新的回复")
        );
        assert_eq!(
            config.job_filter_config.semantic_filter_intent.as_deref(),
            Some("新的意图")
        );
    }

    /// 给 None 表示「用方案里存着的那份」，而不是清空。
    /// 只调一个环节、其它环节保持现状做对照，是测试模式最常见的用法
    #[test]
    fn an_absent_override_keeps_the_saved_prompt_instead_of_clearing_it() {
        let mut config = default_app_config();
        config.greet_config.reply_prompt = Some("方案里的打招呼".to_string());
        config.replay_config.reply_prompt = Some("方案里的回复".to_string());
        let untouched = config.browser_config.clone();

        apply_overrides(
            &mut config,
            &PromptOverrides {
                greet_prompt: None,
                reply_prompt: Some("只改回复".to_string()),
                semantic_filter_intent: None,
            },
        );

        assert_eq!(
            config.greet_config.reply_prompt.as_deref(),
            Some("方案里的打招呼")
        );
        assert_eq!(
            config.replay_config.reply_prompt.as_deref(),
            Some("只改回复")
        );
        assert!(config.job_filter_config.semantic_filter_intent.is_none());
        assert_eq!(config.browser_config, untouched, "覆盖不该波及其它配置");
    }

    /// 被规则挡掉时要说清是哪条规则挡的，只说「没通过」用户无从下手
    #[test]
    fn a_rejected_job_reports_the_rule_that_blocked_it() {
        let mut config = default_app_config();
        config.job_filter_config.exclude_keywords = vec!["外包".to_string()];
        let mut typed = job();
        typed.title = "Java 外包开发".to_string();

        let step = regex_filter_step(&crate::verify::filter_decision(
            &to_rpa_job(&typed),
            &config,
        ));

        assert_eq!(step.stage, Stage::RegexFilter);
        match step.outcome {
            Outcome::Block { reason } => assert!(reason.contains("外包"), "实际：{reason}"),
            other => panic!("命中排除关键词必须拦下，实际：{other:?}"),
        }
        assert_eq!(step.detail["matched"], json!(false));
    }

    /// 敏感话题是求职诈骗的常见开场。除了拦下来，还要把待办类别带给前端，
    /// 让用户看到真实运行时这条会话会被挂进哪一类
    #[test]
    fn a_risky_message_blocks_the_gate_and_names_the_review_kind() {
        let context = context(to_chat_messages(&[PlaygroundMessage {
            text: "麻烦把身份证正反面发我".to_string(),
            received: true,
        }]));

        let step = gate_step(conversation::gate(&context, &limits()));

        assert!(matches!(step.outcome, Outcome::Block { .. }));
        assert_eq!(
            step.detail["kind"],
            json!(ManualReviewReason::RiskKeyword),
            "待办类别必须原样带出，不能让前端从文案反猜"
        );
    }

    /// 对方还没回时闸门是 Skip 不是 Block：本轮什么都不做，但这不算异常
    #[test]
    fn the_gate_skips_rather_than_blocks_when_the_peer_has_not_replied_yet() {
        let context = context(to_chat_messages(&[
            PlaygroundMessage {
                text: "在吗".to_string(),
                received: true,
            },
            PlaygroundMessage {
                text: "在的，我很感兴趣".to_string(),
                received: false,
            },
        ]));

        let step = gate_step(conversation::gate(&context, &limits()));

        assert!(matches!(step.outcome, Outcome::Skip { .. }));
    }

    /// 用户关掉了自动投递时，要指向那个开关，而不是让他去怀疑提示词
    #[test]
    fn a_downgrade_points_at_the_disabled_auto_send_setting() {
        let mut limits = limits();
        limits.allow_auto_send_resume = false;
        let decision = decision(ReplyAction::ReplyAndSendResume, "好的", 95);
        let effective = conversation::reconcile(&decision, ResumeState::Sendable, &limits);

        let step = reconcile_step(&decision, effective, ResumeState::Sendable, &limits);

        assert_eq!(
            step.detail["requested"],
            json!(ReplyAction::ReplyAndSendResume)
        );
        assert_eq!(step.detail["effective"], json!(ReplyAction::Reply));
        assert_eq!(step.detail["changed"], json!(true));
        assert!(step.detail["downgrade_reason"]
            .as_str()
            .unwrap()
            .contains("关闭了"));
    }

    /// 置信度不够时要把分数写出来：用户才知道是差一点还是差得远
    #[test]
    fn a_downgrade_points_at_the_low_confidence_and_shows_the_score() {
        let decision = decision(ReplyAction::ReplyAndSendResume, "好的", 40);
        let effective = conversation::reconcile(&decision, ResumeState::Sendable, &limits());

        let step = reconcile_step(&decision, effective, ResumeState::Sendable, &limits());

        let reason = step.detail["downgrade_reason"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(reason.contains("40"), "实际：{reason}");
        assert!(reason.contains("70"), "实际：{reason}");
    }

    /// 简历入口不可用是页面状态问题，和提示词无关，说清楚才不会让人白改提示词
    #[test]
    fn a_downgrade_points_at_the_unavailable_resume_entry() {
        let decision = decision(ReplyAction::ReplyAndSendResume, "好的", 95);
        let effective = conversation::reconcile(&decision, ResumeState::Unavailable, &limits());

        let step = reconcile_step(&decision, effective, ResumeState::Unavailable, &limits());

        assert!(step.detail["downgrade_reason"]
            .as_str()
            .unwrap()
            .contains("已投递或平台要求先等对方回复"));
    }

    /// 没被改写时不能凭空冒出一条降级原因，那会让用户以为发生了他没察觉的事
    #[test]
    fn an_untouched_action_reports_no_downgrade_reason() {
        let decision = decision(ReplyAction::ReplyAndSendResume, "好的，简历这就发您", 88);
        let effective = conversation::reconcile(&decision, ResumeState::Sendable, &limits());

        let step = reconcile_step(&decision, effective, ResumeState::Sendable, &limits());

        assert_eq!(step.detail["changed"], json!(false));
        assert!(step.detail.get("downgrade_reason").is_none());
        assert!(matches!(step.outcome, Outcome::Pass), "校正只对齐，不拦截");
    }

    /// skip / escalate 本来就不发正文，体检环节该跳过而不是拿空串去判不合格
    #[test]
    fn actions_without_a_body_skip_the_reply_vetting() {
        for action in [ReplyAction::Skip, ReplyAction::Escalate] {
            let step = reply_vet_step(action, "", 200);
            assert!(matches!(step.outcome, Outcome::Skip { .. }), "{action:?}");
        }
    }

    /// 体检会按句截断，用户要核对的是最终发出去的那段，不是模型写的原文
    #[test]
    fn the_vetted_reply_shows_the_text_that_will_actually_be_sent() {
        let long = format!("{}。{}", "很长的第一句".repeat(4), "第二句会被截掉");

        let step = reply_vet_step(ReplyAction::Reply, &long, 30);

        let text = step.detail["text"].as_str().unwrap();
        assert!(matches!(step.outcome, Outcome::Pass));
        assert!(text.chars().count() <= 30, "实际：{text}");
        assert_ne!(text, long, "截断后必须和入参不同");
    }

    /// 一条不合格就整轮不发。调试页要把这个「整轮取消」如实呈现，
    /// 而不是只标红那一条、让用户以为其余的还会发出去
    #[test]
    fn one_declining_line_holds_the_whole_greeting_batch() {
        let resources = vec![
            ReplyResource {
                resource_type: ReplayResourceType::Text,
                content: "您好，注意到您是猎头顾问，我暂时不考虑猎头渠道。".to_string(),
            },
            ReplyResource {
                resource_type: ReplayResourceType::Image,
                content: "C:/resume.png".to_string(),
            },
        ];

        let step = outbound_vet_step(
            Stage::GreetVet,
            conversation::vet_outbound(resources, 200, OutboundKind::Greeting),
        );

        assert_eq!(step.stage, Stage::GreetVet);
        assert!(matches!(step.outcome, Outcome::Block { .. }));
    }

    /// 模板路径整条绕开模型。路由 detail 里必须给出命中的规则名，
    /// 否则用户看到「没走模型」却不知道是哪条规则短路了它
    #[test]
    fn the_template_route_names_the_rule_that_short_circuited_the_model() {
        let mut config = default_app_config();
        config.replay_config.enable_template_reply = true;
        config.replay_config.templates = vec![ReplyTemplate {
            regex_rule: ReplyRegexRule {
                name: "面试邀约".to_string(),
                pattern: "面试".to_string(),
                limit: 2,
            },
            content: vec![ReplyResource {
                resource_type: ReplayResourceType::Text,
                content: "好的，时间我这边可以".to_string(),
            }],
        }];
        let context = context(to_chat_messages(&[PlaygroundMessage {
            text: "下周二方便来面试吗".to_string(),
            received: true,
        }]));

        match conversation::choose_route(&config.replay_config, &context) {
            ReplyRoute::Template(hit) => {
                assert_eq!(hit.display_name(), "面试邀约");
                let step = outbound_vet_step(
                    Stage::ReplyVet,
                    conversation::vet_outbound(hit.resources, 200, OutboundKind::Reply),
                );
                assert!(matches!(step.outcome, Outcome::Pass));
            }
            other => panic!("命中模板时必须走模板路径，实际：{other:?}"),
        }
    }

    // ---- 前后端契约锁 ----
    //
    // 下面这几条断言的是**线上格式**，不是内部实现。前端 `src/types/playground.ts`
    // 里的联合类型是照着这些字面量手写的，两边没有代码生成来保证同步：
    // 谁要是顺手给某个枚举加个 `rename_all`、或者把变体改个名，编译照样过、
    // 测试照样绿，只有界面会在用户点下去的那一刻悄悄渲染成空白。
    // 改这里的期望值时，必须同步改 `src/types/playground.ts`。

    #[test]
    fn every_stage_serializes_to_the_literal_the_frontend_switches_on() {
        let names: Vec<String> = [
            Stage::RegexFilter,
            Stage::SemanticMatch,
            Stage::GreetDecide,
            Stage::GreetCompose,
            Stage::GreetVet,
            Stage::Gate,
            Stage::Route,
            Stage::ReplyDecide,
            Stage::Reconcile,
            Stage::ReplyVet,
        ]
        .iter()
        .map(|stage| serde_json::to_value(stage).expect("序列化环节")
            .as_str()
            .expect("环节是字符串")
            .to_string())
        .collect();

        assert_eq!(
            names,
            vec![
                "regex_filter",
                "semantic_match",
                "greet_decide",
                "greet_compose",
                "greet_vet",
                "gate",
                "route",
                "reply_decide",
                "reconcile",
                "reply_vet",
            ]
        );
    }

    #[test]
    fn outcome_carries_its_variant_in_a_kind_tag_next_to_the_reason() {
        let pass = serde_json::to_value(Outcome::Pass).expect("序列化结论");
        let block = serde_json::to_value(Outcome::Block {
            reason: "内容残留未填充的占位符".to_string(),
        })
        .expect("序列化结论");
        let skip = serde_json::to_value(Outcome::Skip {
            reason: "方案未启用 AI 岗位复核".to_string(),
        })
        .expect("序列化结论");

        assert_eq!(pass["kind"], "pass");
        assert_eq!(block["kind"], "block");
        assert_eq!(block["reason"], "内容残留未填充的占位符");
        assert_eq!(skip["kind"], "skip");
        assert_eq!(skip["reason"], "方案未启用 AI 岗位复核");
    }

    /// 简历状态是 PascalCase——它复用的是 `conversation::ResumeState`，
    /// 那个枚举没标 `rename_all`，和本模块自己定义的几个枚举**不一样**。
    /// 前端下拉框传回来的字面量必须照着这个来，写成 snake_case 会反序列化失败
    #[test]
    fn resume_state_stays_pascal_case_because_it_is_shared_with_the_real_pipeline() {
        let states: Vec<String> = [
            ResumeState::Sendable,
            ResumeState::RequestedByPeer,
            ResumeState::Unavailable,
            ResumeState::Unknown,
        ]
        .iter()
        .map(|state| serde_json::to_value(state).expect("序列化简历状态")
            .as_str()
            .expect("简历状态是字符串")
            .to_string())
        .collect();

        assert_eq!(
            states,
            vec!["Sendable", "RequestedByPeer", "Unavailable", "Unknown"]
        );
        // 反向也要通得过：这正是前端下拉传回来的那条路
        assert_eq!(
            serde_json::from_str::<ResumeState>("\"RequestedByPeer\"").expect("反序列化简历状态"),
            ResumeState::RequestedByPeer
        );
    }

    #[test]
    fn a_report_serializes_with_the_field_names_the_frontend_reads() {
        let mut report = StepReport::new();
        report.trace_ids.push("trace-1".to_string());
        report.push(
            Stage::Gate,
            Outcome::Skip {
                reason: "对方尚未回复，不重复发送".to_string(),
            },
            json!({ "note": "闸门" }),
        );

        let value = serde_json::to_value(&report).expect("序列化报告");

        assert_eq!(value["trace_ids"][0], "trace-1");
        assert_eq!(value["steps"][0]["stage"], "gate");
        assert_eq!(value["steps"][0]["outcome"]["kind"], "skip");
        assert_eq!(value["steps"][0]["detail"]["note"], "闸门");
    }
}
