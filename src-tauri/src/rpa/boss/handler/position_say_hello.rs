use std::collections::HashSet;
use std::time::Duration;

use crate::{
    browser,
    config::{AppRuntimeConfig, JobFilterConfig, ReplayResourceType, ReplyResource},
    dao::{job_detail_dao, model::JobDetail},
    llm::generate_greet_text,
    logger,
    rpa::{
        boss::{handler::send_messages, model::GreetJob},
        run_flow::is_job_task_stop_requested,
        run_flow::PlatformKind,
    },
    utils::salary::decode_salary,
    verify,
};
use chrono::Local;
use rust_drission::{utils::sleep_random_ms, ChromiumPage, DataPacket, Element, Page};
use serde_json::Value;
use urlencoding::encode;

// 岗位打招呼轰炸
pub async fn position_say_hello(
    app_runtime_config: &AppRuntimeConfig,
) -> Result<(), anyhow::Error> {
    let app_runtime_config = app_runtime_config.clone();
    let search_url = build_job_search_url(&app_runtime_config.job_filter_config);
    browser::with_browser(|page| {
        Box::pin(async move {
            // 加载本地已处理的岗位ID，用于去重
            let mut processed_job_ids: HashSet<String> = job_detail_dao::list()
                .unwrap_or_default()
                .into_iter()
                .map(|j| j.id)
                .collect();
            logger::info(format!("本地已存储 {} 条岗位记录", processed_job_ids.len()))?;

            // 在 page.get 之前开始监听，捕获首次加载触发的 joblist 请求
            let joblist_listener = page.listen_url("wapi/zpgeek/search/joblist.json")?;

            page.get(&search_url)?;
            page.wait(".rec-job-list", Duration::from_secs(5))?;

            // 消费首次 page.get 触发的 joblist 响应，初始化 seen_job_ids
            let mut seen_job_ids: HashSet<String> = HashSet::new();
            if let Ok(Some(packet)) = joblist_listener.wait(Duration::from_secs(10)) {
                let (first_ids, _) = parse_joblist_response(&packet);
                seen_job_ids.extend(first_ids);
            }
            let mut no_new_count = 0u32;
            const MAX_NO_NEW_RETRY: u32 = 3;

            loop {
                if is_job_task_stop_requested() {
                    logger::info("求职任务已结束")?;
                    return Ok(());
                }

                let jeb_card_area_eles = page.eles(".card-area")?;
                logger::info(format!("页面加载到{}条岗位卡片", jeb_card_area_eles.len()))?;
                if jeb_card_area_eles.is_empty() {
                    logger::info("暂无岗位列表")?;
                    return Ok(());
                }

                for job_card_area_ele in jeb_card_area_eles {
                    if is_job_task_stop_requested() {
                        logger::info("求职任务已结束")?;
                        return Ok(());
                    }
                    let greet_job =
                        match read_job_card(page, &job_card_area_ele, &processed_job_ids) {
                            Ok(Some(greet_job)) => greet_job,
                            Ok(None) => continue,
                            Err(error) => {
                                logger::warning(card_failure_message(&error))?;
                                continue;
                            }
                        };

                    logger::info(format!(
                        "当前处理岗位:{} 公司:{}",
                        greet_job.title, greet_job.company_name
                    ))?;

                    let filter_decision = verify::filter_decision(&greet_job, &app_runtime_config);
                    if !filter_decision.matched {
                        logger::info(format!("岗位不匹配，跳过：{}", filter_decision.reason))?;
                        continue;
                    }
                    if app_runtime_config.job_filter_config.enable_semantic_filter {
                        match crate::llm::evaluate_job_match(&app_runtime_config, &greet_job).await
                        {
                            Ok(decision) if decision.matched => logger::info(format!(
                                "AI 岗位复核通过（{}分）：{}",
                                decision.score, decision.reason
                            ))?,
                            Ok(decision) => {
                                logger::info(format!(
                                    "AI 岗位复核未通过，跳过（{}分）：{}",
                                    decision.score, decision.reason
                                ))?;
                                continue;
                            }
                            Err(error) => {
                                logger::warning(format!(
                                    "AI 岗位复核失败，为避免误投已跳过：{}",
                                    error
                                ))?;
                                continue;
                            }
                        }
                    }
                    match handle_greet(page, greet_job.clone(), app_runtime_config.clone()).await {
                        Ok(()) => {}
                        Err(error) => {
                            logger::warning(greet_failure_message(
                                &greet_job.title,
                                &greet_job.company_name,
                                &error,
                            ))?;
                            if should_continue_after_greet_failure() {
                                continue;
                            }
                            return Ok(());
                        }
                    }
                    if is_job_task_stop_requested() {
                        logger::info("求职任务已结束")?;
                        return Ok(());
                    }

                    logger::info(format!("{} 初次沟通成功", greet_job.title))?;
                    processed_job_ids.insert(greet_job.platform_job_id.clone());
                    sleep_random_ms(3000, 5000);
                }

                // 检查是否已触底
                if page.ele(".loading-wait")?.is_none() {
                    logger::info("岗位列表已触底")?;
                    break;
                }

                // 设置监听 → 滚动 → 等待 joblist API 响应，检测是否有新岗位
                let joblist_listener = page.listen_url("wapi/zpgeek/search/joblist.json")?;
                scroll_bottom(page)?;

                match joblist_listener.wait(Duration::from_secs(10)) {
                    Ok(Some(packet)) => {
                        let (api_ids, has_more) = parse_joblist_response(&packet);

                        if !has_more {
                            logger::info("接口返回无更多岗位，停止加载")?;
                            break;
                        }

                        let new_ids: HashSet<String> = api_ids
                            .into_iter()
                            .filter(|id| !seen_job_ids.contains(id))
                            .collect();

                        if new_ids.is_empty() {
                            no_new_count += 1;
                            logger::info(format!(
                                "本次无新岗位 ({}/{})",
                                no_new_count, MAX_NO_NEW_RETRY
                            ))?;
                            if no_new_count >= MAX_NO_NEW_RETRY {
                                logger::info("连续多次滚动无新岗位，停止加载")?;
                                break;
                            }
                            continue;
                        }

                        no_new_count = 0;
                        seen_job_ids.extend(new_ids);
                    }
                    _ => {
                        no_new_count += 1;
                        if no_new_count >= MAX_NO_NEW_RETRY {
                            logger::info("等待接口响应超时，停止加载")?;
                            break;
                        }
                        continue;
                    }
                }
            }

            Ok(())
        })
    })
    .await?;

    Ok(())
}

