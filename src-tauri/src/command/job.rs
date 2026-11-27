use crate::command::base::CommandResult;
use crate::config;
use crate::dao::model::{ChatMessageRecord, InterviewJobAnalysis, JobDetail, JobPipelineStage};
use crate::dao::{analysis_dao, chat_message_dao, job_detail_dao};
use crate::job_description::ParsedJobDescription;
use chrono::{Duration, Local, NaiveDate, NaiveDateTime, TimeZone};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[tauri::command]
pub fn job_open_source(id: String) -> CommandResult<()> {
    let result = (|| -> anyhow::Result<()> {
        let job = job_detail_dao::get_by_id(&id)?.ok_or_else(|| anyhow::anyhow!("岗位不存在"))?;
        let source = job.source_url.filter(|url| !url.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("该历史岗位未保存原始JD链接"))?;
        let url = tauri::Url::parse(&source)?;
        let host = url.host_str().unwrap_or("");
        if !matches!(url.scheme(), "http" | "https") || !["zhipin.com", "liepin.com", "51job.com"].iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{domain}"))) {
            anyhow::bail!("职位链接不是受支持平台的网页地址");
        }
        #[cfg(target_os = "windows")]
        { std::process::Command::new("rundll32.exe").arg("url.dll,FileProtocolHandler").arg(url.as_str()).spawn()?; }
        #[cfg(target_os = "macos")]
        { std::process::Command::new("open").arg(url.as_str()).spawn()?; }
        #[cfg(target_os = "linux")]
        { std::process::Command::new("xdg-open").arg(url.as_str()).spawn()?; }
        Ok(())
    })();
    match result { Ok(()) => CommandResult::ok(()), Err(error) => CommandResult::err(error.to_string()) }
}

