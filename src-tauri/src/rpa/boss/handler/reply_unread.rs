use std::{collections::HashSet, time::Duration};

use anyhow::{anyhow, Context};
use rust_drission::utils::sleep_random_ms;

use crate::{
    agent::{tasks::ReplyDecisionTask, AgentRunner},
    auto_analysis,
    browser,
    config::{AnalysisTrigger, AppRuntimeConfig},
    dao::{
        auto_reply_log_dao, chat_message_dao, job_detail_dao,
        model::{AutoReplyAction, ManualReviewReason},
        profile_snapshot_dao,
    },
    logger,
    rpa::{
        boss::{
            handler::{actions::BossActions, chat_list, chat_list::ListState, send_messages},
            model::{ChatMessage, UnreadChat},
            BOSS_CHAT_URL,
        },
        conversation::{
            self, ConversationActions, ConversationContext, GateVerdict, ReplyAction, ReplyLimits,
            ReplyRoute, ResumeState,
        },
        reply_effects::{hold_for_review, record_auto_send, release_if_handled, wait_before_reply},
        run_flow::{is_job_task_stop_requested, PlatformKind},
    },
};

const PLATFORM: &str = "boss";

pub async fn reply_unread(
    app_runtime_config: &AppRuntimeConfig,
) -> Result<Vec<UnreadChat>, anyhow::Error> {
    let app_runtime_config = app_runtime_config.clone();
    browser::with_new_tab(|page| {
        Box::pin(async move { reply_unread_on_page(page, &app_runtime_config).await })
    })
    .await
}

/// Process BOSS unread conversations on a caller-owned task tab.
pub async fn reply_unread_on_page(
    page: &rust_drission::Page,
    app_runtime_config: &AppRuntimeConfig,
) -> Result<Vec<UnreadChat>, anyhow::Error> {
    let app_runtime_config = app_runtime_config.clone();
    logger::info("正在打开沟通页面")?;
    page.get(BOSS_CHAT_URL).context("打开 BOSS 沟通页面失败")?;
    chat_list::wait_for_chat_page(page, Duration::from_secs(20))?;

    // 每轮重新拉一次未读列表再处理一个会话。两个原因：点开会话会让它变已读、
    // 从未读列表消失，整列 DOM 跟着重排，一次性取出的卡片快照从第二个起就指向
    // 了脱离文档的节点，点了不报错也不生效；而列表本身是分页的，处理掉几个之后
    // 后面的才会补上来。handled 只是最后一道防线，兜住「处理完仍赖在列表里」的
    // 会话，免得同一张卡片被无限重试
    let mut handled: HashSet<String> = HashSet::new();
    // 未读堆积时不能一口气全处理完：单轮耗时一旦超过轮询间隔，
    // 后面每一轮都在追赶上一轮的尾巴，节奏彻底乱掉。剩下的留给下一轮
    let max_per_round = app_runtime_config
        .reply_polling_config
        .max_conversations_per_round
        .max(1);
    loop {
        if is_job_task_stop_requested() {
            logger::info("沟通任务已结束")?;
            return Ok(Vec::new());
        }

        if handled.len() >= max_per_round {
            logger::info(format!(
                "本轮已处理 {max_per_round} 个会话，达到单轮上限，其余留待下一轮"
            ))?;
            break;
        }

        let cards = match chat_list::reload_label(page, "未读", Duration::from_secs(15))? {
            ListState::Empty => {
                logger::info("没有未读消息")?;
                break;
            }
            ListState::Unknown => {
                logger::warning(
                    "未读列表既没出现会话卡片也没出现空态提示，状态无法判定，本次跳过",
                )?;
                break;
            }
            ListState::Cards(cards) => cards,
        };

        // 列表分页加载，渲染出来的往往少于总数；标签上的计数才是真实剩余量
        let remaining = chat_list::selected_count(page)?.unwrap_or(cards.len());
        logger::info(format!("当前未读会话数: {remaining}"))?;

        let mut pending = None;
        for card in cards.iter() {
            let key = chat_list::conversation_key(card)?;
            if !key.is_empty() && !handled.contains(&key) {
                pending = Some((card, key));
                break;
            }
        }
        let Some((user_card_ele, key)) = pending else {
            logger::warning(format!(
                "未读列表里这 {} 个会话本轮都已处理过却仍未消失，结束本轮",
                cards.len()
            ))?;
            break;
        };
        handled.insert(key.clone());

        let history_listener = page.listen_url("zpchat/geek/historyMsg")?;
        let boss_data_listener = page.listen_url("zpchat/geek/getBossData")?;
        user_card_ele.click()?;
        sleep_random_ms(500, 800);

        let job_id = match boss_data_listener.wait(Duration::from_secs(10)) {
            Ok(Some(packet)) => packet
                .body
                .and_then(|body| String::from_utf8(body).ok())
                .and_then(|body| parse_encrypt_job_id(&body)),
            _ => None,
        };

        let packet = match history_listener.wait(Duration::from_secs(10)) {
            Ok(Some(p)) => p,
            Ok(None) => {
                logger::warning("等待 historyMsg 响应超时，跳过")?;
                continue;
            }
            Err(e) => {
                logger::warning(format!("监听 historyMsg 失败: {e}，跳过"))?;
                continue;
            }
        };
        let Some(body_bytes) = packet.body else {
            logger::warning("historyMsg 响应 body 为空，跳过")?;
            continue;
        };
        let body_str = String::from_utf8(body_bytes).context("historyMsg 响应非 UTF-8 编码")?;
        let fresh = parse_chat_messages(&body_str)?;

        // 处理这一个会话时出的错不该让整轮未读中断：后面还有别的会话等着
        if let Err(error) =
            handle_conversation(page, &app_runtime_config, job_id.as_deref(), &key, fresh).await
        {
            logger::warning(format!("处理会话失败，跳过该会话继续：{error}"))?;
        }

        sleep_random_ms(3000, 5000);
    }
    Ok(Vec::new())
}

