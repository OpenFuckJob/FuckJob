use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::{
    auto_analysis, browser,
    config::{AnalysisTrigger, AppRuntimeConfig, ReplayResourceType, ReplyResource},
    dao::{job_detail_dao, model::JobDetail},
    logger,
    rpa::{
        common::{upload_image_to_file_input, RpaJob},
        conversation::SendVerdict,
        greet::build_greet_resources,
        human_input,
        human_pace::GreetPacer,
        liepin::LIEPIN_SITE_URL,
        run_flow::is_job_task_stop_requested,
        schedule::{BudgetVerdict, RoundBudget},
    },
    utils::salary::decode_salary,
    verify,
};
use chrono::Local;
use rust_drission::{utils::sleep_random_ms, ChromiumPage, Page};
use serde::Deserialize;
use serde_json::Value;
use urlencoding::encode;

pub async fn position_say_hello(
    config: &AppRuntimeConfig,
    budget: RoundBudget,
) -> Result<(), anyhow::Error> {
    let config = config.clone();
    browser::with_browser(|connection| {
        Box::pin(async move {
            position_say_hello_on_page(connection, connection.tab(), &config, budget).await
        })
    })
    .await
}

/// Run the Liepin job-hunting flow on the task-owned main tab.
pub async fn position_say_hello_on_page(
    connection: &ChromiumPage,
    page: &Page,
    config: &AppRuntimeConfig,
    budget: RoundBudget,
) -> Result<(), anyhow::Error> {
    let config = config.clone();
    let round_started = Instant::now();
    // 页级 RoundStats 每页都会重置，而预算按整轮算，所以另攒两个整轮计数
    let mut round_greeted = 0u32;
    let mut consecutive_greet_failures = 0u32;
    let search_url = build_job_search_url(&config);
    let mut processed_job_ids: HashSet<String> = job_detail_dao::list()
        .unwrap_or_default()
        .into_iter()
        .map(|j| j.id)
        .collect();
    logger::info(format!(
        "猎聘本地已存储 {} 条岗位记录",
        processed_job_ids.len()
    ))?;

    let mut seen_job_ids: HashSet<String> = HashSet::new();
    // 休息节奏从用户设的单轮上限派生。拟人化关着时它是个空壳，
    // 停顿仍是改造前那段固定间隔
    let mut pacer = GreetPacer::new(&config.humanize_config, budget.max_greets);

    logger::info(format!("正在打开猎聘职位搜索页: {}", search_url))?;
    page.get(&search_url)?;
    sleep_random_ms(1200, 2000);
    apply_liepin_filters(page, &config)?;
    sleep_random_ms(1200, 1800);

    loop {
        if is_job_task_stop_requested() {
            logger::info("猎聘求职任务已结束")?;
            return Ok(());
        }
        if let Some(reason) = budget_stop_reason(
            &budget,
            round_greeted,
            round_started,
            consecutive_greet_failures,
        ) {
            logger::info(reason)?;
            return Ok(());
        }

        let jobs = collect_jobs(page)?;
        if jobs.is_empty() {
            logger::info("猎聘暂无可处理岗位")?;
            return Ok(());
        }

        logger::info(format!("猎聘当前加载到{}条岗位", jobs.len()))?;
        let mut stats = RoundStats::default();
        for job in jobs {
            if is_job_task_stop_requested() {
                logger::info("猎聘求职任务已结束")?;
                return Ok(());
            }
            if let Some(reason) = budget_stop_reason(
                &budget,
                round_greeted,
                round_started,
                consecutive_greet_failures,
            ) {
                logger::info(stats.summary())?;
                logger::info(reason)?;
                return Ok(());
            }

            stats.scanned += 1;
            let db_id = format!("liepin:{}", job.platform_job_id);
            if processed_job_ids.contains(&db_id)
                || processed_job_ids.contains(&job.platform_job_id)
                || seen_job_ids.contains(&job.platform_job_id)
            {
                // 逐条打会把日志刷满，这里只计数，本页结束时汇总
                stats.skipped_processed += 1;
                continue;
            }
            seen_job_ids.insert(job.platform_job_id.clone());

            let filter_decision = verify::filter_decision(&job, &config);
            if !filter_decision.matched {
                stats.skipped_rule += 1;
                continue;
            }
            // 「每个符合条件的岗位都投」是人做不到的事。放在语义复核之前，
            // 跳过的岗位不烧模型额度
            if pacer.should_skim() {
                stats.skipped_humanize += 1;
                logger::info(format!("拟人化跳过本岗位：{}", job.title))?;
                continue;
            }

            logger::info(format!(
                "猎聘处理岗位：{} - {}",
                job.title, job.company_name
            ))?;
            if config.job_filter_config.enable_semantic_filter {
                match crate::llm::evaluate_job_match(&config, &job).await {
                    Ok(decision) if decision.matched => logger::info(format!(
                        "猎聘 AI 岗位复核通过（{}分）：{}",
                        decision.score, decision.reason
                    ))?,
                    Ok(decision) => {
                        stats.skipped_ai += 1;
                        logger::info(format!(
                            "猎聘 AI 岗位复核未通过，跳过（{}分）：{}",
                            decision.score, decision.reason
                        ))?;
                        continue;
                    }
                    Err(error) => {
                        stats.skipped_ai += 1;
                        logger::warning(format!(
                            "猎聘 AI 岗位复核失败，为避免误投已跳过：{}",
                            error
                        ))?;
                        continue;
                    }
                }
            }

            // 「筛选通过即分析」不关心后面招呼发没发出去，所以在进入打招呼之前就登记
            auto_analysis::schedule(
                &build_job_detail(&job, &config, false),
                AnalysisTrigger::FilterPassed,
                &config,
            );
            let greeted = match greet_job(connection, page, job.clone(), config.clone()).await {
                Ok(false) => {
                    stats.skipped_hold += 1;
                    consecutive_greet_failures = 0;
                    false
                }
                Ok(true) => {
                    stats.greeted += 1;
                    round_greeted += 1;
                    consecutive_greet_failures = 0;
                    processed_job_ids.insert(format!("liepin:{}", job.platform_job_id));
                    processed_job_ids.insert(job.platform_job_id.clone());
                    true
                }
                Err(error) => {
                    stats.greet_failed += 1;
                    consecutive_greet_failures += 1;
                    logger::warning(greet_failure_message(&job.title, &job.company_name, &error))?;
                    continue;
                }
            };
            // 停顿、以及连投若干条之后的休息都在这里面。收到停止请求时立即收尾，
            // 不能让用户等完一段十几分钟的休息
            if !pacer.after_greet(greeted).await {
                logger::info(stats.summary())?;
                logger::info("猎聘求职任务已结束")?;
                return Ok(());
            }
        }

        logger::info(stats.summary())?;
        if let Some(summary) = pacer.summary() {
            logger::info(summary)?;
        }

        if !scroll_next(page)? {
            logger::info("猎聘岗位列表已触底")?;
            return Ok(());
        }
    }
}

/// 把预算判定翻译成本轮的结束语。预算还够时返回 None。
///
/// 提前结束不是故障，文案必须说清楚「下一轮还会继续」，否则用户会以为投递挂了
fn budget_stop_reason(
    budget: &RoundBudget,
    round_greeted: u32,
    round_started: Instant,
    consecutive_greet_failures: u32,
) -> Option<&'static str> {
    match budget.check(
        round_greeted,
        round_started.elapsed(),
        consecutive_greet_failures,
    ) {
        BudgetVerdict::Continue => None,
        BudgetVerdict::GreetLimit => {
            Some("猎聘本轮打招呼条数已达设定上限，提前结束本轮；周期投递会在下一轮继续")
        }
        BudgetVerdict::TimeLimit => {
            Some("猎聘本轮运行时长已达设定上限，提前结束本轮；周期投递会在下一轮继续")
        }
        BudgetVerdict::FailureLimit => {
            Some("猎聘连续多次打招呼失败，可能已达平台每日沟通上限或触发了安全验证，本轮提前结束")
        }
    }
}

