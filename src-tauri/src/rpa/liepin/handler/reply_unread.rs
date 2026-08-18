use std::{collections::HashSet, time::Duration};

use anyhow::Context;
use rust_drission::{utils::sleep_random_ms, Page};
use serde::Deserialize;

use crate::{
    agent::{tasks::ReplyDecisionTask, AgentRunner},
    auto_analysis,
    browser,
    config::{AnalysisTrigger, AppRuntimeConfig},
    dao::{
        auto_reply_log_dao, chat_message_dao, job_detail_dao,
        model::{AutoReplyAction, JobDetail, ManualReviewReason},
        profile_snapshot_dao,
    },
    logger,
    rpa::{
        common::ChatMessage,
        conversation::{
            self, choose_route, gate, reconcile, ConversationActions, ConversationContext,
            GateVerdict, ReplyAction, ReplyLimits, ReplyRoute, ResumeState,
        },
        liepin::{
            handler::{
                actions::LiepinActions,
                position_say_hello::{install_request_recorder, send_resources},
            },
            LIEPIN_USER_HOME_URL,
        },
        reply_effects::{hold_for_review, record_auto_send, release_if_handled, wait_before_reply},
        run_flow::{is_job_task_stop_requested, PlatformKind},
    },
};

/// 落库时的平台标识。和 [`chat_message_dao`] 里的会话主键保持一致
const PLATFORM: &str = "liepin";

/// 沟通入口气泡，点开后弹出右侧会话抽屉
const CHAT_ENTRY: &str = ".im-ui-basic-entry";
/// 抽屉里的会话卡
const CONTACT_ITEM: &str = ".im-ui-contact-list-item";
/// 聊天窗的消息列表容器。会话列表和聊天窗同屏共存，
/// 从 DOM 取消息必须限定在这个作用域里，否则会把左侧会话摘要一起抓进来
const MSG_LIST: &str = ".im-ui-msg-list-content";

/// 猎聘 IM 接口。抽屉打开时页面自己会拉会话列表，点开会话时会拉历史消息，
/// 被动抓响应即可拿到结构化数据 —— 和 Boss 拦 `zpchat/geek/historyMsg` 是同一套路数。
///
/// 抓包走的是页面里的 fetch/XHR 记录器（`install_request_recorder`），不是 CDP 监听：
/// 同一套记录器在图片发送里已经抓到过 `com.liepin.im.c.chat.send-push`，
/// 而 CDP 那条路在猎聘这个页面上实测收不到包。
const CONTACT_LIST_API: &str = "com.liepin.im.c.contact.get-contact-list";
const CHAT_LIST_API: &str = "com.liepin.im.c.chat.chat-list";
const API_WAIT_MS: u32 = 8000;

/// 列表懒加载的兜底上限。未读消息一定排在列表顶部（按最后消息时间倒序），
/// 滚动只是为了捞长列表里的漏网之鱼，不需要翻到底
const MAX_SCROLL_ROUNDS: usize = 10;
const OPEN_DRAWER_RETRIES: usize = 3;

pub async fn reply_unread(config: &AppRuntimeConfig) -> Result<Vec<ChatMessage>, anyhow::Error> {
    let config = config.clone();

    browser::with_new_tab(|page| Box::pin(async move { reply_unread_on_page(page, &config).await }))
        .await
}

/// Process Liepin unread conversations on a caller-owned task tab.
pub async fn reply_unread_on_page(
    page: &Page,
    config: &AppRuntimeConfig,
) -> Result<Vec<ChatMessage>, anyhow::Error> {
    let config = config.clone();
    logger::info("正在打开猎聘沟通面板")?;
    page.get(LIEPIN_USER_HOME_URL)?;

    // 会话列表是抽屉打开那一刻才拉的，记录器必须赶在那次点击之前挂上
    let mut since = open_chat_drawer(page)?;

    // 处理完的会话未读角标会消失，但列表会随新消息重排，
    // 用会话 id 去重才不会漏处理或重复处理
    let mut handled: HashSet<String> = HashSet::new();
    let mut scrolled = 0usize;
    // 未读堆积时不能一口气全处理完：单轮耗时一旦超过轮询间隔，
    // 后面每一轮都在追赶上一轮的尾巴，节奏彻底乱掉。剩下的留给下一轮
    let max_per_round = config
        .reply_polling_config
        .max_conversations_per_round
        .max(1);

    loop {
        if is_job_task_stop_requested() {
            logger::info("猎聘沟通任务已结束")?;
            return Ok(Vec::new());
        }

        if handled.len() >= max_per_round {
            logger::info(format!(
                "猎聘本轮已处理 {max_per_round} 个会话，达到单轮上限，其余留待下一轮"
            ))?;
            return Ok(Vec::new());
        }

        let (contacts, source) = collect_unread_contacts(page, since)?;
        let pending: Vec<UnreadContact> = contacts
            .into_iter()
            .filter(|contact| !handled.contains(&contact.imid))
            .collect();

        if pending.is_empty() {
            if scrolled >= MAX_SCROLL_ROUNDS {
                logger::info("猎聘会话列表已翻到上限，停止查找")?;
                return Ok(Vec::new());
            }
            // 滚动会触发下一页会话列表，时间基准要往前推，免得读到上一次的响应
            since = install_request_recorder(page)?;
            if !scroll_contact_list(page)? {
                if handled.is_empty() {
                    logger::info("猎聘没有未读消息")?;
                } else {
                    logger::info(format!(
                        "猎聘未读消息已处理完成，共 {} 个会话",
                        handled.len()
                    ))?;
                }
                return Ok(Vec::new());
            }
            scrolled += 1;
            continue;
        }

        logger::info(format!(
            "猎聘当前未读会话数: {}（取自{}）",
            pending.len(),
            source.label()
        ))?;
        let total = pending.len();
        for (index, contact) in pending.into_iter().enumerate() {
            if is_job_task_stop_requested() {
                logger::info("猎聘沟通任务已结束")?;
                return Ok(Vec::new());
            }

            if handled.len() >= max_per_round {
                logger::info(format!(
                    "猎聘本轮已处理 {max_per_round} 个会话，达到单轮上限，其余留待下一轮"
                ))?;
                return Ok(Vec::new());
            }

            // 先记账再点击：这一轮无论成功失败都不再重试同一个会话，避免死循环
            handled.insert(contact.imid.clone());

            // 点开会话时页面会拉历史消息，时间基准同样要抢在点击之前取
            let chat_since = install_request_recorder(page)?;
            if !open_contact(page, &contact.imid)? {
                logger::warning(format!(
                    "猎聘会话「{}」已从列表消失，跳过",
                    truncate_str(&contact.display_name(), 20)
                ))?;
                continue;
            }

            let (fresh_messages, message_source) = collect_chat_messages(page, chat_since)?;
            let latest = fresh_messages
                .last()
                .map(|m| m.text.as_str())
                .unwrap_or("(空)");
            logger::info(format!(
                "处理猎聘第 {}/{} 个会话「{}」，最近消息: {}",
                index + 1,
                total,
                truncate_str(&contact.display_name(), 20),
                truncate_str(latest, 30)
            ))?;

            handle_conversation(page, &config, &contact, &fresh_messages, message_source).await?;

            sleep_random_ms(2500, 4500);
        }
    }
}

