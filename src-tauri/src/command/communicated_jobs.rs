use std::collections::HashSet;
use std::time::Duration;

use anyhow::Context;
use chrono::Local;
use rust_drission::{utils::sleep_random_ms, ChromiumPage, Page};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    browser,
    command::base::CommandResult,
    dao::{chat_message_dao, job_detail_dao, model::JobDetail},
    logger,
    rpa::boss::handler::{parse_chat_messages, parse_encrypt_job_id},
    rpa::run_flow::{is_job_task_stop_requested, try_start_job_task, JobTaskRunningGuard},
    utils::salary::decode_salary,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CollectCommunicatedJobsResult {
    pub inserted: usize,
    pub updated: usize,
    pub skipped: usize,
    pub messages_inserted: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct CommunicatedJobCardSnapshot {
    #[serde(default)]
    title: String,
    #[serde(default)]
    company_name: String,
    #[serde(default)]
    salary: Option<String>,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    detail_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CollectedCommunicatedJob {
    id: String,
    title: String,
    company_name: String,
    detail: String,
    salary: String,
    location: Option<String>,
    captured_at: String,
}

#[derive(Debug, Clone)]
struct CollectedCommunicatedJobsPayload {
    jobs: Vec<CollectedCommunicatedJob>,
    messages_inserted: usize,
}

#[tauri::command]
pub async fn job_collect_communicated(
    page_limit: Option<u32>,
) -> CommandResult<CollectCommunicatedJobsResult> {
    let page_limit = page_limit.unwrap_or(1).clamp(1, 20);
    let running_guard = match try_start_job_task() {
        Ok(guard) => guard,
        Err(error) => return CommandResult::err(error),
    };

    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let result = run_collect_communicated_jobs(page_limit, running_guard);
        let _ = tx.send(result);
    });

    match rx.await {
        Ok(Ok(result)) => CommandResult::ok(result),
        Ok(Err(error)) => CommandResult::err(error),
        Err(error) => CommandResult::err(error),
    }
}

fn run_collect_communicated_jobs(
    page_limit: u32,
    running_guard: JobTaskRunningGuard,
) -> Result<CollectCommunicatedJobsResult, anyhow::Error> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(collect_communicated_jobs_inner(page_limit));
    drop(running_guard);
    result
}

async fn collect_communicated_jobs_inner(
    page_limit: u32,
) -> Result<CollectCommunicatedJobsResult, anyhow::Error> {
    logger::info(format!("开始抓取已沟通过岗位，页数: {page_limit}"))?;
    let captured_at = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let payload = browser::with_browser(|page| {
        Box::pin(async move {
            collect_communicated_jobs_from_browser(page, page_limit, captured_at).await
        })
    })
    .await?;

    upsert_collected_jobs(payload)
}

