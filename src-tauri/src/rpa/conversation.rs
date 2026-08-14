//! 平台无关的会话模型：上下文、决策、动作契约与安全护栏。
//!
//! BOSS 和猎聘的差异只在「怎么读消息、怎么点按钮」，「读到之后怎么判断、判断完做什么」
//! 两边应当完全一致。此前两个平台各写一套，行为悄悄分叉：BOSS 是模板套 LLM、
//! 猎聘是 LLM 优先回落模板，同一份配置在两个平台上的效果不同。这里把共性收拢，
//! 平台侧只需实现 [`ConversationActions`]。

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::agent::output;
use crate::config::{ReplayConfig, ReplayResourceType, ReplyResource, ReplyTemplate};
use crate::dao::model::JobDetail;
use crate::rpa::common::ChatMessage;
use crate::rpa::run_flow::PlatformKind;

/// 一次自动回复所需的全部上下文。
#[derive(Debug, Clone)]
pub struct ConversationContext {
    pub platform: PlatformKind,
    /// 会话稳定标识。BOSS 用 encryptJobId，猎聘用 imid。
    /// 猎聘映射不到岗位时也必须有值，否则聊天记录无法落库
    pub conversation_id: String,
    /// 归属岗位。猎聘存在映射不到的情况，这时岗位描述按「未获取到」处理，
    /// 而不是让整条链路失败
    pub job: Option<JobDetail>,
    /// 按时间升序的完整历史（平台接口本页 ∪ 本地库历史，按 mid 去重）
    pub messages: Vec<ChatMessage>,
    pub resume_state: ResumeState,
}

impl ConversationContext {
    /// 对方（HR）发来的最后一条消息
    pub fn last_received(&self) -> Option<&ChatMessage> {
        self.messages.iter().rev().find(|message| message.received)
    }

    /// 我方已经发出的消息条数，用于判断是否聊得过久该转人工
    pub fn sent_count(&self) -> usize {
        self.messages
            .iter()
            .filter(|message| !message.received)
            .count()
    }