#[tauri::command]
pub fn job_list() -> CommandResult<Vec<JobDetail>> {
    match job_detail_dao::list() {
        Ok(list) => CommandResult::ok(list),
        Err(e) => CommandResult::err(e.to_string()),
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommunicationStatus {
    Rejected,
    Replied,
    NoReply,
}

#[derive(Debug, Serialize)]
pub struct JobListItem {
    #[serde(flatten)]
    pub job: JobDetail,
    pub communication_status: CommunicationStatus,
    pub latest_message: Option<String>,
    pub latest_message_at: Option<i64>,
    pub latest_message_received: Option<bool>,
}

fn is_explicit_rejection(text: &str) -> bool {
    let normalized = text
        .to_lowercase()
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    const REJECTION_PHRASES: &[&str] = &[
        "不太合适",
        "不合适",
        "暂不考虑",
        "暂时不考虑",
        "不匹配",
        "不符合",
        "岗位已招满",
        "已经招满",
        "职位已招满",
        "岗位关闭",
        "职位关闭",
        "停止招聘",
        "没有hc",
        "无hc",
        "不通过",
        "暂时没有合适",
        "目前没有合适",
    ];

    REJECTION_PHRASES
        .iter()
        .any(|phrase| normalized.contains(phrase))
}

fn build_job_list_items(jobs: Vec<JobDetail>, messages: Vec<ChatMessageRecord>) -> Vec<JobListItem> {
    let mut replied_job_ids = HashSet::new();
    let mut rejected_job_ids = HashSet::new();
    let mut latest_messages_by_job_id: HashMap<String, ChatMessageRecord> = HashMap::new();

    for message in messages
        .into_iter()
        .filter(|message| !message.job_id.is_empty() && !message.text.trim().is_empty())
    {
        if message.received {
            replied_job_ids.insert(message.job_id.clone());
            if is_explicit_rejection(&message.text) {
                rejected_job_ids.insert(message.job_id.clone());
            }
        }

        let should_replace = latest_messages_by_job_id
            .get(&message.job_id)
            .map(|current| (message.time, message.mid) > (current.time, current.mid))
            .unwrap_or(true);
        if should_replace {
            latest_messages_by_job_id.insert(message.job_id.clone(), message);
        }
    }

    jobs.into_iter()
        .map(|job| {
            let communication_status = if rejected_job_ids.contains(&job.id) {
                CommunicationStatus::Rejected
            } else if replied_job_ids.contains(&job.id) {
                CommunicationStatus::Replied
            } else {
                CommunicationStatus::NoReply
            };
            let latest_message = latest_messages_by_job_id.get(&job.id);
            JobListItem {
                job,
                communication_status,
                latest_message: latest_message.map(|message| message.text.clone()),
                latest_message_at: latest_message.map(|message| message.time),
                latest_message_received: latest_message.map(|message| message.received),
            }
        })
        .collect()
}

#[tauri::command]
pub fn job_list_with_status() -> CommandResult<Vec<JobListItem>> {
    let result = (|| -> anyhow::Result<Vec<JobListItem>> {
        let jobs = job_detail_dao::list()?;
        let messages = chat_message_dao::list()?;
        Ok(build_job_list_items(jobs, messages))
    })();

    match result {
        Ok(list) => CommandResult::ok(list),
        Err(error) => CommandResult::err(error.to_string()),
    }
}

#[cfg(test)]
mod communication_status_tests {
    use super::{build_job_list_items, is_explicit_rejection, CommunicationStatus, job_list_with_status, build_job_search_overview};
    use crate::dao::job_detail_dao;
    use chrono::Local;
    use crate::dao::model::{ChatMessageRecord, JobDetail};

    fn job(id: &str) -> JobDetail {
        JobDetail {
            resume_delivery_pending: false,
            source_url: None,
            id: id.into(),
            platform: "boss".into(),
            source_task_id: None,
            profile_id: None,
            profile_name: None,
            profile_snapshot_id: None,
            title: format!("{id} title"),
            company_name: format!("{id} company"),
            detail: String::new(),
            salary: String::new(),
            location: None,
            is_reply: false,
            is_send_resume: false,
            created_at: "2026-08-19 09:00:00".into(),
            resume_sent_at: None,
            updated_at: "2026-08-19 09:00:00".into(),
            pipeline_stage: None,
            notes: None,
            interview_at: None,
        }
    }

    #[test]
    fn three_platform_workers_persist_results_for_both_job_management_and_overview() {
        let directory=tempfile::tempdir().unwrap();
        crate::dao::init(directory.path()).unwrap();
        let barrier=std::sync::Arc::new(std::sync::Barrier::new(3));
        let now=Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let workers=[("boss","boss-test-00000001"),("liepin","liepin:123456789"),("51job","51job:123456789")].map(|(platform,id)| {
            let mut record=job(id);
            record.platform=platform.into();
            record.source_url=Some(format!("https://example.test/{platform}/jd"));
            record.is_send_resume=true;
            record.created_at=now.clone();record.updated_at=now.clone();record.resume_sent_at=Some(now.clone());
            let barrier=barrier.clone();
            std::thread::spawn(move || { barrier.wait(); job_detail_dao::create(record).unwrap(); })
        });
        for worker in workers { worker.join().unwrap(); }
        let list=job_list_with_status().data.unwrap();
        assert_eq!(list.len(),3);
        assert!(list.iter().all(|item|item.job.is_send_resume && item.job.source_url.is_some()));
        let overview=build_job_search_overview(0,80).unwrap();
        assert_eq!(overview.metrics.resume_sent_jobs,3);
        assert_eq!(overview.metrics.total_jobs,3);
        assert_eq!(overview.source_distribution.len(),3);
        assert!(overview.source_distribution.iter().any(|slice|slice.source=="前程无忧" && slice.count==1));
    }

    fn message(job_id: &str, mid: i64, received: bool, text: &str, time: i64) -> ChatMessageRecord {
        ChatMessageRecord {
            id: ChatMessageRecord::build_id("boss", job_id, mid),
            job_id: job_id.into(),
            platform: "boss".into(),
            conversation_id: job_id.into(),
            mid,
            received,
            text: text.into(),
            time,
            from_name: if received { "招聘方" } else { "我" }.into(),
        }
    }

    #[test]
    fn recognizes_explicit_rejection_messages() {
        assert!(is_explicit_rejection("您好，您的经历和岗位不太合适"));
        assert!(is_explicit_rejection("这个职位已经招满了"));
        assert!(is_explicit_rejection("目前没有 HC，感谢关注"));
    }

    #[test]
    fn does_not_treat_normal_replies_as_rejection() {
        assert!(!is_explicit_rejection("你好，可以发一份简历吗？"));
        assert!(!is_explicit_rejection("感谢关注，我们先沟通一下"));
    }

    #[test]
    fn keeps_replied_and_rejected_received_only() {
        let items = build_job_list_items(
            vec![job("self-only"), job("hr-replied"), job("hr-rejected")],
            vec![
                message("self-only", 1, false, "我主动发了一条消息", 10),
                message("hr-replied", 2, true, "可以发一份简历吗？", 20),
                message("hr-rejected", 3, true, "这个岗位暂不考虑", 30),
            ],
        );

        let self_only = items.iter().find(|item| item.job.id == "self-only").unwrap();
        let hr_replied = items.iter().find(|item| item.job.id == "hr-replied").unwrap();
        let hr_rejected = items.iter().find(|item| item.job.id == "hr-rejected").unwrap();

        assert_eq!(self_only.communication_status, CommunicationStatus::NoReply);
        assert_eq!(hr_replied.communication_status, CommunicationStatus::Replied);
        assert_eq!(hr_rejected.communication_status, CommunicationStatus::Rejected);
    }

    #[test]
    fn latest_message_ignores_blank_text_and_empty_job_id() {
        let items = build_job_list_items(
            vec![job("job-1"), job("job-2")],
            vec![
                message("job-1", 1, true, "有效消息", 10),
                message("job-1", 2, true, "   ", 20),
                message("", 3, true, "无法关联岗位", 30),
            ],
        );

        let job_1 = items.iter().find(|item| item.job.id == "job-1").unwrap();
        let job_2 = items.iter().find(|item| item.job.id == "job-2").unwrap();

        assert_eq!(job_1.latest_message.as_deref(), Some("有效消息"));
        assert_eq!(job_1.latest_message_at, Some(10));
        assert_eq!(job_2.latest_message, None);
        assert_eq!(job_2.latest_message_at, None);
        assert_eq!(job_2.latest_message_received, None);
    }

    #[test]
    fn latest_message_uses_mid_as_tiebreaker_for_same_millisecond() {
        let items = build_job_list_items(
            vec![job("job-1")],
            vec![
                message("job-1", 10, true, "同毫秒较小 mid", 100),
                message("job-1", 12, false, "同毫秒较大 mid", 100),
                message("job-1", 11, true, "更早时间", 99),
            ],
        );

        let item = items.iter().find(|item| item.job.id == "job-1").unwrap();

        assert_eq!(item.latest_message.as_deref(), Some("同毫秒较大 mid"));
        assert_eq!(item.latest_message_at, Some(100));
        assert_eq!(item.latest_message_received, Some(false));
    }

    #[test]
    fn builds_profile_and_platform_attribution() {
        let mut a = job("boss-1");
        a.profile_name = Some("Rust 后端".into());
        a.is_reply = true;
        a.is_send_resume = true;

        let mut b = job("liepin:2");
        b.platform = "liepin".into();
        b.profile_name = Some("Go 后端".into());

        let jobs = vec![a, b];
        let msgs = vec![
            message("boss-1", 1, true, "发个简历看看", 10),
            message("liepin:2", 2, false, "您好我对岗位感兴趣", 20),
        ];
        let job_refs: Vec<&JobDetail> = jobs.iter().collect();
        let msg_refs: Vec<&ChatMessageRecord> = msgs.iter().collect();

        let by_profile = super::build_attribution(&job_refs, &msg_refs, |j| {
            j.profile_name.clone().unwrap_or_else(|| "未归属".into())
        });
        assert_eq!(by_profile.len(), 2);
        let rust = by_profile.iter().find(|r| r.label == "Rust 后端").unwrap();
        assert_eq!(rust.replied_jobs, 1);
        assert_eq!(rust.communicated_jobs, 1);
        assert_eq!(rust.reply_rate, 100.0);
        assert_eq!(rust.resume_sent_jobs, 1);

        let go = by_profile.iter().find(|r| r.label == "Go 后端").unwrap();
        assert_eq!(go.replied_jobs, 0);
        assert_eq!(go.communicated_jobs, 1);
        assert_eq!(go.reply_rate, 0.0);
    }

    #[test]
    fn exports_csv_with_bom_and_markdown_with_notes_and_jd() {
        let mut record = job("boss-export-1");
        record.title = "Rust 工程师, 后端".into();
        record.company_name = "星辰\"科技\"".into();
        record.salary = "25-40K".into();
        record.detail = "负责核心分布式系统研发".into();
        record.pipeline_stage = Some(super::JobPipelineStage::InterviewScheduled);
        record.interview_at = Some("2026-08-25 14:30".into());
        record.notes = Some("一面问 tokio 调度".into());

        let items = build_job_list_items(
            vec![record],
            vec![message("boss-export-1", 1, true, "周二下午两点方便面试吗？", 100)],
        );
        let mut scores = std::collections::HashMap::new();
        scores.insert("boss-export-1", 92u8);

        let csv = super::render_jobs_csv(&items, &scores);
        assert!(csv.starts_with('\u{FEFF}'));
        assert!(csv.contains("\"Rust 工程师, 后端\""));
        assert!(csv.contains("\"星辰\"\"科技\"\"\""));
        assert!(csv.contains("已约面"));
        assert!(csv.contains("92"));

        let md = super::render_jobs_markdown(&items, &scores);
        assert!(md.contains("## Rust 工程师, 后端 — 星辰\"科技\""));
        assert!(md.contains("一面问 tokio 调度"));
        assert!(md.contains("负责核心分布式系统研发"));
        assert!(md.contains("92 分"));
    }
}

#[derive(Debug, Serialize)]
pub struct OverviewMetrics {
    pub total_jobs: usize,
    pub communicated_jobs: usize,
    pub replied_jobs: usize,
    pub reply_rate: f64,
    pub resume_sent_jobs: usize,
    pub high_match_jobs: usize,
    /// 窗口内已经跑过 AI 分析的岗位数，用于区分「没数据」和「匹配度低」
    pub analyzed_jobs: usize,
}

#[derive(Debug, Serialize)]
pub struct OverviewDailyActivity {
    pub date: String,
    pub jobs: usize,
    pub replies: usize,
    pub communicated: usize,
    pub resume_sent: usize,
    pub high_match: usize,
}

/// 岗位来源分布切片
#[derive(Debug, Serialize)]
pub struct OverviewSourceSlice {
    pub source: String,
    pub count: usize,
}

/// 按求职方案或平台聚合的投递转化对比
#[derive(Debug, Serialize, PartialEq)]
pub struct OverviewAttribution {
    pub label: String,
    pub total_jobs: usize,
    pub communicated_jobs: usize,
    pub replied_jobs: usize,
    pub reply_rate: f64,
    pub resume_sent_jobs: usize,
}

#[derive(Debug, Serialize)]
pub struct OverviewConversation {
    pub job_id: String,
    pub company_name: String,
    pub title: String,
    pub last_message: String,
    pub last_message_at: i64,
    /// 最后一条消息是否来自招聘方
    pub received: bool,
    /// 会话内是否出现过招聘方消息
    pub has_reply: bool,
    pub message_count: usize,
}

#[derive(Debug, Serialize)]
pub struct JobSearchOverview {
    pub days: u32,
    pub metrics: OverviewMetrics,
    /// 上一个等长周期的指标，用于计算环比变化
    pub previous_metrics: OverviewMetrics,
    pub daily_activity: Vec<OverviewDailyActivity>,
    pub source_distribution: Vec<OverviewSourceSlice>,
    pub active_conversations: Vec<OverviewConversation>,
    /// 按求职方案聚合的转化对比（哪个简历/方向方案回复率更高）
    pub profile_attribution: Vec<OverviewAttribution>,
    /// 按招聘平台聚合的转化对比
    pub platform_attribution: Vec<OverviewAttribution>,
    /// 高匹配的判定分数线，取自默认求职方案的岗位分析配置
    pub high_match_score: u8,
}

/// `days` 为 0 表示只统计今日（自然日），其余表示最近 N 天
#[tauri::command]
pub fn job_search_overview(
    app_handle: tauri::AppHandle,
    days: Option<u32>,
) -> CommandResult<JobSearchOverview> {
    // 概览是跨方案的全局视图，阈值取默认方案；读不到配置时回落到内置默认值
    let high_match_score = config::load_app_config_inner(app_handle)
        .ok()
        .and_then(|app_config| {
            app_config
                .job_profile(None)
                .ok()
                .map(|profile| profile.analysis_config.high_match_score)
        })
        .unwrap_or(config::DEFAULT_HIGH_MATCH_SCORE);

    match build_job_search_overview(days.unwrap_or(30).clamp(0, 365), high_match_score) {
        Ok(overview) => CommandResult::ok(overview),
        Err(error) => CommandResult::err(error.to_string()),
    }
}

/// 单个统计周期的取数结果
struct PeriodSnapshot<'a> {
    metrics: OverviewMetrics,
    jobs: Vec<&'a JobDetail>,
    messages: Vec<&'a ChatMessageRecord>,
}

/// 统计窗口取半开区间 [start, end)
fn collect_period<'a>(
    jobs: &'a [JobDetail],
    messages: &'a [ChatMessageRecord],
    high_match_ids: &HashSet<&str>,
    analyzed_ids: &HashSet<&str>,
    start: chrono::DateTime<Local>,
    end: chrono::DateTime<Local>,
) -> PeriodSnapshot<'a> {
    let start_millis = start.timestamp_millis();
    let end_millis = end.timestamp_millis();
    let recent_message_job_ids: HashSet<&str> = messages
        .iter()
        .filter(|message| message.time >= start_millis && message.time < end_millis)
        .map(|message| message.job_id.as_str())
        .collect();

    let selected_jobs: Vec<&JobDetail> = jobs
        .iter()
        .filter(|job| {
            recent_message_job_ids.contains(job.id.as_str())
                || job
                    .resume_sent_at
                    .as_deref()
                    .into_iter()
                    .chain([job.updated_at.as_str(), job.created_at.as_str()])
                    .filter_map(parse_local_datetime)
                    .any(|time| time >= start && time < end)
        })
        .collect();
    let selected_job_ids: HashSet<&str> = selected_jobs.iter().map(|job| job.id.as_str()).collect();
    let selected_messages: Vec<&ChatMessageRecord> = messages
        .iter()
        .filter(|message| {
            selected_job_ids.contains(message.job_id.as_str())
                && message.time >= start_millis
                && message.time < end_millis
        })
        .collect();

    let communicated_ids: HashSet<&str> = selected_messages
        .iter()
        .map(|message| message.job_id.as_str())
        .chain(
            selected_jobs
                .iter()
                .filter(|job| job.is_reply)
                .map(|job| job.id.as_str()),
        )
        .collect();
    let replied_ids: HashSet<&str> = selected_messages
        .iter()
        .filter(|message| message.received)
        .map(|message| message.job_id.as_str())
        .chain(
            selected_jobs
                .iter()
                .filter(|job| job.is_reply)
                .map(|job| job.id.as_str()),
        )
        .collect();
    let communicated_jobs = communicated_ids.len();

    let metrics = OverviewMetrics {
        total_jobs: selected_jobs.len(),
        communicated_jobs,
        replied_jobs: replied_ids.len(),
        reply_rate: if communicated_jobs == 0 {
            0.0
        } else {
            replied_ids.len() as f64 * 100.0 / communicated_jobs as f64
        },
        resume_sent_jobs: selected_jobs
            .iter()
            .filter(|job| job.is_send_resume)
            .count(),
        high_match_jobs: selected_jobs
            .iter()
            .filter(|job| high_match_ids.contains(job.id.as_str()))
            .count(),
        analyzed_jobs: selected_jobs
            .iter()
            .filter(|job| analyzed_ids.contains(job.id.as_str()))
            .count(),
    };

    PeriodSnapshot {
        metrics,
        jobs: selected_jobs,
        messages: selected_messages,
    }
}

