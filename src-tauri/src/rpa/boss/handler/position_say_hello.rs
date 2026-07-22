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
use rust_drission::{utils::sleep_random_ms, ChromiumPage, DataPacket};
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
            let local_job_ids: HashSet<String> = job_detail_dao::list()
                .unwrap_or_default()
                .into_iter()
                .map(|j| j.id)
                .collect();
            logger::info(format!("本地已存储 {} 条岗位记录", local_job_ids.len()))?;

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
                    if job_card_area_ele.attr("class")?.contains("is-seen") {
                        continue;
                    }

                    let job_card_ele = job_card_area_ele.element(".job-card-box")?.unwrap();

                    let job_href = job_card_ele
                        .element(".job-name")?
                        .unwrap()
                        .attr("href")
                        .unwrap_or_default();

                    // 基于本地存储去重
                    let job_id = extract_job_id(&format!("https://www.zhipin.com{}", job_href))
                        .map(|s| s.to_string());
                    if let Some(ref id) = job_id {
                        if local_job_ids.contains(id.as_str()) {
                            continue;
                        }
                    }

                    job_card_ele.click()?;
                    sleep_random_ms(800, 1200);
                    // 岗位详情
                    let job_detail_text = page.ele(".job-detail-body")?.unwrap().text_content()?;
                    // 标题
                    let job_name = job_card_ele.element(".job-name")?.unwrap().text_content()?;
                    // 薪资水平
                    let salary_text = job_card_ele
                        .element(".job-salary")?
                        .unwrap()
                        .text_content()?;
                    // 公司名称
                    let company_text = job_card_ele
                        .element(".boss-name")?
                        .unwrap()
                        .text_content()?;

                    let company_location = job_card_ele
                        .element(".company-location")?
                        .unwrap()
                        .text_content()?;

                    let job_detail_url = format!("https://www.zhipin.com{}", job_href);
                    let platform_job_id = extract_job_id(&job_detail_url)
                        .unwrap_or(&job_detail_url)
                        .to_string();
                    let greet_job = GreetJob {
                        platform: PlatformKind::Boss,
                        platform_job_id,
                        title: job_name.clone(),
                        company_name: company_text,
                        detail: job_detail_text,
                        salary: decode_salary(&salary_text),
                        location: Some(company_location),
                        detail_url: job_detail_url.clone(),
                    };

                    logger::info(format!(
                        "当前处理岗位:{} 公司:{}",
                        greet_job.title, greet_job.company_name
                    ))?;

                    if !verify::filter_verify(&greet_job, &app_runtime_config) {
                        logger::info("岗位不匹配，跳过")?;
                        continue;
                    }
                    match handle_greet(
                        page,
                        job_detail_url,
                        greet_job.clone(),
                        app_runtime_config.clone(),
                    )
                    .await
                    {
                        Ok(()) => {}
                        Err(error) => {
                            logger::warning(greet_failure_message(
                                &greet_job.title,
                                &greet_job.company_name,
                                &error,
                            ))?;
                            return Ok(());
                        }
                    }
                    if is_job_task_stop_requested() {
                        logger::info("求职任务已结束")?;
                        return Ok(());
                    }

                    logger::info(format!("{} 初次沟通成功", job_name))?;
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
        "岗位打招呼失败：{} - {}，错误：{}。停止投递",
        title, company_name, error
    )
}

async fn handle_greet(
    browser_page: &rust_drission::ChromiumPage,
    job_detail_url: String,
    greet_job: GreetJob,
    config: AppRuntimeConfig,
) -> Result<(), anyhow::Error> {
    let page = browser_page.new_tab(None)?;
    let result = async {
        if is_job_task_stop_requested() {
            logger::info("求职任务已结束")?;
            return Ok(());
        }

        page.get(&job_detail_url)?;
        page.wait(".btn.btn-startchat", Duration::from_secs(30))?;
        if is_job_task_stop_requested() {
            logger::info("求职任务已结束")?;
            return Ok(());
        }

        let btn = page
            .ele(".btn.btn-startchat")?
            .ok_or_else(|| anyhow::anyhow!("未找到沟通按钮"))?;
        let redirect_url = btn.attr("redirect-url").unwrap_or_default();
        let data_url = btn.attr("data-url").unwrap_or_default();
        let data_isfriend = btn
            .attr("data-isfriend")
            .map(|v| v == "true")
            .unwrap_or(false);
        if data_isfriend {
            logger::info("岗位已打过招呼 跳过")?;
            return Ok(());
        }

        // 发起打招呼 POST 请求
        let post_url = format!("https://www.zhipin.com{}", data_url);
        let script = build_start_chat_request_script(&post_url);
        page.run_js_await(&script)?;
        if is_job_task_stop_requested() {
            logger::info("求职任务已结束")?;
            return Ok(());
        }
        sleep_random_ms(500, 1000);

        // 跳转到聊天页面
        page.get(&format!("https://zhipin.com{}", redirect_url))?;
        if is_job_task_stop_requested() {
            logger::info("求职任务已结束")?;
            return Ok(());
        }

        // 构建回复资源：优先 LLM 生成，否则使用默认模板
        let resources = build_greet_resources(&config, &greet_job).await?;
        send_if_any(resources, |resources| send_messages(&page, resources))?;

        let job_id = extract_job_id(&job_detail_url).unwrap();

        // 插入一条 job_detail 的记录
        save_job_detail(job_id, &greet_job);

        sleep_random_ms(1200, 2000);

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

fn build_start_chat_request_script(post_url: &str) -> String {
    format!(
        r#"
        (async () => {{
            const response = await fetch({:?}, {{
                method: 'POST',
                headers: {{
                    'Content-Type': 'application/x-www-form-urlencoded',
                    'Accept': 'application/json, text/plain, */*'
                }}
            }});
            return await response.json();
        }})()
        "#,
        post_url
    )
}

// 滚动到底部
pub fn scroll_bottom(page: &ChromiumPage) -> Result<(), anyhow::Error> {
    let script: &str = r#"
    (() => {
    const html = document.documentElement;
    const body = document.body;
    const scrollContainer = html.scrollHeight > html.clientHeight ? html : body;
    scrollContainer.scrollTop = scrollContainer.scrollHeight;
})();
    "#;

    page.run_js_await(script)?;

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
    fn formats_greet_failure_message_with_job_context_and_stop_hint() {
        let error = anyhow::anyhow!("发送按钮不可用");

        let message = greet_failure_message("后端工程师", "示例科技", &error);

        assert!(message.contains("后端工程师"));
        assert!(message.contains("示例科技"));
        assert!(message.contains("发送按钮不可用"));
        assert!(message.contains("停止投递"));
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
