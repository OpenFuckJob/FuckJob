use std::time::Duration;

use crate::{
    browser,
    config::{AppRuntimeConfig, ReplayResourceType, ReplyResource},
    dao::{job_detail_dao, model::JobDetail},
    llm::generate_greet_text,
    logger,
    rpa::{common::RpaJob, liepin::LIEPIN_SITE_URL, run_flow::is_job_task_stop_requested},
    utils::salary::decode_salary,
    verify,
};
use chrono::Local;
use rust_drission::{utils::sleep_random_ms, ChromiumPage, Page};
use serde::Deserialize;
use urlencoding::encode;

pub async fn position_say_hello(config: &AppRuntimeConfig) -> Result<(), anyhow::Error> {
    let config = config.clone();
    let search_url = build_job_search_url(&config);

    browser::with_browser(|page| {
        Box::pin(async move {
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
                for job in jobs {
                    if is_job_task_stop_requested() {
                        logger::info("猎聘求职任务已结束")?;
                        return Ok(());
                    }

                    logger::info(format!(
                        "猎聘当前处理岗位:{} 公司:{}",
                        job.title, job.company_name
                    ))?;

                    if !verify::filter_verify(&job, &config) {
                        logger::info("猎聘岗位不匹配，跳过")?;
                        continue;
                    }

                    match greet_job(page, job.clone(), config.clone()).await {
                        Ok(()) => {}
                        Err(error) => {
                            logger::warning(greet_failure_message(
                                &job.title,
                                &job.company_name,
                                &error,
                            ))?;
                            continue;
                        }
                    }
                    sleep_random_ms(2500, 4500);
                }

                if !scroll_next(page)? {
                    logger::info("猎聘岗位列表已触底")?;
                    return Ok(());
                }
            }
        })
    })
    .await
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

fn apply_liepin_filters(
    page: &ChromiumPage,
    config: &AppRuntimeConfig,
) -> Result<(), anyhow::Error> {
    let value = page.run_js_await(&build_apply_liepin_filter_script(config))?;
    let summary = value.get("value").cloned().unwrap_or(value).to_string();
    logger::info(format!("猎聘筛选应用结果: {}", summary))?;
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

fn collect_jobs(page: &ChromiumPage) -> Result<Vec<RpaJob>, anyhow::Error> {
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

    let page = browser_page.new_tab(None)?;
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

async fn build_greet_resources(
    config: &AppRuntimeConfig,
    job: &RpaJob,
) -> Result<Vec<ReplyResource>, anyhow::Error> {
    let resources = config.greet_config.default_template.clone();

    if config.greet_config.reply_prompt.is_none() {
        return Ok(resources);
    }

    let generated = match generate_greet_text(config.clone(), job).await {
        Ok(result) if result.success && !result.data.trim().is_empty() => Some(result.data),
        Ok(_) => {
            logger::warning("LLM 未生成打招呼内容，仅发送显式模板")?;
            None
        }
        Err(error) => {
            logger::warning(format!("LLM 打招呼生成失败，仅发送显式模板: {}", error))?;
            None
        }
    };
    Ok(resources
        .into_iter()
        .filter_map(|mut resource| {
            if resource.resource_type == ReplayResourceType::LLM {
                let text = generated.as_ref()?;
                resource.content = text.clone();
            }
            (!resource.content.trim().is_empty()).then_some(resource)
        })
        .collect())
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
            ReplayResourceType::Image => {
                logger::warning("猎聘暂不支持自动发送图片资源，已跳过")?;
            }
        }
    }

    Ok(())
}