fn start_of_day(moment: chrono::DateTime<Local>) -> chrono::DateTime<Local> {
    moment
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|naive| Local.from_local_datetime(&naive).single())
        .unwrap_or(moment)
}

/// 岗位来源平台展示名，与岗位管理页的判定保持一致
fn platform_label(job: &JobDetail) -> &'static str {
    if job.platform == "51job" || job.id.starts_with("51job:") {
        "前程无忧"
    } else if job.platform == "liepin" || job.id.starts_with("liepin:") {
        "猎聘"
    } else {
        "BOSS 直聘"
    }
}

fn build_source_distribution(jobs: &[&JobDetail]) -> Vec<OverviewSourceSlice> {
    let mut counter: HashMap<&'static str, usize> = HashMap::new();
    for job in jobs {
        *counter.entry(platform_label(job)).or_default() += 1;
    }
    let mut slices: Vec<OverviewSourceSlice> = counter
        .into_iter()
        .map(|(source, count)| OverviewSourceSlice {
            source: source.to_string(),
            count,
        })
        .collect();
    // 「其他」固定排在末尾，其余按数量降序
    slices.sort_by(|left, right| {
        let rank = |slice: &OverviewSourceSlice| u8::from(slice.source == "其他");
        rank(left)
            .cmp(&rank(right))
            .then(right.count.cmp(&left.count))
            .then(left.source.cmp(&right.source))
    });
    slices
}