    /// 渲染成喂给模型的对话记录。带角色前缀，模型才分得清谁说的
    pub fn transcript(&self) -> String {
        self.messages
            .iter()
            .map(|message| {
                let role = if message.received { "HR" } else { "我" };
                format!("{role}: {}", message.text)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// 简历投递入口在当前会话里的状态。
///
/// 必须先读状态再决定动作。此前 BOSS 侧的做法是不看状态直接点工具栏第一个按钮，
/// 而 `.toolbar-btn` 是「发简历 / 换电话 / 换微信」共用的类名，
/// 盲点等于随机把手机号或微信推给对方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResumeState {
    /// 可以主动发起投递
    Sendable,
    /// 对方发来了索要简历的请求，等我确认
    RequestedByPeer,
    /// 已投递，或平台要求先等对方回复，此时不能再发
    Unavailable,
    /// 页面上找不到可判定的入口。宁可什么都不做，也不要猜着点
    Unknown,
}

/// 模型对一次会话给出的处置决定。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplyDecision {
    pub action: ReplyAction,
    /// 要发送的正文。`Skip` / `Escalate` 时允许为空
    #[serde(default)]
    pub reply: String,
    /// 判断理由，落库供用户复盘，不发给对方
    #[serde(default)]
    pub reason: String,
    /// 自评置信度。低置信度不允许触发投简历这种不可撤销的动作
    #[serde(default)]
    pub confidence: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplyAction {
    /// 只回消息
    Reply,
    /// 回消息，并主动投递简历
    ReplyAndSendResume,
    /// 不回。对方只是客套收尾，或明确表示不合适
    Skip,
    /// 转人工。涉及敏感信息、或超出自动化能覆盖的判断
    Escalate,
}

impl ReplyAction {
    /// 是否需要发送正文
    pub fn needs_text(self) -> bool {
        matches!(self, Self::Reply | Self::ReplyAndSendResume)
    }
}

/// 模型对一个岗位是否该打招呼给出的决定。
///
/// 和 [`ReplyDecision`] 对称。此前打招呼任务只返回纯文本，模型判断出「这岗位不该投」
/// 时没有渠道表达，只能把结论写进正文——于是那句话被当成开场白发给了对方。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GreetDecision {
    pub action: GreetAction,
    /// 开场白正文。`Skip` 时允许为空
    #[serde(default)]
    pub greeting: String,
    /// 判断理由，落日志供复盘，不发给对方
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub confidence: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GreetAction {
    /// 发送开场白
    Send,
    /// 不投这个岗位。整轮发送序列都取消，包括后面的固定文本和图片
    Skip,
}

/// 平台需要提供的动作能力。
///
/// 只放「读状态」和「做动作」，不含任何判断逻辑——判断全在平台无关的这一层，
/// 两个平台的行为才不会再次分叉。
pub trait ConversationActions {
    /// 读当前会话的简历投递入口状态
    fn resume_state(&self, page: &rust_drission::Page) -> Result<ResumeState, anyhow::Error>;

    /// 发送一条文本消息
    fn send_text(&self, page: &rust_drission::Page, text: &str) -> Result<bool, anyhow::Error>;

    /// 主动发起简历投递。返回 false 表示入口不可用（并非错误）
    fn send_resume(&self, page: &rust_drission::Page) -> Result<bool, anyhow::Error>;

    /// 同意对方的简历索要请求。返回 false 表示当前没有待处理的请求
    fn accept_resume_request(&self, page: &rust_drission::Page) -> Result<bool, anyhow::Error>;
}

/// 自动回复的边界参数。集中一处，两个平台不会各配一套
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplyLimits {
    /// 单个会话累计自动回复条数上限，超过转人工。
    /// 没有这条限制时，模型可以和 HR 无限客套下去
    pub max_auto_replies: usize,
    /// 单条回复的字数上限
    pub max_reply_chars: usize,
    /// 是否允许模型自主决定投递简历
    pub allow_auto_send_resume: bool,
    /// 演练模式：全部判断照常，但不实际发送
    pub dry_run: bool,
}

impl ReplyLimits {
    pub fn from_config(config: &ReplayConfig) -> Self {
        Self {
            max_auto_replies: config.max_auto_replies,
            max_reply_chars: config.max_reply_chars,
            allow_auto_send_resume: config.enable_auto_send_resume,
            dry_run: config.dry_run,
        }
    }
}

/// 进入模型判断之前的闸门结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateVerdict {
    /// 可以交给模型判断
    Proceed,
    /// 本轮什么都不做
    Skip(String),
    /// 转人工，附原因
    Escalate(String),
}

/// 会触发转人工的话题。
///
/// 这些内容一旦自动回复就可能造成真实损失：模型没有能力核实对方身份，
/// 而证件、账户、付费、线下见面这些请求正是求职诈骗的常见开场。
const ESCALATE_KEYWORDS: &[&str] = &[
    "身份证",
    "银行卡",
    "银行账户",
    "转账",
    "汇款",
    "打款",
    "培训费",
    "押金",
    "保证金",
    "服装费",
    "体检费",
    "工本费",
    "验证码",
    "营业执照",
    "征信",
    "贷款",
    "刷单",
    "垫付",
];

/// 模型判断之前的确定性闸门。
///
/// 放在模型之前而不是之后，是因为这些情况根本不该消耗额度，
/// 更不该给模型自由发挥的机会。
pub fn gate(context: &ConversationContext, limits: &ReplyLimits) -> GateVerdict {
    if context.messages.is_empty() {
        return GateVerdict::Skip("会话没有可用消息".to_string());
    }

    // 最后一条是自己发的，说明对方还没回。此时再发就是自问自答，
    // 在 HR 眼里等同于骚扰
    let last_is_mine = context
        .messages
        .last()
        .is_some_and(|message| !message.received);
    if last_is_mine {
        return GateVerdict::Skip("对方尚未回复，不重复发送".to_string());
    }

    if context.sent_count() >= limits.max_auto_replies {
        return GateVerdict::Escalate(format!(
            "本会话已自动回复 {} 条，达到上限，转人工接手",
            context.sent_count()
        ));
    }

    if let Some(message) = context.last_received() {
        if let Some(keyword) = ESCALATE_KEYWORDS
            .iter()
            .find(|keyword| message.text.contains(**keyword))
        {
            return GateVerdict::Escalate(format!("对方消息涉及「{keyword}」，需人工确认"));
        }
    }

    GateVerdict::Proceed
}

/// 把本地库里的历史与本轮接口消息合成完整上下文。
///
/// 平台接口只返回最近一页，只有合并之后模型看到的才是整段对话。
/// 同一 `mid` 以本轮接口为准：消息被撤回或编辑后，接口才是现状。
pub fn merge_messages(history: Vec<ChatMessage>, fresh: Vec<ChatMessage>) -> Vec<ChatMessage> {
    let fresh_ids: std::collections::HashSet<i64> =
        fresh.iter().map(|message| message.mid).collect();

    let mut merged: Vec<ChatMessage> = history
        .into_iter()
        .filter(|message| !fresh_ids.contains(&message.mid))
        .chain(fresh)
        .collect();

    merged.sort_by_key(|message| (message.time, message.mid));
    merged
}

/// 命中确定性回复模板时，返回该模板配置的可发送资源。
///
/// 放在模型判断之前：用户显式配了正则和固定话术，就说明这类消息他要的是
/// 稳定可预期的答复，不该每次再花额度让模型重新发挥一遍。
///
/// 模板里 `LLM` 类型的槽位在这里会被忽略——那是旧链路「模板套模型」的产物，
/// 现在模型走的是决策链路，槽位无处可填。全是 LLM 槽的模板等价于没命中，
/// 于是自然落到模型判断，行为与改造前一致。
pub fn match_template<'a>(
    templates: &'a [ReplyTemplate],
    context: &ConversationContext,
) -> Option<TemplateHit<'a>> {
    let template = templates.iter().find(|template| {
        Regex::new(&template.regex_rule.pattern)
            .map(|regex| regex.is_match(&latest_chat_history(context, template.regex_rule.limit)))
            .unwrap_or(false)
    })?;