fn send_text_resource(page: &Page, text: &str) -> Result<(), anyhow::Error> {
    let value = page.run_js_await(&build_send_text_script(text))?;
    let result = value.get("value").cloned().unwrap_or(value);
    let success = result
        .get("success")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    if !success {
        return Err(anyhow::anyhow!(
            "猎聘消息发送失败: {}",
            result
                .get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("未找到可用发送按钮")
        ));
    }

    sleep_random_ms(700, 1000);

    let check = page.run_js_await(&build_check_message_sent_script(text))?;
    let check_result = check.get("value").cloned().unwrap_or(check);
    let sent = check_result
        .get("success")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if sent {
        logger::info(format!("猎聘消息发送点击结果: {}", check_result))?;
        return Ok(());
    }

    Err(anyhow::anyhow!(
        "猎聘消息发送失败: {}",
        check_result
            .get("message")
            .and_then(|value| value.as_str())
            .unwrap_or("已点击发送按钮，但输入框内容未清空")
    ))
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

            const input = Array
                .from(document.querySelectorAll("textarea, input[type='text'], div[contenteditable='true'], [contenteditable='true']"))
                .filter(visible)
                .find((el) => !el.disabled && !el.readOnly);

            if (!input) {{
                return {{ success: false, message: "未找到聊天输入框" }};
            }}

            input.focus();
            if (input.isContentEditable) {{
                input.textContent = message;
            }} else {{
                setNativeValue(input, message);
            }}
            dispatch(input);
            await sleep(500);

            const antImButtons = Array.from(document.querySelectorAll(".ant-im-btn"));
            const preferredButton = antImButtons[1];
            const button = preferredButton && visible(preferredButton) && !isDisabled(preferredButton)
                ? preferredButton
                : Array
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

            if (!button) {{
                return {{
                    success: false,
                    message: "未找到可用发送按钮",
                    inputText: inputValue(input)
                }};
            }}

            button.scrollIntoView({{ block: "center", inline: "center" }});
            await sleep(200);
            button.click();
            await sleep(300);
            return {{
                success: true,
                inputText: inputValue(input),
                buttonText: (button.innerText || button.textContent || "").trim(),
                buttonClass: button.getAttribute("class") || ""
            }};
        }})()
        "#
    )
}

fn build_check_message_sent_script(text: &str) -> String {
    let text_json = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"
        (() => {{
            const message = {text_json};
            const visible = (el) => {{
                if (!el) return false;
                const rect = el.getBoundingClientRect();
                const style = window.getComputedStyle(el);
                return rect.width > 0 && rect.height > 0
                    && style.visibility !== "hidden"
                    && style.display !== "none";
            }};
            const inputValue = (el) => (el.isContentEditable ? el.textContent : el.value) || "";
            const inputs = Array
                .from(document.querySelectorAll("textarea, input[type='text'], div[contenteditable='true'], [contenteditable='true']"))
                .filter(visible);
            const stillPending = inputs.some((input) => inputValue(input).includes(message));
            return stillPending
                ? {{ success: false, message: "已点击发送按钮，但输入框内容未清空" }}
                : {{ success: true, message: "输入框已清空" }};
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

fn scroll_next(page: &ChromiumPage) -> Result<bool, anyhow::Error> {
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

pub(crate) fn click_first(page: &Page, selectors: &[&str]) -> Result<(), anyhow::Error> {
    for selector in selectors {
        if let Some(ele) = page.ele(selector)? {
            ele.click()?;
            return Ok(());
        }
    }
    Err(anyhow::anyhow!(
        "未找到可点击元素: {}",
        selectors.join(", ")
    ))
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
        assert!(script.contains("await sleep(500)"));
        assert!(script.contains("ariaDisabled === \"true\""));
        assert!(script.contains("document.querySelectorAll(\".ant-im-btn\")"));
        assert!(script.contains("antImButtons[1]"));
        assert!(script.contains("button.click()"));
        assert!(script.contains("success: true"));
    }

    #[test]
    fn check_message_sent_script_fails_when_input_still_contains_message() {
        let script = build_check_message_sent_script("你好，想进一步沟通");

        assert!(script.contains("stillPending"));
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
    fn formats_greet_failure_message_with_job_context_and_continue_hint() {
        let error = anyhow::anyhow!("发送失败");

        let message = greet_failure_message("大模型应用工程师", "皓元医药", &error);

        assert!(message.contains("大模型应用工程师"));
        assert!(message.contains("皓元医药"));
        assert!(message.contains("发送失败"));
        assert!(message.contains("跳过该岗位，继续处理下一个"));
    }
}
