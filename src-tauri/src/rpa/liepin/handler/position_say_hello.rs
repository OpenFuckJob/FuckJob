use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::{
    browser,
    config::{AppRuntimeConfig, ReplayResourceType, ReplyResource},
    dao::{job_detail_dao, model::JobDetail},
    logger,
    rpa::{
        common::{upload_image_to_file_input, RpaJob},
        greet::build_greet_resources,
        liepin::LIEPIN_SITE_URL,
        run_flow::is_job_task_stop_requested,
    },
    utils::salary::decode_salary,
    verify,
};
use chrono::Local;
use rust_drission::{utils::sleep_random_ms, ChromiumPage, Page};
use serde::Deserialize;
use urlencoding::encode;

pub async fn position_say_hello(config: &AppRuntimeConfig) -> Result<(), anyhow::Error> {
    let config = config.clone();
    browser::with_browser(|connection| {
        Box::pin(
            async move { position_say_hello_on_page(connection, connection.tab(), &config).await },
        )
    })
    .await
}

/// Run the Liepin job-hunting flow on the task-owned main tab.
pub async fn position_say_hello_on_page(
    connection: &ChromiumPage,
    page: &Page,
    config: &AppRuntimeConfig,
) -> Result<(), anyhow::Error> {
    let config = config.clone();
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

            match greet_job(connection, job.clone(), config.clone()).await {
                Ok(()) => {
                    stats.greeted += 1;
                    processed_job_ids.insert(format!("liepin:{}", job.platform_job_id));
                    processed_job_ids.insert(job.platform_job_id.clone());
                }
                Err(error) => {
                    stats.greet_failed += 1;
                    logger::warning(greet_failure_message(&job.title, &job.company_name, &error))?;
                    continue;
                }
            }
            sleep_random_ms(2500, 4500);
        }

        logger::info(stats.summary())?;

        if !scroll_next(page)? {
            logger::info("猎聘岗位列表已触底")?;
            return Ok(());
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
    greeted: u32,
    greet_failed: u32,
}