/// 处理单个会话：落库 → 合并历史 → 闸门 → 模型决策 → 执行动作。
///
/// 模型调用和消息发送这两处失败只记日志、不往外抛：一次生成超时或一个聊天窗卡住，
/// 不该让整轮未读处理中断，剩下的会话仍然值得处理。
async fn handle_conversation(
    page: &Page,
    config: &AppRuntimeConfig,
    contact: &UnreadContact,
    fresh_messages: &[ChatMessage],
    message_source: Source,
) -> Result<(), anyhow::Error> {
    let name = truncate_str(&contact.display_name(), 20).to_string();

    // 只有接口取数才带稳定的 mid（雪花 id）；DOM 兜底的 mid 是列表下标，
    // 落库会和真实消息串主键、合并历史也会按下标错误去重，
    // 所以那一路只当本轮上下文用，不入库
    let from_api = message_source == Source::Api;

    // 历史必须在写入之前读，否则读回来的就是刚写进去的这一页
    let history = if from_api {
        chat_message_dao::find_by_conversation(PLATFORM, &contact.imid).unwrap_or_default()
    } else {
        Vec::new()
    };
    let messages = conversation::merge_messages(history, fresh_messages.to_vec());

    let owned_job = resolve_owned_job(contact, &messages);
    if from_api {
        let key = chat_message_dao::ConversationKey {
            platform: PLATFORM,
            conversation_id: &contact.imid,
            // 映射不到岗位也要存：会话本身就是主键，岗位归属之后某轮识别出来再回填
            job_id: owned_job.as_ref().map(|job| job.id.as_str()).unwrap_or(""),
        };
        if let Err(error) = chat_message_dao::upsert_incremental(key, fresh_messages) {
            logger::warning(format!("猎聘聊天记录保存失败: {}", error))?;
        }
    }

    let (conversation_config, source) =
        profile_snapshot_dao::resolve_for_job(config, owned_job.as_ref())
            .map_err(anyhow::Error::msg)?;
    match source {
        profile_snapshot_dao::ResolutionSource::Snapshot => {}
        profile_snapshot_dao::ResolutionSource::CurrentProfile => {
            logger::warning("猎聘会话的方案快照缺失，按当前同名方案回复")?;
        }
        profile_snapshot_dao::ResolutionSource::DefaultProfile => {
            logger::warning(format!(
                "猎聘会话「{}」无法可靠映射到历史岗位，按默认方案回复",
                name
            ))?;
        }
    }

    // 登记要在自动回复的开关判断之前：对方回没回和这轮要不要自动回复是两件事，
    // 只开分析、不开自动回复也应该拿得到分析报告
    if let Some(job) = owned_job.as_ref() {
        if messages.iter().any(|message| message.received) {
            auto_analysis::schedule(job, AnalysisTrigger::ReplyReceived, &conversation_config);
        }
    }

    if !conversation_config.replay_config.auto_reply_enabled() {
        logger::info("猎聘会话所属求职方案的 LLM 回复与回复模板都未启用，跳过")?;
        return Ok(());
    }

    let limits = ReplyLimits::from_config(&conversation_config.replay_config);
    let actions = LiepinActions;

    // 用户在平台里自己回过之后，待办就该消掉，不必等他回应用里手工点一次
    release_if_handled(PLATFORM, &contact.imid, &messages);

    let resume_state = actions.resume_state(page)?;

    // 查不到流水按 0 算：节流数据缺失不该让整个会话卡死，
    // 最坏后果是多回一条，比该回的不回轻得多
    let auto_replies_in_window = auto_reply_log_dao::count_replies_within(
        PLATFORM,
        &contact.imid,
        limits.auto_reply_window_hours,
    )
    .unwrap_or(0);

    let context = ConversationContext {
        platform: PlatformKind::Liepin,
        conversation_id: contact.imid.clone(),
        job: owned_job,
        messages,
        resume_state,
        auto_replies_in_window,
    };

    match gate(&context, &limits) {
        GateVerdict::Skip(reason) => {
            logger::info(format!("猎聘会话「{}」跳过：{}", name, reason))?;
            return Ok(());
        }
        // 转人工就是不自动处理，连简历请求都留给用户自己拿主意 ——
        // 触发转人工的正是证件、账户这类不该由机器代答的话题
        GateVerdict::Escalate { reason, kind } => {
            logger::warning(format!("猎聘会话「{}」转人工：{}", name, reason))?;
            hold_for_review(
                PLATFORM,
                &contact.imid,
                conversation::review_draft(context.job.as_ref(), &context.messages),
                kind,
                reason,
            );
            return Ok(());
        }
        GateVerdict::Proceed => {}
    }

    // 对方来要简历就同意，和 Boss 侧"别人请求、我们确认发送"是同一条规则。
    // 放在取完消息之后，这条请求本身也会进上下文，回复时模型知道刚同意过
    if resume_state == ResumeState::RequestedByPeer {
        if limits.dry_run {
            logger::info("猎聘演练模式：对方索要简历，实际运行时会点同意")?;
        } else if actions.accept_resume_request(page)? {
            logger::info("猎聘对方索要简历，已点同意")?;
            record_auto_send(
                PLATFORM,
                &contact.imid,
                job_id_of(&context),
                AutoReplyAction::Resume,
                0,
            );
        }
    }

    match choose_route(&conversation_config.replay_config, &context) {
        // 确定性模板优先于模型：用户显式写了正则和话术，就说明这类消息他要的是
        // 稳定可预期的答复，不该每次再花额度让模型重新发挥一遍
        ReplyRoute::Template(hit) => {
            let rule = hit.display_name();
            let count = hit.resources.len();

            if limits.dry_run {
                logger::info(format!(
                    "猎聘演练模式：命中回复规则「{}」，本应发送 {} 条资源",
                    rule, count
                ))?;
                return Ok(());
            }

            // 固定话术不过发送前体检：那是给模型生成内容做的体检，
            // 拿它去截断或否决用户的原话，只会让"固定回复"变得不固定
            if let Err(error) = send_resources(page, hit.resources) {
                logger::warning(format!("猎聘模板回复发送失败，跳过该会话：{}", error))?;
                return Ok(());
            }
            logger::info(format!(
                "猎聘命中回复规则「{}」，已发送 {} 条资源",
                rule, count
            ))?;
            record_auto_send(
                PLATFORM,
                &contact.imid,
                job_id_of(&context),
                AutoReplyAction::Template,
                0,
            );
            return Ok(());
        }
        ReplyRoute::Skip(reason) => {
            logger::info(format!("猎聘会话「{}」跳过：{}", name, reason))?;
            return Ok(());
        }
        ReplyRoute::Decide => {}
    }

    let decision = match AgentRunner::new(&conversation_config)
        .with_cancel(is_job_task_stop_requested)
        .run(&ReplyDecisionTask::new(&conversation_config, &context))
        .await
    {
        Ok(outcome) => outcome.output,
        Err(error) => {
            logger::warning(format!("猎聘回复决策失败，本轮不回复：{}", error))?;
            return Ok(());
        }
    };

    let action = reconcile(&decision, resume_state, &limits);
    let label = action_label(action);
    if !action.needs_text() {
        // 决策理由不含求职者隐私，打出来用户才知道机器为什么按兵不动
        let message = format!("猎聘会话「{}」{}：{}", name, label, decision.reason);
        if action == ReplyAction::Escalate {
            logger::warning(message)?;
        } else {
            logger::info(message)?;
        }
        return Ok(());
    }

    let text = match conversation::vet_reply(&decision.reply, limits.max_reply_chars) {
        Ok(text) => text,
        Err(reason) => {
            logger::warning(format!("猎聘回复正文未通过体检，不发送：{}", reason))?;
            // 模型确实生成了内容，只是不能发。会话已经被点成已读，
            // 不挂起的话这条消息就彻底没人管了
            hold_for_review(
                PLATFORM,
                &contact.imid,
                conversation::review_draft(context.job.as_ref(), &context.messages),
                ManualReviewReason::VetRejected,
                format!("生成的回复未通过发送前体检：{reason}"),
            );
            return Ok(());
        }
    };

    if limits.dry_run {
        logger::info(format!(
            "猎聘演练模式：本应{}（正文 {} 字），理由：{}",
            label,
            text.chars().count(),
            decision.reason
        ))?;
        return Ok(());
    }

    if !wait_before_reply(&config.reply_polling_config, &context).await {
        logger::info("猎聘等待发送时机时任务已停止，本条回复未发送")?;
        return Ok(());
    }

    // 正文含求职者隐私，日志只留字数和理由
    match actions.send_text(page, &text) {
        Ok(true) => {
            logger::info(format!(
                "猎聘回复已发送（正文 {} 字），理由：{}",
                text.chars().count(),
                decision.reason
            ))?;
            record_auto_send(
                PLATFORM,
                &contact.imid,
                job_id_of(&context),
                AutoReplyAction::Reply,
                text.chars().count(),
            );
        }
        Ok(false) => logger::warning("猎聘回复正文为空，未发送")?,
        Err(error) => {
            logger::warning(format!("猎聘回复发送失败，跳过该会话：{}", error))?;
            return Ok(());
        }
    }

    if action == ReplyAction::ReplyAndSendResume {
        if actions.send_resume(page)? {
            record_auto_send(
                PLATFORM,
                &contact.imid,
                job_id_of(&context),
                AutoReplyAction::Resume,
                0,
            );
        } else {
            logger::info("猎聘没有主动投递简历的入口，本轮只回复了消息")?;
        }
    }

    Ok(())
}