/// 本页岗位处理统计。跳过类逐条打日志会把有效信息淹掉，改为汇总一条。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RoundStats {
    scanned: u32,
    /// 本地库已有记录，之前沟通过
    skipped_processed: u32,
    /// 确定性规则未通过
    skipped_rule: u32,
    /// AI 语义复核未通过或失败
    skipped_ai: u32,
    /// 被拟人化随机跳过（「只看不投」）
    skipped_humanize: u32,
    /// 内容通过了复核，但发送前被闸门整轮拦下（例如模型判断不该投）
    skipped_hold: u32,
    greeted: u32,
    greet_failed: u32,
}

impl RoundStats {
    fn summary(&self) -> String {
        format!(
            "猎聘本页 {} 条岗位：打招呼成功 {} 条，失败 {} 条；已沟通跳过 {} 条，规则过滤跳过 {} 条，AI 复核跳过 {} 条，拟人化跳过 {} 条，发送闸门拦下 {} 条",
            self.scanned,
            self.greeted,
            self.greet_failed,
            self.skipped_processed,
            self.skipped_rule,
            self.skipped_ai,
            self.skipped_humanize,
            self.skipped_hold
        )
    }
}

fn build_job_search_url(config: &AppRuntimeConfig) -> String {
    let query = config
        .job_filter_config
        .query
        .as_deref()
        .unwrap_or_default()
        .trim();

    let liepin_filter = resolve_liepin_filter(config);
    let mut params = vec!["inputFrom=".to_string()];

    if !query.is_empty() {
        params.push(format!("key={}", encode(query)));
    }
    push_optional_param(&mut params, "dq", liepin_filter.dq.as_deref());
    push_optional_param(
        &mut params,
        "salaryCode",
        liepin_filter.salary_code.as_deref(),
    );
    push_optional_param(&mut params, "pubTime", liepin_filter.pub_time.as_deref());
    push_optional_param(
        &mut params,
        "workYearCode",
        liepin_filter.work_year_code.as_deref(),
    );
    push_vec_param(&mut params, "compTag", &liepin_filter.comp_tag);

    if !params
        .iter()
        .any(|param| param.starts_with("workYearCode="))
    {
        params.push("workYearCode=0".to_string());
    }

    format!("{}/zhaopin/?{}", LIEPIN_SITE_URL, params.join("&"))
}

#[derive(Debug, Clone, Default)]
struct ResolvedLiepinFilter {
    dq: Option<String>,
    salary_code: Option<String>,
    pub_time: Option<String>,
    work_year_code: Option<String>,
    comp_tag: Vec<String>,
}

fn resolve_liepin_filter(config: &AppRuntimeConfig) -> ResolvedLiepinFilter {
    let common = &config.job_filter_config;
    let override_filter = &config.platform_filter_config.liepin;

    ResolvedLiepinFilter {
        dq: common
            .city
            .and_then(map_common_city_to_liepin_dq)
            .or_else(|| override_filter.dq.clone()),
        salary_code: override_filter
            .salary_code
            .clone()
            .or_else(|| map_common_salary_to_liepin_salary_code(common.salary)),
        pub_time: override_filter.pub_time.clone(),
        work_year_code: override_filter
            .work_year_code
            .clone()
            .or_else(|| map_common_experience_to_liepin_work_year_code(&common.experience)),
        comp_tag: override_filter.comp_tag.clone(),
    }
}

fn map_common_city_to_liepin_dq(city: i64) -> Option<String> {
    let code = match city {
        101010000 | 101010100 => "010",
        101020000 | 101020100 => "020",
        101030000 | 101030100 => "030",
        101040000 | 101040100 => "040",
        101280100 => "050020",
        101280600 => "050090",
        101190400 => "060080",
        101190100 => "060020",
        101210100 => "070020",
        101070200 => "210040",
        101270100 => "280020",
        101200100 => "170020",
        101110100 => "270020",
        _ => return None,
    };
    Some(code.to_string())
}

fn map_common_salary_to_liepin_salary_code(salary: i64) -> Option<String> {
    let code = match salary {
        402..=404 => "1",
        405 => "3",
        406 => "5",
        407 => "7",
        _ => return None,
    };
    Some(code.to_string())
}

fn map_common_experience_to_liepin_work_year_code(experience: &[i64]) -> Option<String> {
    let selected = experience
        .iter()
        .copied()
        .find(|code| *code != 0 && *code != 101)?;
    let code = match selected {
        102 => "1",
        108 => "2",
        103 => "0$1",
        104 => "1$3",
        105 => "3$5",
        106 => "5$10",
        107 => "10$999",
        _ => return None,
    };
    Some(code.to_string())
}

fn push_optional_param(params: &mut Vec<String>, key: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        params.push(format!("{}={}", key, encode(value)));
    }
}

fn push_vec_param(params: &mut Vec<String>, key: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    let value = values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(",");
    if !value.is_empty() {
        params.push(format!("{}={}", key, encode(&value)));
    }
}