impl RoundStats {
    fn summary(&self) -> String {
        format!(
            "猎聘本页 {} 条岗位：打招呼成功 {} 条，失败 {} 条；已沟通跳过 {} 条，规则过滤跳过 {} 条，AI 复核跳过 {} 条",
            self.scanned,
            self.greeted,
            self.greet_failed,
            self.skipped_processed,
            self.skipped_rule,
            self.skipped_ai
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
        detail_url,
    })
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

async fn greet_job(
    browser_page: &ChromiumPage,
    mut job: RpaJob,
    config: AppRuntimeConfig,
) -> Result<(), anyhow::Error> {
    if job.detail_url.is_empty() {
        logger::warning("猎聘岗位缺少详情链接，跳过")?;
        return Ok(());
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

        click_first(
            &page,
            &[
                ".btn-apply",
                ".apply-btn",
                "button[class*='apply']",
                "a[class*='apply']",
                "button[class*='chat']",
                "a[class*='chat']",
            ],
        )?;
        sleep_random_ms(800, 1200);

        let resources = build_greet_resources(&config, &job).await?;
        send_resources(&page, resources)?;
        save_job_detail(&job);
        logger::info(format!("猎聘 {} 初次沟通成功", job.title))?;
        Ok(())
    }
    .await;
    let close_result = page.close();

    match (result, close_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(err), _) => Err(err),
        (Ok(()), Err(err)) => Err(err.into()),
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

fn send_text_resource(page: &Page, text: &str) -> Result<(), anyhow::Error> {
    let value = page.run_js_await(&build_send_text_script(text))?;
    let result = value.get("value").cloned().unwrap_or(value);
    let success = result
        .get("success")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let message = result
        .get("message")
        .and_then(|value| value.as_str())
        .unwrap_or_default();

    if !success {
        return Err(anyhow::anyhow!("消息发送失败：{}", message));
    }

    // 成功不单独记，末尾的“初次沟通成功”已经覆盖
    Ok(())
}

fn build_send_text_script(text: &str) -> String {
    let text_json = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"
        (async () => {{
            const message = {text_json};
            const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
            const visible = (el) => {{
                if (!el) return false;
                const rect = el.getBoundingClientRect();
                const style = window.getComputedStyle(el);
                return rect.width > 0 && rect.height > 0
                    && style.visibility !== "hidden"
                    && style.display !== "none";
            }};
            const setNativeValue = (el, value) => {{
                const proto = el instanceof HTMLTextAreaElement
                    ? HTMLTextAreaElement.prototype
                    : HTMLInputElement.prototype;
                const descriptor = Object.getOwnPropertyDescriptor(proto, "value");
                if (descriptor && descriptor.set) {{
                    descriptor.set.call(el, value);
                }} else {{
                    el.value = value;
                }}
            }};
            const dispatch = (el) => {{
                el.dispatchEvent(new InputEvent("input", {{ bubbles: true, inputType: "insertText", data: message }}));
                el.dispatchEvent(new Event("change", {{ bubbles: true }}));
                el.dispatchEvent(new KeyboardEvent("keyup", {{ bubbles: true, key: "Enter" }}));
            }};
            const inputValue = (el) => (el.isContentEditable ? el.textContent : el.value) || "";
            const isDisabled = (el) => {{
                const className = el.getAttribute("class") || "";
                const ariaDisabled = el.getAttribute("aria-disabled");
                return Boolean(el.disabled)
                    || ariaDisabled === "true"
                    || className.includes("disabled")
                    || className.includes("ant-im-btn-disabled");
            }};

            // 等状态而不是等时间：条件成立立刻继续，页面慢就自动多等，
            // 超时只是异常上限，不参与正常判定
            const waitFor = async (getter, timeoutMs, label) => {{
                const deadline = performance.now() + timeoutMs;
                for (;;) {{
                    const found = getter();
                    if (found) return found;
                    if (performance.now() >= deadline) {{
                        return null;
                    }}
                    await sleep(150);
                }}
            }};
            const findInput = () => Array
                .from(document.querySelectorAll("textarea, input[type='text'], div[contenteditable='true'], [contenteditable='true']"))
                .filter(visible)
                .find((el) => !el.disabled && !el.readOnly);
            const findSendButton = () => {{
                const antImButtons = Array.from(document.querySelectorAll(".ant-im-btn"));
                const preferred = antImButtons[1];
                if (preferred && visible(preferred) && !isDisabled(preferred)) {{
                    return preferred;
                }}
                return Array
                    .from(document.querySelectorAll("button.im-ui-basic-send-btn, button.ant-im-btn-primary, button, a, div[role='button'], span[role='button'], .btn-send, .send-btn, [class*='send'], [class*='Send']"))
                    .filter(visible)
                    .find((el) => {{
                        const text = (el.innerText || el.textContent || "").trim();
                        const className = el.getAttribute("class") || "";
                        return (text === "发送"
                                || text.includes("发送")
                                || className.includes("im-ui-basic-send-btn")
                                || className.includes("ant-im-btn-primary")
                                || /(^|\s)(btn-send|send-btn)(\s|$)/.test(className)
                                || /send|Send/.test(className))
                            && !isDisabled(el);
                    }});
            }};

            // 聊天窗可能还在加载，等它出现而不是查一次就判死
            const input = await waitFor(findInput, {INPUT_READY_TIMEOUT_MS});
            if (!input) {{
                return {{
                    success: false,
                    message: "聊天输入框在 {INPUT_READY_TIMEOUT_MS} 毫秒内未出现"
                }};
            }}

            input.focus();
            if (input.isContentEditable) {{
                input.textContent = message;
            }} else {{
                setNativeValue(input, message);
            }}
            dispatch(input);

            // 填入内容后按钮才会由 disabled 变可用，同样等状态
            const button = await waitFor(findSendButton, {SEND_BUTTON_READY_TIMEOUT_MS});
            if (!button) {{
                return {{
                    success: false,
                    message: "发送按钮在 {SEND_BUTTON_READY_TIMEOUT_MS} 毫秒内未变为可用",
                    inputText: inputValue(input)
                }};
            }}

            button.scrollIntoView({{ block: "center", inline: "center" }});
            button.click();

            // 发出去的标志是输入框被清空，等这个状态，别按固定时长猜
            const cleared = await waitFor(
                () => !inputValue(input).includes(message) || null,
                {INPUT_CLEARED_TIMEOUT_MS}
            );

            return {{
                success: Boolean(cleared),
                message: cleared ? "输入框已清空" : "已点击发送按钮，但输入框内容未清空",
                inputText: inputValue(input),
                buttonText: (button.innerText || button.textContent || "").trim(),
                buttonClass: button.getAttribute("class") || ""
            }};
        }})()
        "#
    )
}

fn save_job_detail(job: &RpaJob) {
    let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let job_detail = JobDetail {
        id: format!("liepin:{}", job.platform_job_id),
        platform: "liepin".to_string(),
        title: job.title.clone(),
        company_name: job.company_name.clone(),
        detail: job.detail.clone(),
        salary: job.salary.clone(),
        location: job.location.clone(),
        is_reply: false,
        is_send_resume: false,
        created_at: now.clone(),
        resume_sent_at: None,
        updated_at: now,
    };

    if let Err(e) = job_detail_dao::create(job_detail) {
        let _ = logger::warning(format!("保存猎聘岗位数据失败: {}", e));
    }
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
pub(crate) fn click_first(page: &Page, selectors: &[&str]) -> Result<(), anyhow::Error> {
    let deadline = Instant::now() + CLICKABLE_READY_TIMEOUT;
    loop {
        for selector in selectors {
            // 页面正在导航时 ele 会短暂报错，这属于“还没就绪”而不是失败
            if let Ok(Some(ele)) = page.ele(selector) {
                ele.click()?;
                return Ok(());
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
        assert!(script.contains("text.includes(\"发送\")"));
        assert!(script.contains("button.im-ui-basic-send-btn"));
        assert!(script.contains("button.ant-im-btn-primary"));
        assert!(script.contains("ariaDisabled === \"true\""));
        assert!(script.contains("document.querySelectorAll(\".ant-im-btn\")"));
        assert!(script.contains("antImButtons[1]"));
        assert!(script.contains("button.click()"));
    }

    #[test]
    fn send_text_script_waits_for_elements_instead_of_sleeping_a_fixed_time() {
        // “等 500ms 再查一次，查不到就判失败”会在页面慢时误报“未找到可用发送按钮”
        let script = build_send_text_script("你好");

        assert!(!script.contains("await sleep(500)"));
        assert!(script.contains("waitFor(findInput"));
        assert!(script.contains("waitFor(findSendButton"));
        assert!(script.contains("聊天输入框在 15000 毫秒内未出现"));
        assert!(script.contains("发送按钮在 15000 毫秒内未变为可用"));
    }

    #[test]
    fn send_text_script_confirms_delivery_by_waiting_for_the_input_to_clear() {
        let script = build_send_text_script("你好");

        assert!(script.contains("inputValue(input).includes(message)"));
        assert!(script.contains("已点击发送按钮，但输入框内容未清空"));
        assert!(script.contains("输入框已清空"));
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