/// 会话的岗位归属。猎聘经常映射不到岗位，流水里留空即可——
/// 节流和消项都按会话标识走，不依赖岗位
fn job_id_of(context: &ConversationContext) -> &str {
    context
        .job
        .as_ref()
        .map(|job| job.id.as_str())
        .unwrap_or_default()
}

fn action_label(action: ReplyAction) -> &'static str {
    match action {
        ReplyAction::Reply => "回复",
        ReplyAction::ReplyAndSendResume => "回复并投递简历",
        ReplyAction::Skip => "不回复",
        ReplyAction::Escalate => "转人工",
    }
}

/// 猎聘当前接口没有直接给出岗位 ID。只有公司唯一命中，或岗位标题也能从消息中
/// 明确命中时才建立归属；存在歧义时宁可回退默认方案，不能猜测后自动发送。
fn resolve_owned_job(contact: &UnreadContact, messages: &[ChatMessage]) -> Option<JobDetail> {
    if contact.company.trim().is_empty() {
        return None;
    }
    let candidates = job_detail_dao::find_by_company(&contact.company).ok()?;
    select_owned_job(candidates, messages)
}

fn select_owned_job(candidates: Vec<JobDetail>, messages: &[ChatMessage]) -> Option<JobDetail> {
    let liepin = candidates
        .into_iter()
        .filter(|job| job.platform.eq_ignore_ascii_case("liepin"))
        .collect::<Vec<_>>();
    if liepin.len() == 1 {
        return liepin.into_iter().next();
    }
    let title_matches = liepin
        .into_iter()
        .filter(|job| {
            !job.title.trim().is_empty()
                && messages
                    .iter()
                    .any(|message| message.text.contains(&job.title))
        })
        .collect::<Vec<_>>();
    (title_matches.len() == 1).then(|| title_matches[0].clone())
}