async fn collect_communicated_jobs_from_browser(
    page: &ChromiumPage,
    page_limit: u32,
    captured_at: String,
) -> Result<CollectedCommunicatedJobsPayload, anyhow::Error> {
    let mut jobs = Vec::new();
    let mut seen_ids = HashSet::new();
    let mut messages_inserted = 0;

    for page_no in 1..=page_limit {
        if is_job_task_stop_requested() {
            logger::info("抓取已沟通过岗位任务已结束")?;
            break;
        }

        logger::info(format!("正在抓取已沟通过岗位第 {page_no} 页"))?;
        let page_url = build_communicated_jobs_url(page_no);
        page.get(&page_url)?;
        sleep_random_ms(1500, 2500);
        wait_for_communicated_job_cards(page);
        scroll_page_once(page)?;
        sleep_random_ms(600, 1000);

        let snapshots = collect_card_snapshots(page)?;
        logger::info(format!(
            "第 {page_no} 页读取到 {} 个岗位卡片",
            snapshots.len()
        ))?;
        if snapshots.is_empty() {
            break;
        }

        for snapshot in snapshots {
            if is_job_task_stop_requested() {
                logger::info("抓取已沟通过岗位任务已结束")?;
                break;
            }

            let title = snapshot.title.trim().to_string();
            let company_name = snapshot.company_name.trim().to_string();
            if title.is_empty() && company_name.is_empty() {
                continue;
            }

            let normalized_url = snapshot.detail_url.as_deref().and_then(normalize_boss_url);
            let id = stable_job_id(&title, &company_name, normalized_url.as_deref());
            if !seen_ids.insert(id.clone()) {
                continue;
            }

            match collect_conversation_messages(page, &snapshot, &id) {
                Ok(inserted) => {
                    messages_inserted += inserted;
                    if inserted > 0 {
                        logger::info(format!(
                            "已保存沟通记录: {title} - {company_name}, 新增 {inserted} 条"
                        ))?;
                    }
                }
                Err(error) => {
                    logger::warning(format!(
                        "抓取沟通记录失败: {title} - {company_name}: {error}"
                    ))?;
                }
            }

            let detail = if let Some(ref detail_url) = normalized_url {
                logger::info(format!("正在获取岗位 JD: {title} - {company_name}"))?;
                let detail_tab = page.new_tab(Some(detail_url))?;
                sleep_random_ms(1500, 2500);
                let text_result = extract_detail_text(&detail_tab);
                let close_result = detail_tab.close();
                match (text_result, close_result) {
                    (Ok(text), Ok(())) => text,
                    (Ok(text), Err(error)) => {
                        logger::warning(format!("关闭岗位详情页失败: {error}"))?;
                        text
                    }
                    (Err(error), _) => return Err(error),
                }
            } else {
                String::new()
            };

            let salary = snapshot
                .salary
                .map(|salary| decode_salary(salary.trim()))
                .filter(|salary| !salary.is_empty())
                .unwrap_or_default();

            jobs.push(CollectedCommunicatedJob {
                id,
                title,
                company_name,
                detail,
                salary,
                location: snapshot
                    .location
                    .map(|location| location.trim().to_string())
                    .filter(|location| !location.is_empty()),
                captured_at: captured_at.clone(),
            });
        }
    }

    Ok(CollectedCommunicatedJobsPayload {
        jobs,
        messages_inserted,
    })
}

fn upsert_collected_jobs(
    payload: CollectedCommunicatedJobsPayload,
) -> Result<CollectCommunicatedJobsResult, anyhow::Error> {
    let jobs = payload.jobs;
    let total = jobs.len();
    let mut inserted = 0;
    let mut updated = 0;
    let mut skipped = 0;

    for collected in jobs {
        let existing = job_detail_dao::get_by_id(&collected.id)?;
        let existed = existing.is_some();
        let job_id = collected.id.clone();
        let job = merge_collected_job_detail(existing, collected);

        if existed {
            if job_detail_dao::update(&job_id, job)? {
                updated += 1;
            } else {
                skipped += 1;
            }
        } else {
            job_detail_dao::create(job)?;
            inserted += 1;
        }
    }

    logger::info(format!(
        "已沟通过岗位抓取完成，新增 {inserted} 条，更新 {updated} 条，跳过 {skipped} 条"
    ))?;

    Ok(CollectCommunicatedJobsResult {
        inserted,
        updated,
        skipped,
        messages_inserted: payload.messages_inserted,
        total,
    })
}

fn collect_conversation_messages(
    page: &ChromiumPage,
    snapshot: &CommunicatedJobCardSnapshot,
    fallback_job_id: &str,
) -> Result<usize, anyhow::Error> {
    let history_listener = page.listen_url("zpchat/geek/historyMsg")?;
    let boss_data_listener = page.listen_url("zpchat/geek/getBossData")?;
    click_communicated_job_card(page, snapshot)?;
    sleep_random_ms(700, 1200);

    let job_id = match boss_data_listener.wait(Duration::from_secs(6)) {
        Ok(Some(packet)) => packet
            .body
            .and_then(|body| String::from_utf8(body).ok())
            .and_then(|body| parse_encrypt_job_id(&body))
            .unwrap_or_else(|| fallback_job_id.to_string()),
        _ => fallback_job_id.to_string(),
    };

    let Some(packet) = history_listener.wait(Duration::from_secs(8))? else {
        return Ok(0);
    };

    let Some(body) = packet.body else {
        return Ok(0);
    };

    let body_str = String::from_utf8(body).context("historyMsg 响应非 UTF-8 编码")?;
    let chat_messages = parse_chat_messages(&body_str)?;
    chat_message_dao::save_incremental(&job_id, &chat_messages)
}