/// 处理单个已打开的会话。
///
/// 顺序上有两处是刻意安排的：
/// 一是先读完消息再处理简历请求，这样「对方索要简历」这条本身也进上下文，
/// 模型回复时知道刚刚同意过；
/// 二是先读简历入口状态再决定动作，绝不先点了按钮再看发生了什么。
async fn handle_conversation(
    page: &rust_drission::Page,
    app_runtime_config: &AppRuntimeConfig,
    job_id: Option<&str>,
    card_key: &str,
    fresh: Vec<ChatMessage>,
) -> Result<(), anyhow::Error> {
    let Some(job_id) = job_id else {
        // 拿不到 jobId 就没有稳定的会话标识，落库和历史合并都无从谈起。
        // 这种会话交给「读取聊天消息」任务补齐，不在这里猜。
        //
        // 但消息已经被点成已读了，就这么放着等于把它从手机上的未读里抹掉却没人回。
        // 卡片自己的标识足够在待办列表里定位到这个会话，用它挂起——只用于挂待办，
        // 不能拿来当聊天记录的主键，那会和正常路径的 jobId 主键格式撞成两份记录
        logger::warning("未获取到 jobId，本会话挂起等待人工处理")?;
        hold_for_review(
            PLATFORM,
            card_key,
            conversation::review_draft(None, &fresh),
            ManualReviewReason::MissingJobId,
            "会话标识读取失败，自动回复无法进行".to_string(),
        );
        return Ok(());
    };

    let key = chat_message_dao::ConversationKey {
        platform: PLATFORM,
        conversation_id: job_id,
        job_id,
    };
    // 历史必须在写库之前读，否则读回来的就是刚写进去的那一页
    let history = chat_message_dao::find_by_conversation(PLATFORM, job_id).unwrap_or_default();
    if let Err(error) = chat_message_dao::upsert_incremental(key, &fresh) {
        logger::warning(format!("保存聊天记录失败: {error}"))?;
    }
    let messages = conversation::merge_messages(history, fresh);

    let job = job_detail_dao::find_by_platform_job_id(PLATFORM, job_id)
        .ok()
        .flatten();
    let (conversation_config, source) =
        profile_snapshot_dao::resolve_for_job(app_runtime_config, job.as_ref())
            .map_err(anyhow::Error::msg)?;
    match source {
        profile_snapshot_dao::ResolutionSource::Snapshot => {}
        profile_snapshot_dao::ResolutionSource::CurrentProfile => {
            logger::warning("岗位方案快照缺失，按当前同名方案回复")?;
        }
        profile_snapshot_dao::ResolutionSource::DefaultProfile => {
            logger::warning("会话没有可恢复的求职方案归属，按默认方案回复")?;
        }
    }

    // 登记要在自动回复的开关判断之前：对方回没回和这轮要不要自动回复是两件事，
    // 只开分析、不开自动回复也应该拿得到分析报告
    if let Some(job) = job.as_ref() {
        if messages.iter().any(|message| message.received) {
            auto_analysis::schedule(job, AnalysisTrigger::ReplyReceived, &conversation_config);
        }
    }

    if !conversation_config.replay_config.auto_reply_enabled() {
        logger::info("该会话所属求职方案的 LLM 回复与回复模板都未启用，跳过")?;
        return Ok(());
    }

    let actions = BossActions;
    let limits = ReplyLimits::from_config(&conversation_config.replay_config);

    // 用户在平台里自己回过之后，待办就该消掉，不必等他回应用里手工点一次
    release_if_handled(PLATFORM, job_id, &messages);

    let resume_state = actions.resume_state(page)?;

    // 查不到流水按 0 算：节流数据缺失不该让整个会话卡死，
    // 最坏后果是多回一条，比该回的不回轻得多
    let auto_replies_in_window = auto_reply_log_dao::count_replies_within(
        PLATFORM,
        job_id,
        limits.auto_reply_window_hours,
    )
    .unwrap_or(0);

    let context = ConversationContext {
        platform: PlatformKind::Boss,
        conversation_id: job_id.to_string(),
        job,
        messages,
        resume_state,
        auto_replies_in_window,
    };

    logger::info(format!(
        "处理会话（{} 条消息，简历入口：{}）",
        context.messages.len(),
        describe_resume_state(resume_state)
    ))?;

    match conversation::gate(&context, &limits) {
        GateVerdict::Skip(reason) => {
            logger::info(format!("本会话跳过：{reason}"))?;
            return Ok(());
        }
        GateVerdict::Escalate { reason, kind } => {
            logger::warning(format!("本会话转人工，未自动处理：{reason}"))?;
            hold_for_review(
                PLATFORM,
                job_id,
                conversation::review_draft(context.job.as_ref(), &context.messages),
                kind,
                reason,
            );
            return Ok(());
        }
        GateVerdict::Proceed => {}
    }

    // 对方主动索要简历就同意——这是既定策略，不占模型的决策位。
    // 放在闸门之后：闸门拦下的正是诈骗特征那类会话，那时把简历发出去最不该
    if resume_state == ResumeState::RequestedByPeer {
        if limits.dry_run {
            logger::info("[演练] 对方索要简历，实际运行时会自动同意")?;
        } else if actions.accept_resume_request(page)? {
            logger::info("对方索要简历，已同意并发送")?;
            mark_resume_sent(job_id);
            record_auto_send(PLATFORM, job_id, job_id, AutoReplyAction::Resume, 0);
        }
    }

    match conversation::choose_route(&conversation_config.replay_config, &context) {
        // 用户显式配了正则和固定话术，说明这类消息他要的是稳定可预期的答复，
        // 不该每次再花额度让模型重新发挥一遍
        ReplyRoute::Template(hit) => {
            let rule = hit.display_name();
            let count = hit.resources.len();
            if limits.dry_run {
                logger::info(format!(
                    "[演练] 命中回复规则「{rule}」，本应发送 {count} 条内容"
                ))?;
                return Ok(());
            }
            // 固定话术不过发送前体检：那是给模型生成内容做的，
            // 拿它截断或否决用户写死的原话，只会让「固定回复」变得不固定
            send_messages(page, hit.resources)?;
            logger::info(format!("已按回复规则「{rule}」发送 {count} 条内容"))?;
            record_auto_send(PLATFORM, job_id, job_id, AutoReplyAction::Template, 0);
            return Ok(());
        }
        ReplyRoute::Skip(reason) => {
            logger::info(format!("本会话跳过：{reason}"))?;
            return Ok(());
        }
        ReplyRoute::Decide => {}
    }

    let outcome = AgentRunner::new(&conversation_config)
        .with_cancel(is_job_task_stop_requested)
        .run(&ReplyDecisionTask::new(&conversation_config, &context))
        .await?;
    let decision = outcome.output;
    let action = conversation::reconcile(&decision, resume_state, &limits);

    // 决策理由不含求职者隐私，可以完整记；正文不能记
    logger::info(format!(
        "模型判断：{}（把握 {} 分）——{}",
        describe_action(action),
        decision.confidence,
        decision.reason
    ))?;

    if !action.needs_text() {
        return Ok(());
    }

    let text = match conversation::vet_reply(&decision.reply, limits.max_reply_chars) {
        Ok(text) => text,
        Err(reason) => {
            logger::warning(format!("生成的回复未通过发送前体检，已放弃发送：{reason}"))?;
            // 模型确实生成了内容，只是不能发。会话已经被点成已读，
            // 不挂起的话这条消息就彻底没人管了
            hold_for_review(
                PLATFORM,
                job_id,
                conversation::review_draft(context.job.as_ref(), &context.messages),
                ManualReviewReason::VetRejected,
                format!("生成的回复未通过发送前体检：{reason}"),
            );
            return Ok(());
        }
    };

    if limits.dry_run {
        logger::info(format!(
            "[演练] 将发送 {} 字回复，不实际投递",
            text.chars().count()
        ))?;
        return Ok(());
    }

    if !wait_before_reply(&app_runtime_config.reply_polling_config, &context).await {
        logger::info("等待发送时机时任务已停止，本条回复未发送")?;
        return Ok(());
    }

    if !actions.send_text(page, &text)? {
        logger::warning("回复发送失败")?;
        return Ok(());
    }
    logger::info(format!("回复已发送（{} 字）", text.chars().count()))?;
    record_auto_send(PLATFORM, job_id, job_id, AutoReplyAction::Reply, text.chars().count());

    if action == ReplyAction::ReplyAndSendResume {
        sleep_random_ms(800, 1200);
        if actions.send_resume(page)? {
            logger::info("已主动投递简历")?;
            mark_resume_sent(job_id);
            record_auto_send(PLATFORM, job_id, job_id, AutoReplyAction::Resume, 0);
        } else {
            logger::warning("简历投递入口不可用，本轮只发送了回复")?;
        }
    }

    Ok(())
}