/// 打开右侧会话抽屉，返回抓包的时间基准。
///
/// 首页水合完成前点入口，轻则抽屉不弹，重则触发一次整页重载（两种都实测到过）。
/// 重载会把页面里的请求记录器一起冲掉，所以记录器必须每次点击前重挂一遍，
/// 返回的基准也只能取自真正打开抽屉的那一次点击。
fn open_chat_drawer(page: &Page) -> Result<f64, anyhow::Error> {
    for attempt in 1..=OPEN_DRAWER_RETRIES {
        page.wait(CHAT_ENTRY, Duration::from_secs(15))
            .context("没找到猎聘沟通入口，请确认已登录候选人端")?;
        sleep_random_ms(1200, 1800);

        let since = install_request_recorder(page)?;
        page.click(CHAT_ENTRY)?;

        if page.wait(CONTACT_ITEM, Duration::from_secs(10)).is_ok() {
            sleep_random_ms(600, 1000);
            return Ok(since);
        }

        logger::warning(format!(
            "猎聘会话列表没打开（第 {}/{} 次），重试",
            attempt, OPEN_DRAWER_RETRIES
        ))?;
    }

    Err(anyhow::anyhow!("猎聘会话抽屉打不开，已重试多次"))
}

#[derive(Debug, Clone)]
struct UnreadContact {
    /// 会话唯一 id。接口里叫 `oppositeImId`，DOM 埋点里叫 `data-tlg-ext.to_imid`，
    /// 实测两者是同一个值，所以接口取数、DOM 点击可以混用
    imid: String,
    name: String,
    company: String,
}

impl UnreadContact {
    fn display_name(&self) -> String {
        if self.company.is_empty() {
            self.name.clone()
        } else {
            format!("{}·{}", self.name, self.company)
        }
    }
}

/// 数据来源，用来在日志里说清这一轮是接口还是页面元素给的
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Api,
    Dom,
}

impl Source {
    fn label(self) -> &'static str {
        match self {
            Source::Api => "接口",
            Source::Dom => "页面元素",
        }
    }
}

/// 取未读会话：优先用接口响应，拿不到再退回 DOM。
///
/// 接口给的是精确的 `unReadCnt`；DOM 兜底看的是红色角标 `ant-im-badge-count`。
/// 两条路都不碰会话摘要里那个 `[未读]`（`im-ui-message-read-status`）——
/// 它表示"我方发出的消息对方还没看"，语义相反，用它会把所有已回复的会话又发一遍。
fn collect_unread_contacts(
    page: &Page,
    since: f64,
) -> Result<(Vec<UnreadContact>, Source), anyhow::Error> {
    // 列表翻到底时页面本来就不会再发请求，读不到不算异常，静默回落即可
    if let Some(body) = read_api_body(page, CONTACT_LIST_API, since)? {
        match parse_unread_contacts(&body) {
            Ok(contacts) => return Ok((contacts, Source::Api)),
            Err(error) => {
                logger::warning(format!("猎聘会话列表接口解析失败，改用页面元素: {}", error))?
            }
        }
    }

    Ok((collect_unread_contacts_from_dom(page)?, Source::Dom))
}

fn parse_unread_contacts(body: &str) -> Result<Vec<UnreadContact>, anyhow::Error> {
    let response: ContactListResponse =
        serde_json::from_str(body).context("会话列表响应不是预期的 JSON")?;

    Ok(response
        .data
        .map(|data| data.list)
        .unwrap_or_default()
        .into_iter()
        .filter(|contact| contact.un_read_cnt > 0 && !contact.opposite_im_id.is_empty())
        .map(|contact| UnreadContact {
            imid: contact.opposite_im_id,
            name: contact.name,
            company: contact.company,
        })
        .collect())
}