fn apply_liepin_filters(page: &Page, config: &AppRuntimeConfig) -> Result<(), anyhow::Error> {
    let value = page.run_js_await(&build_apply_liepin_filter_script(config))?;
    let result = value.get("value").cloned().unwrap_or(value);

    // 顺利应用时不必汇报，只有页面上没找到对应筛选项才值得提醒
    let missing = result
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|item| item.get("status").and_then(|v| v.as_str()) == Some("missing"))
                .filter_map(|item| item.get("key").and_then(|v| v.as_str()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !missing.is_empty() {
        logger::warning(format!(
            "猎聘页面上未找到筛选项 {}，该条件改由搜索链接参数生效",
            missing.join("、")
        ))?;
    }

    Ok(())
}

fn build_apply_liepin_filter_script(config: &AppRuntimeConfig) -> String {
    let filter = resolve_liepin_filter(config);
    let mut items: Vec<(&str, &str)> = Vec::new();

    if let Some(value) = filter
        .dq
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        items.push(("dq", value.trim()));
    }
    if let Some(value) = filter
        .salary_code
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        items.push(("salaryCode", value.trim()));
    }
    if let Some(value) = filter
        .pub_time
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        items.push(("pubTime", value.trim()));
    }
    if let Some(value) = filter
        .work_year_code
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        items.push(("workYearCode", value.trim()));
    }
    for value in filter.comp_tag.iter().map(|value| value.trim()) {
        if !value.is_empty() {
            items.push(("compTag", value));
        }
    }

    let items_json = serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string());

    format!(
        r#"
        (async () => {{
            const filters = {items_json};
            const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
            const cssEscape = (value) => {{
                if (window.CSS && CSS.escape) return CSS.escape(value);
                return String(value).replace(/["\\]/g, "\\$&");
            }};
            const results = [];

            for (const [key, code] of filters) {{
                const selector = `[data-nick="search-jobs-filter-options-item"][data-key="${{cssEscape(key)}}"][data-code="${{cssEscape(code)}}"]`;
                const option = document.querySelector(selector);
                if (!option) {{
                    results.push({{ key, code, status: "missing" }});
                    continue;
                }}
                if (option.classList.contains("selected")) {{
                    results.push({{ key, code, status: "already_selected" }});
                    continue;
                }}
                option.click();
                results.push({{ key, code, status: "clicked", text: (option.innerText || "").trim() }});
                await sleep(700);
            }}

            return results;
        }})()
        "#
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LiepinJobCandidate {
    link_text: String,
    card_text: String,
    href: String,
}

fn job_card_selectors() -> &'static [&'static str] {
    &[
        "[data-tlg-elem-id='c_pc_search_job_listcard']",
        "div.job-card-pc-container",
        "div[class*='job-card-pc-container']",
    ]
}

fn collect_jobs(page: &Page) -> Result<Vec<RpaJob>, anyhow::Error> {
    let value = page.run_js_await(&build_collect_jobs_script())?;
    let raw = value.get("value").cloned().unwrap_or(value);
    let candidates = serde_json::from_value::<Vec<LiepinJobCandidate>>(raw)?;

    Ok(candidates
        .into_iter()
        .filter_map(candidate_to_rpa_job)
        .collect())
}

fn build_collect_jobs_script() -> String {
    let selectors = job_card_selectors()
        .iter()
        .map(|selector| serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".to_string()))
        .collect::<Vec<_>>()
        .join(",");

    format!(
        r#"
        (() => {{
            const selectors = [{selectors}];
            const seen = new Set();
            const text = (el) => (el?.innerText || el?.textContent || "").trim().replace(/\s+/g, " ");
            const cards = selectors.flatMap((selector) => Array.from(document.querySelectorAll(selector)));
            return cards.filter((card) => {{
                const key = card.getAttribute("data-tlg-scm")
                    || card.getAttribute("data-tlg-ext")
                    || text(card);
                if (!key || seen.has(key)) return false;
                seen.add(key);
                return true;
            }}).map((card) => {{
                const link = card.querySelector("a[href*='/job/'], a[href*='/a/'], a[href*='job']");
                return {{
                    linkText: text(link),
                    cardText: text(card),
                    href: link ? (link.href || link.getAttribute("href") || "") : ""
                }};
            }}).filter((item) => item.href && item.linkText);
        }})()
        "#
    )
}

fn candidate_to_rpa_job(candidate: LiepinJobCandidate) -> Option<RpaJob> {
    let detail_url = normalize_url(&candidate.href);
    let platform_job_id = extract_job_id(&detail_url)?;
    let title = parse_title_from_link_text(&candidate.link_text);
    if title.is_empty() {
        return None;
    }

    Some(RpaJob {
        platform: crate::rpa::run_flow::PlatformKind::Liepin,
        platform_job_id,
        title,
        company_name: parse_company_from_card_text(&candidate.card_text, &candidate.link_text)
            .unwrap_or_else(|| "未知公司".to_string()),
        detail: candidate.card_text.clone(),
        salary: decode_salary(&parse_salary_from_link_text(&candidate.link_text)),
        location: parse_location_from_link_text(&candidate.link_text),
        recruiter_active_time: parse_recruiter_active_time_from_card_text(&candidate.card_text),
        detail_url,
    })
}

fn parse_recruiter_active_time_from_card_text(card_text: &str) -> Option<String> {
    card_text
        .split_whitespace()
        .rev()
        .find(|value| value.contains("在线") || value.contains("活跃"))
        .map(str::to_string)
}

fn parse_title_from_link_text(link_text: &str) -> String {
    link_text
        .split('【')
        .next()
        .unwrap_or(link_text)
        .trim()
        .to_string()
}

fn parse_location_from_link_text(link_text: &str) -> Option<String> {
    let start = link_text.find('【')? + '【'.len_utf8();
    let end = link_text[start..].find('】')? + start;
    non_empty(link_text[start..end].trim().to_string())
}

fn parse_salary_from_link_text(link_text: &str) -> String {
    link_text
        .split_whitespace()
        .find(|token| {
            let lower = token.to_ascii_lowercase();
            lower.contains('k') || token.contains('万')
        })
        .unwrap_or_default()
        .to_string()
}

fn parse_company_from_card_text(card_text: &str, link_text: &str) -> Option<String> {
    let remaining = card_text
        .strip_prefix(link_text)
        .unwrap_or(card_text)
        .trim();
    remaining
        .split_whitespace()
        .find(|value| {
            !value.contains('·')
                && !value.ends_with("在线")
                && !value.ends_with("广告")
                && !value.chars().all(|c| c.is_ascii_digit())
        })
        .map(str::to_string)
}

/// 返回 false 表示这一轮被闸门拦下、什么都没发出去。
/// 区分出来是为了不把「拦下」计成「打招呼成功」，那会让统计骗人
async fn greet_job(
    browser_page: &ChromiumPage,
    main_page: &Page,
    mut job: RpaJob,
    config: AppRuntimeConfig,
) -> Result<bool, anyhow::Error> {
    if job.detail_url.is_empty() {
        logger::warning("猎聘岗位缺少详情链接，跳过")?;
        return Ok(false);
    }

    let page = browser::new_stealth_tab(browser_page)?;
    let result = async {
        page.get(&job.detail_url)?;
        sleep_random_ms(1200, 2000);

        if job.detail.trim().is_empty() {
            job.detail = text_from_first(
                &page,
                &[
                    ".job-intro-container",
                    ".job-detail-box",
                    ".job-description",
                    "[class*='job-intro']",
                    "[class*='description']",
                ],
            )?;
        }

        if is_external_apply_only(&page)? {
            logger::info(format!(
                "猎聘跳过 {}，该岗位仅支持外部网申，未生成或发送站内消息",
                job.title
            ))?;
            return Ok(false);
        }

        let resume_sent = match build_greet_resources(&config, &job).await? {
            // 整轮取消：既不发文本也不发图片，也不记为已沟通
            SendVerdict::Hold(reason) => {
                logger::info(format!("猎聘跳过 {}，未发送任何内容：{reason}", job.title))?;
                return Ok(false);
            }
            SendVerdict::Send(resources) => {
                let entry = click_first(
                    &page,
                    LIEPIN_CONTACT_ENTRY_SELECTORS,
                )?;
                if entry_sends_resume(entry) {
                    logger::info("猎聘已点击投简历入口，继续发送招呼消息")?;
                }
                sleep_random_ms(800, 1200);
                send_resources(&page, resources)?;
                let resume_sent = confirm_resume_delivery(&page)?;
                if !resume_sent {
                    logger::warning("猎聘消息已发送，但尚未确认简历投递，保留简历未投递状态")?;
                }
                resume_sent
            }
        };
        let saved = save_job_detail(&job, &config, resume_sent);
        auto_analysis::schedule(&saved, AnalysisTrigger::GreetSent, &config);
        logger::info(format!("猎聘 {} 初次沟通成功", job.title))?;
        Ok(true)
    }
    .await;
    if let Err(error) = main_page.run_cdp("Page.bringToFront", None) {
        let _ = logger::warning(format!("猎聘恢复主搜索页失败，继续清理详情页：{error}"));
    }
    let close_result = page.close();

    match (result, close_result) {
        (Ok(sent), Ok(())) => Ok(sent),
        (Err(err), _) => Err(err),
        (Ok(_), Err(err)) => Err(err.into()),
    }
}

fn greet_failure_message(title: &str, company_name: &str, error: &anyhow::Error) -> String {
    format!(
        "猎聘岗位打招呼失败：{} - {}，错误：{}。跳过该岗位，继续处理下一个",
        title, company_name, error
    )
}

pub(crate) fn send_resources(
    page: &Page,
    resources: Vec<ReplyResource>,
) -> Result<(), anyhow::Error> {
    for resource in resources {
        if resource.content.trim().is_empty() {
            continue;
        }

        match resource.resource_type {
            ReplayResourceType::Text | ReplayResourceType::LLM => {
                send_text_resource(page, &resource.content)?;
            }
            // 图片属于附加内容，失败只告警不中断，避免因一张图让整个岗位打招呼判失败
            ReplayResourceType::Image => {
                if let Err(error) = send_image_resource(page, &resource.content) {
                    logger::warning(format!("猎聘图片发送失败，已跳过该条：{}", error))?;
                }
            }
        }
    }

    Ok(())
}

/// 猎聘聊天窗的上传控件是 rc-upload（`ant-im-upload`），
/// input 写的是 `accept="jpg, jpeg, png, bmp"`，不含 image 字样，
/// 因此不能沿用 Boss 的 `accept*="image"` 选择器。
const LIEPIN_IMAGE_INPUT_SELECTORS: &[&str] = &[
    ".im-ui-upload-container input[type='file']",
    ".ant-im-upload input[type='file']",
    "input[type='file'][accept*='jpg']",
    "input[type='file'][accept*='png']",
    "input[type='file'][accept*='image']",
];

/// 与猎聘上传控件 accept 保持一致，其余格式它不接收
const LIEPIN_SUPPORTED_IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "bmp"];

fn ensure_supported_image(image_path: &Path) -> Result<(), anyhow::Error> {
    let extension = image_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !LIEPIN_SUPPORTED_IMAGE_EXTENSIONS.contains(&extension.as_str()) {
        return Err(anyhow::anyhow!(
            "猎聘仅支持 jpg/jpeg/png/bmp 图片，当前文件格式为 {}",
            if extension.is_empty() {
                "(无扩展名)"
            } else {
                extension.as_str()
            }
        ));
    }

    if !image_path.is_file() {
        return Err(anyhow::anyhow!("图片文件不存在: {}", image_path.display()));
    }

    Ok(())
}

fn send_image_resource(page: &Page, image_path: &str) -> Result<(), anyhow::Error> {
    let path = Path::new(image_path.trim());
    ensure_supported_image(path)?;

    // 上传成不成功由真实的网络响应说了算。先在页面里挂上请求记录器，
    // 再触发上传，最后读记录——不再靠“等几秒看看 DOM 变没变”这种猜测。
    let since = install_request_recorder(page)?;
    let outcome = upload_image_to_file_input(page, path, LIEPIN_IMAGE_INPUT_SELECTORS)?;
    if !outcome.success {
        return Err(anyhow::anyhow!("找不到图片上传入口（{}）", outcome.message));
    }

    // 成功路径只说结果；诊断细节留给失败分支，避免刷屏
    let delivery = wait_for_image_delivery(page, since)?;
    if !delivery.uploaded {
        return Err(anyhow::anyhow!(
            "{} 秒内没有观察到图片上传请求{}",
            delivery.waited_ms / 1000,
            format_seen_requests(&delivery.seen)
        ));
    }
    if !delivery.sent {
        // 文件传上去了但没发成消息，这是实测出现过的情况，必须报出来
        return Err(anyhow::anyhow!(
            "图片已上传但未发出消息{}{}",
            if delivery.clicked {
                "（已补点发送按钮仍未发出）"
            } else {
                ""
            },
            format_seen_requests(&delivery.seen)
        ));
    }

    logger::info("猎聘图片已发送")?;

    Ok(())
}

fn format_seen_requests(seen: &[String]) -> String {
    if seen.is_empty() {
        return "，期间页面没有发出任何 POST 请求".to_string();
    }
    format!("，期间的 POST 请求：{}", seen.join("；"))
}

/// 在页面里挂 fetch / XMLHttpRequest 记录器，返回本次的时间基准。
///
/// 重复注入是安全的（第二次直接复用），标签页关闭后自然消失，
/// 不像 CDP 监听那样会留下后台线程和连接。
pub(crate) fn install_request_recorder(page: &Page) -> Result<f64, anyhow::Error> {
    let value = page.run_js_await(INSTALL_REQUEST_RECORDER_SCRIPT)?;
    let result = value.get("value").cloned().unwrap_or(value);
    Ok(result
        .get("now")
        .and_then(|value| value.as_f64())
        .unwrap_or_default())
}

const INSTALL_REQUEST_RECORDER_SCRIPT: &str = r#"
    (() => {
        if (!window.__fjRequestRecords) {
            window.__fjRequestRecords = [];
        }
        const push = (url, status, body) => {
            const address = String(url || "");
            // 判断发送成败只要个开头就够；但 IM 的会话列表和聊天记录要整段留下来给解析用，
            // 截断会直接把 JSON 弄坏
            const limit = /com\.liepin\.im\./.test(address) ? 400000 : 4000;
            window.__fjRequestRecords.push({
                url: address,
                status: Number(status) || 0,
                body: String(body || "").slice(0, limit),
                at: performance.now()
            });
            // 只留最近的记录，避免长会话里无限增长
            if (window.__fjRequestRecords.length > 60) {
                window.__fjRequestRecords.splice(0, window.__fjRequestRecords.length - 60);
            }
        };

        if (!window.__fjRecorderInstalled) {
            const originalFetch = window.fetch;
            if (typeof originalFetch === "function") {
                window.fetch = function (...args) {
                    const request = args[0];
                    const url = typeof request === "string" ? request : (request && request.url) || "";
                    const method = String(
                        (args[1] && args[1].method) || (request && request.method) || "GET"
                    ).toUpperCase();
                    return originalFetch.apply(this, args).then((response) => {
                        if (method === "POST") {
                            response.clone().text()
                                .then((text) => push(url, response.status, text))
                                .catch(() => push(url, response.status, ""));
                        }
                        return response;
                    });
                };
            }

            const originalOpen = XMLHttpRequest.prototype.open;
            const originalSend = XMLHttpRequest.prototype.send;
            XMLHttpRequest.prototype.open = function (method, url, ...rest) {
                this.__fjMethod = String(method || "").toUpperCase();
                this.__fjUrl = url;
                return originalOpen.call(this, method, url, ...rest);
            };
            XMLHttpRequest.prototype.send = function (...args) {
                this.addEventListener("loadend", () => {
                    if (this.__fjMethod === "POST") {
                        let text = "";
                        try {
                            text = this.responseType === "" || this.responseType === "text"
                                ? this.responseText
                                : JSON.stringify(this.response);
                        } catch (error) {
                            text = "";
                        }
                        push(this.__fjUrl, this.status, text);
                    }
                });
                return originalSend.apply(this, args);
            };

            window.__fjRecorderInstalled = true;
        }

        return { now: performance.now() };
    })()
"#;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ImageDelivery {
    /// 文件已传到 file.liepin.com
    uploaded: bool,
    /// 已经通过 IM 接口发成消息。只上传不发送是实测出现过的失败形态
    sent: bool,
    /// 是否补点过发送按钮
    clicked: bool,
    waited_ms: i64,
    /// 判定失败时把期间的 POST 请求带回来，接口改版时能直接看出新地址
    seen: Vec<String>,
}

/// 轮询请求记录，直到图片既传上去、又发成了消息。
///
/// 实测猎聘分两步：`file.liepin.com/upload/public-file.json` 传文件，
/// `api-c.liepin.com/api/com.liepin.im.c.chat.send-push` 发消息。
/// 只有上传成功而消息没发出的情况真实出现过，所以两步都要确认。
fn wait_for_image_delivery(page: &Page, since: f64) -> Result<ImageDelivery, anyhow::Error> {
    let value = page.run_js_await(&build_wait_image_delivery_script(since))?;
    let result = value.get("value").cloned().unwrap_or(value);
    let flag = |key: &str| {
        result
            .get(key)
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    };

    Ok(ImageDelivery {
        uploaded: flag("uploaded"),
        sent: flag("sent"),
        clicked: flag("clicked"),
        waited_ms: result
            .get("waitedMs")
            .and_then(|value| value.as_i64())
            .unwrap_or_default(),
        seen: result
            .get("seen")
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

fn build_wait_image_delivery_script(since: f64) -> String {
    format!(
        r#"
        (async () => {{
            const since = {since};
            const stepMs = 200;
            // 仅作异常上限：两步都确认到就立刻返回，不会等满
            const maxAttempts = 90;
            // 上传完成后消息迟迟没发出，就补点一次发送按钮
            const clickFallbackAfterMs = 2500;
            const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
            const recent = () => (window.__fjRequestRecords || []).filter((item) => item.at > since);
            const ok = (item) => item.status >= 200 && item.status < 300;
            // 按接口地址判定，而不是猜返回体结构
            const isUpload = (item) => ok(item) && /\/upload\/|file\.liepin\.com/i.test(item.url);
            const isSend = (item) => ok(item) && /chat\.send-push|chat\.send|send-push/i.test(item.url);

            const visible = (el) => {{
                if (!el) return false;
                const rect = el.getBoundingClientRect();
                const style = window.getComputedStyle(el);
                return rect.width > 0 && rect.height > 0
                    && style.visibility !== "hidden"
                    && style.display !== "none";
            }};
            const isDisabled = (el) => {{
                const className = el.getAttribute("class") || "";
                return Boolean(el.disabled)
                    || el.getAttribute("aria-disabled") === "true"
                    || className.includes("disabled")
                    || className.includes("ant-im-btn-disabled");
            }};
            const clickSendIfEnabled = () => {{
                const antImButtons = Array.from(document.querySelectorAll(".ant-im-btn")).filter(visible);
                const button = antImButtons[1] || Array
                    .from(document.querySelectorAll("button.im-ui-basic-send-btn, button.ant-im-btn-primary, .btn-send, .send-btn"))
                    .filter(visible)
                    .find((el) => {{
                        const text = (el.innerText || el.textContent || "").trim();
                        const className = el.getAttribute("class") || "";
                        return text.includes("发送") || /send/i.test(className);
                    }});
                if (!button || isDisabled(button)) return false;
                button.click();
                return true;
            }};

            let uploaded = false;
            let uploadedAtMs = -1;
            let clicked = false;

            for (let attempt = 0; attempt < maxAttempts; attempt += 1) {{
                const waitedMs = attempt * stepMs;
                const records = recent();

                if (!uploaded && records.some(isUpload)) {{
                    uploaded = true;
                    uploadedAtMs = waitedMs;
                }}
                if (uploaded && records.some(isSend)) {{
                    return {{ uploaded: true, sent: true, clicked, waitedMs }};
                }}
                // 传完了却没发出去：按钮一旦可用就补点一次救回来。
                // 不可用只说明此刻还不能点，下一轮继续看，点成功一次即止。
                if (uploaded && !clicked && waitedMs - uploadedAtMs >= clickFallbackAfterMs) {{
                    clicked = clickSendIfEnabled();
                }}

                await sleep(stepMs);
            }}

            return {{
                uploaded,
                sent: false,
                clicked,
                waitedMs: maxAttempts * stepMs,
                seen: recent().map((item) => item.url + " -> " + item.status)
            }};
        }})()
        "#
    )
}

/// 聊天窗可能还在加载，这些超时只是异常上限，元素一就绪就立刻继续
const INPUT_READY_TIMEOUT_MS: u32 = 15000;
const SEND_BUTTON_READY_TIMEOUT_MS: u32 = 15000;
const INPUT_CLEARED_TIMEOUT_MS: u32 = 8000;
const CHAT_INPUT_MARKER_SELECTOR: &str = "[data-fj-liepin-chat-input='1']";
const LIEPIN_CONTACT_ENTRY_SELECTORS: &[&str] = &[
    "a[data-selector='apply-job']",
    "a[data-selector='chat-chat']",
    "a.btn-chat",
    ".btn-apply",
    ".apply-btn",
    "button[class*='apply']",
    "a[class*='apply']",
    "button[class*='chat']",
    "a[class*='chat']",
];
const LIEPIN_EXTERNAL_APPLY_SELECTOR: &str = "a[data-selector='apply-ats']";

fn is_external_apply_only(page: &Page) -> Result<bool, anyhow::Error> {
    for selector in LIEPIN_CONTACT_ENTRY_SELECTORS {
        if page.ele(selector)?.is_some() {
            return Ok(false);
        }
    }
    Ok(page.ele(LIEPIN_EXTERNAL_APPLY_SELECTOR)?.is_some())
}

fn send_text_resource(page: &Page, text: &str) -> Result<(), anyhow::Error> {
    install_request_recorder(page)?;
    unwrap_runtime_value(page.run_js_await(&build_mark_chat_input_script())?)?;
    if let Some(input) = page.ele(CHAT_INPUT_MARKER_SELECTOR)? {
        let _ = human_input::type_text(page, &input, text);
    }
    if is_job_task_stop_requested() {
        return Err(anyhow::anyhow!("任务已停止，本条猎聘消息未发送"));
    }
    let result = unwrap_runtime_value(page.run_js_await(&build_send_text_script(text))?)?;
    let success = result
        .get("success")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let reason = result
        .get("reason")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");

    if !success {
        return Err(anyhow::anyhow!("消息发送失败：{}", reason));
    }

    logger::info(format!(
        "猎聘文本发送确认：尝试 {} 次，弹窗关闭 {}，请求确认 {}，气泡确认 {}",
        result
            .get("attempts")
            .and_then(|value| value.as_u64())
            .unwrap_or_default(),
        result
            .get("popupClosed")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        result
            .get("requestSucceeded")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        result
            .get("bubbleSeen")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
    ))?;
    Ok(())
}

fn unwrap_runtime_value(value: Value) -> Result<Value, anyhow::Error> {
    if value.get("subtype").and_then(Value::as_str) == Some("error") {
        let class_name = value
            .get("className")
            .and_then(Value::as_str)
            .unwrap_or("JavaScriptError");
        let description = value
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("页面脚本未返回详情");
        return Err(anyhow::anyhow!("猎聘页面脚本异常：{class_name}: {description}"));
    }
    Ok(value.get("value").cloned().unwrap_or(value))
}

fn liepin_send_script_source() -> String {
    include_str!("../send_text.js")
        .replace("export { markLiepinChatInput, runLiepinSend };", "")
}

fn build_mark_chat_input_script() -> String {
    let mut script = String::from("(() => {\n");
    script.push_str(&liepin_send_script_source());
    script.push_str("\nreturn markLiepinChatInput(document);\n})()");
    script
}

fn build_send_text_script(text: &str) -> String {
    let text_json = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string());
    let mut script = String::from("(async () => {\n");
    script.push_str(&liepin_send_script_source());
    script.push_str(&format!(
        r#"
        return await runLiepinSend(document, {{
            message: {text_json},
            inputTimeoutMs: {INPUT_READY_TIMEOUT_MS},
            buttonTimeoutMs: {SEND_BUTTON_READY_TIMEOUT_MS},
            proofTimeoutMs: {INPUT_CLEARED_TIMEOUT_MS},
            requestSince: performance.now()
        }});
        }})()
        "#
    ));
    script
}