fn build_attribution<F>(
    jobs: &[&JobDetail],
    messages: &[&ChatMessageRecord],
    label_of: F,
) -> Vec<OverviewAttribution>
where
    F: Fn(&JobDetail) -> String,
{
    let message_job_ids: HashSet<&str> = messages
        .iter()
        .map(|message| message.job_id.as_str())
        .collect();
    let received_job_ids: HashSet<&str> = messages
        .iter()
        .filter(|message| message.received)
        .map(|message| message.job_id.as_str())
        .collect();

    struct Bucket {
        total_jobs: usize,
        communicated_jobs: usize,
        replied_jobs: usize,
        resume_sent_jobs: usize,
    }

    let mut grouped: HashMap<String, Bucket> = HashMap::new();
    for job in jobs {
        let label = label_of(job);
        let entry = grouped.entry(label).or_insert(Bucket {
            total_jobs: 0,
            communicated_jobs: 0,
            replied_jobs: 0,
            resume_sent_jobs: 0,
        });
        entry.total_jobs += 1;
        let communicated = job.is_reply || message_job_ids.contains(job.id.as_str());
        let replied = job.is_reply || received_job_ids.contains(job.id.as_str());
        if communicated {
            entry.communicated_jobs += 1;
        }
        if replied {
            entry.replied_jobs += 1;
        }
        if job.is_send_resume {
            entry.resume_sent_jobs += 1;
        }
    }

    let mut rows: Vec<OverviewAttribution> = grouped
        .into_iter()
        .map(|(label, bucket)| OverviewAttribution {
            label,
            total_jobs: bucket.total_jobs,
            communicated_jobs: bucket.communicated_jobs,
            replied_jobs: bucket.replied_jobs,
            reply_rate: if bucket.communicated_jobs == 0 {
                0.0
            } else {
                bucket.replied_jobs as f64 * 100.0 / bucket.communicated_jobs as f64
            },
            resume_sent_jobs: bucket.resume_sent_jobs,
        })
        .collect();

    rows.sort_by(|left, right| {
        right
            .total_jobs
            .cmp(&left.total_jobs)
            .then_with(|| left.label.cmp(&right.label))
    });
    rows
}