fn describe_resume_state(state: ResumeState) -> &'static str {
    match state {
        ResumeState::Sendable => "可投递",
        ResumeState::RequestedByPeer => "对方索要中",
        ResumeState::Unavailable => "不可投递",
        ResumeState::Unknown => "未识别",
    }
}

fn describe_action(action: ReplyAction) -> &'static str {
    match action {
        ReplyAction::Reply => "回复",
        ReplyAction::ReplyAndSendResume => "回复并投递简历",
        ReplyAction::Skip => "不回复",
        ReplyAction::Escalate => "转人工",
    }
}

// 从 historyMsg 接口响应中解析消息列表
//
// 发送方判断逻辑：
//   第一条 body.type == 8 的消息携带 body.jobDesc.boss.uid（招聘者 uid）。
//   后续每条消息：from.uid == boss_uid 则 received=true（招聘者发来），否则 received=false（自己发送）。
pub(crate) fn parse_chat_messages(body: &str) -> Result<Vec<ChatMessage>, anyhow::Error> {
    let root: serde_json::Value = serde_json::from_str(body).context("historyMsg JSON 解析失败")?;

    let code = root.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code != 0 {
        return Err(anyhow!(
            "historyMsg 接口业务错误: code={}, message={}",
            code,
            root.get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
        ));
    }

    let messages = root
        .pointer("/zpData/messages")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("historyMsg 响应缺少 zpData.messages 数组"))?;

    // 从第一条 body.type == 8 的消息中取出 boss uid
    let boss_uid: Option<i64> = messages.iter().find_map(|msg| {
        let body_type = msg.pointer("/body/type").and_then(|v| v.as_i64())?;
        if body_type == 8 {
            msg.pointer("/body/jobDesc/boss/uid")
                .and_then(|v| v.as_i64())
        } else {
            None
        }
    });

    fn first_attachment_name(value: &serde_json::Value) -> Option<String> {
        match value {
            serde_json::Value::Object(map) => {
                for key in [
                    "fileName",
                    "filename",
                    "file_name",
                    "resumeName",
                    "attachmentName",
                ] {
                    if let Some(name) = map
                        .get(key)
                        .and_then(serde_json::Value::as_str)
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                    {
                        return Some(name.to_string());
                    }
                }
                map.values().find_map(first_attachment_name)
            }
            serde_json::Value::Array(items) => items.iter().find_map(first_attachment_name),
            _ => None,
        }
    }

    fn resume_attachment_text(body: &serde_json::Value) -> Option<String> {
        if body.get("type").and_then(serde_json::Value::as_i64) == Some(8) {
            return None;
        }

        let serialized = serde_json::to_string(body).ok()?.to_lowercase();
        let has_structured_resume = serialized.contains("\"resumeinfo\"")
            || serialized.contains("\"resumefile\"")
            || serialized.contains("\"resumeurl\"")
            || serialized.contains("\"resumename\"");
        let has_resume_name = serialized.contains("简历")
            || serialized.contains("resume")
            || serialized.contains("cv.");
        let has_file_marker = serialized.contains(".pdf")
            || serialized.contains(".doc")
            || serialized.contains("\"filename\"")
            || serialized.contains("\"file_name\"")
            || serialized.contains("\"fileurl\"")
            || serialized.contains("\"file_url\"")
            || serialized.contains("\"attachment");
        if !has_structured_resume && !(has_resume_name && has_file_marker) {
            return None;
        }

        Some(match first_attachment_name(body) {
            Some(name) => format!("简历文件：{name}"),
            None => "简历文件：已交换简历".to_string(),
        })
    }

    let result = messages
        .iter()
        .filter_map(|msg| {
            let body_type = msg.pointer("/body/type").and_then(|v| v.as_i64())?;
            let text = if body_type == 1 {
                msg.pointer("/body/text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            } else {
                resume_attachment_text(msg.get("body")?)?
            };
            let mid = msg.get("mid").and_then(|v| v.as_i64())?;
            let from_uid = msg.pointer("/from/uid").and_then(|v| v.as_i64());
            // received=true 表示招聘者（boss）发来的消息，false 表示自己发送的
            let received = match (boss_uid, from_uid) {
                (Some(boss), Some(from)) => from == boss,
                _ => true, // 无法判断时默认视为对方发来
            };
            let time = msg.get("time").and_then(|v| v.as_i64()).unwrap_or(0);
            let from_name = msg
                .pointer("/from/name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            Some(ChatMessage {
                mid,
                received,
                text,
                time,
                from_name,
            })
        })
        .collect();

    Ok(result)
}