/// 按当前任务上下文拼出岗位记录。
/// 打招呼后落库和「筛选通过即分析」共用它——后者触发时岗位还没入库，只能拿这份内存数据去分析。
fn build_job_detail(job: &RpaJob, config: &AppRuntimeConfig, resume_sent: bool) -> JobDetail {
    let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let active = config.active_job_profile.as_ref();
    JobDetail {
        id: format!("liepin:{}", job.platform_job_id),
        platform: "liepin".to_string(),
        source_task_id: crate::rpa::run_flow::current_job_task_id(),
        profile_id: active.map(|profile| profile.id.clone()),
        profile_name: active.map(|profile| profile.name.clone()),
        profile_snapshot_id: active.map(|profile| profile.snapshot_id.clone()),
        title: job.title.clone(),
        company_name: job.company_name.clone(),
        detail: job.detail.clone(),
        salary: job.salary.clone(),
        location: job.location.clone(),
        is_reply: false,
        is_send_resume: resume_sent,
        created_at: now.clone(),
        resume_sent_at: resume_sent.then(|| now.clone()),
        updated_at: now,
    }
}

fn save_job_detail(job: &RpaJob, config: &AppRuntimeConfig, resume_sent: bool) -> JobDetail {
    let job_detail = build_job_detail(job, config, resume_sent);

    if let Err(e) = job_detail_dao::create(job_detail.clone()) {
        let _ = logger::warning(format!("保存猎聘岗位数据失败: {}", e));
    }
    job_detail
}

