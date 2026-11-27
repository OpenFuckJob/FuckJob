use std::{collections::HashSet, time::Duration};

use anyhow::Context;
use chrono::Local;
use rust_drission::utils::sleep_random_ms;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    browser,
    command::job::{is_explicit_rejection, is_interview_invite},
    dao::{
        job_detail_dao,
        model::JobDetail,
        sync_state_dao::{self, PlatformSyncMeta, SyncedJobSignal},
    },
    logger,
    rpa::{
        boss::{
            handler::chat_list::{self, ListState},
            BOSS_CHAT_URL,
        },
        run_flow::is_job_task_stop_requested,
    },
};

const PLATFORM: &str = "boss";
const MAX_ZERO_CLICK_SCROLLS: usize = 8;

#[derive(Debug, Default, Clone, Copy, Serialize)]
pub struct ChatHistorySyncResult {
    pub scanned: usize,
    pub conversations: usize,
    pub skipped_unread: usize,
    pub jobs_inserted: usize,
    pub jobs_updated: usize,
    pub inserted: usize,
    pub updated: usize,
    pub unread_total: u32,
    pub interview_detected: usize,
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

#[derive(Debug, Clone)]
pub(crate) struct BossListEntry {
    pub dedup_key: String,
    pub job_id: Option<String>,
    pub title: Option<String>,
    pub company_name: Option<String>,
    pub salary: Option<String>,
    pub location: Option<String>,
    pub unread_count: u32,
    pub last_message: Option<String>,
    pub last_message_at: i64,
    pub last_message_received: Option<bool>,
    pub status_tag: Option<String>,
    pub resume_sent: bool,
    pub rejected: bool,
    pub interview_invited: bool,
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

fn read_str_flat(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(text) = obj
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            return Some(text.to_string());
        }
    }
    None
}