fn card_failure_message(error: &anyhow::Error) -> String {
    format!("处理岗位卡片失败，跳过当前岗位，继续处理：{error}")
}

fn read_job_card(
    page: &ChromiumPage,
    job_card_area_ele: &Element,
    processed_job_ids: &HashSet<String>,
) -> Result<Option<GreetJob>, anyhow::Error> {
    if job_card_area_ele.attr("class")?.contains("is-seen") {
        return Ok(None);
    }

    let job_card_ele = job_card_area_ele
        .element(".job-card-box")?
        .ok_or_else(|| anyhow::anyhow!("未找到岗位卡片主体"))?;

    let job_href = job_card_ele
        .element(".job-name")?
        .ok_or_else(|| anyhow::anyhow!("未找到岗位名称链接"))?
        .attr("href")
        .unwrap_or_default();

    let job_id =
        extract_job_id(&format!("https://www.zhipin.com{}", job_href)).map(|s| s.to_string());
    if let Some(ref id) = job_id {
        if processed_job_ids.contains(id.as_str()) {
            return Ok(None);
        }
    }

    job_card_ele.click()?;
    sleep_random_ms(800, 1200);

    let job_detail_text = page
        .ele(".job-detail-body")?
        .ok_or_else(|| anyhow::anyhow!("未加载岗位详情"))?
        .text_content()?;
    let job_name = job_card_ele
        .element(".job-name")?
        .ok_or_else(|| anyhow::anyhow!("未找到岗位标题"))?
        .text_content()?;
    let salary_text = job_card_ele
        .element(".job-salary")?
        .ok_or_else(|| anyhow::anyhow!("未找到岗位薪资"))?
        .text_content()?;
    let company_text = job_card_ele
        .element(".boss-name")?
        .ok_or_else(|| anyhow::anyhow!("未找到公司名称"))?
        .text_content()?;
    let company_location = job_card_ele
        .element(".company-location")?
        .ok_or_else(|| anyhow::anyhow!("未找到公司地址"))?
        .text_content()?;

    let job_detail_url = format!("https://www.zhipin.com{}", job_href);
    let platform_job_id = extract_job_id(&job_detail_url)
        .unwrap_or(&job_detail_url)
        .to_string();

    Ok(Some(GreetJob {
        platform: PlatformKind::Boss,
        platform_job_id,
        title: job_name,
        company_name: company_text,
        detail: job_detail_text,
        salary: decode_salary(&salary_text),
        location: Some(company_location),
        detail_url: job_detail_url,
    }))
}