fn build_job_search_overview(
    days: u32,
    high_match_score: u8,
) -> anyhow::Result<JobSearchOverview> {
    let jobs = job_detail_dao::list()?;
    let messages = chat_message_dao::list()?;
    let analyses = analysis_dao::list()?;
    let now = Local::now();
    let today_start = start_of_day(now);
    // 统计窗口右端固定到今天结束，days = 0 时只覆盖今天
    let end = today_start + Duration::days(1);
    let start = if days == 0 {
        today_start
    } else {
        now - Duration::days(i64::from(days))
    };
    let span = end - start;

    let high_match_ids: HashSet<&str> = analyses
        .iter()
        .filter(|analysis| analysis.match_score >= high_match_score)
        .map(|analysis| analysis.job_id.as_str())
        .collect();
    let analyzed_ids: HashSet<&str> = analyses
        .iter()
        .map(|analysis| analysis.job_id.as_str())
        .collect();

    let current = collect_period(&jobs, &messages, &high_match_ids, &analyzed_ids, start, end);
    let previous = collect_period(
        &jobs,
        &messages,
        &high_match_ids,
        &analyzed_ids,
        start - span,
        start,
    );
    let source_distribution = build_source_distribution(&current.jobs);
    let profile_attribution = build_attribution(&current.jobs, &current.messages, |job| {
        job.profile_name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or("历史 / 未归属")
            .to_string()
    });
    let platform_attribution = build_attribution(&current.jobs, &current.messages, |job| {
        platform_label(job).to_string()
    });

    // 趋势图独立于统计窗口取全量数据，保证「今日」视图仍能看到走势
    let trend_days = days.clamp(7, 30);
    let mut jobs_by_date: HashMap<NaiveDate, usize> = HashMap::new();
    let mut resume_sent_by_date: HashMap<NaiveDate, usize> = HashMap::new();
    let mut high_match_by_date: HashMap<NaiveDate, usize> = HashMap::new();
    for job in &jobs {
        let created = job
            .resume_sent_at
            .as_deref()
            .and_then(parse_local_datetime)
            .or_else(|| parse_local_datetime(&job.created_at));
        if let Some(time) = created {
            *jobs_by_date.entry(time.date_naive()).or_default() += 1;
            if high_match_ids.contains(job.id.as_str()) {
                *high_match_by_date.entry(time.date_naive()).or_default() += 1;
            }
        }
        if job.is_send_resume {
            if let Some(time) = job.resume_sent_at.as_deref().and_then(parse_local_datetime) {
                *resume_sent_by_date.entry(time.date_naive()).or_default() += 1;
            }
        }
    }
    let mut replies_by_date: HashMap<NaiveDate, usize> = HashMap::new();
    let mut communicated_by_date: HashMap<NaiveDate, HashSet<&str>> = HashMap::new();
    for message in &messages {
        let Some(time) = Local.timestamp_millis_opt(message.time).single() else {
            continue;
        };
        let date = time.date_naive();
        if message.received {
            *replies_by_date.entry(date).or_default() += 1;
        }
        communicated_by_date
            .entry(date)
            .or_default()
            .insert(message.job_id.as_str());
    }
    let daily_activity = (0..trend_days)
        .rev()
        .map(|offset| {
            let date = (now - Duration::days(i64::from(offset))).date_naive();
            OverviewDailyActivity {
                date: date.format("%m-%d").to_string(),
                jobs: jobs_by_date.get(&date).copied().unwrap_or(0),
                replies: replies_by_date.get(&date).copied().unwrap_or(0),
                communicated: communicated_by_date.get(&date).map_or(0, HashSet::len),
                resume_sent: resume_sent_by_date.get(&date).copied().unwrap_or(0),
                high_match: high_match_by_date.get(&date).copied().unwrap_or(0),
            }
        })
        .collect();

    let job_map: HashMap<&str, &JobDetail> =
        jobs.iter().map(|job| (job.id.as_str(), job)).collect();
    let mut grouped: HashMap<&str, Vec<&ChatMessageRecord>> = HashMap::new();
    for message in current.messages {
        grouped
            .entry(message.job_id.as_str())
            .or_default()
            .push(message);
    }
    let mut active_conversations: Vec<OverviewConversation> = grouped
        .into_iter()
        .filter_map(|(job_id, mut items)| {
            let job = job_map.get(job_id)?;
            items.sort_by_key(|message| message.time);
            let last = items.last()?;
            Some(OverviewConversation {
                job_id: job_id.to_string(),
                company_name: job.company_name.clone(),
                title: job.title.clone(),
                last_message: last.text.clone(),
                last_message_at: last.time,
                received: last.received,
                has_reply: items.iter().any(|message| message.received),
                message_count: items.len(),
            })
        })
        .collect();
    active_conversations
        .sort_by_key(|conversation| std::cmp::Reverse(conversation.last_message_at));
    active_conversations.truncate(8);

    Ok(JobSearchOverview {
        days,
        metrics: current.metrics,
        previous_metrics: previous.metrics,
        daily_activity,
        source_distribution,
        active_conversations,
        profile_attribution,
        platform_attribution,
        high_match_score,
    })
}

fn parse_local_datetime(value: &str) -> Option<chrono::DateTime<Local>> {
    NaiveDateTime::parse_from_str(value.trim(), "%Y-%m-%d %H:%M:%S")
        .ok()
        .and_then(|value| Local.from_local_datetime(&value).single())
}

#[tauri::command]
pub fn job_get(id: String) -> CommandResult<JobDetail> {
    match job_detail_dao::get_by_id(&id) {
        Ok(Some(job)) => CommandResult::ok(job),
        Ok(None) => CommandResult::err(format!("岗位不存在: {}", id)),
        Err(e) => CommandResult::err(e.to_string()),
    }
}

#[tauri::command]
pub fn job_create(job: JobDetail) -> CommandResult<()> {
    match job_detail_dao::create(job) {
        Ok(()) => CommandResult::ok(()),
        Err(e) => CommandResult::err(e.to_string()),
    }
}

#[tauri::command]
pub fn job_update(id: String, job: JobDetail) -> CommandResult<()> {
    match job_detail_dao::update(&id, job) {
        Ok(true) => CommandResult::ok(()),
        Ok(false) => CommandResult::err(format!("岗位不存在: {}", id)),
        Err(e) => CommandResult::err(e.to_string()),
    }
}

#[tauri::command]
pub fn job_delete(id: String) -> CommandResult<()> {
    match job_detail_dao::delete(&id) {
        Ok(true) => CommandResult::ok(()),
        Ok(false) => CommandResult::err(format!("岗位不存在: {}", id)),
        Err(e) => CommandResult::err(e.to_string()),
    }
}

