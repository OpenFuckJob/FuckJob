use std::{collections::HashSet, time::Duration};

use anyhow::Context;
use chrono::Local;
use rust_drission::{utils::sleep_random_ms, Page};
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
        liepin::{
            handler::position_say_hello::install_request_recorder,
            LIEPIN_USER_HOME_URL,
        },
        run_flow::is_job_task_stop_requested,
    },
};

const PLATFORM: &str = "liepin";
const CHAT_ENTRY: &str = ".im-ui-basic-entry";
const CONTACT_ITEM: &str = ".im-ui-contact-list-item";
const CONTACT_LIST_API: &str = "com.liepin.im.c.contact.get-contact-list";
const MAX_SCROLL_PAGES: usize = 6;

#[derive(Debug, Default, Clone, Copy, Serialize)]
pub struct LiepinStatusSyncResult {
    pub scanned: usize,
    pub matched_jobs: usize,
    pub jobs_updated: usize,
    pub unread_total: u32,
    pub interview_detected: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct LiepinListEntry {
    pub imid: String,
    pub company: String,
    pub job_title: String,
    pub unread_count: u32,
    pub last_message: Option<String>,
    pub last_message_at: i64,
    pub last_message_received: Option<bool>,
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

fn read_str_any(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(val) = obj
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(val.to_string());
        }
    }
    None
}

fn read_i64_any(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> i64 {
    for key in keys {
        if let Some(v) = obj.get(*key) {
            if let Some(n) = v.as_i64() {
                return n;
            }
            if let Some(s) = v.as_str().and_then(|s| s.trim().parse::<i64>().ok()) {
                return s;
            }
        }
    }
    0
}

/// 宽松解析 `com.liepin.im.c.contact.get-contact-list` 响应，提取每个会话的零点击状态摘要。
pub(crate) fn parse_liepin_contact_list(body: &str) -> Result<Vec<LiepinListEntry>, anyhow::Error> {
    let root: Value = serde_json::from_str(body).context("猎聘会话列表响应不是合法 JSON")?;
    let Some(list) = root
        .pointer("/data/list")
        .or_else(|| root.pointer("/data/contacts"))
        .and_then(Value::as_array)
    else {
        return Ok(Vec::new());
    };

    let mut entries = Vec::new();
    for item in list {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let imid = read_str_any(obj, &["oppositeImId", "imId", "toImId"]).unwrap_or_default();
        if imid.is_empty() {
            continue;
        }
        let unread_count = read_i64_any(obj, &["unReadCnt", "unreadCount", "unreadNum"]).max(0) as u32;
        let company = read_str_any(obj, &["company", "compName", "companyName"]).unwrap_or_default();
        let job_title = read_str_any(obj, &["jobTitle", "title", "positionName"]).unwrap_or_default();
        let last_message = read_str_any(
            obj,
            &[
                "lastMsg",
                "lastMsgContent",
                "latestMsg",
                "lastMessage",
                "msgContent",
                "msg",
            ],
        );
        let last_message_at = normalize_timestamp_ms(read_i64_any(
            obj,
            &["lastMsgTime", "latestMsgTime", "msgTime", "updateTime"],
        ));
        let last_message_received = match read_str_any(obj, &["direction", "lastDirection"]).as_deref() {
            Some("1") => Some(true),
            Some("0") => Some(false),
            _ => {
                if unread_count > 0 {
                    Some(true)
                } else {
                    None
                }
            }
        };

        entries.push(LiepinListEntry {
            imid,
            company,
            job_title,
            unread_count,
            last_message,
            last_message_at,
            last_message_received,
        });
    }

    Ok(entries)
}

#[derive(Debug, Deserialize)]
struct DomLiepinContact {
    imid: String,
    company: String,
    unread_count: u32,
    preview: String,
}

fn collect_liepin_contacts_from_dom(page: &Page) -> Result<Vec<LiepinListEntry>, anyhow::Error> {
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
                    const imid = String(ext.to_imid || "");
                    if (!imid) return null;
                    const badgeEl = item.querySelector(".ant-im-badge-count");
                    const badgeText = badgeEl ? (badgeEl.innerText || "").trim() : "";
                    const parsedBadge = parseInt(badgeText.replace(/\D/g, ""), 10);
                    const unread_count = Number.isFinite(parsedBadge) && parsedBadge > 0
                        ? parsedBadge
                        : (badgeEl || ext.unread === true ? 1 : 0);
                    const info = item.querySelector(".im-ui-contact-item-info");
                    const lines = ((info && info.innerText) || "")
                        .split("\n")
                        .map((line) => line.trim())
                        .filter(Boolean);
                    const company = lines.length >= 3 ? lines[1] : "";
                    const preview = lines.length >= 2 ? lines[lines.length - 1] : "";
                    return { imid, company, unread_count, preview };
                }).filter(Boolean);
            })()
            "#,
        )
        .context("零点击读取猎聘会话列表 DOM 失败")?;

    let raw = value.get("value").cloned().unwrap_or(value);
    let dom_items: Vec<DomLiepinContact> = serde_json::from_value(raw).unwrap_or_default();
    Ok(dom_items
        .into_iter()
        .map(|item| LiepinListEntry {
            imid: item.imid,
            company: item.company,
            job_title: String::new(),
            unread_count: item.unread_count,
            last_message: (!item.preview.is_empty()).then_some(item.preview),
            last_message_at: 0,
            last_message_received: (item.unread_count > 0).then_some(true),
        })
        .collect())
}