/// 从 joblist API 响应中提取所有 encryptJobId 和 hasMore 标志
fn parse_joblist_response(packet: &DataPacket) -> (Vec<String>, bool) {
    let body = match &packet.body {
        Some(b) => b,
        None => return (Vec::new(), true),
    };
    let body_str = match String::from_utf8(body.clone()) {
        Ok(s) => s,
        Err(_) => return (Vec::new(), true),
    };
    let root: serde_json::Value = match serde_json::from_str(&body_str) {
        Ok(v) => v,
        Err(_) => return (Vec::new(), true),
    };

    let has_more = root
        .pointer("/zpData/hasMore")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let job_ids = root
        .pointer("/zpData/jobList")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|job| {
                    job.get("encryptJobId")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
                .collect()
        })
        .unwrap_or_default();

    (job_ids, has_more)
}

fn join_vec(values: &[i64]) -> Option<String> {
    if values.is_empty() {
        None
    } else {
        Some(
            values
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(","),
        )
    }
}

// https://www.zhipin.com/web/geek/jobs?city=101290100&position=100121,101313&jobType=1901&salary=405&experience=102,101&degree=208,202,206&industry=100007,100003,100028&scale=302,303&stage=803,805,804&query=Python
fn build_job_search_url(job_filter_config: &JobFilterConfig) -> String {
    let base_url = "https://www.zhipin.com/web/geek/jobs";

    let mut params: Vec<String> = Vec::new();

    // city
    if let Some(city) = job_filter_config.city {
        params.push(format!("city={}", city));
    }

    // jobType
    if job_filter_config.job_type > 0 {
        params.push(format!("jobType={}", job_filter_config.job_type));
    }

    // salary
    if job_filter_config.salary > 0 {
        params.push(format!("salary={}", job_filter_config.salary));
    }

    // experience
    if let Some(experience) = join_vec(&job_filter_config.experience) {
        params.push(format!("experience={}", experience));
    }

    // degree
    if let Some(degree) = join_vec(&job_filter_config.dgree) {
        params.push(format!("degree={}", degree));
    }

    // industry
    if let Some(industry) = join_vec(&job_filter_config.industry) {
        params.push(format!("industry={}", industry));
    }

    // scale
    if let Some(scale) = join_vec(&job_filter_config.scale) {
        params.push(format!("scale={}", scale));
    }

    // stage
    if let Some(stage) = join_vec(&job_filter_config.stage) {
        params.push(format!("stage={}", stage));
    }

    // query
    if let Some(query) = &job_filter_config.query {
        if !query.trim().is_empty() {
            params.push(format!("query={}", encode(query)));
        }
    }

    if params.is_empty() {
        base_url.to_string()
    } else {
        format!("{}?{}", base_url, params.join("&"))
    }
}