/// 从 getBossData 响应中提取 encryptJobId
pub(crate) fn parse_encrypt_job_id(body: &str) -> Option<String> {
    let root: serde_json::Value = serde_json::from_str(body).ok()?;
    let code = root.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code != 0 {
        return None;
    }
    root.pointer("/zpData/data/encryptJobId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn mark_resume_sent(job_id: &str) {
    if let Some(mut job) = job_detail_dao::get_by_id(job_id).unwrap_or(None) {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        job.is_send_resume = true;
        job.resume_sent_at = Some(now.clone());
        job.updated_at = now;
        if let Err(e) = job_detail_dao::update(job_id, job) {
            let _ = logger::warning(format!("更新简历投递状态失败: {}", e));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_detects_resume_attachment_messages() {
        let messages = parse_chat_messages(
            r#"{
              "code": 0,
              "zpData": {
                "messages": [
                  {
                    "mid": 1,
                    "time": 1000,
                    "from": {"uid": 10, "name": "招聘者"},
                    "body": {"type": 8, "jobDesc": {"boss": {"uid": 10}}}
                  },
                  {
                    "mid": 2,
                    "time": 2000,
                    "from": {"uid": 20, "name": "我"},
                    "body": {
                      "type": 7,
                      "resumeInfo": {
                        "fileName": "张三-Java开发简历.pdf",
                        "fileUrl": "https://example.invalid/resume.pdf"
                      }
                    }
                  }
                ]
              }
            }"#,
        )
        .expect("简历附件消息应能解析");

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "简历文件：张三-Java开发简历.pdf");
        assert!(!messages[0].received);
    }

    #[test]
    fn extracts_encrypt_job_id_only_on_success_code() {
        assert_eq!(
            parse_encrypt_job_id(r#"{"code":0,"zpData":{"data":{"encryptJobId":"abc"}}}"#),
            Some("abc".to_string())
        );
        assert_eq!(
            parse_encrypt_job_id(r#"{"code":1,"zpData":{"data":{"encryptJobId":"abc"}}}"#),
            None
        );
    }

    #[test]
    fn resume_state_is_described_for_every_variant() {
        for state in [
            ResumeState::Sendable,
            ResumeState::RequestedByPeer,
            ResumeState::Unavailable,
            ResumeState::Unknown,
        ] {
            assert!(!describe_resume_state(state).is_empty());
        }
    }
}