fn read_i64_flat(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> i64 {
    for key in keys {
        if let Some(value) = obj.get(*key) {
            if let Some(n) = value.as_i64() {
                return n;
            }
            if let Some(s) = value.as_str().and_then(|s| s.trim().parse::<i64>().ok()) {
                return s;
            }
        }
    }
    0
}

fn read_bool_flat(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<bool> {
    for key in keys {
        if let Some(value) = obj.get(*key) {
            if let Some(b) = value.as_bool() {
                return Some(b);
            }
            if let Some(n) = value.as_i64() {
                return Some(n != 0);
            }
        }
    }
    None
}

fn normalize_timestamp_ms(raw: i64) -> i64 {
    if raw <= 0 {
        0
    } else if raw < 10_000_000_000 {
        raw * 1000
    } else {
        raw
    }
}

fn is_resume_preview(text: &str) -> bool {
    let lower = text.trim().to_lowercase();
    if lower.starts_with("简历文件：") || lower.starts_with("简历文件:") || lower.contains("[简历]") {
        return true;
    }
    let has_resume = lower.contains("简历") || lower.contains("resume") || lower.contains("cv.");
    let has_indicator =
        lower.contains(".pdf") || lower.contains(".docx") || lower.contains("已发送") || lower.contains("附件");
    has_resume && has_indicator
}

/// 零点击解析 BOSS 会话列表接口（`wapi/zprelation/friend/getGeekFriendList` 等）。
pub(crate) fn parse_boss_friend_list(body: &str) -> Result<Vec<BossListEntry>, anyhow::Error> {
    let root: Value = serde_json::from_str(body).context("BOSS 会话列表响应不是合法 JSON")?;
    let list = root
        .pointer("/zpData/result")
        .or_else(|| root.pointer("/zpData/friendList"))
        .or_else(|| root.pointer("/zpData/list"))
        .or_else(|| root.pointer("/zpData/data/result"))
        .and_then(Value::as_array);

    let Some(items) = list else {
        return Ok(Vec::new());
    };

    let mut entries = Vec::new();
    for item in items {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let job_id = read_str_flat(obj, &["encryptJobId", "jobId", "securityId"]);
        let title = read_str_flat(obj, &["jobName", "positionName", "jobTitle", "title"]);
        let company_name = read_str_flat(obj, &["brandName", "companyName", "brandFullName"]);
        let salary = read_str_flat(obj, &["salaryDesc", "salary"]);
        let location = read_str_flat(obj, &["cityName", "locationName"]);
        let boss_name = read_str_flat(obj, &["name", "bossName"]).unwrap_or_default();

        let unread_count =
            read_i64_flat(obj, &["unreadMsgCount", "unreadCount", "unreadNum"]).max(0) as u32;
        let last_message = read_str_flat(
            obj,
            &["lastText", "lastMsg", "lastMessage", "msgText", "preview"],
        );
        let last_message_at = normalize_timestamp_ms(read_i64_flat(
            obj,
            &["lastTS", "lastMsgTime", "updateTime", "lastTime"],
        ));

        let last_is_self = read_bool_flat(obj, &["lastIsSelf", "isSelf"]);
        let last_message_received = match last_is_self {
            Some(is_self) => Some(!is_self),
            None => (unread_count > 0).then_some(true),
        };

        let raw_status = read_str_flat(obj, &["statusText", "relationStatusDesc", "friendStatusDesc"]);
        let preview_text = last_message.as_deref().unwrap_or("");
        let replied = unread_count > 0 || last_message_received == Some(true);
        let rejected = raw_status
            .as_deref()
            .is_some_and(|s| s.contains("不合适") || s.contains("已结束"))
            || (replied && is_explicit_rejection(preview_text));
        let interview_invited = raw_status
            .as_deref()
            .is_some_and(|s| s.contains("面试"))
            || (replied && is_interview_invite(preview_text));
        let resume_sent = raw_status
            .as_deref()
            .is_some_and(|s| s.contains("已投") || s.contains("简历"))
            || is_resume_preview(preview_text);

        let status_tag = if rejected {
            Some("不合适".to_string())
        } else if interview_invited {
            Some("收到约面".to_string())
        } else if unread_count > 0 {
            Some(format!("{} 条未读", unread_count))
        } else {
            raw_status
        };

        let dedup_key = job_id.clone().unwrap_or_else(|| {
            format!(
                "{}|{}|{}",
                boss_name,
                company_name.as_deref().unwrap_or(""),
                title.as_deref().unwrap_or("")
            )
        });
        if dedup_key.trim_matches('|').is_empty() {
            continue;
        }

        entries.push(BossListEntry {
            dedup_key,
            job_id,
            title,
            company_name,
            salary,
            location,
            unread_count,
            last_message,
            last_message_at,
            last_message_received,
            status_tag,
            resume_sent,
            rejected,
            interview_invited,
        });
    }

    Ok(entries)
}

#[derive(Debug, Deserialize)]
struct DomBossCard {
    key: String,
    company: String,
    title: String,
    preview: String,
    unread_count: u32,
    status_tag: String,
}

/// 零点击 DOM 兜底采集：仅读取左侧 `.friend-content` 卡片上的可见文本与红点数，绝不点击任何卡片。
fn collect_boss_cards_zero_click(page: &rust_drission::Page) -> Result<Vec<BossListEntry>, anyhow::Error> {
    let value = page
        .run_js_await(
            r#"
            (() => {
              const cards = Array.from(document.querySelectorAll('.user-list-content .friend-content'));
              return cards.map((card) => {
                const titleBox = card.querySelector('.title-box');
                const key = ((titleBox && titleBox.innerText) || '').replace(/\s+/g, ' ').trim();
                if (!key) return null;
                const spans = titleBox
                  ? Array.from(titleBox.querySelectorAll('span')).map((el) => (el.innerText || '').trim()).filter(Boolean)
                  : [];
                const company = spans[1] || '';
                const title = spans[2] || '';
                const msgEl = card.querySelector('.last-msg-text, .msg-text, .gray, .text');
                const preview = msgEl ? (msgEl.innerText || '').replace(/\s+/g, ' ').trim() : '';
                const badgeEl = card.querySelector('.notice-badge, .badge-count, .unread-count, [class*="unread"]');
                const badgeRaw = badgeEl ? (badgeEl.innerText || '').trim() : '';
                const badgeNum = parseInt(badgeRaw.replace(/\D/g, ''), 10);
                const unread_count = Number.isFinite(badgeNum) && badgeNum > 0 ? badgeNum : (badgeEl ? 1 : 0);
                const tagEl = card.querySelector('.status-tag, .label-tag, .boss-tag');
                const status_tag = tagEl ? (tagEl.innerText || '').trim() : '';
                return { key, company, title, preview, unread_count, status_tag };
              }).filter(Boolean);
            })()
            "#,
        )
        .context("零点击读取 BOSS 沟通列表失败")?;

    let raw = value.get("value").cloned().unwrap_or(value);
    let cards: Vec<DomBossCard> = serde_json::from_value(raw).unwrap_or_default();

    Ok(cards
        .into_iter()
        .map(|card| {
            let preview_opt = (!card.preview.is_empty()).then_some(card.preview.clone());
            let replied = card.unread_count > 0;
            let rejected = card.status_tag.contains("不合适")
                || (replied && is_explicit_rejection(&card.preview));
            let interview_invited = card.status_tag.contains("面试")
                || (replied && is_interview_invite(&card.preview));
            let resume_sent = is_resume_preview(&card.preview);
            let status_tag = if rejected {
                Some("不合适".to_string())
            } else if interview_invited {
                Some("收到约面".to_string())
            } else if card.unread_count > 0 {
                Some(format!("{} 条未读", card.unread_count))
            } else if !card.status_tag.is_empty() {
                Some(card.status_tag)
            } else {
                None
            };
            BossListEntry {
                dedup_key: card.key,
                job_id: None,
                title: (!card.title.is_empty()).then_some(card.title),
                company_name: (!card.company.is_empty()).then_some(card.company),
                salary: None,
                location: None,
                unread_count: card.unread_count,
                last_message: preview_opt,
                last_message_at: 0,
                last_message_received: (card.unread_count > 0).then_some(true),
                status_tag,
                resume_sent,
                rejected,
                interview_invited,
            }
        })
        .collect())
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

/// Uses the same successful, job-ID-correlated response already read by auto reply.
pub(super) fn live_job_context(job_id: &str, body: &str) -> Option<JobDetail> {
    if super::reply_unread::parse_encrypt_job_id(body).as_deref() != Some(job_id) {
        return None;
    }
    let snapshot = parse_job_snapshot(body);
    Some(JobDetail {
        resume_delivery_pending: false,
        source_url: None,
        id: job_id.into(),
        platform: "boss".into(),
        source_task_id: None,
        profile_id: None,
        profile_name: None,
        profile_snapshot_id: None,
        title: snapshot.title?,
        detail: snapshot.detail?,
        company_name: snapshot.company_name.unwrap_or_default(),
        salary: snapshot.salary.unwrap_or_default(),
        location: snapshot.location,
        is_reply: true,
        is_send_resume: false,
        pipeline_stage: None,
        notes: None,
        interview_at: None,
        created_at: String::new(),
        resume_sent_at: None,
        updated_at: String::new(),
    })
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

#[cfg(test)]
fn is_resume_file_message(message: &crate::rpa::boss::model::ChatMessage) -> bool {
    is_resume_preview(&message.text)
}

fn match_local_boss_job(entry: &BossListEntry, local_jobs: &[JobDetail]) -> Option<JobDetail> {
    if let Some(ref job_id) = entry.job_id {
        if let Some(found) = local_jobs.iter().find(|job| job.id == *job_id) {
            return Some(found.clone());
        }
    }
    let company = entry.company_name.as_deref().unwrap_or("").trim().to_lowercase();
    if company.is_empty() {
        return None;
    }
    let candidates: Vec<&JobDetail> = local_jobs
        .iter()
        .filter(|job| {
            (job.platform.is_empty() || job.platform.eq_ignore_ascii_case(PLATFORM))
                && !job.company_name.trim().is_empty()
                && (job.company_name.to_lowercase().contains(&company)
                    || company.contains(&job.company_name.to_lowercase()))
        })
        .collect();

    if candidates.len() == 1 {
        return Some(candidates[0].clone());
    }
    let title = entry.title.as_deref().unwrap_or("").trim().to_lowercase();
    if title.is_empty() {
        return None;
    }
    candidates
        .into_iter()
        .find(|job| {
            let t = job.title.trim().to_lowercase();
            !t.is_empty() && (t.contains(&title) || title.contains(&t))
        })
        .cloned()
}

fn apply_boss_list_entry(
    entry: &BossListEntry,
    local_jobs: &mut Vec<JobDetail>,
) -> Result<(Option<String>, JobSyncChange), anyhow::Error> {
    let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let replied = entry.unread_count > 0 || entry.last_message_received == Some(true);

    if let Some(mut job) = match_local_boss_job(entry, local_jobs) {
        let mut changed = false;
        if replied && !job.is_reply {
            job.is_reply = true;
            changed = true;
        }
        if entry.resume_sent && !job.is_send_resume {
            job.is_send_resume = true;
            if job.resume_sent_at.is_none() {
                job.resume_sent_at = Some(now.clone());
            }
            changed = true;
        }
        changed |= fill_if_empty_or_placeholder(&mut job.title, entry.title.clone(), &["聊天同步岗位"]);
        changed |= fill_if_empty_or_placeholder(
            &mut job.company_name,
            entry.company_name.clone(),
            &["BOSS 会话"],
        );
        changed |= fill_if_empty(&mut job.salary, entry.salary.clone());
        if job.location.as_deref().is_none_or(str::is_empty) && entry.location.is_some() {
            job.location = entry.location.clone();
            changed = true;
        }
        let job_id = job.id.clone();
        if changed {
            job.updated_at = now;
            job_detail_dao::update(&job_id, job.clone())?;
            if let Some(slot) = local_jobs.iter_mut().find(|item| item.id == job_id) {
                *slot = job;
            }
            return Ok((Some(job_id), JobSyncChange::Updated));
        }
        return Ok((Some(job_id), JobSyncChange::None));
    }

    // 仅当结构化接口提供了真实的 encryptJobId 与岗位名时，才把手机端新发起的沟通自动补录进本地看板
    let (Some(job_id), Some(title)) = (entry.job_id.clone(), entry.title.clone()) else {
        return Ok((None, JobSyncChange::None));
    };
    if title.trim().is_empty() || title.trim() == "聊天同步岗位" {
        return Ok((None, JobSyncChange::None));
    }

    let created = JobDetail {
        resume_delivery_pending: false,
        source_url: None,
        id: job_id.clone(),
        platform: PLATFORM.to_string(),
        source_task_id: None,
        profile_id: None,
        profile_name: None,
        profile_snapshot_id: None,
        title,
        company_name: entry.company_name.clone().unwrap_or_default(),
        detail: String::new(),
        salary: entry.salary.clone().unwrap_or_default(),
        location: entry.location.clone(),
        is_reply: replied,
        is_send_resume: entry.resume_sent,
        pipeline_stage: None,
        notes: None,
        interview_at: None,
        created_at: now.clone(),
        resume_sent_at: entry.resume_sent.then(|| now.clone()),
        updated_at: now,
    };
    job_detail_dao::create(created.clone())?;
    local_jobs.push(created);
    Ok((Some(job_id), JobSyncChange::Inserted))
}

fn scroll_conversation_list(page: &rust_drission::Page, reset: bool) -> Result<(), anyhow::Error> {
    let target = if reset {
        "0"
    } else {
        "scroller.scrollTop + Math.max(scroller.clientHeight * 0.85, 320)"
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
    sleep_random_ms(350, 550);
    Ok(())
}

/// 零点击同步 BOSS 沟通列表与未读状态（不点开任何会话卡片，完全保留未读红点）。
pub async fn sync_chat_history() -> Result<ChatHistorySyncResult, anyhow::Error> {
    browser::with_new_tab(|page| Box::pin(async move { sync_chat_history_on_page(page).await }))
        .await
}

/// Synchronize BOSS conversation status on a caller-owned task tab with zero card clicks.
pub async fn sync_chat_history_on_page(
    page: &rust_drission::Page,
) -> Result<ChatHistorySyncResult, anyhow::Error> {
    logger::info("开始 BOSS 零点击状态同步（仅读取会话列表，不点开卡片、不消未读红点）")?;
    let prev_meta = sync_state_dao::get_platform_meta(PLATFORM);
    let watermark_ms = prev_meta.watermark_ms;

    // 在导航前先挂载 CDP 列表监听器，被动捕获 BOSS 沟通页自动拉取的会话列表 JSON
    let friend_list_listener = page.listen_url("zprelation/friend/").ok();

    page.get(BOSS_CHAT_URL).context("打开 BOSS 沟通页面失败")?;
    chat_list::wait_for_chat_page(page, Duration::from_secs(20))?;

    if matches!(
        chat_list::switch_label_and_wait(page, "全部", Duration::from_secs(15))?,
        ListState::Empty
    ) {
        logger::info("BOSS 沟通列表为空，无需同步")?;
        let _ = sync_state_dao::save_platform_meta(PlatformSyncMeta {
            platform: PLATFORM.to_string(),
            last_synced_at: Some(Local::now().format("%Y-%m-%d %H:%M:%S").to_string()),
            watermark_ms,
            last_scanned: 0,
            last_updated: 0,
            last_unread_total: 0,
            last_error: None,
        });
        return Ok(ChatHistorySyncResult::default());
    }

    let mut local_jobs = job_detail_dao::list()?;
    let mut result = ChatHistorySyncResult::default();
    let mut seen_keys: HashSet<String> = HashSet::new();
    let mut signals_to_save: Vec<SyncedJobSignal> = Vec::new();
    let mut max_seen_ts = watermark_ms;

    let ingest_entries = |entries: Vec<BossListEntry>,
                              local_jobs: &mut Vec<JobDetail>,
                              result: &mut ChatHistorySyncResult,
                              seen_keys: &mut HashSet<String>,
                              signals_to_save: &mut Vec<SyncedJobSignal>,
                              max_seen_ts: &mut i64|
     -> Result<(usize, bool, bool), anyhow::Error> {
        let mut discovered = 0usize;
        let mut all_older_than_watermark = true;
        let mut has_any_timestamp = false;

        for entry in entries {
            if !seen_keys.insert(entry.dedup_key.clone()) {
                continue;
            }
            discovered += 1;
            result.scanned += 1;
            result.unread_total = result.unread_total.saturating_add(entry.unread_count);

            if entry.last_message_at > 0 {
                has_any_timestamp = true;
                if entry.last_message_at > *max_seen_ts {
                    *max_seen_ts = entry.last_message_at;
                }
                if entry.last_message_at > watermark_ms {
                    all_older_than_watermark = false;
                }
            }

            let (matched_job_id, change) = apply_boss_list_entry(&entry, local_jobs)?;
            match change {
                JobSyncChange::Inserted => result.jobs_inserted += 1,
                JobSyncChange::Updated => result.jobs_updated += 1,
                JobSyncChange::None => {}
            }

            if let Some(job_id) = matched_job_id {
                result.conversations += 1;
                if entry.interview_invited {
                    result.interview_detected += 1;
                }
                signals_to_save.push(SyncedJobSignal {
                    job_id,
                    platform: PLATFORM.to_string(),
                    unread_count: entry.unread_count,
                    last_message_preview: entry.last_message,
                    last_message_at: (entry.last_message_at > 0).then_some(entry.last_message_at),
                    last_message_received: entry.last_message_received,
                    platform_status_tag: entry.status_tag,
                    interview_invited: entry.interview_invited,
                    rejected_by_platform: entry.rejected,
                    updated_at: String::new(),
                });
            }
        }
        Ok((discovered, has_any_timestamp, all_older_than_watermark))
    };

    // 1. 优先消费首屏被动截获的结构化列表包（零点击、信息最全）
    if let Some(listener) = friend_list_listener {
        if let Ok(Some(packet)) = listener.wait(Duration::from_secs(3)) {
            if let Some(body) = packet.body.and_then(|b| String::from_utf8(b).ok()) {
                if let Ok(entries) = parse_boss_friend_list(&body) {
                    let _ = ingest_entries(
                        entries,
                        &mut local_jobs,
                        &mut result,
                        &mut seen_keys,
                        &mut signals_to_save,
                        &mut max_seen_ts,
                    )?;
                }
            }
        }
    }

    // 2. 零点击轻量滚动列表，叠加后续分页接口包 + DOM 可见卡片摘要，遇水位线即提前截断
    scroll_conversation_list(page, true)?;
    let mut stagnant_rounds = 0;
    for _ in 0..MAX_ZERO_CLICK_SCROLLS {
        if is_job_task_stop_requested() {
            break;
        }
        let next_listener = page.listen_url("zprelation/friend/").ok();
        let dom_entries = collect_boss_cards_zero_click(page)?;
        let (dom_new, _, _) = ingest_entries(
            dom_entries,
            &mut local_jobs,
            &mut result,
            &mut seen_keys,
            &mut signals_to_save,
            &mut max_seen_ts,
        )?;

        scroll_conversation_list(page, false)?;

        let mut api_new = 0usize;
        let mut api_has_ts = false;
        let mut api_all_old = true;
        if let Some(listener) = next_listener {
            if let Ok(Some(packet)) = listener.wait(Duration::from_millis(1200)) {
                if let Some(body) = packet.body.and_then(|b| String::from_utf8(b).ok()) {
                    if let Ok(entries) = parse_boss_friend_list(&body) {
                        let (n, has_ts, all_old) = ingest_entries(
                            entries,
                            &mut local_jobs,
                            &mut result,
                            &mut seen_keys,
                            &mut signals_to_save,
                            &mut max_seen_ts,
                        )?;
                        api_new = n;
                        api_has_ts = has_ts;
                        api_all_old = all_old;
                    }
                }
            }
        }

        if watermark_ms > 0 && api_has_ts && api_all_old {
            logger::info("BOSS 同步已抵达上次增量水位线，提前结束翻页")?;
            break;
        }
        if dom_new == 0 && api_new == 0 {
            stagnant_rounds += 1;
            if stagnant_rounds >= 2 {
                break;
            }
        } else {
            stagnant_rounds = 0;
        }
    }

    let signals_changed = sync_state_dao::upsert_signals(signals_to_save)?;
    result.updated = signals_changed;
    result.jobs_updated = result.jobs_updated.max(signals_changed);

    sync_state_dao::save_platform_meta(PlatformSyncMeta {
        platform: PLATFORM.to_string(),
        last_synced_at: Some(Local::now().format("%Y-%m-%d %H:%M:%S").to_string()),
        watermark_ms: max_seen_ts,
        last_scanned: result.scanned,
        last_updated: result.jobs_updated + result.jobs_inserted,
        last_unread_total: result.unread_total,
        last_error: None,
    })?;

    logger::info(format!(
        "BOSS 零点击同步完成：扫描 {} 个会话，匹配 {} 个岗位，新增 {}，更新 {}，未读 {} 条，约面信号 {} 条",
        result.scanned,
        result.conversations,
        result.jobs_inserted,
        result.jobs_updated,
        result.unread_total,
        result.interview_detected,
    ))?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpa::boss::{
        handler::{parse_chat_messages, MessageDirection},
        model::ChatMessage,
    };

    #[test]
    fn live_job_context_requires_matching_id_and_complete_duties() {
        let body = r#"{"code":0,"zpData":{"data":{"encryptJobId":"job-a","jobName":"Java 后端","jobDescription":"Spring Boot 服务研发"}}}"#;
        let job = live_job_context("job-a", body).unwrap();
        assert_eq!(job.title, "Java 后端");
        assert_eq!(job.detail, "Spring Boot 服务研发");
        assert!(live_job_context("job-b", body).is_none());
        assert!(live_job_context(
            "job-a",
            r#"{"code":0,"zpData":{"data":{"encryptJobId":"job-a","jobName":"Java"}}}"#
        )
        .is_none());
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
            &mut MessageDirection::default(),
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

    #[test]
    fn parses_boss_friend_list_zero_click_with_unread_and_interview_signals() {
        let body = r#"{
            "code": 0,
            "zpData": {
                "result": [
                    {
                        "encryptJobId": "boss-job-99",
                        "jobName": "高级前端工程师",
                        "brandName": "极光科技",
                        "salaryDesc": "25-40K",
                        "cityName": "上海",
                        "unreadMsgCount": 3,
                        "lastText": "您好，邀请您参加周四下午两点的视频面试",
                        "lastTS": 1732691234,
                        "lastIsSelf": false
                    },
                    {
                        "encryptJobId": "boss-job-100",
                        "jobName": "全栈工程师",
                        "brandName": "星云数据",
                        "unreadMsgCount": 0,
                        "lastText": "不好意思，目前该岗位暂不考虑",
                        "lastTS": 1732691000000,
                        "lastIsSelf": false,
                        "statusText": "不合适"
                    }
                ]
            }
        }"#;

        let entries = parse_boss_friend_list(body).unwrap();
        assert_eq!(entries.len(), 2);

        assert_eq!(entries[0].job_id.as_deref(), Some("boss-job-99"));
        assert_eq!(entries[0].unread_count, 3);
        assert_eq!(entries[0].last_message_at, 1_732_691_234_000);
        assert!(entries[0].interview_invited);
        assert!(!entries[0].rejected);
        assert_eq!(entries[0].status_tag.as_deref(), Some("收到约面"));

        assert_eq!(entries[1].job_id.as_deref(), Some("boss-job-100"));
        assert!(entries[1].rejected);
        assert_eq!(entries[1].status_tag.as_deref(), Some("不合适"));
    }
}