fn extract_job_id(url: &str) -> Option<&str> {
    let prefix = "/job_detail/";
    let suffix = ".html";

    let start = url.find(prefix)? + prefix.len();
    let end = url[start..].find(suffix)? + start;

    Some(&url[start..end])
}

fn save_job_detail(job_id: &str, greet_job: &GreetJob) {
    let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let job_detail = JobDetail {
        id: job_id.to_string(),
        platform: "boss".to_string(),
        title: greet_job.title.clone(),
        company_name: greet_job.company_name.clone(),
        detail: greet_job.detail.clone(),
        salary: greet_job.salary.clone(),
        location: greet_job.location.clone(),
        is_reply: false,
        is_send_resume: false,
        created_at: now.clone(),
        resume_sent_at: None,
        updated_at: now,
    };

    if let Err(e) = job_detail_dao::create(job_detail) {
        let _ = logger::warning(format!("保存岗位数据失败: {}", e));
    }
}

fn greet_failure_message(title: &str, company_name: &str, error: &anyhow::Error) -> String {
    format!(
        "岗位打招呼失败：{} - {}，错误：{}。跳过当前岗位，继续处理",
        title, company_name, error
    )
}

const GREET_BUTTON_SELECTOR: &str = "a.op-btn.op-btn-chat, a.btn.btn-startchat";
const GREET_CONFIRM_SELECTOR: &str = "span.btn.btn-sure[ka=\"dialog_confirm\"], \
    .chat-block-container .sure-btn, .greet-boss-container .sure-btn";
const SCROLL_BOTTOM_SCRIPT: &str = r#"
(() => {
    const html = document.documentElement;
    const body = document.body;
    const scrollContainer = html.scrollHeight > html.clientHeight ? html : body;
    scrollContainer.scrollTop = scrollContainer.scrollHeight;
})();
"#;

fn clean_html_url(url: &str) -> String {
    url.replace("&amp;", "&")
}

fn greet_button_matches_job(data_url: &str, redirect_url: &str, ka: &str, job_id: &str) -> bool {
    let data_url = clean_html_url(data_url);
    let redirect_url = clean_html_url(redirect_url);
    data_url.contains(job_id) || redirect_url.contains(job_id) || ka.contains(job_id)
}

fn absolute_boss_url(url: &str) -> Option<String> {
    let cleaned = clean_html_url(url);
    let url = cleaned.trim();
    if url.is_empty() || url.starts_with("javascript:") {
        None
    } else if url.starts_with("https://") || url.starts_with("http://") {
        Some(url.to_string())
    } else if url.starts_with("//") {
        Some(format!("https:{url}"))
    } else if url.starts_with('/') {
        Some(format!("https://www.zhipin.com{url}"))
    } else {
        Some(format!("https://www.zhipin.com/{url}"))
    }
}

fn is_chat_page_url(url: &str) -> bool {
    url.contains("/web/geek/chat")
}

fn valid_chat_redirect_url(url: &str) -> Option<String> {
    let url = absolute_boss_url(url)?;

    is_chat_page_url(&url).then_some(url)
}

fn is_greet_success_dialog_text(text: &str) -> bool {
    text.contains("已向BOSS发送消息")
}

fn should_continue_after_greet_failure() -> bool {
    true
}

fn chat_wait_error(page: &Page, source: impl std::fmt::Display) -> anyhow::Error {
    let current_url = page.url().unwrap_or_else(|_| "未知 URL".to_string());
    let dialog_text = page
        .ele(".greet-boss-container, .chat-block-container, .dialog-container, .boss-popup")
        .ok()
        .flatten()
        .and_then(|element| element.text_content().ok())
        .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "未检测到弹窗".to_string());
    anyhow::anyhow!(
        "聊天输入区未就绪，当前页面: {current_url}，页面提示: {dialog_text}，原始错误: {source}"
    )
}