fn match_local_liepin_job(entry: &LiepinListEntry, local_jobs: &[JobDetail]) -> Option<JobDetail> {
    let company = entry.company.trim().to_lowercase();
    if company.is_empty() {
        return None;
    }
    let candidates: Vec<&JobDetail> = local_jobs
        .iter()
        .filter(|job| {
            job.platform.eq_ignore_ascii_case(PLATFORM)
                && !job.company_name.trim().is_empty()
                && (job.company_name.to_lowercase().contains(&company)
                    || company.contains(&job.company_name.to_lowercase()))
        })
        .collect();

    if candidates.len() == 1 {
        return Some(candidates[0].clone());
    }

    let title_hint = entry.job_title.trim().to_lowercase();
    let msg_hint = entry.last_message.as_deref().unwrap_or("").to_lowercase();
    candidates
        .into_iter()
        .find(|job| {
            let t = job.title.trim().to_lowercase();
            !t.is_empty() && ((!title_hint.is_empty() && (t.contains(&title_hint) || title_hint.contains(&t))) || msg_hint.contains(&t))
        })
        .cloned()
}

fn read_recorded_contact_body(page: &Page, since: f64) -> Result<Option<String>, anyhow::Error> {
    let script = format!(
        r#"
        (async () => {{
            const deadline = Date.now() + 5000;
            while (Date.now() < deadline) {{
                const records = window.__liepinRequests || [];
                const hit = records
                    .filter((item) => item.ts > {since} && String(item.url || "").includes({api}))
                    .pop();
                if (hit && hit.body) return String(hit.body);
                await new Promise((resolve) => setTimeout(resolve, 200));
            }}
            return "";
        }})()
        "#,
        since = since,
        api = serde_json::to_string(CONTACT_LIST_API).unwrap_or_else(|_| "\"\"".to_string()),
    );
    let value = page.run_js_await(&script)?;
    let raw = value.get("value").cloned().unwrap_or(value);
    let body = raw.as_str().unwrap_or_default().to_string();
    Ok(if body.is_empty() { None } else { Some(body) })
}

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
            await new Promise((resolve) => setTimeout(resolve, 650));
            return el.scrollTop !== before;
        })()
        "#,
    )?;
    let raw = value.get("value").cloned().unwrap_or(value);
    Ok(raw.as_bool().unwrap_or(false))
}

fn is_resume_preview(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("简历") && (lower.contains("已发送") || lower.contains(".pdf") || lower.contains("附件"))
}

/// 猎聘零点击列表状态同步：打开沟通抽屉并被动抓取会话列表，不点开任何会话卡片，完全保留未读红点。
pub async fn sync_chat_status() -> Result<LiepinStatusSyncResult, anyhow::Error> {
    browser::with_new_tab(|page| Box::pin(async move { sync_chat_status_on_page(page).await })).await
}

