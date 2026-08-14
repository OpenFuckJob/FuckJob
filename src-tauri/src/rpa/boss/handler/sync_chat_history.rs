use std::{collections::HashSet, time::Duration};

use anyhow::Context;
use chrono::Local;
use rust_drission::utils::sleep_random_ms;
use serde::Serialize;
use serde_json::Value;

use crate::{
    browser,
    dao::{chat_message_dao, job_detail_dao, model::JobDetail},
    logger,
    rpa::{
        boss::{
            handler::{parse_chat_messages, parse_encrypt_job_id},
            model::ChatMessage,
            BOSS_CHAT_URL,
        },
        run_flow::is_job_task_stop_requested,
    },
};

#[derive(Debug, Default, Clone, Copy, Serialize)]
pub struct ChatHistorySyncResult {
    pub scanned: usize,
    pub conversations: usize,
    pub skipped_unread: usize,
    pub jobs_inserted: usize,
    pub jobs_updated: usize,
    pub inserted: usize,
    pub updated: usize,
}

#[derive(Debug, Default)]
struct SyncedJobSnapshot {
    title: Option<String>,
    company_name: Option<String>,
    detail: Option<String>,
    salary: Option<String>,
    location: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JobSyncChange {
    None,
    Inserted,
    Updated,
}

fn first_nonempty_string(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(text) = map
                    .get(*key)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                {
                    return Some(text.to_string());
                }
            }
            map.values()
                .find_map(|child| first_nonempty_string(child, keys))
        }
        Value::Array(items) => items
            .iter()
            .find_map(|child| first_nonempty_string(child, keys)),
        _ => None,
    }
}