async fn handle_greet(
    browser_page: &ChromiumPage,
    greet_job: GreetJob,
    config: AppRuntimeConfig,
) -> Result<(), anyhow::Error> {
    // 工作标签直接打开已知岗位详情，避免重新定位列表中的分页卡片。
    let work_page = browser::new_stealth_tab(browser_page)?;
    let result = async {
        work_page.goto(work_tab_navigation_url(&greet_job))?;
        work_page.wait(GREET_BUTTON_SELECTOR, Duration::from_secs(30))?;
        sleep_random_ms(500, 800);
        handle_greet_on_work_tab(browser_page, &work_page, greet_job, config).await
    }
    .await;
    let close_result = work_page.close();

    match (result, close_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(error.into()),
    }
}

fn work_tab_navigation_url(greet_job: &GreetJob) -> &str {
    &greet_job.detail_url
}

async fn handle_greet_on_work_tab(
    browser_page: &ChromiumPage,
    work_page: &Page,
    greet_job: GreetJob,
    config: AppRuntimeConfig,
) -> Result<(), anyhow::Error> {
    // 1. 在工作标签的岗位详情页中等待并匹配沟通按钮
    let mut current_job_ready = false;
    let mut target_btn = None;

    for _ in 0..30 {
        if let Some(btn) = work_page.ele(GREET_BUTTON_SELECTOR)? {
            let data_url = btn.attr("data-url").unwrap_or_default();
            let redirect_url = btn.attr("redirect-url").unwrap_or_default();
            let ka = btn.attr("ka").unwrap_or_default();
            if greet_button_matches_job(&data_url, &redirect_url, &ka, &greet_job.platform_job_id) {
                current_job_ready = true;
                target_btn = Some(btn);
                break;
            }
        }
        sleep_random_ms(400, 600);
    }

    if !current_job_ready {
        return Err(anyhow::anyhow!(
            "工作标签岗位详情页未找到当前岗位的立即沟通按钮: {}",
            greet_job.platform_job_id
        ));
    }

    let btn = target_btn.ok_or_else(|| anyhow::anyhow!("未找到当前岗位的立即沟通按钮"))?;
    let chat_redirect_url = btn.attr("redirect-url").unwrap_or_default();
    let mut chat_redirect_url = valid_chat_redirect_url(&chat_redirect_url);

    let data_isfriend = btn
        .attr("data-isfriend")
        .map(|v| v == "true")
        .unwrap_or(false);
    if data_isfriend {
        logger::info("岗位已打过招呼 跳过")?;
        return Ok(());
    }

    // 2. BOSS 的建联按钮使用站内 JavaScript 路由，会将当前工作标签切换到聊天页。
    btn.click()?;
    if is_job_task_stop_requested() {
        logger::info("求职任务已结束")?;
        return Ok(());
    }

    // 3. 在岗位详情页处理确认弹窗（例如“继续沟通”、“确定”、“好”）
    let mut last_confirm_text = String::new();
    let mut confirm_dialog_handled = false;
    let mut greet_success_seen = false;
    for _ in 0..15 {
        if is_job_task_stop_requested() {
            logger::info("求职任务已结束")?;
            return Ok(());
        }

        // BOSS 可能直接将当前工作标签导航到聊天页，并不会给出二次确认
        // 弹窗或带岗位 ID 的 URL。这是可用的建联成功信号。
        if is_chat_page_url(&work_page.url()?) {
            logger::info("工作标签已进入聊天页，正在发送招呼")?;
            return handle_send_message_on_chat_page(work_page, greet_job, config).await;
        }

        if let Ok(Some(dialog_title)) = work_page.ele(".greet-boss-dialog h3") {
            if is_greet_success_dialog_text(&dialog_title.text_content()?) {
                greet_success_seen = true;
            }
        }

        // 尝试通用 JavaScript 寻找点击确认按钮
        let click_script = r#"
            (() => {
                const candidates = Array.from(document.querySelectorAll("a, button, span, div, p"));
                for (const el of candidates) {
                    const text = (el.innerText || el.textContent || "").trim();
                    if ((text === "继续沟通" || text === "确定" || text === "好") && el.offsetHeight > 0 && el.offsetWidth > 0) {
                        el.click();
                        return text;
                    }
                }
                return null;
            })()
        "#;

        if let Ok(val) = work_page.run_js_await(click_script) {
            if let Some(clicked_text) = val.as_str().filter(|s| !s.is_empty()) {
                if clicked_text != last_confirm_text {
                    logger::info(format!("检测到沟通确认提示，已自动点击“{clicked_text}”"))?;
                    last_confirm_text = clicked_text.to_string();
                    confirm_dialog_handled = true;
                    sleep_random_ms(600, 1000);
                }
            }
        }

        if let Ok(Some(confirm_btn)) = work_page.ele(GREET_CONFIRM_SELECTOR) {
            if let Ok(text) = confirm_btn.text_content() {
                let text = text.trim();
                if !text.is_empty() && text != last_confirm_text {
                    logger::info(format!("检测到沟通确认提示，正在点击“{text}”"))?;
                    let _ = confirm_btn.click();
                    last_confirm_text = text.to_string();
                    confirm_dialog_handled = true;
                    sleep_random_ms(600, 1000);
                }
            }
        }

        // 站点可能在点击后才异步写入 redirect-url；没有弹窗时也必须等待，
        // 避免在请求尚未完成时就把 15 次轮询耗尽。
        if let Some(current_btn) = work_page.ele(GREET_BUTTON_SELECTOR)? {
            let data_url = current_btn.attr("data-url").unwrap_or_default();
            let redirect_url = current_btn.attr("redirect-url").unwrap_or_default();
            let ka = current_btn.attr("ka").unwrap_or_default();
            if greet_button_matches_job(&data_url, &redirect_url, &ka, &greet_job.platform_job_id) {
                chat_redirect_url = valid_chat_redirect_url(&redirect_url).or(chat_redirect_url);
                if current_btn
                    .attr("data-isfriend")
                    .map(|value| value == "true")
                    .unwrap_or(false)
                {
                    logger::info("岗位建联状态已确认")?;
                    break;
                }
            }
        }

        sleep_random_ms(300, 500);
    }

    if is_job_task_stop_requested() {
        logger::info("求职任务已结束")?;
        return Ok(());
    }

    // 已显示成功提示但站点没有跳转聊天页时，默认招呼已经真实发出；不能误判为失败。
    if greet_success_seen {
        logger::info("站点已确认向 BOSS 发送消息，未跳转聊天页，当前岗位视为建联成功")?;
        save_job_detail(&greet_job.platform_job_id, &greet_job);
        return Ok(());
    }

    // 4. 当前页未自动进入聊天时，使用按钮真实提供的聊天地址打开新 Tab。
    // 不生成任何通用聊天页兜底地址，以免无目标会话被误当作成功。
    let chat_url = match chat_redirect_url {
        Some(url) => url,
        None => {
            let confirm_status = if confirm_dialog_handled {
                "已处理确认弹窗"
            } else {
                "未检测到确认弹窗"
            };
            return Err(anyhow::anyhow!(
                "点击后页面未进入聊天区且未获得可用聊天地址（{confirm_status}，岗位 ID: {}）",
                greet_job.platform_job_id
            ));
        }
    };

    let page = browser::new_stealth_tab(browser_page)?;
    page.goto(&chat_url)?;
    let result = handle_send_message_on_chat_page(&page, greet_job, config).await;
    let close_result = page.close();

    match (result, close_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(error.into()),
    }
}