fn click_communicated_job_card(
    page: &ChromiumPage,
    snapshot: &CommunicatedJobCardSnapshot,
) -> Result<(), anyhow::Error> {
    let title_json = serde_json::to_string(snapshot.title.trim())?;
    let company_json = serde_json::to_string(snapshot.company_name.trim())?;
    let detail_url_json = serde_json::to_string(
        &snapshot
            .detail_url
            .as_deref()
            .and_then(normalize_boss_url)
            .unwrap_or_default(),
    )?;
    let script = format!(
        r#"
        (() => {{
          const clean = (value) => (value || '').replace(/\s+/g, ' ').trim();
          const title = {title_json};
          const company = {company_json};
          const detailUrl = {detail_url_json};
          const cards = Array.from(document.querySelectorAll('.item-boss'));
          const target = cards.find((card) => {{
            const text = clean(card.innerText || card.textContent);
            const href = card.querySelector('.job-name a.name, a.name')?.href || '';
            if (detailUrl && href && href.split('?')[0] === detailUrl.split('?')[0]) return true;
            return (!title || text.includes(title)) && (!company || text.includes(company));
          }});
          if (!target) return false;
          target.scrollIntoView({{ block: 'center' }});
          target.click();
          return true;
        }})()
        "#
    );

    let value = page.run_js_await(&script)?;
    if extract_remote_value(value).as_bool().unwrap_or(false) {
        Ok(())
    } else {
        Err(anyhow::anyhow!("未找到对应的已沟通岗位卡片"))
    }
}

fn build_communicated_jobs_url(page: u32) -> String {
    let page = page.max(1);
    format!("https://www.zhipin.com/web/geek/recommend?tab=2&page={page}&tag=3")
}

fn normalize_boss_url(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() || trimmed.starts_with("javascript:") {
        return None;
    }

    if trimmed.starts_with("https://www.zhipin.com/") {
        return Some(trimmed.to_string());
    }

    if trimmed.starts_with("https://zhipin.com/") {
        return Some(trimmed.replacen("https://zhipin.com", "https://www.zhipin.com", 1));
    }

    if trimmed.starts_with("//www.zhipin.com/") {
        return Some(format!("https:{trimmed}"));
    }

    if trimmed.starts_with('/') {
        return Some(format!("https://www.zhipin.com{trimmed}"));
    }

    None
}

fn stable_job_id(title: &str, company_name: &str, detail_url: Option<&str>) -> String {
    if let Some(normalized_url) = detail_url.and_then(normalize_boss_url) {
        let path = normalized_url.split('?').next().unwrap_or(&normalized_url);
        if let Some((_, id_part)) = path.split_once("/job_detail/") {
            let id = id_part.trim_end_matches(".html").trim_matches('/');
            if !id.is_empty() {
                return id.to_string();
            }
        }
    }

    format!("manual-{}-{}", slug_text(title), slug_text(company_name))
}

fn merge_collected_job_detail(
    existing: Option<JobDetail>,
    collected: CollectedCommunicatedJob,
) -> JobDetail {
    match existing {
        Some(existing) => JobDetail {
            id: collected.id,
            platform: "boss".to_string(),
            title: prefer_new(collected.title, existing.title),
            company_name: prefer_new(collected.company_name, existing.company_name),
            detail: prefer_new(collected.detail, existing.detail),
            salary: prefer_new(collected.salary, existing.salary),
            location: prefer_new_option(collected.location, existing.location),
            is_reply: true,
            is_send_resume: existing.is_send_resume,
            created_at: existing.created_at,
            resume_sent_at: existing.resume_sent_at,
            updated_at: collected.captured_at,
        },
        None => JobDetail {
            id: collected.id,
            platform: "boss".to_string(),
            title: collected.title,
            company_name: collected.company_name,
            detail: collected.detail,
            salary: collected.salary,
            location: collected.location,
            is_reply: true,
            is_send_resume: false,
            created_at: collected.captured_at.clone(),
            resume_sent_at: None,
            updated_at: collected.captured_at,
        },
    }
}