fn scroll_next(page: &Page) -> Result<bool, anyhow::Error> {
    let before =
        page.run_js_await("document.documentElement.scrollTop || document.body.scrollTop")?;
    page.run_js_await(
        r#"
        (() => {
            const html = document.documentElement;
            const body = document.body;
            const scrollContainer = html.scrollHeight > html.clientHeight ? html : body;
            scrollContainer.scrollTop += Math.max(window.innerHeight, 600);
            return scrollContainer.scrollTop;
        })()
        "#,
    )?;
    std::thread::sleep(Duration::from_millis(800));
    let after =
        page.run_js_await("document.documentElement.scrollTop || document.body.scrollTop")?;
    Ok(before != after)
}

pub(crate) fn text_from_first(page: &Page, selectors: &[&str]) -> Result<String, anyhow::Error> {
    for selector in selectors {
        if let Some(ele) = page.ele(selector)? {
            return Ok(ele.text_content()?);
        }
    }
    Ok(String::new())
}

/// 可点击元素的等待上限。慢网络下聊天入口渲染得晚，
/// 扫一遍就判死会让整个岗位白跑（含已花掉的 AI 复核）。
const CLICKABLE_READY_TIMEOUT: Duration = Duration::from_secs(15);

/// 等待任一选择器出现并点击：命中立刻返回，没出现就继续等到上限。
fn entry_sends_resume(selector: &str) -> bool {
    selector.contains("apply") && !selector.contains("apply-ats")
}