async fn handle_send_message_on_chat_page(
    page: &Page,
    greet_job: GreetJob,
    config: AppRuntimeConfig,
) -> Result<(), anyhow::Error> {
    if is_job_task_stop_requested() {
        logger::info("求职任务已结束")?;
        return Ok(());
    }

    // 等待聊天输入区与发送按钮加载完成
    page.wait("#chat-input", Duration::from_secs(30))
        .map_err(|error| chat_wait_error(page, error))?;
    page.wait(".chat-op .btn-send", Duration::from_secs(15))
        .map_err(|error| chat_wait_error(page, error))?;

    // 构建招呼资源：优先 LLM 生成，否则使用默认模板
    let resources = build_greet_resources(&config, &greet_job).await?;
    send_if_any(resources, |resources| send_messages(page, resources))?;

    // 保存该岗位至本地数据库
    save_job_detail(&greet_job.platform_job_id, &greet_job);

    sleep_random_ms(1200, 2000);

    Ok(())
}

fn send_if_any<F>(resources: Vec<ReplyResource>, send: F) -> Result<bool, anyhow::Error>
where
    F: FnOnce(Vec<ReplyResource>) -> Result<bool, anyhow::Error>,
{
    if resources.is_empty() {
        return Ok(false);
    }
    send(resources)
}