    let resources: Vec<ReplyResource> = template
        .content
        .iter()
        .filter(|resource| resource.resource_type != ReplayResourceType::LLM)
        .filter(|resource| !resource.content.trim().is_empty())
        .cloned()
        .collect();

    (!resources.is_empty()).then_some(TemplateHit {
        rule_name: template.regex_rule.name.as_str(),
        resources,
    })
}

/// 命中的模板及其可发送资源。
///
/// 规则名要跟着资源一起回传：日志里说清「命中了哪条规则」，用户才能判断
/// 是规则写宽了还是模型没被触发。靠资源内容反查规则名会在两条规则配了
/// 相同话术时认错，那种日志比没有更误导人。
#[derive(Debug, Clone)]
pub struct TemplateHit<'a> {
    pub rule_name: &'a str,
    pub resources: Vec<ReplyResource>,
}

/// 取最近 N 条消息的正文拼成一段，供正则匹配。
///
/// 不区分收发方向是沿用改造前的语义：用户已有的正则是照着这个行为写的，
/// 改成只匹配对方消息会让存量规则悄悄失效。
fn latest_chat_history(context: &ConversationContext, limit: i32) -> String {
    let limit = usize::try_from(limit.max(1)).unwrap_or(1);
    let start = context.messages.len().saturating_sub(limit);
    context.messages[start..]
        .iter()
        .map(|message| message.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 一整轮对外发送的最终裁决。
#[derive(Debug, Clone, PartialEq)]
pub enum SendVerdict {
    /// 按序发送这些资源
    Send(Vec<ReplyResource>),
    /// 整轮都不发，附原因
    Hold(String),
}

/// 所有对外发送的最后一道门。打招呼和自动回复共用。
///
/// **任何一条内容不合格，整轮都不发**，而不是只丢掉那一条。这是实测教训：
/// 模型写出「注意到您是猎头顾问，我暂时不考虑猎头渠道」被当成开场白发了出去，
/// 而发送序列里紧跟着的那条固定简历图片照发不误——对方收到的是
/// 「我不考虑你」外加一张简历截图。逐条过滤在这种场景下比不过滤更糟：
/// 它让一段本该整体取消的动作，退化成一段语义残缺的动作。
///
/// 图片这类非文本资源无法体检，但它们的去留跟着整轮走，不会单独发出去。
pub fn vet_outbound(resources: Vec<ReplyResource>, max_chars: usize) -> SendVerdict {
    let mut checked: Vec<ReplyResource> = Vec::with_capacity(resources.len());

    for mut resource in resources {
        if resource.resource_type == ReplayResourceType::Image {
            checked.push(resource);
            continue;
        }

        let text = resource.content.trim();
        if text.is_empty() {
            continue;
        }
        if output::looks_like_refusal(text) {
            return SendVerdict::Hold("内容是模型的拒答话术".to_string());
        }
        if output::declines_opportunity(text) {
            return SendVerdict::Hold("内容里表达了不考虑该机会，整轮取消发送".to_string());
        }
        if output::has_placeholder(text) {
            return SendVerdict::Hold("内容残留未填充的占位符".to_string());
        }

        resource.content = output::truncate_at_sentence(text, max_chars);
        checked.push(resource);
    }

    if checked.is_empty() {
        return SendVerdict::Hold("没有可发送的内容".to_string());
    }
    SendVerdict::Send(checked)
}

/// 单条回复的发送前体检。通过则返回可发送的文本（必要时已截断）。
///
/// 与 [`vet_outbound`] 的差别是**刻意的**：这里不查「婉拒」。
/// 开场白里写婉拒等于这个岗位根本不该发，但在回复里礼貌地表达「这个岗位我可能不太合适」
/// 本身就是一条合理的消息——模型真要终止这段对话，有 `ReplyAction::Skip` 可用。
///
/// 模型解析成功不代表内容能见人：拒答话术、没替换掉的占位符发出去，
/// 对面一眼就知道是机器人在聊。
pub fn vet_reply_text(text: &str, max_chars: usize) -> Result<String, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("回复正文为空".to_string());
    }
    if output::looks_like_refusal(trimmed) {
        return Err("回复是模型的拒答话术，不能发给对方".to_string());
    }
    if output::has_placeholder(trimmed) {
        return Err("回复残留未填充的占位符，不能发给对方".to_string());
    }

    Ok(output::truncate_at_sentence(trimmed, max_chars))
}