pub async fn sync_chat_status_on_page(page: &Page) -> Result<LiepinStatusSyncResult, anyhow::Error> {
    logger::info("开始猎聘零点击状态同步（不打开会话，保留未读红点）")?;
    let prev_meta = sync_state_dao::get_platform_meta(PLATFORM);
    let watermark_ms = prev_meta.watermark_ms;

    page.get(LIEPIN_USER_HOME_URL).context("打开猎聘个人中心失败")?;
    let entry_btn = page
        .wait(CHAT_ENTRY, Duration::from_secs(15))
        .context("未找到猎聘沟通入口，请先确认已登录猎聘")?;

    let mut since = install_request_recorder(page)?;
    entry_btn.click()?;
    sleep_random_ms(600, 900);
    let _ = page.wait(CONTACT_ITEM, Duration::from_secs(8));

    let local_jobs = job_detail_dao::list()?;
    let mut result = LiepinStatusSyncResult::default();
    let mut seen_imids: HashSet<String> = HashSet::new();
    let mut signals_to_save: Vec<SyncedJobSignal> = Vec::new();
    let mut max_seen_ts = watermark_ms;

    for scroll_round in 0..MAX_SCROLL_PAGES {
        if is_job_task_stop_requested() {
            break;
        }
        let batch = match read_recorded_contact_body(page, since)? {
            Some(body) => parse_liepin_contact_list(&body).unwrap_or_default(),
            None if scroll_round == 0 => collect_liepin_contacts_from_dom(page)?,
            None => Vec::new(),
        };

        let mut new_in_batch = 0usize;
        let mut all_older_than_watermark = true;
        let mut has_any_timestamp = false;

        for entry in batch {
            if !seen_imids.insert(entry.imid.clone()) {
                continue;
            }
            new_in_batch += 1;
            result.scanned += 1;
            result.unread_total = result.unread_total.saturating_add(entry.unread_count);

            if entry.last_message_at > 0 {
                has_any_timestamp = true;
                if entry.last_message_at > max_seen_ts {
                    max_seen_ts = entry.last_message_at;
                }
                if entry.last_message_at > watermark_ms {
                    all_older_than_watermark = false;
                }
            }

            let Some(mut local_job) = match_local_liepin_job(&entry, &local_jobs) else {
                continue;
            };
            result.matched_jobs += 1;

            let preview = entry.last_message.as_deref().unwrap_or("");
            let is_replied = entry.unread_count > 0 || entry.last_message_received == Some(true);
            let rejected = is_replied && is_explicit_rejection(preview);
            let invited = is_replied && is_interview_invite(preview);
            if invited {
                result.interview_detected += 1;
            }

            let mut job_changed = false;
            if is_replied && !local_job.is_reply {
                local_job.is_reply = true;
                job_changed = true;
            }
            if is_resume_preview(preview) && !local_job.is_send_resume {
                local_job.is_send_resume = true;
                if local_job.resume_sent_at.is_none() {
                    local_job.resume_sent_at =
                        Some(Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
                }
                job_changed = true;
            }
            if job_changed {
                local_job.updated_at = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                let _ = job_detail_dao::update(&local_job.id.clone(), local_job.clone());
                result.jobs_updated += 1;
            }

            let tag = if rejected {
                Some("不合适".to_string())
            } else if invited {
                Some("收到约面".to_string())
            } else if entry.unread_count > 0 {
                Some(format!("{} 条未读", entry.unread_count))
            } else {
                None
            };

            signals_to_save.push(SyncedJobSignal {
                job_id: local_job.id,
                platform: PLATFORM.to_string(),
                unread_count: entry.unread_count,
                last_message_preview: entry.last_message,
                last_message_at: (entry.last_message_at > 0).then_some(entry.last_message_at),
                last_message_received: entry.last_message_received,
                platform_status_tag: tag,
                interview_invited: invited,
                rejected_by_platform: rejected,
                updated_at: String::new(),
            });
        }

        if new_in_batch == 0 || (watermark_ms > 0 && has_any_timestamp && all_older_than_watermark) {
            break;
        }
        since = install_request_recorder(page)?;
        if !scroll_contact_list(page)? {
            break;
        }
    }

    let signals_changed = sync_state_dao::upsert_signals(signals_to_save)?;
    result.jobs_updated = result.jobs_updated.max(signals_changed);

    sync_state_dao::save_platform_meta(PlatformSyncMeta {
        platform: PLATFORM.to_string(),
        last_synced_at: Some(Local::now().format("%Y-%m-%d %H:%M:%S").to_string()),
        watermark_ms: max_seen_ts,
        last_scanned: result.scanned,
        last_updated: result.jobs_updated,
        last_unread_total: result.unread_total,
        last_error: None,
    })?;

    logger::info(format!(
        "猎聘零点击同步完成：扫描 {} 个会话，匹配 {} 个岗位，更新 {} 条，未读 {} 条",
        result.scanned, result.matched_jobs, result.jobs_updated, result.unread_total
    ))?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_liepin_contact_list_with_unread_and_preview() {
        let body = r#"{
            "flag": 1,
            "data": {
                "list": [
                    {
                        "oppositeImId": "im-001",
                        "unReadCnt": 2,
                        "name": "张女士",
                        "company": "星辰科技",
                        "jobTitle": "前端专家",
                        "lastMsg": "您好，邀请您参加明天下午3点的视频面试",
                        "lastMsgTime": 1732690000,
                        "direction": "1"
                    }
                ]
            }
        }"#;
        let entries = parse_liepin_contact_list(body).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].imid, "im-001");
        assert_eq!(entries[0].unread_count, 2);
        assert_eq!(entries[0].last_message_at, 1_732_690_000_000);
        assert_eq!(entries[0].last_message_received, Some(true));
        assert!(is_interview_invite(entries[0].last_message.as_deref().unwrap()));
    }
}