async fn build_greet_resources(
    config: &AppRuntimeConfig,
    greet_job: &GreetJob,
) -> Result<Vec<ReplyResource>, anyhow::Error> {
    let greet_config = &config.greet_config;
    let resources = greet_config.default_template.clone();

    let Some(_prompt) = &greet_config.reply_prompt else {
        return Ok(resources);
    };

    let generated = match generate_greet_text(config.clone(), greet_job).await {
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

// 滚动到底部
pub fn scroll_bottom(page: &ChromiumPage) -> Result<(), anyhow::Error> {
    page.run_js_await(SCROLL_BOTTOM_SCRIPT)?;

    Ok(())
}

fn _is_bottom_value(value: &Value) -> Result<bool, anyhow::Error> {
    value
        .get("value")
        .and_then(Value::as_bool)
        .or_else(|| value.as_bool())
        .ok_or_else(|| anyhow::anyhow!("页面滚动状态返回值非布尔值"))
}

// 判断是否到底部
pub fn _is_bottom(page: &ChromiumPage) -> Result<bool, anyhow::Error> {
    let script: &str = r#"
(()=>{
const html = document.documentElement;
    const body = document.body;
    const scrollContainer = html.scrollHeight > html.clientHeight ? html : body;
    const tolerance = 5; // 允许误差
    const scrollPosition = scrollContainer.scrollTop + window.innerHeight;
    const totalHeight = scrollContainer.scrollHeight;
    return scrollPosition >= totalHeight - tolerance;
})();
    "#;

    let value = page.run_js_await(script)?;

    _is_bottom_value(&value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_redirect_does_not_require_the_current_job_id() {
        assert_eq!(valid_chat_redirect_url(""), None);
        assert_eq!(
            valid_chat_redirect_url("/web/geek/chat"),
            Some("https://www.zhipin.com/web/geek/chat".to_string())
        );
        assert_eq!(
            valid_chat_redirect_url("/web/geek/chat?id=conversation-123"),
            Some("https://www.zhipin.com/web/geek/chat?id=conversation-123".to_string())
        );
        assert_eq!(valid_chat_redirect_url("/web/geek/jobs"), None);
    }

    #[test]
    fn current_page_navigation_to_chat_is_a_success_signal() {
        assert!(is_chat_page_url("https://www.zhipin.com/web/geek/chat"));
        assert!(is_chat_page_url(
            "https://www.zhipin.com/web/geek/chat?id=conversation-123"
        ));
        assert!(!is_chat_page_url("https://www.zhipin.com/web/geek/jobs"));
    }

    #[test]
    fn a_single_greet_failure_does_not_stop_the_job_loop() {
        assert!(should_continue_after_greet_failure());
    }

    #[test]
    fn greeting_success_dialog_is_not_treated_as_a_failure() {
        assert!(is_greet_success_dialog_text("已向BOSS发送消息"));
        assert!(is_greet_success_dialog_text(
            "提示：已向BOSS发送消息，请等待回复"
        ));
        assert!(!is_greet_success_dialog_text("今日沟通次数已达上限"));
    }

    #[test]
    fn work_tab_navigates_to_job_detail_page_directly() {
        let job = GreetJob {
            platform: PlatformKind::Boss,
            platform_job_id: "a5de0e2bc67cb6a90nFz3925FlpZ".to_string(),
            title: "AI应用开发工程师".to_string(),
            company_name: "示例科技".to_string(),
            detail: "岗位描述".to_string(),
            salary: "30-60K".to_string(),
            location: Some("深圳".to_string()),
            detail_url: "https://www.zhipin.com/job_detail/a5de0e2bc67cb6a90nFz3925FlpZ.html"
                .to_string(),
        };

        assert_eq!(
            work_tab_navigation_url(&job),
            "https://www.zhipin.com/job_detail/a5de0e2bc67cb6a90nFz3925FlpZ.html"
        );
    }

    #[test]
    fn formats_greet_failure_message_with_job_context_and_continue_hint() {
        let error = anyhow::anyhow!("发送按钮不可用");

        let message = greet_failure_message("后端工程师", "示例科技", &error);

        assert!(message.contains("后端工程师"));
        assert!(message.contains("示例科技"));
        assert!(message.contains("发送按钮不可用"));
        assert!(message.contains("跳过当前岗位，继续处理"));
    }

    #[test]
    fn card_failure_message_includes_error_and_continue_hint() {
        let error = anyhow::anyhow!("未加载岗位详情");

        let message = card_failure_message(&error);

        assert!(message.contains("未加载岗位详情"));
        assert!(message.contains("跳过当前岗位"));
        assert!(message.contains("继续处理"));
    }

    #[test]
    fn matches_current_job_from_start_chat_button_urls() {
        let job_id = "ec828fa57528b93e0nd83NW7FlRX";

        assert!(greet_button_matches_job(
            "/wapi/zpgeek/friend/add.json?jobId=ec828fa57528b93e0nd83NW7FlRX",
            "",
            "",
            job_id,
        ));
        assert!(greet_button_matches_job(
            "",
            "/web/geek/chat?id=abc&jobId=ec828fa57528b93e0nd83NW7FlRX",
            "",
            job_id,
        ));
        assert!(greet_button_matches_job(
            "",
            "",
            "cpc_job_list_chat_ec828fa57528b93e0nd83NW7FlRX",
            job_id,
        ));
        assert!(!greet_button_matches_job(
            "/wapi/zpgeek/friend/add.json?jobId=another-job",
            "/web/geek/chat?jobId=another-job",
            "cpc_job_list_chat_another-job",
            job_id,
        ));
    }

    #[test]
    fn normalizes_boss_chat_redirect_urls() {
        assert_eq!(
            absolute_boss_url("/web/geek/chat?id=abc"),
            Some("https://www.zhipin.com/web/geek/chat?id=abc".to_string())
        );
        assert_eq!(
            absolute_boss_url("https://www.zhipin.com/web/geek/chat?id=abc"),
            Some("https://www.zhipin.com/web/geek/chat?id=abc".to_string())
        );
        assert_eq!(absolute_boss_url("javascript:;"), None);
        assert_eq!(absolute_boss_url("  "), None);
    }

    #[test]
    fn generation_failure_with_no_explicit_fallback_never_calls_send() {
        let calls = std::cell::Cell::new(0);
        let sent = send_if_any(Vec::new(), |_| {
            calls.set(calls.get() + 1);
            Ok(true)
        })
        .unwrap();

        assert!(!sent);
        assert_eq!(calls.get(), 0);
    }
}