#[tauri::command]
pub fn job_batch_delete(ids: Vec<String>) -> CommandResult<usize> {
    match job_detail_dao::delete_many(&ids) {
        Ok(removed) => CommandResult::ok(removed),
        Err(e) => CommandResult::err(e.to_string()),
    }
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct TodayGreetCounts {
    pub boss: u32,
    pub liepin: u32,
    pub job51: u32,
}

#[tauri::command]
pub fn job_today_greet_counts() -> CommandResult<TodayGreetCounts> {
    let result = (|| -> anyhow::Result<TodayGreetCounts> {
        let jobs = job_detail_dao::list()?;
        let today = Local::now().format("%Y-%m-%d").to_string();
        Ok(TodayGreetCounts {
            boss: job_detail_dao::count_jobs_on_date_for_platform(&jobs, "boss", &today),
            liepin: job_detail_dao::count_jobs_on_date_for_platform(&jobs, "liepin", &today),
            job51: job_detail_dao::count_jobs_on_date_for_platform(&jobs, "51job", &today),
        })
    })();
    match result {
        Ok(counts) => CommandResult::ok(counts),
        Err(error) => CommandResult::err(error.to_string()),
    }
}

fn pipeline_stage_label(item: &JobListItem) -> &'static str {
    if let Some(stage) = item.job.pipeline_stage {
        return match stage {
            JobPipelineStage::Pending => "待投递",
            JobPipelineStage::Applied => "已投递",
            JobPipelineStage::Replied => "已回复",
            JobPipelineStage::InterviewScheduled => "已约面",
            JobPipelineStage::Interviewing => "面试中",
            JobPipelineStage::Offer => "已拿 Offer",
            JobPipelineStage::Rejected => "已婉拒",
            JobPipelineStage::Archived => "已归档",
        };
    }
    if item.communication_status == CommunicationStatus::Rejected {
        "已婉拒"
    } else if item.job.is_reply || item.communication_status == CommunicationStatus::Replied {
        "已回复"
    } else if item.job.is_send_resume {
        "已投递"
    } else {
        "待投递"
    }
}

fn communication_status_label(status: CommunicationStatus) -> &'static str {
    match status {
        CommunicationStatus::Replied => "对方已回复",
        CommunicationStatus::Rejected => "明确婉拒",
        CommunicationStatus::NoReply => "暂无回复",
    }
}

fn csv_escape(field: &str) -> String {
    let sanitized = field.replace('\r', "");
    if sanitized.contains([',', '"', '\n']) {
        format!("\"{}\"", sanitized.replace('"', "\"\""))
    } else {
        sanitized
    }
}

fn render_jobs_csv(
    items: &[JobListItem],
    scores_by_job_id: &HashMap<&str, u8>,
) -> String {
    // 加上 UTF-8 BOM，Windows Excel 直接双击打开也不会乱码
    let mut out = String::from("\u{FEFF}平台,岗位名称,公司名称,薪资,城市,求职方案,推进阶段,沟通状态,AI匹配度,已投简历,面试时间,面试笔记,最新消息,原始链接,创建时间\n");
    for item in items {
        let score = scores_by_job_id
            .get(item.job.id.as_str())
            .map(|value| value.to_string())
            .unwrap_or_default();
        let row = [
            platform_label(&item.job),
            item.job.title.as_str(),
            item.job.company_name.as_str(),
            item.job.salary.as_str(),
            item.job.location.as_deref().unwrap_or(""),
            item.job.profile_name.as_deref().unwrap_or(""),
            pipeline_stage_label(item),
            communication_status_label(item.communication_status),
            score.as_str(),
            if item.job.is_send_resume { "是" } else { "否" },
            item.job.interview_at.as_deref().unwrap_or(""),
            item.job.notes.as_deref().unwrap_or(""),
            item.latest_message.as_deref().unwrap_or(""),
            item.job.source_url.as_deref().unwrap_or(""),
            item.job.created_at.as_str(),
        ];
        let line = row
            .iter()
            .map(|cell| csv_escape(cell))
            .collect::<Vec<_>>()
            .join(",");
        out.push_str(&line);
        out.push('\n');
    }
    out
}

fn render_jobs_markdown(
    items: &[JobListItem],
    scores_by_job_id: &HashMap<&str, u8>,
) -> String {
    let exported_at = Local::now().format("%Y-%m-%d %H:%M").to_string();
    let mut out = format!(
        "# OfferFlow 投递与面试追踪导出\n\n> 导出时间：{exported_at} · 共 {} 条岗位\n\n",
        items.len()
    );
    for item in items {
        let score = scores_by_job_id
            .get(item.job.id.as_str())
            .map(|value| format!("{value} 分"))
            .unwrap_or_else(|| "未分析".to_string());
        out.push_str(&format!(
            "## {} — {}\n\n",
            item.job.title.trim(),
            item.job.company_name.trim()
        ));
        out.push_str(&format!("- **平台**：{}\n", platform_label(&item.job)));
        out.push_str(&format!("- **薪资**：{}\n", item.job.salary.trim()));
        if let Some(location) = item.job.location.as_deref().filter(|v| !v.trim().is_empty()) {
            out.push_str(&format!("- **城市**：{}\n", location.trim()));
        }
        if let Some(profile) = item
            .job
            .profile_name
            .as_deref()
            .filter(|v| !v.trim().is_empty())
        {
            out.push_str(&format!("- **求职方案**：{}\n", profile.trim()));
        }
        out.push_str(&format!("- **推进阶段**：{}\n", pipeline_stage_label(item)));
        out.push_str(&format!(
            "- **沟通状态**：{}\n",
            communication_status_label(item.communication_status)
        ));
        out.push_str(&format!("- **AI 匹配度**：{score}\n"));
        out.push_str(&format!(
            "- **已投简历**：{}\n",
            if item.job.is_send_resume { "是" } else { "否" }
        ));
        if let Some(interview_at) = item
            .job
            .interview_at
            .as_deref()
            .filter(|v| !v.trim().is_empty())
        {
            out.push_str(&format!("- **面试时间**：{}\n", interview_at.trim()));
        }
        if let Some(url) = item
            .job
            .source_url
            .as_deref()
            .filter(|v| !v.trim().is_empty())
        {
            out.push_str(&format!("- **原始链接**：{url}\n"));
        }
        if let Some(latest) = item
            .latest_message
            .as_deref()
            .filter(|v| !v.trim().is_empty())
        {
            out.push_str(&format!("- **最新消息**：{}\n", latest.trim()));
        }
        if let Some(notes) = item.job.notes.as_deref().filter(|v| !v.trim().is_empty()) {
            out.push_str(&format!("\n### 面试笔记\n\n{}\n", notes.trim()));
        }
        let cleaned_jd = crate::job_description::clean_text(&item.job.detail, &item.job.platform);
        if !cleaned_jd.trim().is_empty() {
            out.push_str(&format!("\n### JD 快照\n\n{}\n", cleaned_jd.trim()));
        }
        out.push_str("\n---\n\n");
    }
    out
}