fn collect_unread_contacts_from_dom(page: &Page) -> Result<Vec<UnreadContact>, anyhow::Error> {
    let value = page
        .run_js_await(
            r#"
            (() => {
                const items = Array.from(document.querySelectorAll(".im-ui-contact-list-item"));
                return items.map((item) => {
                    let ext = {};
                    try {
                        ext = JSON.parse(decodeURIComponent(item.getAttribute("data-tlg-ext") || "")) || {};
                    } catch (e) {}
                    const unread = !!item.querySelector(".ant-im-badge-count") || ext.unread === true;
                    const imid = String(ext.to_imid || "");
                    if (!unread || !imid) return null;
                    const info = item.querySelector(".im-ui-contact-item-info");
                    const lines = ((info && info.innerText) || "")
                        .split("\n")
                        .map((line) => line.trim())
                        .filter(Boolean);
                    return { imid, name: lines[0] || "", company: "" };
                }).filter(Boolean);
            })()
            "#,
        )
        .context("读取猎聘未读会话失败")?;

    let raw = value.get("value").cloned().unwrap_or(value);
    let contacts: Vec<DomContact> = serde_json::from_value(raw).context("解析猎聘未读会话失败")?;

    Ok(contacts
        .into_iter()
        .map(|contact| UnreadContact {
            imid: contact.imid,
            name: contact.name,
            company: contact.company,
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct DomContact {
    imid: String,
    name: String,
    company: String,
}

/// 按会话 id 点开聊天窗。返回 false 表示卡片已经不在列表里（被新消息挤走或列表重排）
fn open_contact(page: &Page, imid: &str) -> Result<bool, anyhow::Error> {
    // imid 是十六进制串，URL 编码后原样保留，可以直接做属性子串匹配
    let selector = format!("{}[data-tlg-ext*='{}']", CONTACT_ITEM, imid);
    let Some(card) = page.ele(&selector)? else {
        return Ok(false);
    };

    card.click()?;
    page.wait(MSG_LIST, Duration::from_secs(10))
        .context("猎聘聊天窗加载超时")?;
    sleep_random_ms(800, 1200);
    Ok(true)
}

/// 取聊天记录：优先用接口响应，拿不到再退回 DOM。
///
/// 一并返回来源，是因为两条路的 `mid` 含义不同：接口给的是雪花 id，
/// DOM 兜底只能拿列表下标顶上，后者不能落库。
fn collect_chat_messages(
    page: &Page,
    since: f64,
) -> Result<(Vec<ChatMessage>, Source), anyhow::Error> {
    match read_api_body(page, CHAT_LIST_API, since)? {
        // 整段都是系统提示/营销卡时会解析成空，这时 DOM 也给不出更多，直接返回空
        Some(body) => match parse_chat_messages(&body) {
            Ok(messages) => Ok((messages, Source::Api)),
            Err(error) => {
                logger::warning(format!("猎聘聊天记录接口解析失败，改用页面元素: {}", error))?;
                Ok((collect_chat_messages_from_dom(page)?, Source::Dom))
            }
        },
        None => {
            logger::warning("猎聘聊天记录接口未捕获，改用页面元素")?;
            Ok((collect_chat_messages_from_dom(page)?, Source::Dom))
        }
    }
}

/// 解析 `com.liepin.im.c.chat.chat-list` 响应。
///
/// 几个坑：接口按时间倒序返回，要反转成正序；`payload` 是被转义了一层的 JSON 字符串，
/// 得二次解析；卡片类消息的 `msgType` 也是 `txt`，只能靠 `ext.extType` 区分。
pub(crate) fn parse_chat_messages(body: &str) -> Result<Vec<ChatMessage>, anyhow::Error> {
    let response: ChatListResponse =
        serde_json::from_str(body).context("聊天记录响应不是预期的 JSON")?;

    let mut messages: Vec<ChatMessage> = response
        .data
        .map(|data| data.list)
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .filter_map(|(index, record)| {
            if record.revoke_flag {
                return None;
            }

            let payload: MessagePayload = serde_json::from_str(&record.payload).ok()?;
            let text = payload_text(&payload)?;

            Some(ChatMessage {
                // 雪花 id 超出 JS 安全整数所以接口发的是字符串，i64 装得下
                mid: record.msg_id.parse::<i64>().unwrap_or(index as i64),
                // direction: "0" 是自己发的，"1" 是对方发来的
                received: record.direction == "1",
                text,
                time: record.msg_time,
                from_name: String::new(),
            })
        })
        .collect();

    // 接口是新→旧，模板匹配和 LLM 都按旧→新读
    messages.reverse();
    Ok(messages)
}

/// 把 payload 还原成一句能喂给模板/LLM 的话。
/// 返回 None 表示这条不该进上下文（系统提示、营销卡片、空消息）。
fn payload_text(payload: &MessagePayload) -> Option<String> {
    let ext_type = payload.ext.as_ref().map(|ext| ext.ext_type).unwrap_or(1);
    let biz_data = payload
        .ext
        .as_ref()
        .and_then(|ext| ext.ext_body.as_ref())
        .and_then(|body| body.biz_data.as_ref());

    match ext_type {
        // 岗位卡片：占位符是"[职位卡片]"，没有信息量，换成真正的岗位信息
        201 => {
            let job = biz_data?;
            let title = job.job_title.as_deref().unwrap_or_default().trim();
            if title.is_empty() {
                return None;
            }
            let mut text = format!("[职位] {}", title);
            if let Some(company) = job
                .job_company
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                text.push_str(&format!("｜{}", company));
            }
            if let Some(salary) = job
                .job_salary
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                text.push_str(&format!("｜{}", salary));
            }
            Some(text)
        }
        // 200 系统提示（"您向对方发起沟通"、防诈骗提醒）、214 优先沟通营销卡，都是噪音
        200 | 214 => None,
        _ => {
            let body = payload.bodies.first()?;
            if body.body_type == "img" {
                // 图片没有 msg 字段，用文件名比"[图片]"更有信息量
                return Some(match body.filename.as_deref().map(str::trim) {
                    Some(name) if !name.is_empty() => format!("[图片] {}", name),
                    _ => "[图片]".to_string(),
                });
            }
            let text = body.msg.as_deref().unwrap_or_default().trim();
            if text.is_empty() {
                None
            } else {
                Some(text.to_string())
            }
        }
    }
}

fn collect_chat_messages_from_dom(page: &Page) -> Result<Vec<ChatMessage>, anyhow::Error> {
    let value = page
        .run_js_await(
            r#"
            (() => {
                const list = document.querySelector(".im-ui-msg-list-content");
                if (!list) return [];
                const messages = [];
                Array.from(list.querySelectorAll(".im-ui-message-item")).forEach((node, index) => {
                    // 安全提示、"您向对方发起沟通"、运营卡片都没有 send/receive 主体，不是真实对话
                    const body = node.querySelector(
                        ".im-ui-message-item-send, .im-ui-message-item-receive"
                    );
                    if (!body) return;
                    const received = body.classList.contains("im-ui-message-item-receive");
                    const txt = body.querySelector(".im-ui-txt");
                    let text = txt ? (txt.innerText || "").trim() : "";
                    if (!text && body.querySelector(".im-ui-img-message")) text = "[图片]";
                    if (!text) text = (body.innerText || "").trim();
                    if (!text) return;
                    messages.push({
                        mid: index,
                        received,
                        text,
                        time: Date.now(),
                        from_name: ""
                    });
                });
                return messages;
            })()
            "#,
        )
        .context("读取猎聘聊天记录失败")?;

    let raw = value.get("value").cloned().unwrap_or(value);
    let messages =
        serde_json::from_value::<Vec<ChatMessage>>(raw).context("解析猎聘聊天记录失败")?;

    Ok(messages)
}

/// 从页面的请求记录里捞 `since` 之后这个接口的最后一条成功响应，捞不到返回 None。
///
/// 记录器只收 POST，猎聘 IM 接口都是 POST；同一个会话反复点开会有多条记录，取最后一条。
fn read_api_body(page: &Page, api: &str, since: f64) -> Result<Option<String>, anyhow::Error> {
    let script = format!(
        r#"
        (async () => {{
            const since = {since};
            const api = {api};
            const deadline = performance.now() + {API_WAIT_MS};
            const pick = () => (window.__fjRequestRecords || [])
                .filter((item) => item.at > since
                    && item.status >= 200 && item.status < 300
                    && String(item.url || "").includes(api)
                    && item.body)
                .pop();

            let hit = pick();
            while (!hit && performance.now() < deadline) {{
                await new Promise((resolve) => setTimeout(resolve, 200));
                hit = pick();
            }}
            return hit ? hit.body : "";
        }})()
        "#,
        since = since,
        api = serde_json::to_string(api).unwrap_or_else(|_| "\"\"".to_string()),
    );

    let value = page
        .run_js_await(&script)
        .with_context(|| format!("读取猎聘 {} 响应失败", api))?;
    let raw = value.get("value").cloned().unwrap_or(value);
    let body = raw.as_str().unwrap_or_default().to_string();

    Ok(if body.is_empty() { None } else { Some(body) })
}

#[derive(Debug, Deserialize)]
struct ContactListResponse {
    data: Option<ContactListData>,
}

#[derive(Debug, Deserialize)]
struct ContactListData {
    #[serde(default)]
    list: Vec<ContactRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContactRecord {
    #[serde(default)]
    opposite_im_id: String,
    #[serde(default)]
    un_read_cnt: i64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    company: String,
}

#[derive(Debug, Deserialize)]
struct ChatListResponse {
    data: Option<ChatListData>,
}

#[derive(Debug, Deserialize)]
struct ChatListData {
    #[serde(default)]
    list: Vec<ChatRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatRecord {
    #[serde(default)]
    msg_id: String,
    /// "0" 自己发出，"1" 对方发来。接口给的是字符串不是数字
    #[serde(default)]
    direction: String,
    #[serde(default)]
    msg_time: i64,
    /// 转义了一层的 JSON 字符串
    #[serde(default)]
    payload: String,
    #[serde(default)]
    revoke_flag: bool,
}

#[derive(Debug, Deserialize)]
struct MessagePayload {
    #[serde(default)]
    bodies: Vec<PayloadBody>,
    #[serde(default)]
    ext: Option<PayloadExt>,
}

#[derive(Debug, Deserialize)]
struct PayloadBody {
    #[serde(default, rename = "type")]
    body_type: String,
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    filename: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PayloadExt {
    /// 1 普通消息，200 系统提示，201 岗位卡片，214 优先沟通卡
    #[serde(default = "default_ext_type")]
    ext_type: i64,
    #[serde(default)]
    ext_body: Option<PayloadExtBody>,
}

fn default_ext_type() -> i64 {
    1
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PayloadExtBody {
    #[serde(default)]
    biz_data: Option<BizData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BizData {
    #[serde(default)]
    job_title: Option<String>,
    #[serde(default)]
    job_company: Option<String>,
    #[serde(default)]
    job_salary: Option<String>,
}

/// 会话列表的滚动容器是 `im-ui-contacts-wrap` 里一个没有 class 的 div，
/// 只能从会话卡往上找第一个能滚的祖先
fn scroll_contact_list(page: &Page) -> Result<bool, anyhow::Error> {
    let value = page.run_js_await(
        r#"
        (async () => {
            const item = document.querySelector(".im-ui-contact-list-item");
            if (!item) return false;
            let el = item;
            while (el && el.scrollHeight <= el.clientHeight + 5) el = el.parentElement;
            if (!el) return false;
            const before = el.scrollTop;
            el.scrollTop += el.clientHeight;
            await new Promise((resolve) => setTimeout(resolve, 800));
            return el.scrollTop !== before;
        })()
        "#,
    )?;

    let raw = value.get("value").cloned().unwrap_or(value);
    Ok(raw.as_bool().unwrap_or(false))
}

fn truncate_str(s: &str, max_len: usize) -> &str {
    if s.chars().count() <= max_len {
        s
    } else {
        let end = s
            .char_indices()
            .nth(max_len)
            .map(|(i, _)| i)
            .unwrap_or(s.len());
        &s[..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ReplayResourceType, ReplyResource, ReplyTemplate};

    fn job(id: &str, title: &str) -> JobDetail {
        JobDetail {
            id: id.into(),
            platform: "liepin".into(),
            source_task_id: None,
            profile_id: Some(id.into()),
            profile_name: None,
            profile_snapshot_id: None,
            title: title.into(),
            company_name: "同一家公司".into(),
            detail: String::new(),
            salary: String::new(),
            location: None,
            is_reply: false,
            is_send_resume: false,
            created_at: String::new(),
            resume_sent_at: None,
            updated_at: String::new(),
        }
    }

    fn message(text: &str) -> ChatMessage {
        ChatMessage {
            mid: 1,
            received: true,
            text: text.into(),
            time: 1,
            from_name: String::new(),
        }
    }

    #[test]
    fn only_contacts_with_unread_count_are_returned() {
        let body = r#"{
            "flag": 1,
            "data": {
                "list": [
                    {"oppositeImId":"aaa","unReadCnt":0,"name":"张女士","company":"甲公司"},
                    {"oppositeImId":"bbb","unReadCnt":2,"name":"姜女士","company":"乙公司"},
                    {"oppositeImId":"","unReadCnt":5,"name":"没有会话 id","company":""}
                ]
            }
        }"#;

        let contacts = parse_unread_contacts(body).unwrap();

        assert_eq!(contacts.len(), 1);
        assert_eq!(contacts[0].imid, "bbb");
        assert_eq!(contacts[0].display_name(), "姜女士·乙公司");
    }

    /// direction 是字符串 "0"/"1"，且接口按时间倒序下发
    #[test]
    fn messages_are_reordered_and_direction_is_mapped() {
        let body = r#"{
            "data": {
                "list": [
                    {"msgId":"1363464068980146182","direction":"1","msgTime":1786588521000,
                     "payload":"{\"bodies\":[{\"type\":\"txt\",\"msg\":\"周期一年的项目考虑吗\"}],\"ext\":{\"extType\":1}}"},
                    {"msgId":"1363464068980146100","direction":"0","msgTime":1786538091000,
                     "payload":"{\"bodies\":[{\"type\":\"txt\",\"msg\":\"你好，岗位还在招吗\"}],\"ext\":{\"extType\":1}}"}
                ]
            }
        }"#;

        let messages = parse_chat_messages(body).unwrap();

        assert_eq!(messages.len(), 2);
        // 旧的排前面
        assert_eq!(messages[0].text, "你好，岗位还在招吗");
        assert!(!messages[0].received, "direction=0 是自己发的");
        assert_eq!(messages[1].text, "周期一年的项目考虑吗");
        assert!(messages[1].received, "direction=1 是对方发来的");
        assert_eq!(messages[1].mid, 1363464068980146182);
        assert_eq!(messages[1].time, 1786588521000);
    }

    /// 卡片消息的 msgType 也是 txt，只能靠 extType 区分
    #[test]
    fn system_and_marketing_cards_are_dropped_but_job_card_is_kept() {
        let body = r#"{
            "data": {
                "list": [
                    {"msgId":"4","direction":"0","msgTime":4,
                     "payload":"{\"bodies\":[{\"type\":\"txt\",\"msg\":\"[优先沟通]\"}],\"ext\":{\"extType\":214}}"},
                    {"msgId":"3","direction":"0","msgTime":3,
                     "payload":"{\"bodies\":[{\"type\":\"txt\",\"msg\":\"[职位卡片]\"}],\"ext\":{\"extType\":201,\"extBody\":{\"bizData\":{\"jobTitle\":\"AI应用工程师\",\"jobCompany\":\"宏远科创\",\"jobSalary\":\"8-14k\"}}}}"},
                    {"msgId":"2","direction":"0","msgTime":2,
                     "payload":"{\"bodies\":[{\"type\":\"img\",\"filename\":\"简历图片.png\"}],\"ext\":{\"extType\":1}}"},
                    {"msgId":"1","direction":"0","msgTime":1,
                     "payload":"{\"bodies\":[{\"type\":\"txt\",\"msg\":\"您向对方发起沟通\"}],\"ext\":{\"extType\":200}}"}
                ]
            }
        }"#;

        let texts: Vec<String> = parse_chat_messages(body)
            .unwrap()
            .into_iter()
            .map(|message| message.text)
            .collect();

        assert_eq!(
            texts,
            vec![
                "[图片] 简历图片.png",
                "[职位] AI应用工程师｜宏远科创｜8-14k"
            ]
        );
    }

    /// 对方索要简历是 extType=206，正文就是那句问话，必须留在上下文里，
    /// 否则回复时看不出刚刚同意过发简历
    #[test]
    fn resume_request_card_stays_in_context() {
        let body = r#"{
            "data": {
                "list": [
                    {"msgId":"1","direction":"1","msgTime":1,
                     "payload":"{\"bodies\":[{\"type\":\"txt\",\"msg\":\"我想要一份你的简历，你是否同意？\"}],\"ext\":{\"extType\":206,\"extBody\":{\"bizType\":\"7\",\"bizData\":{\"bizKey\":\"20003\",\"status\":\"1\"}}},\"push\":\"1\"}"}
                ]
            }
        }"#;

        let messages = parse_chat_messages(body).unwrap();

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "我想要一份你的简历，你是否同意？");
        assert!(messages[0].received);
    }

    #[test]
    fn revoked_messages_are_skipped() {
        let body = r#"{
            "data": {
                "list": [
                    {"msgId":"1","direction":"1","msgTime":1,"revokeFlag":true,
                     "payload":"{\"bodies\":[{\"type\":\"txt\",\"msg\":\"说错了\"}],\"ext\":{\"extType\":1}}"}
                ]
            }
        }"#;

        assert!(parse_chat_messages(body).unwrap().is_empty());
    }

    #[test]
    fn empty_payload_list_is_not_an_error() {
        assert!(parse_chat_messages(r#"{"flag":1,"data":{"list":[]}}"#)
            .unwrap()
            .is_empty());
        assert!(parse_unread_contacts(r#"{"flag":1}"#).unwrap().is_empty());
    }

    fn chat(mid: i64, time: i64, received: bool, text: &str) -> ChatMessage {
        ChatMessage {
            mid,
            received,
            text: text.into(),
            time,
            from_name: String::new(),
        }
    }

    /// 接口只给最近一页，缺了前面的往来模型会把聊过的问题重新问一遍
    #[test]
    fn history_and_current_page_are_merged_in_time_order() {
        let history = vec![
            chat(1, 100, false, "您好，岗位还在招吗"),
            chat(2, 200, true, "在的，方便发下简历吗"),
        ];
        let fresh = [chat(3, 300, true, "周期一年的项目考虑吗")];

        let merged = conversation::merge_messages(history, fresh.to_vec());

        let texts: Vec<&str> = merged.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "您好，岗位还在招吗",
                "在的，方便发下简历吗",
                "周期一年的项目考虑吗"
            ]
        );
    }

    /// 库里存的正是上一轮写进去的接口消息，不按 mid 去重就会整页翻倍
    #[test]
    fn overlapping_messages_are_deduplicated_by_mid() {
        let history = vec![chat(1, 100, true, "在吗"), chat(2, 200, false, "在的")];
        let fresh = [
            chat(2, 200, false, "在的"),
            chat(3, 300, true, "什么时候方便"),
        ];

        let merged = conversation::merge_messages(history, fresh.to_vec());

        assert_eq!(merged.len(), 3);
        assert_eq!(merged.iter().filter(|m| m.mid == 2).count(), 1);
    }

    /// 撤回、编辑之后接口给的才是现状，同一 mid 不能被库里的旧内容盖回去
    #[test]
    fn the_current_page_wins_when_the_same_message_changed() {
        let history = vec![chat(7, 100, true, "旧内容")];
        let fresh = [chat(7, 100, true, "新内容")];

        let merged = conversation::merge_messages(history, fresh.to_vec());

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].text, "新内容");
    }

    /// 接口按时间倒序下发，库里也可能乱序，合并后必须重新按时间升序
    #[test]
    fn merged_messages_are_sorted_even_when_both_sides_are_unordered() {
        let history = vec![chat(9, 900, true, "第四"), chat(1, 100, false, "第一")];
        let fresh = [chat(5, 500, true, "第三"), chat(3, 300, false, "第二")];

        let merged = conversation::merge_messages(history, fresh.to_vec());

        let times: Vec<i64> = merged.iter().map(|m| m.time).collect();
        assert_eq!(times, vec![100, 300, 500, 900]);
    }

    #[test]
    fn merging_without_history_keeps_the_current_page_as_is() {
        let fresh = [chat(1, 100, true, "在吗")];

        assert_eq!(
            conversation::merge_messages(Vec::new(), fresh.to_vec()).len(),
            1
        );
        assert!(
            conversation::merge_messages(vec![chat(1, 100, true, "在吗")], Vec::new()).len() == 1
        );
    }

    fn context(messages: Vec<ChatMessage>) -> ConversationContext {
        ConversationContext {
            platform: PlatformKind::Liepin,
            conversation_id: "imid-abc".into(),
            job: None,
            messages,
            resume_state: ResumeState::Unavailable,
            auto_replies_in_window: 0,
        }
    }

    fn template(name: &str, pattern: &str, content: Vec<ReplyResource>) -> ReplyTemplate {
        ReplyTemplate {
            regex_rule: crate::config::ReplyRegexRule {
                name: name.into(),
                pattern: pattern.into(),
                limit: 2,
            },
            content,
        }
    }

    fn text_resource(content: &str) -> ReplyResource {
        ReplyResource {
            resource_type: ReplayResourceType::Text,
            content: content.into(),
        }
    }

    /// 两条路径都开着的方案，模板和模型的分流由平台无关层统一决定
    fn replay_config(templates: Vec<ReplyTemplate>) -> crate::config::ReplayConfig {
        let mut config = crate::config::default_app_config().replay_config;
        config.enable_template_reply = true;
        config.enable_llm = true;
        config.templates = templates;
        config
    }

    /// 用户写死的话术必须原样发出，不能被模型链路顶掉，
    /// 否则这次改造等于把存量正则配置全部架空
    #[test]
    fn a_matching_template_short_circuits_the_decision_chain() {
        let config = replay_config(vec![template(
            "面试邀约",
            "面试",
            vec![text_resource("好的，时间我这边可以")],
        )]);

        let route = choose_route(
            &config,
            &context(vec![chat(1, 100, true, "下周二方便来面试吗")]),
        );

        match route {
            ReplyRoute::Template(hit) => {
                assert_eq!(hit.display_name(), "面试邀约");
                assert_eq!(hit.resources, vec![text_resource("好的，时间我这边可以")]);
            }
            other => panic!("应命中模板，实际：{other:?}"),
        }
    }

    /// 两条规则配了相同话术时，日志必须仍能认出是哪条命中——
    /// 这正是规则名要跟着资源一起回传、而不是靠内容反查的原因
    #[test]
    fn the_matched_rule_is_identified_even_when_two_rules_share_the_same_reply() {
        let config = replay_config(vec![
            template("薪资询问", "薪资", vec![text_resource("方便电话聊吗")]),
            template("面试邀约", "面试", vec![text_resource("方便电话聊吗")]),
        ]);

        let route = choose_route(
            &config,
            &context(vec![chat(1, 100, true, "下周二方便来面试吗")]),
        );

        match route {
            ReplyRoute::Template(hit) => assert_eq!(hit.display_name(), "面试邀约"),
            other => panic!("应命中面试邀约，实际：{other:?}"),
        }
    }

    /// 出厂默认模板是「.* + 一条空的 LLM 槽」。它要是算命中，
    /// 所有会话都会被拦死在模板这一步，模型永远轮不上
    #[test]
    fn the_default_llm_only_template_still_reaches_the_decision_chain() {
        let config = replay_config(vec![template(
            "默认",
            ".*",
            vec![ReplyResource {
                resource_type: ReplayResourceType::LLM,
                content: String::new(),
            }],
        )]);

        assert!(matches!(
            choose_route(&config, &context(vec![chat(1, 100, true, "在吗")])),
            ReplyRoute::Decide
        ));
    }

    #[test]
    fn unmatched_messages_go_to_the_decision_chain() {
        let config = replay_config(vec![template(
            "面试邀约",
            "面试",
            vec![text_resource("好的")],
        )]);

        assert!(matches!(
            choose_route(&config, &context(vec![chat(1, 100, true, "期望薪资多少")])),
            ReplyRoute::Decide
        ));
    }

    #[test]
    fn ambiguous_company_never_guesses_job_ownership() {
        let candidates = vec![job("one", "Rust 工程师"), job("two", "Java 工程师")];
        assert!(select_owned_job(candidates, &[message("方便聊聊吗")]).is_none());
    }

    #[test]
    fn ambiguous_company_can_use_unique_explicit_job_title() {
        let candidates = vec![job("one", "Rust 工程师"), job("two", "Java 工程师")];
        let selected = select_owned_job(candidates, &[message("Rust 工程师岗位方便聊聊吗")]);
        assert_eq!(selected.map(|job| job.id), Some("one".into()));
    }
}