fn resume_delivery_label_confirmed(label: &str) -> bool {
    matches!(label.trim(), "已投递" | "已投递简历" | "已申请")
}

fn confirm_resume_delivery(page: &Page) -> Result<bool, anyhow::Error> {
    // Clicking the application entry can merely open a dialog. Only the
    // job's own application status is evidence of delivery.
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(entry) = page.ele("a[data-selector='apply-job']")? {
            if resume_delivery_label_confirmed(&entry.text_content()?) {
                return Ok(true);
            }
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

pub(crate) fn click_first<'a>(
    page: &Page,
    selectors: &'a [&'a str],
) -> Result<&'a str, anyhow::Error> {
    let deadline = Instant::now() + CLICKABLE_READY_TIMEOUT;
    loop {
        for selector in selectors {
            // 页面正在导航时 ele 会短暂报错，这属于“还没就绪”而不是失败
            if let Ok(Some(ele)) = page.ele(selector) {
                ele.click()?;
                return Ok(selector);
            }
        }

        if Instant::now() >= deadline {
            return Err(anyhow::anyhow!(
                "{} 秒内未出现可点击元素: {}",
                CLICKABLE_READY_TIMEOUT.as_secs(),
                selectors.join(", ")
            ));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn normalize_url(href: &str) -> String {
    if href.starts_with("http") {
        href.to_string()
    } else if href.starts_with("//") {
        format!("https:{}", href)
    } else if href.starts_with('/') {
        format!("{}{}", LIEPIN_SITE_URL, href)
    } else {
        format!("{}/{}", LIEPIN_SITE_URL, href)
    }
}

fn extract_job_id(url: &str) -> Option<String> {
    url.split(['/', '?', '&'])
        .find(|part| part.chars().any(|c| c.is_ascii_digit()) && part.len() >= 6)
        .map(str::to_string)
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_app_config;

    #[test]
    fn build_job_search_url_uses_liepin_search_results_page() {
        let mut config = default_app_config();
        config.job_filter_config.query = Some("java".to_string());

        let url = build_job_search_url(&config);

        assert!(url.starts_with("https://www.liepin.com/zhaopin/?"));
        assert!(url.contains("key=java"));
        assert!(url.contains("workYearCode=0"));
        assert!(!url.contains("/zhaogongzuo/"));
    }

    #[test]
    fn build_job_search_url_includes_liepin_platform_filters() {
        let mut config = default_app_config();
        config.job_filter_config.query = Some("大模型应用".to_string());
        config.platform_filter_config.liepin.dq = Some("020".to_string());
        config.platform_filter_config.liepin.salary_code = Some("4".to_string());
        config.platform_filter_config.liepin.pub_time = Some("7".to_string());
        config.platform_filter_config.liepin.work_year_code = Some("3$5".to_string());
        config.platform_filter_config.liepin.comp_tag =
            vec!["qua_0001".to_string(), "qua_0008".to_string()];

        let url = build_job_search_url(&config);

        assert!(url.contains("key=%E5%A4%A7%E6%A8%A1%E5%9E%8B%E5%BA%94%E7%94%A8"));
        assert!(url.contains("dq=020"));
        assert!(url.contains("salaryCode=4"));
        assert!(url.contains("pubTime=7"));
        assert!(url.contains("workYearCode=3%245"));
        assert!(url.contains("compTag=qua_0001%2Cqua_0008"));
    }

    #[test]
    fn build_job_search_url_maps_common_filter_to_liepin_params() {
        let mut config = default_app_config();
        config.job_filter_config.query = Some("大模型应用".to_string());
        config.job_filter_config.city = Some(101020100);
        config.job_filter_config.salary = 406;
        config.job_filter_config.experience = vec![105];

        let url = build_job_search_url(&config);

        assert!(url.contains("key=%E5%A4%A7%E6%A8%A1%E5%9E%8B%E5%BA%94%E7%94%A8"));
        assert!(url.contains("dq=020"));
        assert!(url.contains("salaryCode=5"));
        assert!(url.contains("workYearCode=3%245"));
    }

    #[test]
    fn common_city_overrides_hidden_liepin_dq_when_present() {
        let mut config = default_app_config();
        config.job_filter_config.city = Some(101200100);
        config.job_filter_config.salary = 406;
        config.job_filter_config.experience = vec![105];
        config.platform_filter_config.liepin.dq = Some("020".to_string());
        config.platform_filter_config.liepin.salary_code = Some("4".to_string());
        config.platform_filter_config.liepin.work_year_code = Some("1$3".to_string());

        let url = build_job_search_url(&config);

        assert!(url.contains("dq=170020"));
        assert!(url.contains("salaryCode=4"));
        assert!(url.contains("workYearCode=1%243"));
        assert!(!url.contains("dq=020"));
        assert!(!url.contains("salaryCode=5"));
        assert!(!url.contains("workYearCode=3%245"));
    }

    #[test]
    fn apply_liepin_filter_script_clicks_options_by_data_key_and_code() {
        let mut config = default_app_config();
        config.platform_filter_config.liepin.dq = Some("020".to_string());
        config.platform_filter_config.liepin.salary_code = Some("4".to_string());
        config.platform_filter_config.liepin.pub_time = Some("7".to_string());
        config.platform_filter_config.liepin.work_year_code = Some("3$5".to_string());
        config.platform_filter_config.liepin.comp_tag = vec!["qua_0001".to_string()];

        let script = build_apply_liepin_filter_script(&config);

        assert!(script.contains("data-key"));
        assert!(script.contains("data-code"));
        assert!(script.contains("\"dq\""));
        assert!(script.contains("\"020\""));
        assert!(script.contains("\"salaryCode\""));
        assert!(script.contains("\"4\""));
        assert!(script.contains("\"pubTime\""));
        assert!(script.contains("\"7\""));
        assert!(script.contains("\"workYearCode\""));
        assert!(script.contains("\"3$5\""));
        assert!(script.contains("\"compTag\""));
        assert!(script.contains("\"qua_0001\""));
    }

    #[test]
    fn apply_liepin_filter_script_uses_common_filter_mapping() {
        let mut config = default_app_config();
        config.job_filter_config.city = Some(101020100);
        config.job_filter_config.salary = 405;
        config.job_filter_config.experience = vec![104];

        let script = build_apply_liepin_filter_script(&config);

        assert!(script.contains("\"dq\""));
        assert!(script.contains("\"020\""));
        assert!(script.contains("\"salaryCode\""));
        assert!(script.contains("\"3\""));
        assert!(script.contains("\"workYearCode\""));
        assert!(script.contains("\"1$3\""));
    }

    #[test]
    fn send_text_script_dispatches_input_and_clicks_send_button() {
        let script = build_send_text_script("你好，想进一步沟通");

        assert!(script.contains("InputEvent(\"input\""));
        assert!(script.contains("textOf(node).includes(\"发送\")"));
        assert!(script.contains("button.im-ui-basic-send-btn"));
        assert!(script.contains("button.ant-im-btn-primary"));
        assert!(script.contains("im-ui-msg-list-content"));
        assert!(!script.contains("antImButtons[1]"));
        assert!(!script.contains("[class*='send']"));
        assert!(script.contains("activeButton.click()"));
        assert!(script.contains("send-push"));
    }

    #[test]
    fn send_text_script_waits_for_elements_instead_of_sleeping_a_fixed_time() {
        // “等 500ms 再查一次，查不到就判失败”会在页面慢时误报“未找到可用发送按钮”
        let script = build_send_text_script("你好");

        assert!(!script.contains("await sleep(500)"));
        assert!(script.contains("inputTimeoutMs: 15000"));
        assert!(script.contains("buttonTimeoutMs: 15000"));
        assert!(script.contains("proofTimeoutMs: 8000"));
        assert!(script.contains("observeSendState"));
    }

    #[test]
    fn send_text_script_confirms_delivery_by_waiting_for_the_input_to_clear() {
        let script = build_send_text_script("你好");

        assert!(script.contains("inputCleared"));
        assert!(script.contains("requestSucceeded"));
        assert!(script.contains("input-cleared-without-proof"));
    }

    #[test]
    fn send_text_script_does_not_choose_buttons_by_page_order() {
        let script = build_send_text_script("你好");

        assert!(!script.contains("antImButtons[1]"));
        assert!(!script.contains("[class*='send']"));
        assert!(script.contains("im-ui-msg-list-content"));
    }

    #[test]
    fn greeting_is_generated_before_opening_the_chat_window() {
        let source = include_str!("position_say_hello.rs");
        let decision = source.find("match build_greet_resources").unwrap();
        let click = source[decision..].find("click_first").unwrap() + decision;

        assert!(decision < click);
    }

    #[test]
    fn send_text_resource_checks_stop_after_human_input() {
        let source = include_str!("position_say_hello.rs");
        let start = source.find("fn send_text_resource").unwrap();
        let end = source[start..].find("fn build_send_text_script").unwrap() + start;
        let body = &source[start..end];
        let typed = body.find("human_input::type_text").unwrap();
        let stopped = body[typed..]
            .find("is_job_task_stop_requested")
            .unwrap()
            + typed;
        let send = body[stopped..].find("build_send_text_script").unwrap() + stopped;

        assert!(typed < stopped && stopped < send);
    }

    #[test]
    fn main_search_page_is_restored_before_detail_tab_cleanup() {
        let source = include_str!("position_say_hello.rs");
        let start = source.find("async fn greet_job").unwrap();
        let end = source[start..].find("fn greet_failure_message").unwrap() + start;
        let body = &source[start..end];
        let restore = body.find("Page.bringToFront").unwrap();
        let close = body.find("let close_result = page.close()").unwrap();

        assert!(restore < close);
    }

    #[test]
    fn generated_send_scripts_are_scoped_independently() {
        let mark = build_mark_chat_input_script();
        let send = build_send_text_script("你好");

        assert!(mark.trim_start().starts_with("(() => {"));
        assert!(mark.trim_end().ends_with("})()"));
        assert!(send.trim_start().starts_with("(async () => {"));
        assert!(send.trim_end().ends_with("})()"));
    }

    #[test]
    fn runtime_errors_keep_the_cdp_description() {
        let error = unwrap_runtime_value(serde_json::json!({
            "type": "object",
            "subtype": "error",
            "className": "SyntaxError",
            "description": "SyntaxError: Identifier already declared"
        }))
        .unwrap_err();

        assert!(error.to_string().contains("SyntaxError"));
        assert!(error.to_string().contains("already declared"));
    }

    #[test]
    fn contact_entry_selectors_prefer_chat_and_exclude_external_apply() {
        assert_eq!(LIEPIN_CONTACT_ENTRY_SELECTORS[0], "a[data-selector='apply-job']");
        assert_eq!(LIEPIN_CONTACT_ENTRY_SELECTORS[1], "a[data-selector='chat-chat']");
        assert_eq!(LIEPIN_EXTERNAL_APPLY_SELECTOR, "a[data-selector='apply-ats']");
    }

    #[test]
    fn only_apply_entries_are_resume_attempts() {
        assert!(entry_sends_resume("a[data-selector='apply-job']"));
        assert!(entry_sends_resume(".btn-apply"));
        assert!(!entry_sends_resume("a[data-selector='chat-chat']"));
        assert!(!entry_sends_resume("a.btn-chat"));
    }

    #[test]
    fn resume_delivery_requires_an_explicit_completed_status() {
        for label in ["投简历", "继续聊", "投递中", "投递失败", "", "未投递"] {
            assert!(!resume_delivery_label_confirmed(label), "{label}");
        }
        for label in ["已投递", "已投递简历", " 已申请 "] {
            assert!(resume_delivery_label_confirmed(label), "{label}");
        }
    }

    #[test]
    fn applied_job_records_resume_delivery_time() {
        let candidate = LiepinJobCandidate {
            link_text: "AI应用工程师 【 深圳 】 15-25k 应届 本科".to_string(),
            card_text: "AI应用工程师 【 深圳 】 15-25k 应届 本科 示例公司".to_string(),
            href: "https://www.liepin.com/lptjob/12345678".to_string(),
        };
        let job = candidate_to_rpa_job(candidate).unwrap();

        let detail = build_job_detail(&job, &default_app_config(), true);

        assert!(detail.is_send_resume);
        assert!(detail.resume_sent_at.is_some());
    }


    #[test]
    fn job_card_selectors_exclude_hot_job_category_items() {
        let selectors = job_card_selectors();

        assert!(selectors
            .iter()
            .any(|selector| selector.contains("c_pc_search_job_listcard")));
        assert!(!selectors.contains(&"div[class*='job-card']"));
        assert!(!selectors.contains(&"li[class*='job']"));
    }

    #[test]
    fn collect_jobs_script_uses_real_liepin_card_container() {
        let script = build_collect_jobs_script();

        assert!(script.contains("c_pc_search_job_listcard"));
        assert!(script.contains("job-card-pc-container"));
        assert!(script.contains("a[href*='/job/'], a[href*='/a/']"));
    }

    #[test]
    fn parses_liepin_card_text_into_job_fields() {
        let candidate = LiepinJobCandidate {
            link_text:
                "大模型应用工程师(J11355) 【 上海-浦东新区 】 15-30k·13薪 2年以上 统招本科"
                    .to_string(),
            card_text:
                "大模型应用工程师(J11355) 【 上海-浦东新区 】 15-30k·13薪 2年以上 统招本科 皓元医药 制药2000-5000人 张女士·HRBP经理 2天前在线"
                    .to_string(),
            href: "https://www.liepin.com/job/1979771045.shtml".to_string(),
        };

        let job = candidate_to_rpa_job(candidate).unwrap();

        assert_eq!(job.title, "大模型应用工程师(J11355)");
        assert_eq!(job.location, Some("上海-浦东新区".to_string()));
        assert_eq!(job.salary, "15-30k·13薪");
        assert_eq!(job.company_name, "皓元医药");
        assert_eq!(job.platform_job_id, "1979771045.shtml");
    }

    #[test]
    fn image_input_selectors_match_liepin_ant_im_upload_instead_of_accept_image() {
        // 猎聘 input 写的是 accept="jpg, jpeg, png, bmp"，Boss 那套 accept*="image" 命中不到
        assert_eq!(
            LIEPIN_IMAGE_INPUT_SELECTORS[0],
            ".im-ui-upload-container input[type='file']"
        );
        assert!(LIEPIN_IMAGE_INPUT_SELECTORS
            .iter()
            .any(|selector| selector.contains("ant-im-upload")));
        assert!(LIEPIN_IMAGE_INPUT_SELECTORS
            .iter()
            .any(|selector| selector.contains("accept*='jpg'")));
    }

    #[test]
    fn rejects_image_formats_liepin_upload_does_not_accept() {
        let error = ensure_supported_image(Path::new("C:/tmp/demo.gif")).unwrap_err();

        assert!(error.to_string().contains("jpg/jpeg/png/bmp"));
        assert!(error.to_string().contains("gif"));
    }

    #[test]
    fn reports_missing_file_for_supported_extension() {
        let error =
            ensure_supported_image(Path::new("./__not_exists__/liepin-greet.png")).unwrap_err();

        assert!(error.to_string().contains("图片文件不存在"));
    }

    #[test]
    fn delivery_is_confirmed_by_the_real_liepin_endpoints() {
        let script = build_wait_image_delivery_script(1234.5);

        assert!(script.contains("const since = 1234.5"));
        assert!(script.contains("item.status >= 200 && item.status < 300"));
        // 实测接口：file.liepin.com 传文件，chat.send-push 发消息
        assert!(script.contains(r"/\/upload\/|file\.liepin\.com/i"));
        assert!(script.contains(r"/chat\.send-push|chat\.send|send-push/i"));
    }

    #[test]
    fn uploaded_but_unsent_image_is_a_failure_not_a_success() {
        // 实测出现过：文件传上去了，但没有 send-push，图片其实没发出去
        let script = build_wait_image_delivery_script(0.0);

        assert!(script.contains("uploaded: true, sent: true"));
        assert!(script.contains("sent: false"));
        assert!(script.contains("clicked = clickSendIfEnabled()"));
    }

    #[test]
    fn missing_delivery_reports_the_post_requests_it_saw() {
        // 接口改版时把期间的 POST 请求带回日志，能直接看出新地址
        let script = build_wait_image_delivery_script(0.0);

        assert!(script.contains("seen: recent().map((item) => item.url + \" -> \" + item.status)"));
        assert!(format_seen_requests(&[]).contains("没有发出任何 POST 请求"));
        assert!(format_seen_requests(&[
            "https://file.liepin.com/upload/public-file.json -> 200".to_string()
        ])
        .contains("public-file.json"));
    }

    #[test]
    fn request_recorder_hooks_both_fetch_and_xhr_and_is_idempotent() {
        assert!(INSTALL_REQUEST_RECORDER_SCRIPT.contains("window.fetch = function"));
        assert!(INSTALL_REQUEST_RECORDER_SCRIPT.contains("XMLHttpRequest.prototype.send"));
        // 同一标签页发多张图会重复注入，必须幂等
        assert!(INSTALL_REQUEST_RECORDER_SCRIPT.contains("if (!window.__fjRecorderInstalled)"));
        assert!(INSTALL_REQUEST_RECORDER_SCRIPT.contains("return { now: performance.now() }"));
    }

    /// 会话列表实测 30KB、聊天记录 6KB，按 4000 截断会直接把 JSON 弄坏，
    /// 沟通那边就只能退回刮 DOM
    #[test]
    fn im_responses_are_recorded_in_full_while_others_stay_truncated() {
        assert!(INSTALL_REQUEST_RECORDER_SCRIPT
            .contains(r#"/com\.liepin\.im\./.test(address) ? 400000 : 4000"#));
    }

    #[test]
    fn send_button_fallback_only_clicks_when_enabled() {
        let script = build_wait_image_delivery_script(0.0);

        assert!(script.contains("ant-im-btn-disabled"));
        assert!(script.contains("if (!button || isDisabled(button)) return false;"));
    }

    #[test]
    fn round_summary_replaces_per_job_skip_logs_with_one_line() {
        let stats = RoundStats {
            scanned: 42,
            skipped_processed: 28,
            skipped_rule: 8,
            skipped_ai: 3,
            skipped_humanize: 2,
            skipped_hold: 4,
            greeted: 2,
            greet_failed: 1,
        };

        let summary = stats.summary();

        assert!(summary.contains("42 条岗位"));
        assert!(summary.contains("成功 2 条"));
        assert!(summary.contains("失败 1 条"));
        assert!(summary.contains("已沟通跳过 28 条"));
        assert!(summary.contains("规则过滤跳过 8 条"));
        assert!(summary.contains("AI 复核跳过 3 条"));
        // 拟人化跳过必须和「规则不匹配」分开记，否则用户会以为自己筛选条件写错了
        assert!(summary.contains("拟人化跳过 2 条"));
        assert!(summary.contains("发送闸门拦下 4 条"));
    }

    #[test]
    fn formats_greet_failure_message_with_job_context_and_continue_hint() {
        let error = anyhow::anyhow!("发送失败");

        let message = greet_failure_message("大模型应用工程师", "皓元医药", &error);

        assert!(message.contains("大模型应用工程师"));
        assert!(message.contains("皓元医药"));
        assert!(message.contains("发送失败"));
        assert!(message.contains("跳过该岗位，继续处理下一个"));
    }
}