/// 导出岗位追踪数据为 Excel 兼容 CSV 或 Obsidian/Notion Markdown。
#[tauri::command]
pub fn job_export(
    path: String,
    format: String,
    job_ids: Option<Vec<String>>,
) -> CommandResult<usize> {
    let result = (|| -> anyhow::Result<usize> {
        let target_path = path.trim();
        if target_path.is_empty() {
            anyhow::bail!("导出路径不能为空");
        }
        let jobs = job_detail_dao::list()?;
        let messages = chat_message_dao::list()?;
        let analyses = analysis_dao::list()?;
        let mut items = build_job_list_items(jobs, messages);
        if let Some(ids) = job_ids.filter(|list| !list.is_empty()) {
            let allow: HashSet<String> = ids.into_iter().collect();
            items.retain(|item| allow.contains(&item.job.id));
        }
        items.sort_by(|left, right| right.job.updated_at.cmp(&left.job.updated_at));

        let scores_by_job_id: HashMap<&str, u8> = analyses
            .iter()
            .filter(|analysis| analysis.parse_error.is_none())
            .map(|analysis| (analysis.job_id.as_str(), analysis.match_score))
            .collect();

        let content = match format.trim().to_ascii_lowercase().as_str() {
            "csv" => render_jobs_csv(&items, &scores_by_job_id),
            "markdown" | "md" => render_jobs_markdown(&items, &scores_by_job_id),
            other => anyhow::bail!("不支持的导出格式：{other}"),
        };

        std::fs::write(target_path, content.as_bytes())?;
        Ok(items.len())
    })();

    match result {
        Ok(count) => CommandResult::ok(count),
        Err(error) => CommandResult::err(error.to_string()),
    }
}

/// 岗位描述的结构化视图。
///
/// 前端不再自己洗一遍 JD：清洗规则跟着平台页面结构走，前后端各存一份必然漂移。
/// 页面要展示什么就从这个出口取，和喂给模型的是同一份文本
#[tauri::command]
pub fn job_description_view(job_id: String) -> CommandResult<ParsedJobDescription> {
    match job_detail_dao::get_by_id(&job_id) {
        Ok(Some(job)) => CommandResult::ok(crate::job_description::parse(
            &job.detail,
            &job.platform,
        )),
        Ok(None) => CommandResult::err(format!("岗位不存在: {}", job_id)),
        Err(e) => CommandResult::err(e.to_string()),
    }
}

#[tauri::command]
pub fn chat_messages_by_job(job_id: String) -> CommandResult<Vec<ChatMessageRecord>> {
    match chat_message_dao::find_by_job_id(&job_id) {
        Ok(list) => CommandResult::ok(list),
        Err(e) => CommandResult::err(e.to_string()),
    }
}

#[derive(serde::Deserialize)]
pub struct JobQueryParam {
    pub company_name: Option<String>,
    pub replied_only: Option<bool>,
    pub resume_sent_only: Option<bool>,
}

#[tauri::command]
pub fn job_query(param: JobQueryParam) -> CommandResult<Vec<JobDetail>> {
    let result = if let Some(ref name) = param.company_name {
        job_detail_dao::find_by_company(name)
    } else if param.replied_only == Some(true) {
        job_detail_dao::find_replied()
    } else if param.resume_sent_only == Some(true) {
        job_detail_dao::find_resume_sent()
    } else {
        job_detail_dao::list()
    };

    match result {
        Ok(list) => CommandResult::ok(list),
        Err(e) => CommandResult::err(e.to_string()),
    }
}

#[derive(Deserialize)]
struct LlmAnalysisOutput {
    fit_summary: String,
    match_score: u8,
    strengths: Vec<String>,
    risks: Vec<String>,
    skill_matrix: Vec<crate::dao::model::SkillEvidence>,
    likely_questions: Vec<crate::dao::model::InterviewQuestion>,
    questions_to_ask_interviewer: Vec<String>,
}

fn build_analysis_prompt(job: &JobDetail) -> String {
    // 抓下来的 JD 混着反爬注入的样式代码和噪声词，原样喂进去既占额度又干扰判断，
    // 统一走 job_description 这个出口洗一遍
    let detail = crate::job_description::clean_text(&job.detail, &job.platform);
    // 只填岗位骨架；resume_context / background_context / chat_context 这些
    // 业务变量留给后续的模板渲染，两层的替换时机不同不能混做
    crate::agent::prompts::compose(
        &crate::agent::prompts::with_shared(crate::agent::prompts::JOB_ANALYSIS),
        &[
            ("JOB_TITLE", &job.title),
            ("JOB_COMPANY", &job.company_name),
            ("JOB_SALARY", &job.salary),
            ("JOB_LOCATION", job.location.as_deref().unwrap_or("-")),
            ("JOB_DETAIL", &detail),
        ],
    )
}