/// 从聊天面板中的岗位元素 `<div class="position-content">` 提取岗位快照。
fn extract_job_snapshot_from_dom(page: &rust_drission::Page) -> SyncedJobSnapshot {
    let position_div = match page.element(".position-content") {
        Ok(Some(el)) => el,
        _ => return SyncedJobSnapshot::default(),
    };

    let title = position_div
        .element(".position-name")
        .ok()
        .flatten()
        .and_then(|el| el.text_content().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let salary = position_div
        .element(".salary")
        .ok()
        .flatten()
        .and_then(|el| el.text_content().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let location = position_div
        .element(".city")
        .ok()
        .flatten()
        .and_then(|el| el.text_content().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    SyncedJobSnapshot {
        title,
        salary,
        location,
        ..Default::default()
    }
}

fn parse_job_snapshot(body: &str) -> SyncedJobSnapshot {
    let Ok(root) = serde_json::from_str::<Value>(body) else {
        return SyncedJobSnapshot::default();
    };
    let scope = root.pointer("/zpData/data").unwrap_or(&root);
    SyncedJobSnapshot {
        title: first_nonempty_string(
            scope,
            &[
                "jobName",
                "positionName",
                "jobTitle",
                "positionTitle",
                "jobTitleName",
                "position",
            ],
        ),
        company_name: first_nonempty_string(
            scope,
            &[
                "brandName",
                "brandFullName",
                "companyName",
                "companyShortName",
                "company",
            ],
        ),
        detail: first_nonempty_string(scope, &["jobDetail", "jobDescription", "postDescription"]),
        salary: first_nonempty_string(scope, &["salaryDesc", "salaryDescription", "salary"]),
        location: first_nonempty_string(scope, &["cityName", "businessDistrict", "jobAddress"]),
    }
}

fn merge_snapshot(primary: SyncedJobSnapshot, fallback: SyncedJobSnapshot) -> SyncedJobSnapshot {
    SyncedJobSnapshot {
        title: primary.title.or(fallback.title),
        company_name: primary.company_name.or(fallback.company_name),
        detail: primary.detail.or(fallback.detail),
        salary: primary.salary.or(fallback.salary),
        location: primary.location.or(fallback.location),
    }
}

fn fill_if_empty(target: &mut String, value: Option<String>) -> bool {
    if !target.trim().is_empty() {
        return false;
    }
    let Some(value) = value.filter(|value| !value.trim().is_empty()) else {
        return false;
    };
    *target = value;
    true
}

fn fill_if_empty_or_placeholder(
    target: &mut String,
    value: Option<String>,
    placeholders: &[&str],
) -> bool {
    let current = target.trim();
    if !current.is_empty() && !placeholders.contains(&current) {
        return false;
    }
    let Some(value) = value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && !placeholders.contains(&value.as_str()))
    else {
        return false;
    };
    if current == value {
        return false;
    }
    *target = value;
    true
}

fn is_resume_file_message(message: &ChatMessage) -> bool {
    let normalized = message.text.trim().to_lowercase();
    if normalized.starts_with("简历文件：") || normalized.starts_with("简历文件:") {
        return true;
    }

    let has_resume_name =
        normalized.contains("简历") || normalized.contains("resume") || normalized.contains("cv.");
    let has_document_extension =
        normalized.contains(".pdf") || normalized.contains(".docx") || normalized.contains(".doc");
    has_resume_name && has_document_extension
}

fn upsert_synced_job(
    job_id: &str,
    snapshot: SyncedJobSnapshot,
    messages: &[ChatMessage],
) -> Result<JobSyncChange, anyhow::Error> {
    let has_received = messages.iter().any(|message| message.received);
    let has_resume_file = messages.iter().any(is_resume_file_message);
    let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    if let Some(mut job) = job_detail_dao::get_by_id(job_id)? {
        let mut changed = false;
        if has_received && !job.is_reply {
            job.is_reply = true;
            changed = true;
        }
        if has_resume_file && !job.is_send_resume {
            job.is_send_resume = true;
            if job.resume_sent_at.is_none() {
                job.resume_sent_at = Some(now.clone());
            }
            changed = true;
        }
        changed |= fill_if_empty_or_placeholder(&mut job.title, snapshot.title, &["聊天同步岗位"]);
        changed |= fill_if_empty_or_placeholder(
            &mut job.company_name,
            snapshot.company_name,
            &["BOSS 会话"],
        );
        changed |= fill_if_empty(&mut job.detail, snapshot.detail);
        changed |= fill_if_empty(&mut job.salary, snapshot.salary);
        if job.location.as_deref().is_none_or(str::is_empty) && snapshot.location.is_some() {
            job.location = snapshot.location;
            changed = true;
        }
        if changed {
            job.updated_at = now;
            job_detail_dao::update(job_id, job)?;
            return Ok(JobSyncChange::Updated);
        }
        return Ok(JobSyncChange::None);
    }

    let SyncedJobSnapshot {
        title,
        company_name,
        detail,
        salary,
        location,
    } = snapshot;
    let Some(title) = title
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && value != "聊天同步岗位")
    else {
        logger::warning(format!(
            "同步会话 {job_id} 时未获取到真实岗位名称，本次只保存聊天记录，不创建占位岗位"
        ))?;
        return Ok(JobSyncChange::None);
    };

    job_detail_dao::create(JobDetail {
        id: job_id.to_string(),
        platform: "boss".to_string(),
        source_task_id: None,
        profile_id: None,
        profile_name: None,
        profile_snapshot_id: None,
        title,
        company_name: company_name
            .map(|value| value.trim().to_string())
            .filter(|value| value != "BOSS 会话")
            .unwrap_or_default(),
        detail: detail.unwrap_or_default(),
        salary: salary.unwrap_or_default(),
        location,
        is_reply: has_received,
        is_send_resume: has_resume_file,
        created_at: now.clone(),
        resume_sent_at: has_resume_file.then(|| now.clone()),
        updated_at: now,
    })?;
    Ok(JobSyncChange::Inserted)
}

fn conversation_key(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn click_label(page: &rust_drission::Page, name: &str) -> Result<bool, anyhow::Error> {
    for label in page.eles(".label-list .label-name")? {
        if label.text_content()?.trim().starts_with(name) {
            label.click()?;
            sleep_random_ms(500, 800);
            return Ok(true);
        }
    }
    Ok(false)
}

fn scroll_conversation_list(page: &rust_drission::Page, reset: bool) -> Result<(), anyhow::Error> {
    let target = if reset {
        "0"
    } else {
        "scroller.scrollTop + Math.max(scroller.clientHeight * 0.8, 300)"
    };
    page.run_js_await(&format!(
        r#"
        (() => {{
          const list = document.querySelector('.user-list-content');
          if (!list) return;
          let scroller = list;
          while (scroller.parentElement && scroller.scrollHeight <= scroller.clientHeight) {{
            scroller = scroller.parentElement;
          }}
          scroller.scrollTop = {target};
          if ({reset}) return;
          const cards = list.querySelectorAll('.friend-content');
          cards[cards.length - 1]?.scrollIntoView({{ block: 'end' }});
        }})()
        "#
    ))?;
    sleep_random_ms(400, 650);
    Ok(())
}

/// 扫描当前分类中的会话标识，不点击任何会话卡片。
fn collect_current_conversation_keys(
    page: &rust_drission::Page,
) -> Result<HashSet<String>, anyhow::Error> {
    let mut keys = HashSet::new();
    let mut stagnant_rounds = 0;

    for _ in 0..50 {
        if is_job_task_stop_requested() {
            break;
        }
        let mut discovered = 0;
        for card in page.elements(".user-list-content .friend-content")? {
            let key = conversation_key(&card.text_content()?);
            if !key.is_empty() && keys.insert(key) {
                discovered += 1;
            }
        }
        if discovered == 0 {
            stagnant_rounds += 1;
            if stagnant_rounds >= 3 {
                break;
            }
        } else {
            stagnant_rounds = 0;
        }
        scroll_conversation_list(page, false)?;
    }

    Ok(keys)
}

/// 遍历 BOSS 沟通列表并增量同步历史消息。
pub async fn sync_chat_history() -> Result<ChatHistorySyncResult, anyhow::Error> {
    browser::with_new_tab(|page| Box::pin(async move { sync_chat_history_on_page(page).await }))
        .await
}

/// Synchronize BOSS history on a caller-owned task tab.
pub async fn sync_chat_history_on_page(
    page: &rust_drission::Page,
) -> Result<ChatHistorySyncResult, anyhow::Error> {
    logger::info("周期间歇：开始同步 BOSS 历史对话")?;
    page.get(BOSS_CHAT_URL)?;
    page.wait(".user-list-content", Duration::from_secs(15))?;
    sleep_random_ms(800, 1200);

    // 先在“未读”分类中只扫描标识，绝不点击会话卡片。
    // 如果无法识别未读分类，为避免误把消息标记为已读，本次同步直接跳过。
    if !click_label(page, "未读")? {
        logger::warning("未找到 BOSS 未读分类，为保护未读状态，本次不同步历史对话")?;
        return Ok(ChatHistorySyncResult::default());
    }
    let unread_keys = collect_current_conversation_keys(page)?;
    logger::info(format!(
        "检测到未读会话 {} 个，同步时将全部跳过",
        unread_keys.len()
    ))?;

    if !click_label(page, "全部")? {
        logger::warning("未找到 BOSS 全部会话分类，本次不同步历史对话")?;
        return Ok(ChatHistorySyncResult::default());
    }
    scroll_conversation_list(page, true)?;

    let mut result = ChatHistorySyncResult::default();
    let mut seen = HashSet::new();
    let mut stagnant_rounds = 0;

    // 会话列表为虚拟滚动列表：不断处理当前已渲染项并向下滚动，
    // 连续三轮没有新会话时认为到达底部。
    for _ in 0..50 {
        if is_job_task_stop_requested() {
            break;
        }

        let cards = page.elements(".user-list-content .friend-content")?;
        let mut discovered = 0;
        for card in cards {
            if is_job_task_stop_requested() {
                break;
            }

            let key = conversation_key(&card.text_content()?);
            if key.is_empty() || !seen.insert(key.clone()) {
                continue;
            }
            discovered += 1;
            result.scanned += 1;

            // 第一层：未读分类快照；第二层：点击前检查常见未读标记。
            // 任一命中都跳过，避免同步动作改变未读状态。
            let has_unread_marker = card
                        .element(
                            "[class*='unread'], .badge, .badge-count, .unread-count, .red-dot, .notice-badge",
                        )?
                        .is_some();
            if unread_keys.contains(&key) || has_unread_marker {
                result.skipped_unread += 1;
                continue;
            }

            let history_listener = page.listen_url("zpchat/geek/historyMsg")?;
            let boss_data_listener = page.listen_url("zpchat/geek/getBossData")?;
            card.click()?;
            sleep_random_ms(350, 600);

            let boss_data_body = boss_data_listener
                .wait(Duration::from_secs(6))
                .ok()
                .flatten()
                .and_then(|packet| packet.body)
                .and_then(|body| String::from_utf8(body).ok());
            let job_id = boss_data_body.as_deref().and_then(parse_encrypt_job_id);

            let history_body = history_listener
                .wait(Duration::from_secs(8))
                .ok()
                .flatten()
                .and_then(|packet| packet.body)
                .and_then(|body| String::from_utf8(body).ok());

            let (Some(job_id), Some(history_body)) = (job_id, history_body) else {
                logger::warning("同步会话时未获取到 jobId 或 historyMsg，已跳过")?;
                continue;
            };

            let messages =
                parse_chat_messages(&history_body).context("解析周期间歇历史对话失败")?;
            let dom_snapshot = extract_job_snapshot_from_dom(page);
            let api_snapshot = merge_snapshot(
                boss_data_body
                    .as_deref()
                    .map(parse_job_snapshot)
                    .unwrap_or_default(),
                parse_job_snapshot(&history_body),
            );
            let snapshot = merge_snapshot(dom_snapshot, api_snapshot);
            match upsert_synced_job(&job_id, snapshot, &messages)? {
                JobSyncChange::Inserted => result.jobs_inserted += 1,
                JobSyncChange::Updated => result.jobs_updated += 1,
                JobSyncChange::None => {}
            }
            let saved = chat_message_dao::upsert_incremental(
                chat_message_dao::ConversationKey {
                    platform: "boss",
                    conversation_id: &job_id,
                    job_id: &job_id,
                },
                &messages,
            )?;
            result.conversations += 1;
            result.inserted += saved.inserted;
            result.updated += saved.updated;
        }

        if discovered == 0 {
            stagnant_rounds += 1;
            if stagnant_rounds >= 3 {
                break;
            }
        } else {
            stagnant_rounds = 0;
        }

        scroll_conversation_list(page, false)?;
    }

    logger::info(format!(
                "BOSS 历史对话同步完成：扫描 {} 个，同步 {} 个，跳过未读 {} 个，新增岗位 {} 个，更新岗位 {} 个，新增消息 {} 条，更新消息 {} 条",
                result.scanned,
                result.conversations,
                result.skipped_unread,
                result.jobs_inserted,
                result.jobs_updated,
                result.inserted,
                result.updated
            ))?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_key_ignores_whitespace_differences() {
        assert_eq!(
            conversation_key("招聘者\n  示例公司\tJava 工程师"),
            "招聘者 示例公司 Java 工程师"
        );
    }

    #[test]
    fn conversation_key_rejects_blank_cards() {
        assert!(conversation_key(" \n\t ").is_empty());
    }

    #[test]
    fn parses_job_snapshot_from_boss_data() {
        let snapshot = parse_job_snapshot(
            r#"{
              "zpData": {
                "data": {
                  "jobName": "Java 开发工程师",
                  "brandName": "示例科技",
                  "salaryDesc": "15-25K",
                  "cityName": "上海"
                }
              }
            }"#,
        );

        assert_eq!(snapshot.title.as_deref(), Some("Java 开发工程师"));
        assert_eq!(snapshot.company_name.as_deref(), Some("示例科技"));
        assert_eq!(snapshot.salary.as_deref(), Some("15-25K"));
        assert_eq!(snapshot.location.as_deref(), Some("上海"));
    }

    #[test]
    fn parses_alternative_job_snapshot_fields() {
        let snapshot = parse_job_snapshot(
            r#"{
              "zpData": {
                "data": {
                  "jobInfo": {
                    "jobTitleName": "后端开发工程师",
                    "brandFullName": "示例信息技术有限公司"
                  }
                }
              }
            }"#,
        );

        assert_eq!(snapshot.title.as_deref(), Some("后端开发工程师"));
        assert_eq!(
            snapshot.company_name.as_deref(),
            Some("示例信息技术有限公司")
        );
    }

    #[test]
    fn replaces_chat_sync_placeholders_with_real_values() {
        let mut title = "聊天同步岗位".to_string();
        let mut company = "BOSS 会话".to_string();

        assert!(fill_if_empty_or_placeholder(
            &mut title,
            Some("Java 开发工程师".to_string()),
            &["聊天同步岗位"],
        ));
        assert!(fill_if_empty_or_placeholder(
            &mut company,
            Some("示例科技".to_string()),
            &["BOSS 会话"],
        ));
        assert_eq!(title, "Java 开发工程师");
        assert_eq!(company, "示例科技");
    }

    #[test]
    fn keeps_existing_real_job_name() {
        let mut title = "高级 Java 工程师".to_string();

        assert!(!fill_if_empty_or_placeholder(
            &mut title,
            Some("其他岗位".to_string()),
            &["聊天同步岗位"],
        ));
        assert_eq!(title, "高级 Java 工程师");
    }

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
                    "body": {
                      "type": 8,
                      "jobDesc": {"boss": {"uid": 10}}
                    }
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
        assert!(is_resume_file_message(&messages[0]));
    }

    #[test]
    fn plain_resume_request_is_not_treated_as_resume_file() {
        let message = ChatMessage {
            mid: 1,
            received: true,
            text: "方便发一份简历吗？".to_string(),
            time: 1000,
            from_name: "招聘者".to_string(),
        };

        assert!(!is_resume_file_message(&message));
    }
}