fn prefer_new(next: String, fallback: String) -> String {
    if next.trim().is_empty() {
        fallback
    } else {
        next
    }
}

fn prefer_new_option(next: Option<String>, fallback: Option<String>) -> Option<String> {
    next.filter(|value| !value.trim().is_empty()).or(fallback)
}

fn wait_for_communicated_job_cards(page: &ChromiumPage) {
    let _ = page.wait(".item-boss", Duration::from_secs(10));
}

fn collect_card_snapshots(
    page: &ChromiumPage,
) -> Result<Vec<CommunicatedJobCardSnapshot>, anyhow::Error> {
    let script = r#"
    (() => {
      const clean = (value) => (value || '').replace(/\s+/g, ' ').trim();
      const pickText = (root, selectors) => {
        for (const selector of selectors) {
          const element = root.querySelector(selector);
          const text = clean(element && element.textContent);
          if (text) return text;
        }
        return '';
      };
      const pickHref = (root) => {
        const element = root.querySelector('.job-name a.name, a.name');
        return element ? (element.getAttribute('href') || '') : '';
      };
      const pickSalary = (root) => {
        const grayP = root.querySelector('.job-info p.gray');
        if (!grayP) return '';
        const clone = grayP.cloneNode(true);
        clone.querySelectorAll('span').forEach(el => el.remove());
        return clean(clone.textContent);
      };
      const isClosed = (root) => {
        const el = root.querySelector('.info-header-close');
        return Boolean(el && clean(el.textContent).includes('职位已关闭'));
      };

      return Array.from(document.querySelectorAll('.item-boss')).filter(card => !isClosed(card)).map((card) => {
        const title = pickText(card, ['.job-name-text', '.name']);
        const company_name = pickText(card, ['.company-info .text b a', '.company-name']);
        const salary = pickSalary(card);
        const location = pickText(card, ['.location em', '.location']);
        const detail_url = pickHref(card);

        return {
          title,
          company_name,
          salary: salary || null,
          location: location || null,
          detail_url: detail_url || null
        };
      }).filter((item) => item.title || item.company_name || item.detail_url);
    })()
    "#;

    let value = page.run_js_await(script)?;
    serde_json::from_value(extract_remote_value(value)).context("解析已沟通过岗位卡片快照失败")
}

fn extract_detail_text(page: &Page) -> Result<String, anyhow::Error> {
    let script = r#"
    (() => {
      const clean = (value) => (value || '').replace(/\n{3,}/g, '\n\n').trim();
      const selectors = [
        '.job-detail-body',
        '.job-detail-section',
        '.job-sec-text',
        '.job-detail',
        '.detail-content',
        '.job-detail-box'
      ];
      for (const selector of selectors) {
        const element = document.querySelector(selector);
        const text = clean(element && element.innerText);
        if (text) return text;
      }
      return '';
    })()
    "#;
    let value = page.run_js_await(script)?;
    let raw_text = extract_remote_value(value)
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();

    Ok(sanitize_boss_job_detail(&raw_text))
}

fn sanitize_boss_job_detail(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let marker = "职位描述";
    let body = normalized
        .find(marker)
        .map(|index| &normalized[index + marker.len()..])
        .unwrap_or(normalized.as_str());

    let cleaned = body.trim().trim_start_matches([':', '：', '-', '—']).trim();

    cleaned.replace("\n\n\n", "\n\n")
}

fn scroll_page_once(page: &ChromiumPage) -> Result<(), anyhow::Error> {
    let script = r#"
    (() => {
      const html = document.documentElement;
      const body = document.body;
      const scrollContainer = html.scrollHeight > html.clientHeight ? html : body;
      scrollContainer.scrollTop = scrollContainer.scrollHeight;
    })()
    "#;
    page.run_js_await(script)?;
    Ok(())
}

fn extract_remote_value(value: Value) -> Value {
    value.get("value").cloned().unwrap_or(value)
}