fn format_chat_context(messages: &[ChatMessageRecord]) -> String {
    messages
        .iter()
        .map(|message| {
            let role = if message.received { "招聘方" } else { "我" };
            format!("{}({}): {}", message.from_name, role, message.text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tauri::command]
pub async fn job_analyze(
    app_handle: tauri::AppHandle,
    job_id: String,
) -> CommandResult<InterviewJobAnalysis> {
    let job = match job_detail_dao::get_by_id(&job_id) {
        Ok(Some(j)) => j,
        Ok(None) => return CommandResult::err(format!("岗位不存在: {}", job_id)),
        Err(e) => return CommandResult::err(e.to_string()),
    };

    let app_config = match config::load_app_config_inner(app_handle) {
        Ok(c) => c,
        Err(e) => return CommandResult::err(format!("加载配置失败: {}", e)),
    };

    match analyze_job(&job, &app_config).await {
        Ok(analysis) => CommandResult::ok(analysis),
        Err(error) => CommandResult::err(error.to_string()),
    }
}

#[derive(Debug, Default, Serialize)]
pub struct BatchAnalysisResult {
    pub analyzed: usize,
    pub skipped: usize,
    pub failed: usize,
    /// 「岗位名：原因」形式的失败摘要，界面直接展示
    pub failures: Vec<String>,
}

/// 批量分析选中的岗位。
///
/// 串行执行：分析是重调用，并发跑既容易触发服务端限流，也会和求职任务抢配额。
#[tauri::command]
pub async fn job_analyze_batch(
    app_handle: tauri::AppHandle,
    job_ids: Vec<String>,
    skip_analyzed: Option<bool>,
) -> CommandResult<BatchAnalysisResult> {
    let app_config = match config::load_app_config_inner(app_handle) {
        Ok(c) => c,
        Err(e) => return CommandResult::err(format!("加载配置失败: {}", e)),
    };
    if app_config.llm_chain().is_empty() {
        return CommandResult::err(if app_config.llm_configured() {
            "大模型已停用，请先启用模型服务".to_string()
        } else {
            "请先配置大模型服务".to_string()
        });
    }
    let skip_analyzed = skip_analyzed.unwrap_or(true);

    let mut result = BatchAnalysisResult::default();
    for job_id in job_ids {
        let job = match job_detail_dao::get_by_id(&job_id) {
            Ok(Some(job)) => job,
            Ok(None) => {
                result.failed += 1;
                result.failures.push(format!("{job_id}：岗位不存在"));
                continue;
            }
            Err(error) => {
                result.failed += 1;
                result.failures.push(format!("{job_id}：{error}"));
                continue;
            }
        };
        if skip_analyzed
            && matches!(analysis_dao::get_by_job_id(&job_id), Ok(Some(existing)) if existing.parse_error.is_none())
        {
            result.skipped += 1;
            continue;
        }
        match analyze_job(&job, &app_config).await {
            Ok(analysis) if analysis.parse_error.is_none() => {
                result.analyzed += 1;
                let _ = crate::logger::info(format!(
                    "已分析岗位「{}」，匹配度 {} 分",
                    job.title, analysis.match_score
                ));
            }
            Ok(_) => {
                result.failed += 1;
                result
                    .failures
                    .push(format!("{}：模型输出解析不完整", job.title));
            }
            Err(error) => {
                result.failed += 1;
                result.failures.push(format!("{}：{error}", job.title));
            }
        }
    }

    CommandResult::ok(result)
}

/// 跑一次岗位分析并落库。
///
/// 界面手动触发和 RPA 自动触发共用这条路径，区别只在于谁提供配置快照——
/// 自动触发时配置来自任务绑定的方案快照，不能再回头去读当前界面上的配置。
pub async fn analyze_job(
    job: &JobDetail,
    app_config: &config::AppRuntimeConfig,
) -> anyhow::Result<InterviewJobAnalysis> {
    let job_id = job.id.clone();
    let resume_context = app_config
        .resume_config
        .resume_content
        .clone()
        .unwrap_or_default();
    let background_context = app_config
        .replay_config
        .background_context
        .clone()
        .unwrap_or_default();
    let chat_messages = match chat_message_dao::find_by_job_id(&job_id) {
        Ok(mut messages) => {
            messages.sort_by_key(|message| message.time);
            messages
        }
        Err(e) => anyhow::bail!("加载沟通记录失败: {}", e),
    };
    let chat_context = format_chat_context(&chat_messages);

    let prompt_template = build_analysis_prompt(job);
    let params = serde_json::json!({
        "resume_context": resume_context,
        "background_context": background_context,
        "chat_context": chat_context,
    });
    // 与其他所有模型用途一样走统一的 Agent 循环：流式采集、重试降级、输出净化
    let task = crate::agent::tasks::TemplateTask::new("岗位面试分析", &prompt_template, params);
    let raw = match crate::agent::run(&task, app_config).await {
        Ok(outcome) => outcome.output,
        Err(e) => anyhow::bail!("生成分析失败: {}", e),
    };
    let analyzed_at = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let analysis = match serde_json::from_str::<LlmAnalysisOutput>(&raw) {
        Ok(output) => InterviewJobAnalysis {
            job_id: job_id.clone(),
            analyzed_at,
            fit_summary: output.fit_summary,
            match_score: output.match_score,
            strengths: output.strengths,
            risks: output.risks,
            skill_matrix: output.skill_matrix,
            likely_questions: output.likely_questions,
            questions_to_ask_interviewer: output.questions_to_ask_interviewer,
            search_summary: String::new(),
            search_sources: vec![],
            chat_context,
            raw_response: raw,
            parse_error: None,
        },
        Err(e) => InterviewJobAnalysis {
            job_id: job_id.clone(),
            analyzed_at,
            fit_summary: String::new(),
            match_score: 0,
            strengths: vec![],
            risks: vec![],
            skill_matrix: vec![],
            likely_questions: vec![],
            questions_to_ask_interviewer: vec![],
            search_summary: String::new(),
            search_sources: vec![],
            chat_context,
            raw_response: raw,
            parse_error: Some(e.to_string()),
        },
    };

    let save_result = match analysis_dao::get_by_job_id(&job_id) {
        Ok(Some(_)) => analysis_dao::update(&job_id, analysis.clone()).map(|_| ()),
        Ok(None) => analysis_dao::create(analysis.clone()),
        Err(e) => Err(e),
    };
    if let Err(e) = save_result {
        anyhow::bail!("保存分析结果失败: {}", e);
    }

    Ok(analysis)
}
