use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use anyhow::Context;
use chrono::{Local, TimeZone};
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

#[derive(Debug, Clone, Default)]
pub(crate) struct BossLabelMeta {
    pub job_name: Option<String>,
    pub position_name: Option<String>,
    pub job_city: Option<String>,
    pub brand_name: Option<String>,
    pub boss_name: Option<String>,
    pub update_time: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct BossListEntry {
    pub dedup_key: String,
    pub uid: Option<String>,
    pub encrypt_boss_id: Option<String>,
    pub boss_name: Option<String>,
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
    pub is_top: bool,
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

fn read_id_flat(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        let Some(value) = obj.get(*key) else {
            continue;
        };
        if let Some(s) = value.as_str().map(str::trim).filter(|s| !s.is_empty()) {
            return Some(s.to_string());
        }
        if let Some(n) = value.as_i64().filter(|n| *n > 0) {
            return Some(n.to_string());
        }
        if let Some(n) = value.as_u64().filter(|n| *n > 0) {
            return Some(n.to_string());
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

fn merge_label_meta_slot(slot: &mut BossLabelMeta, incoming: &BossLabelMeta) {
    if slot.job_name.is_none() {
        slot.job_name = incoming.job_name.clone();
    }
    if slot.position_name.is_none() {
        slot.position_name = incoming.position_name.clone();
    }
    if slot.job_city.is_none() {
        slot.job_city = incoming.job_city.clone();
    }
    if slot.brand_name.is_none() {
        slot.brand_name = incoming.brand_name.clone();
    }
    if slot.boss_name.is_none() {
        slot.boss_name = incoming.boss_name.clone();
    }
    if incoming.update_time > slot.update_time {
        slot.update_time = incoming.update_time;
    }
}

/// 解析 `/wapi/zprelation/friend/geekFilterByLabel` 返回的 `zpData.friendList`，
/// 建立 `friendId` / `encryptFriendId` 到真实岗位名、城市与公司信息的映射。
/// 若 `friendList` 为空（如 `labelId=-1`），直接跳过不清空已有映射。
pub(crate) fn parse_boss_label_meta(
    body: &str,
    meta_by_uid: &mut HashMap<String, BossLabelMeta>,
) -> usize {
    let Ok(root) = serde_json::from_str::<Value>(body) else {
        return 0;
    };
    let Some(items) = root.pointer("/zpData/friendList").and_then(Value::as_array) else {
        return 0;
    };
    if items.is_empty() {
        return 0;
    }

    let mut count = 0usize;
    for item in items {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let friend_id = read_id_flat(obj, &["friendId", "uid"]);
        let encrypt_friend_id = read_str_flat(obj, &["encryptFriendId", "encryptBossId"]);
        if friend_id.is_none() && encrypt_friend_id.is_none() {
            continue;
        }
        let meta = BossLabelMeta {
            job_name: read_str_flat(obj, &["jobName"]),
            position_name: read_str_flat(obj, &["positionName"]),
            job_city: read_str_flat(obj, &["jobCity", "cityName", "locationName"]),
            brand_name: read_str_flat(obj, &["brandName", "companyName", "brandFullName"]),
            boss_name: read_str_flat(obj, &["name", "bossName"]),
            update_time: normalize_timestamp_ms(read_i64_flat(obj, &["updateTime", "lastTS"])),
        };
        if let Some(key) = friend_id {
            merge_label_meta_slot(meta_by_uid.entry(key).or_default(), &meta);
        }
        if let Some(key) = encrypt_friend_id {
            merge_label_meta_slot(meta_by_uid.entry(key).or_default(), &meta);
        }
        count += 1;
    }
    count
}

fn backfill_entry_from_label_meta(
    entry: &mut BossListEntry,
    label_meta: &HashMap<String, BossLabelMeta>,
) {
    let meta = entry
        .uid
        .as_deref()
        .and_then(|k| label_meta.get(k))
        .or_else(|| entry.encrypt_boss_id.as_deref().and_then(|k| label_meta.get(k)));
    let Some(meta) = meta else {
        return;
    };
    if entry.title.is_none() {
        entry.title = meta.job_name.clone().or_else(|| meta.position_name.clone());
    }
    if entry.location.is_none() {
        entry.location = meta.job_city.clone();
    }
    if entry.company_name.is_none() {
        entry.company_name = meta.brand_name.clone();
    }
    if entry.boss_name.is_none() {
        entry.boss_name = meta.boss_name.clone();
    }
    if entry.last_message_at == 0 && meta.update_time > 0 {
        entry.last_message_at = meta.update_time;
    }
}

/// 零点击解析 BOSS 会话列表接口（`wapi/zprelation/friend/getGeekFriendList.json`）。
#[cfg(test)]
pub(crate) fn parse_boss_friend_list(body: &str) -> Result<Vec<BossListEntry>, anyhow::Error> {
    parse_boss_friend_list_with_meta(body, &HashMap::new())
}

pub(crate) fn parse_boss_friend_list_with_meta(
    body: &str,
    label_meta: &HashMap<String, BossLabelMeta>,
) -> Result<Vec<BossListEntry>, anyhow::Error> {
    let root: Value = serde_json::from_str(body).context("BOSS 会话列表响应不是合法 JSON")?;
    let list = root
        .pointer("/zpData/result")
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
        let uid = read_id_flat(obj, &["uid", "friendId"]);
        let encrypt_boss_id = read_str_flat(obj, &["encryptBossId", "encryptFriendId"]);
        let meta = uid
            .as_deref()
            .and_then(|k| label_meta.get(k))
            .or_else(|| encrypt_boss_id.as_deref().and_then(|k| label_meta.get(k)));

        // 严格只读真实岗位 ID：优先 encryptJobId，其次 jobId；绝不误用 securityId
        let job_id = read_str_flat(obj, &["encryptJobId"]).or_else(|| read_id_flat(obj, &["jobId"]));
        // `getGeekFriendList` 里的 `title` 是 HR 职位（如“招聘经理”），绝不能当岗位名；
        // 真实岗位名来自自身 `jobName`/`positionName` 或 `geekFilterByLabel` 关联映射
        let title = read_str_flat(obj, &["jobName", "positionName", "jobTitle"]).or_else(|| {
            meta.and_then(|m| m.job_name.clone().or_else(|| m.position_name.clone()))
        });
        let company_name = read_str_flat(obj, &["brandName", "companyName", "brandFullName"])
            .or_else(|| meta.and_then(|m| m.brand_name.clone()));
        let salary = read_str_flat(obj, &["salaryDesc", "salary"]);
        let location = read_str_flat(obj, &["jobCity", "cityName", "locationName"])
            .or_else(|| meta.and_then(|m| m.job_city.clone()));
        let boss_name = read_str_flat(obj, &["name", "bossName"])
            .or_else(|| meta.and_then(|m| m.boss_name.clone()));
        let is_top = read_bool_flat(obj, &["isTop"]).unwrap_or(false);

        let unread_count =
            read_i64_flat(obj, &["unreadMsgCount", "unreadCount", "unreadNum"]).max(0) as u32;

        let last_msg_info = obj.get("lastMessageInfo").and_then(Value::as_object);
        let last_message = last_msg_info
            .and_then(|info| read_str_flat(info, &["showText", "text", "msgText"]))
            .or_else(|| {
                read_str_flat(
                    obj,
                    &["lastText", "lastMsg", "lastMessage", "msgText", "preview"],
                )
            });

        let raw_msg_time = last_msg_info
            .map(|info| read_i64_flat(info, &["msgTime", "time", "createTime"]))
            .filter(|ts| *ts > 0)
            .unwrap_or_else(|| {
                read_i64_flat(obj, &["lastTS", "lastMsgTime", "updateTime", "lastTime"])
            });
        let mut last_message_at = normalize_timestamp_ms(raw_msg_time);
        if last_message_at == 0 {
            if let Some(m) = meta {
                last_message_at = m.update_time;
            }
        }

        let from_id = last_msg_info.and_then(|info| read_id_flat(info, &["fromId", "fromUid"]));
        let to_id = last_msg_info.and_then(|info| read_id_flat(info, &["toId", "toUid"]));
        let last_message_received = if let Some(ref boss_uid) = uid {
            if from_id.as_deref() == Some(boss_uid.as_str()) {
                Some(true)
            } else if to_id.as_deref() == Some(boss_uid.as_str()) {
                Some(false)
            } else {
                match read_bool_flat(obj, &["lastIsSelf", "isSelf"]) {
                    Some(is_self) => Some(!is_self),
                    None => (unread_count > 0).then_some(true),
                }
            }
        } else {
            match read_bool_flat(obj, &["lastIsSelf", "isSelf"]) {
                Some(is_self) => Some(!is_self),
                None => (unread_count > 0).then_some(true),
            }
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

        let dedup_key = job_id
            .clone()
            .or_else(|| encrypt_boss_id.clone())
            .or_else(|| uid.clone())
            .unwrap_or_else(|| {
                format!(
                    "{}|{}|{}",
                    boss_name.as_deref().unwrap_or(""),
                    company_name.as_deref().unwrap_or(""),
                    title.as_deref().unwrap_or("")
                )
            });
        if dedup_key.trim_matches('|').is_empty() {
            continue;
        }

        entries.push(BossListEntry {
            dedup_key,
            uid,
            encrypt_boss_id,
            boss_name,
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
            is_top,
        });
    }

    Ok(entries)
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct DomBossCard {
    pub key: String,
    #[serde(default)]
    pub boss_name: String,
    #[serde(default)]
    pub company: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub boss_title: String,
    #[serde(default)]
    pub preview: String,
    #[serde(default)]
    pub unread_count: u32,
    #[serde(default)]
    pub status_tag: String,
}

/// 零点击 DOM 卡片读取：严格按实测结构读取 HR 姓名 / 公司 / HR 头衔、精确消息预览与可见未读角标，绝不点击任何卡片。
fn collect_boss_dom_cards(page: &rust_drission::Page) -> Result<Vec<DomBossCard>, anyhow::Error> {
    let value = page
        .run_js_await(
            r#"
            (() => {
              const isVisible = (el) => {
                if (!el) return false;
                const style = window.getComputedStyle(el);
                return style.display !== 'none'
                  && style.visibility !== 'hidden'
                  && el.offsetWidth > 0
                  && el.offsetHeight > 0;
              };
              const cards = Array.from(document.querySelectorAll('.user-list-content .friend-content'));
              return cards.map((card) => {
                const titleBox = card.querySelector('.title-box');
                const key = ((titleBox && titleBox.innerText) || '').replace(/\s+/g, ' ').trim();
                if (!key) return null;
                const nameBox = card.querySelector('.title-box .name-box');
                const boss_name = nameBox
                  ? ((nameBox.querySelector('.name-text')?.innerText) || '').trim()
                  : '';
                const infoSpans = nameBox
                  ? Array.from(nameBox.children)
                      .filter((el) => el.tagName === 'SPAN' && !el.classList.contains('name-text'))
                      .map((el) => (el.innerText || '').trim())
                      .filter(Boolean)
                  : [];
                const company = infoSpans[0] || '';
                const boss_title = infoSpans[1] || '';
                const msgEl = card.querySelector('.last-msg-text')
                  || card.querySelector('.msg-text')
                  || card.querySelector('.gray.last-msg');
                const preview = msgEl ? (msgEl.innerText || '').replace(/\s+/g, ' ').trim() : '';
                const badgeEl = Array.from(
                  card.querySelectorAll('.notice-badge, .badge-count, .unread-count')
                ).find(isVisible);
                const badgeRaw = badgeEl ? (badgeEl.innerText || '').trim() : '';
                const badgeNum = parseInt(badgeRaw.replace(/\D/g, ''), 10);
                const unread_count = Number.isFinite(badgeNum) && badgeNum > 0 ? badgeNum : 0;
                const tagEl = card.querySelector('.status-tag, .label-tag, .boss-tag');
                const status_tag = tagEl ? (tagEl.innerText || '').trim() : '';
                return { key, boss_name, company, boss_title, preview, unread_count, status_tag };
              }).filter(Boolean);
            })()
            "#,
        )
        .context("零点击读取 BOSS 沟通列表失败")?;

    let raw = value.get("value").cloned().unwrap_or(value);
    Ok(serde_json::from_value(raw).unwrap_or_default())
}

/// 读取沟通页左侧分类栏里「未读(N)」标签上的实时总数（零点击、不切换分类）。
fn read_unread_label_count(page: &rust_drission::Page) -> Option<u32> {
    let value = page
        .run_js_await(
            r#"
            (() => {
              const items = Array.from(document.querySelectorAll('.label-list li .label-name'));
              const unread = items.find((el) => (el.innerText || '').trim().startsWith('未读'));
              if (!unread) return null;
              const txt = (unread.innerText || '').trim();
              const m = txt.match(/(\d+)/);
              return m ? parseInt(m[1], 10) : 0;
            })()
            "#,
        )
        .ok()?;
    let raw = value.get("value").unwrap_or(&value);
    raw.as_u64().map(|n| n as u32)
}

/// 等待首屏 `.notice-badge` 完成 MQTT 渲染后再读首屏 DOM：
/// BOSS 沟通页先返回 `getGeekFriendList.json`（其中 `unreadMsgCount` 常为 0），约 0.5–2s 后
/// 再由前端状态/MQTT 把真实红点数挂到 `.notice-badge` 和「未读(N)」标签上。
/// 若水位线早退路径未等角标挂载就读 DOM，会把真实未读误判为 0。
fn collect_initial_dom_with_badge_wait(
    page: &rust_drission::Page,
    _prev_unread_total: u32,
) -> Result<Vec<DomBossCard>, anyhow::Error> {
    let start = std::time::Instant::now();
    // 实测 BOSS 沟通页 MQTT 红点（.notice-badge 与「未读(N)」）通常在卡片渲染后 3–4.5s 才同步挂上；
    // 一旦检测到 dom_unread > 0 立即返回，仅当暂时为 0 时耐心等满 4.8s 确认并非 MQTT 延迟。
    let max_wait = Duration::from_millis(6500);
    let min_settle_if_zero = Duration::from_millis(4800);

    let mut latest = collect_boss_dom_cards(page)?;
    loop {
        let dom_unread: u32 = latest.iter().map(|c| c.unread_count).sum();
        let label_unread = read_unread_label_count(page);
        let elapsed = start.elapsed();

        if dom_unread > 0 {
            if label_unread.is_none_or(|expected| dom_unread >= expected)
                || elapsed >= Duration::from_millis(2500)
            {
                return Ok(latest);
            }
        } else if label_unread.unwrap_or(0) == 0 && elapsed >= min_settle_if_zero {
            return Ok(latest);
        }

        if elapsed >= max_wait {
            return Ok(latest);
        }
        sleep_random_ms(280, 380);
        latest = collect_boss_dom_cards(page)?;
    }
}

fn normalize_preview_key(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace())
        .take(16)
        .collect::<String>()
        .to_lowercase()
}

fn preview_matches(a: &str, b: &str) -> bool {
    let na = normalize_preview_key(a);
    let nb = normalize_preview_key(b);
    if na.is_empty() || nb.is_empty() {
        return false;
    }
    na.starts_with(&nb) || nb.starts_with(&na) || na.contains(&nb) || nb.contains(&na)
}

/// 将 DOM 上真实可见的 `.notice-badge` 未读角标融合回 `getGeekFriendList` 结构化条目，
/// 避免 BOSS 接口 `unreadMsgCount` 恒为 0 导致漏报未读，同时绝不重复插入 DOM 条目。
pub(crate) fn enrich_entries_with_dom_unread(
    entries: &mut [BossListEntry],
    dom_cards: &[DomBossCard],
) {
    for dom in dom_cards {
        if dom.unread_count == 0 {
            continue;
        }
        let dom_boss = dom.boss_name.trim().to_lowercase();
        let dom_company = crate::verify::normalize_company_name(&dom.company);

        let matched_idx = if !dom_boss.is_empty() && !dom_company.is_empty() {
            let candidates: Vec<usize> = entries
                .iter()
                .enumerate()
                .filter(|(_, e)| {
                    e.boss_name
                        .as_deref()
                        .map(|s| s.trim().to_lowercase())
                        .as_deref()
                        == Some(dom_boss.as_str())
                        && crate::verify::normalize_company_name(
                            e.company_name.as_deref().unwrap_or(""),
                        ) == dom_company
                })
                .map(|(idx, _)| idx)
                .collect();
            if candidates.len() == 1 {
                Some(candidates[0])
            } else if candidates.len() > 1 && !dom.preview.is_empty() {
                let by_preview: Vec<usize> = candidates
                    .into_iter()
                    .filter(|&idx| {
                        entries[idx]
                            .last_message
                            .as_deref()
                            .is_some_and(|p| preview_matches(p, &dom.preview))
                    })
                    .collect();
                (by_preview.len() == 1).then(|| by_preview[0])
            } else {
                None
            }
        } else {
            None
        }
        .or_else(|| {
            if dom_company.is_empty() || dom.preview.is_empty() {
                return None;
            }
            let by_company_preview: Vec<usize> = entries
                .iter()
                .enumerate()
                .filter(|(_, e)| {
                    crate::verify::normalize_company_name(e.company_name.as_deref().unwrap_or(""))
                        == dom_company
                        && e.last_message
                            .as_deref()
                            .is_some_and(|p| preview_matches(p, &dom.preview))
                })
                .map(|(idx, _)| idx)
                .collect();
            (by_company_preview.len() == 1).then(|| by_company_preview[0])
        });

        let Some(idx) = matched_idx else {
            continue;
        };
        let entry = &mut entries[idx];
        entry.unread_count = entry.unread_count.max(dom.unread_count);
        if entry.unread_count > 0 && entry.last_message_received.is_none() {
            entry.last_message_received = Some(true);
        }
        let replied = entry.unread_count > 0 || entry.last_message_received == Some(true);
        let preview = entry.last_message.as_deref().unwrap_or("");
        if replied {
            if is_explicit_rejection(preview) {
                entry.rejected = true;
            }
            if is_interview_invite(preview) {
                entry.interview_invited = true;
            }
        }
        if entry.rejected {
            entry.status_tag = Some("不合适".to_string());
        } else if entry.interview_invited {
            entry.status_tag = Some("收到约面".to_string());
        } else if entry.unread_count > 0 {
            entry.status_tag = Some(format!("{} 条未读", entry.unread_count));
        }
    }
}

fn dom_cards_to_fallback_entries(cards: Vec<DomBossCard>) -> Vec<BossListEntry> {
    cards
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
                uid: None,
                encrypt_boss_id: None,
                boss_name: (!card.boss_name.is_empty()).then_some(card.boss_name),
                job_id: None,
                // DOM 卡片不含真实岗位名称（第 3 个 span 是 HR 头衔），切勿将 HR 头衔填入 title
                title: None,
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
                is_top: false,
            }
        })
        .collect()
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

fn normalize_boss_job_id(raw: &str) -> &str {
    let trimmed = raw.trim();
    trimmed.strip_prefix("boss:").unwrap_or(trimmed)
}

fn titles_conflict(a: &str, b: &str) -> bool {
    let a = a.trim().to_lowercase();
    let b = b.trim().to_lowercase();
    if a.is_empty() || b.is_empty() {
        return false;
    }
    !(a == b || a.contains(&b) || b.contains(&a))
}

/// 严格岗位匹配：结构化条目携带 `job_id` 时仅按标准化 ID 精确匹配，绝不退回同公司模糊匹配；
/// 无 ID 的 DOM 兜底条目要求标准化公司名精确命中、岗位名不冲突且候选唯一。
fn match_local_boss_job(entry: &BossListEntry, local_jobs: &[JobDetail]) -> Option<JobDetail> {
    if let Some(ref job_id) = entry.job_id {
        let target = normalize_boss_job_id(job_id);
        if target.is_empty() {
            return None;
        }
        return local_jobs
            .iter()
            .find(|job| {
                (job.platform.is_empty() || job.platform.eq_ignore_ascii_case(PLATFORM))
                    && normalize_boss_job_id(&job.id) == target
            })
            .cloned();
    }

    let company = crate::verify::normalize_company_name(entry.company_name.as_deref().unwrap_or(""));
    if company.is_empty() {
        return None;
    }
    let entry_title = entry.title.as_deref().unwrap_or("").trim();
    let candidates: Vec<&JobDetail> = local_jobs
        .iter()
        .filter(|job| {
            (job.platform.is_empty() || job.platform.eq_ignore_ascii_case(PLATFORM))
                && crate::verify::normalize_company_name(&job.company_name) == company
                && !titles_conflict(&job.title, entry_title)
        })
        .collect();

    if candidates.len() == 1 {
        return Some(candidates[0].clone());
    }
    if !entry_title.is_empty() {
        let et = entry_title.to_lowercase();
        let refined: Vec<&JobDetail> = candidates
            .into_iter()
            .filter(|job| {
                let t = job.title.trim().to_lowercase();
                !t.is_empty() && (t == et || t.contains(&et) || et.contains(&t))
            })
            .collect();
        if refined.len() == 1 {
            return Some(refined[0].clone());
        }
    }
    None
}

/// 从历史消息时间推导同步补录岗位的 `created_at`，避免历史会话被计入今日打招呼配额。
pub(crate) fn synced_job_created_at(last_message_at_ms: i64) -> String {
    if last_message_at_ms > 0 {
        if let chrono::LocalResult::Single(dt) = Local.timestamp_millis_opt(last_message_at_ms) {
            return dt.format("%Y-%m-%d %H:%M:%S").to_string();
        }
    }
    "2000-01-01 00:00:00".to_string()
}

fn apply_boss_list_entry(
    entry: &BossListEntry,
    local_jobs: &mut Vec<JobDetail>,
) -> Result<(Option<String>, JobSyncChange), anyhow::Error> {
    let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let replied = entry.unread_count > 0 || entry.last_message_received == Some(true);

    if let Some(matched) = match_local_boss_job(entry, local_jobs) {
        let job_id = matched.id.clone();
        let entry_title = entry.title.clone();
        let entry_company = entry.company_name.clone();
        let entry_salary = entry.salary.clone();
        let entry_location = entry.location.clone();
        let resume_sent = entry.resume_sent;
        let mut modified = false;

        let updated = job_detail_dao::patch_by_id(&job_id, |job| {
            let mut changed = false;
            if replied && !job.is_reply {
                job.is_reply = true;
                changed = true;
            }
            if resume_sent && !job.is_send_resume {
                job.is_send_resume = true;
                if job.resume_sent_at.is_none() {
                    job.resume_sent_at = Some(now.clone());
                }
                changed = true;
            }
            changed |= fill_if_empty_or_placeholder(
                &mut job.title,
                entry_title.clone(),
                &["聊天同步岗位"],
            );
            changed |= fill_if_empty_or_placeholder(
                &mut job.company_name,
                entry_company.clone(),
                &["BOSS 会话"],
            );
            changed |= fill_if_empty(&mut job.salary, entry_salary.clone());
            if job.location.as_deref().is_none_or(str::is_empty) && entry_location.is_some() {
                job.location = entry_location.clone();
                changed = true;
            }
            if changed {
                job.updated_at = now.clone();
            }
            modified = changed;
            changed
        })?;

        if let Some(job) = updated {
            if modified {
                if let Some(slot) = local_jobs.iter_mut().find(|item| item.id == job_id) {
                    *slot = job;
                }
                return Ok((Some(job_id), JobSyncChange::Updated));
            }
            return Ok((Some(job_id), JobSyncChange::None));
        }
        return Ok((None, JobSyncChange::None));
    }

    // 仅当结构化接口提供了真实的 encryptJobId 与岗位名时，才把手机端新发起的沟通自动补录进本地看板
    let (Some(job_id), Some(title)) = (entry.job_id.clone(), entry.title.clone()) else {
        return Ok((None, JobSyncChange::None));
    };
    if title.trim().is_empty() || title.trim() == "聊天同步岗位" {
        return Ok((None, JobSyncChange::None));
    }

    let created_at = synced_job_created_at(entry.last_message_at);
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
        created_at: created_at.clone(),
        resume_sent_at: entry.resume_sent.then_some(created_at),
        updated_at: now,
    };
    job_detail_dao::create(created.clone())?;
    local_jobs.push(created);
    Ok((Some(job_id), JobSyncChange::Inserted))
}

fn drain_friend_packets(
    listener: &rust_drission::Listener,
    initial_wait: Duration,
    idle_wait: Duration,
) -> Vec<(String, String)> {
    let mut packets = Vec::new();
    let mut first = true;
    loop {
        let wait_dur = if first { initial_wait } else { idle_wait };
        first = false;
        match listener.wait(wait_dur) {
            Ok(Some(packet)) => {
                if packet.is_failed {
                    continue;
                }
                let url = if !packet.response.url.is_empty() {
                    packet.response.url
                } else {
                    packet.request.url
                };
                if let Some(body) = packet.body.and_then(|b| String::from_utf8(b).ok()) {
                    packets.push((url, body));
                }
            }
            _ => break,
        }
    }
    packets
}

/// 两阶段解析已排空的 `zprelation/friend/*` 数据包：
/// 先合并所有 `geekFilterByLabel` 映射，再解析 `getGeekFriendList` 条目，保证岗位名与城市完整关联。
pub(crate) fn parse_boss_friend_packets(
    packets: &[(String, String)],
    label_meta: &mut HashMap<String, BossLabelMeta>,
) -> Vec<BossListEntry> {
    let mut friend_list_bodies = Vec::new();

    for (url, body) in packets {
        let is_label = url.contains("geekFilterByLabel");
        let is_friend_list = url.contains("getGeekFriendList");
        if is_label {
            parse_boss_label_meta(body, label_meta);
        } else if is_friend_list {
            friend_list_bodies.push(body.as_str());
        } else if let Ok(root) = serde_json::from_str::<Value>(body) {
            if root.pointer("/zpData/friendList").is_some()
                && root.pointer("/zpData/result").is_none()
            {
                parse_boss_label_meta(body, label_meta);
            } else {
                friend_list_bodies.push(body.as_str());
            }
        }
    }

    let mut entries = Vec::new();
    for body in friend_list_bodies {
        if let Ok(parsed) = parse_boss_friend_list_with_meta(body, label_meta) {
            entries.extend(parsed);
        }
    }
    entries
}

/// 判定本批结构化会话是否已经安全越过增量水位线（`>=` 边界保守不早退，忽略置顶旧会话）。
fn batch_crossed_watermark(entries: &[BossListEntry], watermark_ms: i64) -> bool {
    if watermark_ms <= 0 {
        return false;
    }
    let non_top_ts: Vec<i64> = entries
        .iter()
        .filter(|e| !e.is_top && e.last_message_at > 0)
        .map(|e| e.last_message_at)
        .collect();
    if let Some(&oldest_non_top) = non_top_ts.last() {
        return oldest_non_top < watermark_ms;
    }
    let all_ts: Vec<i64> = entries
        .iter()
        .filter(|e| e.last_message_at > 0)
        .map(|e| e.last_message_at)
        .collect();
    !all_ts.is_empty() && all_ts.iter().all(|&ts| ts < watermark_ms)
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

    // 整轮同步仅保留单个 CDP 监听器，避免循环内反复创建 WebSocket 监听线程泄漏
    let friend_list_listener = page.listen_url("zprelation/friend/").ok();

    page.get(BOSS_CHAT_URL).context("打开 BOSS 沟通页面失败")?;
    chat_list::wait_for_chat_page(page, Duration::from_secs(45))?;

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
    let mut pending_local_unread: HashSet<String> = sync_state_dao::list_signals()
        .into_values()
        .filter(|sig| {
            sig.platform.eq_ignore_ascii_case(PLATFORM)
                && sig.unread_count > 0
                && sig.last_message_at.unwrap_or(0) <= watermark_ms
        })
        .map(|sig| sig.job_id)
        .collect();

    let mut label_meta: HashMap<String, BossLabelMeta> = HashMap::new();
    let mut seen_api_keys: HashSet<String> = HashSet::new();
    let mut collected_api_entries: Vec<BossListEntry> = Vec::new();
    let mut collected_dom_by_key: HashMap<String, DomBossCard> = HashMap::new();
    let mut dom_key_order: Vec<String> = Vec::new();
    let mut max_seen_ts = watermark_ms;
    let mut scan_completed_cleanly = false;

    let record_dom_cards = |cards: Vec<DomBossCard>,
                            dom_map: &mut HashMap<String, DomBossCard>,
                            dom_order: &mut Vec<String>|
     -> usize {
        let mut newly_seen = 0usize;
        for card in cards {
            match dom_map.get_mut(&card.key) {
                Some(existing) => {
                    existing.unread_count = existing.unread_count.max(card.unread_count);
                    if existing.preview.is_empty() && !card.preview.is_empty() {
                        existing.preview = card.preview;
                    }
                }
                None => {
                    dom_order.push(card.key.clone());
                    dom_map.insert(card.key.clone(), card);
                    newly_seen += 1;
                }
            }
        }
        newly_seen
    };

    let record_api_entries = |entries: Vec<BossListEntry>,
                              local_jobs: &[JobDetail],
                              seen_keys: &mut HashSet<String>,
                              collected: &mut Vec<BossListEntry>,
                              pending_unread: &mut HashSet<String>,
                              max_ts: &mut i64|
     -> (usize, bool) {
        let crossed = batch_crossed_watermark(&entries, watermark_ms);
        let mut discovered = 0usize;
        for entry in entries {
            if !seen_keys.insert(entry.dedup_key.clone()) {
                continue;
            }
            if entry.last_message_at > *max_ts {
                *max_ts = entry.last_message_at;
            }
            if let Some(matched) = match_local_boss_job(&entry, local_jobs) {
                pending_unread.remove(&matched.id);
            }
            collected.push(entry);
            discovered += 1;
        }
        (discovered, crossed)
    };

    // 1. 排空首屏被动截获的 `geekFilterByLabel` + `getGeekFriendList` 数据包，并采集首屏 DOM 卡片用于未读角标融合
    let mut page1_crossed_watermark = false;
    if let Some(ref listener) = friend_list_listener {
        let packets = drain_friend_packets(
            listener,
            Duration::from_secs(3),
            Duration::from_millis(500),
        );
        let entries = parse_boss_friend_packets(&packets, &mut label_meta);
        let (_, crossed) = record_api_entries(
            entries,
            &local_jobs,
            &mut seen_api_keys,
            &mut collected_api_entries,
            &mut pending_local_unread,
            &mut max_seen_ts,
        );
        page1_crossed_watermark = crossed;
    }

    let initial_dom = collect_initial_dom_with_badge_wait(page, prev_meta.last_unread_total)?;
    record_dom_cards(initial_dom, &mut collected_dom_by_key, &mut dom_key_order);

    if !collected_api_entries.is_empty()
        && watermark_ms > 0
        && page1_crossed_watermark
        && pending_local_unread.is_empty()
    {
        logger::info("BOSS 首屏已抵达上次增量水位线，跳过翻页")?;
        scan_completed_cleanly = true;
    } else {
        // 2. 零点击轻量滚动列表，复用同一个 CDP 监听器收集后续分页包与 DOM 可见未读角标
        scroll_conversation_list(page, true)?;
        let mut stagnant_rounds = 0;
        for _ in 0..MAX_ZERO_CLICK_SCROLLS {
            if is_job_task_stop_requested() {
                break;
            }
            let dom_cards = collect_boss_dom_cards(page)?;
            let dom_new =
                record_dom_cards(dom_cards, &mut collected_dom_by_key, &mut dom_key_order);

            scroll_conversation_list(page, false)?;

            let mut api_new = 0usize;
            let mut api_crossed = false;
            if let Some(ref listener) = friend_list_listener {
                let packets = drain_friend_packets(
                    listener,
                    Duration::from_millis(1200),
                    Duration::from_millis(350),
                );
                let prev_meta_len = label_meta.len();
                let entries = parse_boss_friend_packets(&packets, &mut label_meta);
                if label_meta.len() > prev_meta_len {
                    for existing in &mut collected_api_entries {
                        backfill_entry_from_label_meta(existing, &label_meta);
                    }
                }
                let (n, crossed) = record_api_entries(
                    entries,
                    &local_jobs,
                    &mut seen_api_keys,
                    &mut collected_api_entries,
                    &mut pending_local_unread,
                    &mut max_seen_ts,
                );
                api_new = n;
                api_crossed = crossed;
            }

            if watermark_ms > 0 && api_crossed && pending_local_unread.is_empty() {
                logger::info("BOSS 同步已抵达上次增量水位线，提前结束翻页")?;
                scan_completed_cleanly = true;
                break;
            }
            if dom_new == 0 && api_new == 0 {
                stagnant_rounds += 1;
                if stagnant_rounds >= 2 {
                    scan_completed_cleanly = true;
                    break;
                }
            } else {
                stagnant_rounds = 0;
            }
        }
    }

    let ordered_dom_cards: Vec<DomBossCard> = dom_key_order
        .into_iter()
        .filter_map(|key| collected_dom_by_key.remove(&key))
        .collect();

    // 当捕获到结构化 API 条目时，仅用 DOM 角标融合未读数，绝不重复入库 DOM 卡片；
    // 仅在完全未捕获到任何 API 条目时才回落为 DOM 兜底条目。
    let final_entries = if !collected_api_entries.is_empty() {
        for entry in &mut collected_api_entries {
            backfill_entry_from_label_meta(entry, &label_meta);
        }
        enrich_entries_with_dom_unread(&mut collected_api_entries, &ordered_dom_cards);
        collected_api_entries
    } else {
        dom_cards_to_fallback_entries(ordered_dom_cards)
    };

    let mut result = ChatHistorySyncResult::default();
    let mut signals_to_save: Vec<SyncedJobSignal> = Vec::new();
    for entry in final_entries {
        result.scanned += 1;
        result.unread_total = result.unread_total.saturating_add(entry.unread_count);

        let (matched_job_id, change) = apply_boss_list_entry(&entry, &mut local_jobs)?;
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

    let signals_changed = sync_state_dao::upsert_signals(signals_to_save)?;
    result.updated = signals_changed;
    let signal_only_updates = signals_changed.saturating_sub(result.jobs_inserted);
    result.jobs_updated = result.jobs_updated.max(signal_only_updates);

    let next_watermark_ms = if scan_completed_cleanly {
        max_seen_ts
    } else {
        watermark_ms
    };
    sync_state_dao::save_platform_meta(PlatformSyncMeta {
        platform: PLATFORM.to_string(),
        last_synced_at: Some(Local::now().format("%Y-%m-%d %H:%M:%S").to_string()),
        watermark_ms: next_watermark_ms,
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

    #[test]
    fn joins_geek_filter_by_label_and_friend_list_with_last_message_info_and_rejects_hr_title_and_security_id() {
        let label_0 = r#"{
            "code": 0,
            "zpData": {
                "friendList": [
                    {
                        "friendId": 88001,
                        "encryptFriendId": "enc-hr-1",
                        "brandName": "深蓝智能",
                        "jobName": "Rust 后端架构师",
                        "positionName": "后端开发",
                        "jobCity": "杭州",
                        "name": "陈女士",
                        "bossTitle": "招聘经理",
                        "updateTime": 1732695000000
                    }
                ]
            }
        }"#;
        let label_empty = r#"{"code": 0, "zpData": {"friendList": []}}"#;
        let friend_list = r#"{
            "code": 0,
            "zpData": {
                "result": [
                    {
                        "uid": 88001,
                        "encryptBossId": "enc-hr-1",
                        "encryptJobId": "28charRealEncryptJobId000001",
                        "jobId": 999111,
                        "securityId": "sec-should-never-be-job-id",
                        "brandName": "深蓝智能",
                        "name": "陈女士",
                        "title": "招聘经理",
                        "lastMsg": "旧摘要",
                        "lastTS": 1732694000000,
                        "unreadMsgCount": 0,
                        "lastMessageInfo": {
                            "msgId": 5001,
                            "showText": "方便发一份最新简历吗？",
                            "fromId": 88001,
                            "toId": 10001,
                            "msgTime": 1732695500000
                        }
                    },
                    {
                        "uid": 88002,
                        "encryptBossId": "enc-hr-2",
                        "securityId": "sec-only-no-job-id",
                        "brandName": "星图科技",
                        "name": "刘先生",
                        "title": "HRBP",
                        "lastMsg": "已发送招呼",
                        "lastTS": 1732691000000,
                        "unreadMsgCount": 0,
                        "lastMessageInfo": {
                            "msgId": 5002,
                            "showText": "您好，我对岗位很感兴趣",
                            "fromId": 10001,
                            "toId": 88002,
                            "msgTime": 1732691000000
                        }
                    }
                ]
            }
        }"#;

        let packets = vec![
            (
                "https://www.zhipin.com/wapi/zprelation/friend/geekFilterByLabel?labelId=0"
                    .to_string(),
                label_0.to_string(),
            ),
            (
                "https://www.zhipin.com/wapi/zprelation/friend/geekFilterByLabel?labelId=-1"
                    .to_string(),
                label_empty.to_string(),
            ),
            (
                "https://www.zhipin.com/wapi/zprelation/friend/getGeekFriendList.json".to_string(),
                friend_list.to_string(),
            ),
        ];

        let mut label_meta = HashMap::new();
        let entries = parse_boss_friend_packets(&packets, &mut label_meta);
        assert_eq!(entries.len(), 2);

        // Entry 0 joined real job title & city from geekFilterByLabel, direction from lastMessageInfo.fromId == uid
        assert_eq!(
            entries[0].job_id.as_deref(),
            Some("28charRealEncryptJobId000001")
        );
        assert_eq!(entries[0].title.as_deref(), Some("Rust 后端架构师"));
        assert_eq!(entries[0].location.as_deref(), Some("杭州"));
        assert_eq!(entries[0].last_message.as_deref(), Some("方便发一份最新简历吗？"));
        assert_eq!(entries[0].last_message_at, 1_732_695_500_000);
        assert_eq!(entries[0].last_message_received, Some(true));

        // Entry 1 has only securityId & HR title ("HRBP"): neither may be promoted to job_id / title
        assert_eq!(entries[1].job_id, None);
        assert_eq!(entries[1].title, None);
        assert_eq!(entries[1].last_message_received, Some(false));
    }

    #[test]
    fn dom_unread_fusion_enriches_api_entries_without_double_counting() {
        let mut entries = parse_boss_friend_list(
            r#"{
                "code": 0,
                "zpData": {
                    "result": [
                        {
                            "uid": 88001,
                            "encryptJobId": "boss-job-live-1",
                            "jobName": "Rust 工程师",
                            "brandName": "深蓝智能",
                            "name": "陈女士",
                            "unreadMsgCount": 0,
                            "lastMessageInfo": {
                                "showText": "约明天下午三点面试方便吗？",
                                "msgTime": 1732695500000
                            }
                        }
                    ]
                }
            }"#,
        )
        .unwrap();
        assert_eq!(entries[0].unread_count, 0);

        let dom_cards = vec![DomBossCard {
            key: "陈女士 深蓝智能 招聘经理".into(),
            boss_name: "陈女士".into(),
            company: "深蓝智能".into(),
            boss_title: "招聘经理".into(),
            preview: "约明天下午三点面试方便吗？".into(),
            unread_count: 2,
            status_tag: String::new(),
        }];

        enrich_entries_with_dom_unread(&mut entries, &dom_cards);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].unread_count, 2);
        assert_eq!(entries[0].last_message_received, Some(true));
        assert!(entries[0].interview_invited);
        assert_eq!(entries[0].status_tag.as_deref(), Some("收到约面"));
    }

    #[test]
    fn strict_boss_job_matching_never_falls_through_when_job_id_is_present() {
        let existing = JobDetail {
            resume_delivery_pending: false,
            source_url: None,
            id: "boss-job-A".into(),
            platform: "boss".into(),
            source_task_id: None,
            profile_id: None,
            profile_name: None,
            profile_snapshot_id: None,
            title: "前端工程师".into(),
            company_name: "同一科技".into(),
            detail: String::new(),
            salary: String::new(),
            location: None,
            is_reply: false,
            is_send_resume: false,
            pipeline_stage: None,
            notes: None,
            interview_at: None,
            created_at: String::new(),
            resume_sent_at: None,
            updated_at: String::new(),
        };
        let entry_b = BossListEntry {
            dedup_key: "boss-job-B".into(),
            uid: Some("1".into()),
            encrypt_boss_id: None,
            boss_name: Some("王先生".into()),
            job_id: Some("boss-job-B".into()),
            title: Some("后端工程师".into()),
            company_name: Some("同一科技".into()),
            salary: None,
            location: None,
            unread_count: 1,
            last_message: Some("你好".into()),
            last_message_at: 1_000,
            last_message_received: Some(true),
            status_tag: None,
            resume_sent: false,
            rejected: false,
            interview_invited: false,
            is_top: false,
        };
        assert!(match_local_boss_job(&entry_b, &[existing.clone()]).is_none());

        // Normalized ID matches with or without `boss:` prefix
        let mut entry_a = entry_b.clone();
        entry_a.job_id = Some("boss:boss-job-A".into());
        assert_eq!(
            match_local_boss_job(&entry_a, &[existing.clone()]).map(|j| j.id),
            Some("boss-job-A".into())
        );

        // ID-less DOM fallback matches unique normalized company when title doesn't conflict
        let mut dom_entry = entry_b;
        dom_entry.job_id = None;
        dom_entry.title = None;
        assert_eq!(
            match_local_boss_job(&dom_entry, &[existing]).map(|j| j.id),
            Some("boss-job-A".into())
        );
    }

    #[test]
    fn synced_job_created_at_uses_historical_timestamp_or_epoch_sentinel() {
        assert_eq!(synced_job_created_at(0), "2000-01-01 00:00:00");
        assert_eq!(synced_job_created_at(-1), "2000-01-01 00:00:00");
        // 2024-01-15 12:00:00 UTC in ms -> formats as 2024-01-15 across UTC-12..UTC+11
        let formatted = synced_job_created_at(1_705_320_000_000);
        assert!(formatted.starts_with("2024-01-15 "), "{formatted}");
    }
}
