use std::time::Duration;

use anyhow::{anyhow, Context};
use regex::Regex;
use rust_drission::utils::sleep_random_ms;

use crate::{
    browser,
    config::{AppRuntimeConfig, ReplayResourceType, ReplyResource, ReplyTemplate},
    dao::{chat_message_dao, job_detail_dao},
    llm, logger,
    rpa::{
        boss::{
            handler::{send_messages, send_resume},
            model::{ChatMessage, UnreadChat},
            BOSS_CHAT_URL,
        },
        run_flow::is_job_task_stop_requested,
    },
};

// 点击不同 label 的聊天
fn click_label(page: &rust_drission::Page, name: &str) -> Result<(), anyhow::Error> {
    let label_name_eles = page.eles(".label-list .label-name")?;

    for label_name_ele in label_name_eles {
        let label_content = label_name_ele.text_content()?.trim().to_string();
        if label_content == name || label_content.starts_with(name) {
            label_name_ele.click()?;
            break;
        }
    }
    Ok(())
}

pub async fn reply_unread(
    app_runtime_config: &AppRuntimeConfig,
) -> Result<Vec<UnreadChat>, anyhow::Error> {
    let app_runtime_config = app_runtime_config.clone();
    browser::with_new_tab(|page| {
        Box::pin(async move {
            logger::info("正在打开沟通页面")?;
            page.get(BOSS_CHAT_URL)?;
            page.wait(".label-list .label-name", Duration::from_secs(10))?;

            let replay_config = app_runtime_config.replay_config.clone();
            loop {
                if is_job_task_stop_requested() {
                    logger::info("沟通任务已结束")?;
                    return Ok(());
                }
                click_label(page, "未读")?;
                page.wait(".user-list-content", Duration::from_secs(10))?;
                sleep_random_ms(800, 1000);
                let user_card_eles = page.elements(".user-list-content .friend-content")?;
                if user_card_eles.is_empty() {
                    logger::info("没有未读消息")?;
                    break;
                }
                logger::info(format!("当前未读会话数: {}", user_card_eles.len()))?;
                for user_card_ele in user_card_eles {
                    if is_job_task_stop_requested() {
                        logger::info("沟通任务已结束")?;
                        return Ok(());
                    }

                    let history_listener = page.listen_url("zpchat/geek/historyMsg")?;
                    let boss_data_listener = page.listen_url("zpchat/geek/getBossData")?;
                    user_card_ele.click()?;

                    // 检查发送简历 是否被禁用 回复阶段 若对方没有回复 则无法发送简历
                    let btn = page.element(".toolbar-btn.unable")?;
                    if let Some(btn) = btn {
                        let aria_label = btn.attr("aria-label")?;
                        if aria_label.contains("等待对方回复") {
                            logger::info("简历已投递，跳过后续处理")?;
                            continue;
                        }
                    }

                    sleep_random_ms(500, 800);

                    let job_id = match boss_data_listener.wait(Duration::from_secs(10)) {
                        Ok(Some(packet)) => match packet.body {
                            Some(b) => {
                                let body_str = String::from_utf8(b).unwrap_or_default();
                                parse_encrypt_job_id(&body_str)
                            }
                            None => None,
                        },
                        _ => None,
                    };
                    // 别人请求给我们 我们确认并发送简历
                    if page.ele(".toolbar-btn")?.is_some() {
                        // 点击确认
                        page.click(".toolbar-btn")?;
                        // 同意
                        sleep_random_ms(500, 800);
                        if page.ele(".panel-resume")?.is_some() {
                            page.click(".panel-resume .btns .btn-sure-v2")?;
                            continue;
                        }
                    } else {
                        // 主动发送简历投递请求
                        send_resume(page)?;
                        logger::info("主动发起简历投递请求成功")?;
                        continue;
                    }

                    if let Some(ref jid) = job_id {
                        logger::info(format!("获取到 jobId: {}", jid))?;
                        mark_resume_sent(jid);
                    }

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

                    let body_bytes = match packet.body {
                        Some(b) => b,
                        None => {
                            logger::warning("historyMsg 响应 body 为空，跳过")?;
                            continue;
                        }
                    };
                    let body_str =
                        String::from_utf8(body_bytes).context("historyMsg 响应非 UTF-8 编码")?;

                    let chat_messages = parse_chat_messages(&body_str)?;
                    let last_msg = chat_messages
                        .last()
                        .map(|m| m.text.as_str())
                        .unwrap_or("(空)");
                    logger::info(format!(
                        "处理第会话，最近消息: {}",
                        truncate_str(last_msg, 30)
                    ))?;

                    // 增量保存聊天记录
                    if let Some(ref jid) = job_id {
                        if let Err(e) = chat_message_dao::save_incremental(jid, &chat_messages) {
                            let _ = logger::warning(format!("保存聊天记录失败: {}", e));
                        }
                    }

                    if let Some(resources) = resolve_reply_resources(
                        &app_runtime_config,
                        &replay_config,
                        &chat_messages,
                        job_id,
                    )
                    .await?
                    {
                        send_messages(page, resources)?;
                        logger::info("回复消息已发送")?;
                    } else {
                        logger::info("未匹配到回复模板，跳过")?;
                    }

                    sleep_random_ms(3000, 5000);
                }
                if is_job_task_stop_requested() {
                    break;
                }
            }
            Ok(())
        })
    })
    .await?;

    Ok(Vec::new())
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

    let result = messages
        .iter()
        .filter_map(|msg| {
            // 只保留 body.type == 1 的普通文本消息，其余消息（系统卡片等）过滤掉
            let body_type = msg.pointer("/body/type").and_then(|v| v.as_i64())?;
            if body_type != 1 {
                return None;
            }
            let text = msg
                .pointer("/body/text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
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

fn latest_chat_history(chat_messages: &[ChatMessage], limit: i32) -> String {
    let limit = usize::try_from(limit.max(1)).unwrap_or(1);
    let start = chat_messages.len().saturating_sub(limit);
    chat_messages[start..]
        .iter()
        .map(|message| message.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn match_reply_template<'a>(
    templates: &'a [ReplyTemplate],
    chat_messages: &[ChatMessage],
) -> Option<&'a ReplyTemplate> {
    templates.iter().find(|template| {
        Regex::new(&template.regex_rule.pattern)
            .map(|regex| {
                regex.is_match(&latest_chat_history(
                    chat_messages,
                    template.regex_rule.limit,
                ))
            })
            .unwrap_or(false)
    })
}

async fn resolve_reply_resources(
    app_runtime_config: &AppRuntimeConfig,
    replay_config: &crate::config::ReplayConfig,
    chat_messages: &[ChatMessage],
    job_id: Option<String>,
) -> Result<Option<Vec<ReplyResource>>, anyhow::Error> {
    logger::info(format!(
        "处理聊天记录（{} 条，内容已隐藏）",
        chat_messages.len()
    ))?;
    let matched = match_reply_template(&replay_config.templates, chat_messages);
    let Some(template) = matched else {
        return Ok(None);
    };

    let mut resources: Vec<ReplyResource> = Vec::new();

    for mut r in template.content.iter().cloned() {
        if r.resource_type == ReplayResourceType::LLM {
            if !replay_config.enable_llm || chat_messages.is_empty() {
                continue;
            }
            let Some(ref jid) = job_id else {
                logger::warning("LLM 资源需要 jobId，跳过")?;
                continue;
            };
            let result =
                match llm::generate_replay_text(jid.clone(), app_runtime_config, chat_messages)
                    .await
                {
                    Ok(result) if result.success && !result.data.trim().is_empty() => result,
                    Ok(_) => {
                        logger::warning("LLM 未生成可发送内容，已跳过自动发送")?;
                        continue;
                    }
                    Err(error) => {
                        logger::warning(format!("LLM 生成回复失败，已跳过自动发送: {}", error))?;
                        continue;
                    }
                };
            r.content = result.data;
        } else if r.content.trim().is_empty() {
            continue;
        }
        resources.push(r);
    }

    if resources.is_empty() {
        logger::info("模板匹配成功但无有效资源，跳过")?;
        return Ok(None);
    }
    Ok(Some(resources))
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