fn slug_text(text: &str) -> String {
    let slug = text
        .split_whitespace()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "unknown".to_string()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dao::model::JobDetail;

    fn existing_job() -> JobDetail {
        JobDetail {
            id: "abc123".to_string(),
            platform: "boss".to_string(),
            title: "旧标题".to_string(),
            company_name: "旧公司".to_string(),
            detail: "旧 JD".to_string(),
            salary: "10-15K".to_string(),
            location: Some("北京".to_string()),
            is_reply: false,
            is_send_resume: true,
            created_at: "2026-06-01 10:00:00".to_string(),
            resume_sent_at: Some("2026-06-02 10:00:00".to_string()),
            updated_at: "2026-06-01 10:00:00".to_string(),
        }
    }

    #[test]
    fn builds_communicated_jobs_url_for_requested_page() {
        assert_eq!(
            build_communicated_jobs_url(3),
            "https://www.zhipin.com/web/geek/recommend?tab=2&page=3&tag=3"
        );
        assert_eq!(
            build_communicated_jobs_url(0),
            "https://www.zhipin.com/web/geek/recommend?tab=2&page=1&tag=3"
        );
    }

    #[test]
    fn normalizes_boss_urls() {
        assert_eq!(
            normalize_boss_url("/job_detail/abc123.html"),
            Some("https://www.zhipin.com/job_detail/abc123.html".to_string())
        );
        assert_eq!(
            normalize_boss_url("https://zhipin.com/job_detail/abc123.html"),
            Some("https://www.zhipin.com/job_detail/abc123.html".to_string())
        );
        assert_eq!(normalize_boss_url("javascript:void(0)"), None);
        assert_eq!(normalize_boss_url(""), None);
    }

    #[test]
    fn derives_stable_job_id_from_detail_url_when_available() {
        assert_eq!(
            stable_job_id(
                "后端工程师",
                "示例科技",
                Some("https://www.zhipin.com/job_detail/abc123.html?ka=communicated")
            ),
            "abc123"
        );
        assert_eq!(
            stable_job_id("后端 工程师", "示例科技", None),
            "manual-后端-工程师-示例科技"
        );
    }

    #[test]
    fn sanitizes_boss_detail_text_before_job_description_heading() {
        let raw = "微信扫码分享\n举\n报\n职位描述\n负责推荐系统后端开发。\n要求熟悉 Rust。";

        assert_eq!(
            sanitize_boss_job_detail(raw),
            "负责推荐系统后端开发。\n要求熟悉 Rust。"
        );
    }

    #[test]
    fn builds_new_replied_job_detail_from_collected_job() {
        let collected = CollectedCommunicatedJob {
            id: "abc123".to_string(),
            title: "后端工程师".to_string(),
            company_name: "示例科技".to_string(),
            detail: "负责 Rust 服务开发".to_string(),
            salary: "20-30K".to_string(),
            location: Some("上海".to_string()),
            captured_at: "2026-06-11 12:00:00".to_string(),
        };

        let job = merge_collected_job_detail(None, collected);

        assert_eq!(job.id, "abc123");
        assert_eq!(job.platform, "boss");
        assert!(job.is_reply);
        assert!(!job.is_send_resume);
        assert_eq!(job.created_at, "2026-06-11 12:00:00");
        assert_eq!(job.updated_at, "2026-06-11 12:00:00");
    }

    #[test]
    fn updates_existing_job_and_preserves_delivery_fields() {
        let collected = CollectedCommunicatedJob {
            id: "abc123".to_string(),
            title: "新标题".to_string(),
            company_name: "新公司".to_string(),
            detail: "新 JD".to_string(),
            salary: "20-30K".to_string(),
            location: Some("上海".to_string()),
            captured_at: "2026-06-11 12:00:00".to_string(),
        };

        let job = merge_collected_job_detail(Some(existing_job()), collected);

        assert_eq!(job.title, "新标题");
        assert_eq!(job.company_name, "新公司");
        assert_eq!(job.detail, "新 JD");
        assert_eq!(job.salary, "20-30K");
        assert_eq!(job.location, Some("上海".to_string()));
        assert!(job.is_reply);
        assert!(job.is_send_resume);
        assert_eq!(job.created_at, "2026-06-01 10:00:00");
        assert_eq!(job.resume_sent_at, Some("2026-06-02 10:00:00".to_string()));
        assert_eq!(job.updated_at, "2026-06-11 12:00:00");
    }
}