/// 决策与实际能力的一致性校正。
///
/// 模型不知道页面上简历入口是什么状态，也不知道用户是否允许自动投递，
/// 所以它给出的投递意图必须在这里对齐现实，否则就会出现「决定投递但入口已禁用」
/// 这种执行期才炸的情况。
pub fn reconcile(
    decision: &ReplyDecision,
    resume_state: ResumeState,
    limits: &ReplyLimits,
) -> ReplyAction {
    if decision.action != ReplyAction::ReplyAndSendResume {
        return decision.action;
    }

    if !limits.allow_auto_send_resume {
        return ReplyAction::Reply;
    }
    // 不可撤销的动作要求模型有把握。低置信度时只回消息，把投递留给下一轮
    if decision.confidence < 70 {
        return ReplyAction::Reply;
    }
    if resume_state != ResumeState::Sendable {
        return ReplyAction::Reply;
    }

    ReplyAction::ReplyAndSendResume
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> ReplyLimits {
        ReplyLimits {
            max_auto_replies: 5,
            max_reply_chars: 200,
            allow_auto_send_resume: true,
            dry_run: false,
        }
    }

    fn message(received: bool, text: &str) -> ChatMessage {
        ChatMessage {
            mid: text.len() as i64,
            received,
            text: text.to_string(),
            time: 0,
            from_name: String::new(),
        }
    }

    fn context(messages: Vec<ChatMessage>) -> ConversationContext {
        ConversationContext {
            platform: PlatformKind::Boss,
            conversation_id: "c1".to_string(),
            job: None,
            messages,
            resume_state: ResumeState::Sendable,
        }
    }

    #[test]
    fn does_not_reply_when_the_last_message_is_mine() {
        let verdict = gate(
            &context(vec![
                message(true, "你好"),
                message(false, "您好，我很感兴趣"),
            ]),
            &limits(),
        );

        assert!(matches!(verdict, GateVerdict::Skip(_)));
    }

    #[test]
    fn escalates_once_the_reply_budget_is_used_up() {
        let mut messages = Vec::new();
        for index in 0..5 {
            messages.push(message(false, &format!("我方第 {index} 条")));
        }
        messages.push(message(true, "还在吗"));

        let verdict = gate(&context(messages), &limits());

        assert!(matches!(verdict, GateVerdict::Escalate(_)));
    }

    /// 求职诈骗的常见开场，绝不能让模型自由发挥
    #[test]
    fn escalates_on_sensitive_requests() {
        let verdict = gate(
            &context(vec![message(true, "麻烦把身份证正反面发我一下")]),
            &limits(),
        );

        match verdict {
            GateVerdict::Escalate(reason) => assert!(reason.contains("身份证")),
            other => panic!("敏感请求必须转人工，实际：{other:?}"),
        }
    }

    #[test]
    fn normal_hr_question_passes_the_gate() {
        assert_eq!(
            gate(
                &context(vec![message(true, "方便说下你的期望薪资吗")]),
                &limits()
            ),
            GateVerdict::Proceed
        );
    }

    #[test]
    fn resume_is_not_sent_when_the_entry_is_unavailable() {
        let decision = ReplyDecision {
            action: ReplyAction::ReplyAndSendResume,
            reply: "好的".to_string(),
            reason: String::new(),
            confidence: 90,
        };

        assert_eq!(
            reconcile(&decision, ResumeState::Unavailable, &limits()),
            ReplyAction::Reply
        );
    }

    #[test]
    fn resume_is_not_sent_when_the_model_is_unsure() {
        let decision = ReplyDecision {
            action: ReplyAction::ReplyAndSendResume,
            reply: "好的".to_string(),
            reason: String::new(),
            confidence: 40,
        };

        assert_eq!(
            reconcile(&decision, ResumeState::Sendable, &limits()),
            ReplyAction::Reply
        );
    }

    #[test]
    fn resume_is_not_sent_when_the_user_disabled_it() {
        let decision = ReplyDecision {
            action: ReplyAction::ReplyAndSendResume,
            reply: "好的".to_string(),
            reason: String::new(),
            confidence: 95,
        };
        let mut limits = limits();
        limits.allow_auto_send_resume = false;

        assert_eq!(
            reconcile(&decision, ResumeState::Sendable, &limits),
            ReplyAction::Reply
        );
    }

    #[test]
    fn confident_decision_sends_the_resume_when_everything_lines_up() {
        let decision = ReplyDecision {
            action: ReplyAction::ReplyAndSendResume,
            reply: "好的，简历这就发您".to_string(),
            reason: String::new(),
            confidence: 88,
        };

        assert_eq!(
            reconcile(&decision, ResumeState::Sendable, &limits()),
            ReplyAction::ReplyAndSendResume
        );
    }

    fn image(path: &str) -> ReplyResource {
        ReplyResource {
            resource_type: ReplayResourceType::Image,
            content: path.to_string(),
        }
    }

    /// 一条不合格就整轮不发。逐条过滤会让动作退化成语义残缺的一半——
    /// 实测里那半截就是「我不考虑你」后面跟一张简历图
    #[test]
    fn one_bad_item_holds_the_entire_batch() {
        let verdict = vet_outbound(
            vec![
                text_resource("您好，我对这个岗位很感兴趣"),
                text_resource("不过我暂时不考虑猎头渠道"),
                image("C:/resume.png"),
            ],
            200,
        );

        assert!(matches!(verdict, SendVerdict::Hold(_)));
    }

    #[test]
    fn a_clean_batch_passes_with_images_intact() {
        match vet_outbound(
            vec![text_resource("您好，我有相关项目经验"), image("C:/a.png")],
            200,
        ) {
            SendVerdict::Send(resources) => {
                assert_eq!(resources.len(), 2);
                assert_eq!(resources[1].resource_type, ReplayResourceType::Image);
            }
            other => panic!("干净的内容不该被拦，实际：{other:?}"),
        }
    }

    /// 图片路径不是给人读的文本，不能拿正文那套规则去体检它
    #[test]
    fn image_paths_are_never_checked_as_prose() {
        assert!(matches!(
            vet_outbound(
                vec![image("C:/xx公司/todo.png"), text_resource("您好")],
                200
            ),
            SendVerdict::Send(_)
        ));
    }

    fn stamped(mid: i64, time: i64, text: &str) -> ChatMessage {
        ChatMessage {
            mid,
            received: true,
            text: text.to_string(),
            time,
            from_name: String::new(),
        }
    }

    /// 接口只给最近一页，不合并历史等于让模型每轮都失忆
    #[test]
    fn history_and_fresh_pages_merge_into_one_timeline() {
        let merged = merge_messages(
            vec![stamped(1, 100, "很早以前"), stamped(2, 200, "稍早")],
            vec![stamped(3, 300, "本轮")],
        );

        let texts: Vec<&str> = merged.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(texts, vec!["很早以前", "稍早", "本轮"]);
    }

    /// 消息被撤回或编辑后，接口才是现状，库里那份是旧的
    #[test]
    fn the_fresh_page_wins_on_duplicate_ids() {
        let merged = merge_messages(
            vec![stamped(1, 100, "编辑前")],
            vec![stamped(1, 100, "编辑后")],
        );

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].text, "编辑后");
    }

    #[test]
    fn merging_sorts_by_time_even_when_both_sides_are_out_of_order() {
        let merged = merge_messages(
            vec![stamped(9, 900, "晚"), stamped(1, 100, "早")],
            vec![stamped(5, 500, "中")],
        );

        assert_eq!(
            merged.iter().map(|m| m.mid).collect::<Vec<_>>(),
            vec![1, 5, 9]
        );
    }

    fn template(pattern: &str, resources: Vec<ReplyResource>) -> ReplyTemplate {
        ReplyTemplate {
            regex_rule: crate::config::ReplyRegexRule {
                name: "测试规则".to_string(),
                pattern: pattern.to_string(),
                limit: 2,
            },
            content: resources,
        }
    }

    fn text_resource(content: &str) -> ReplyResource {
        ReplyResource {
            resource_type: ReplayResourceType::Text,
            content: content.to_string(),
        }
    }

    #[test]
    fn a_matching_template_short_circuits_the_model() {
        let templates = vec![template(
            "面试",
            vec![text_resource("好的，时间我这边可以")],
        )];
        let context = context(vec![message(true, "下周二方便来面试吗")]);

        let hit = match_template(&templates, &context).expect("命中的模板必须返回资源");

        assert_eq!(hit.rule_name, "测试规则");
        assert_eq!(hit.resources.len(), 1);
        assert_eq!(hit.resources[0].content, "好的，时间我这边可以");
    }

    /// 出厂默认模板就是「pattern: .* + 一条空的 LLM 槽」。
    /// 它必须等价于没命中，否则所有会话都会被这条模板拦死，模型永远轮不上
    #[test]
    fn a_template_with_only_llm_slots_falls_through_to_the_model() {
        let templates = vec![template(
            ".*",
            vec![ReplyResource {
                resource_type: ReplayResourceType::LLM,
                content: String::new(),
            }],
        )];

        assert!(match_template(&templates, &context(vec![message(true, "在吗")])).is_none());
    }

    /// 写错的正则不该让整轮回复崩掉，跳过这条继续看下一条
    #[test]
    fn an_invalid_pattern_is_skipped_instead_of_failing() {
        let templates = vec![
            template("[未闭合", vec![text_resource("不该命中")]),
            template("在吗", vec![text_resource("在的")]),
        ];

        let hit = match_template(&templates, &context(vec![message(true, "在吗")])).unwrap();

        assert_eq!(hit.resources[0].content, "在的");
    }

    #[test]
    fn transcript_labels_who_said_what() {
        let transcript = context(vec![message(true, "在吗"), message(false, "在的")]).transcript();

        assert_eq!(transcript, "HR: 在吗\n我: 在的");
    }
}
