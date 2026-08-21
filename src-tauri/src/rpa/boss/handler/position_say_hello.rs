use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::{
    auto_analysis, browser,
    config::{
        AnalysisTrigger, AppRuntimeConfig, BossFilterConfig, BossRecruiterActiveThreshold,
        JobFilterConfig, ReplyResource,
    },
    dao::{job_detail_dao, model::JobDetail},
    logger,
    rpa::{
        boss::{handler::send_messages, model::GreetJob},
        conversation::SendVerdict,
        greet::build_greet_resources,
        human_input,
        human_pace::GreetPacer,
        run_flow::is_job_task_stop_requested,
        run_flow::PlatformKind,
        schedule::{BudgetVerdict, RoundBudget},
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
    budget: RoundBudget,
) -> Result<(), anyhow::Error> {
    let app_runtime_config = app_runtime_config.clone();
    browser::with_browser(|connection| {
        Box::pin(async move {
            position_say_hello_on_page(connection, connection.tab(), &app_runtime_config, budget)
                .await
        })
    })
    .await
}

/// Run the BOSS job-hunting flow on the task-owned main tab.
pub async fn position_say_hello_on_page(
    connection: &ChromiumPage,
    page: &Page,
    app_runtime_config: &AppRuntimeConfig,
    budget: RoundBudget,
) -> Result<(), anyhow::Error> {
    let app_runtime_config = app_runtime_config.clone();
    let round_started = Instant::now();
    let search_url = build_job_search_url(&app_runtime_config.job_filter_config);
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
    if let Err(error) = page.wait(".rec-job-list", JOB_LIST_WAIT_TIMEOUT) {
        // 直接把 CDP 超时抛给用户没有任何可操作性，这里补齐当前页面上下文再给中文提示
        let current_url = page.url().unwrap_or_else(|_| "未知 URL".to_string());
        let page_text = page_text_snippet(page);
        return Err(anyhow::anyhow!(job_list_wait_error(
            &current_url,
            &page_text,
            &error.to_string(),
        )));
    }

    // 消费首次 page.get 触发的 joblist 响应，初始化 seen_job_ids
    let mut seen_job_ids: HashSet<String> = HashSet::new();
    if let Ok(Some(packet)) = joblist_listener.wait(Duration::from_secs(10)) {
        let (first_ids, _) = parse_joblist_response(&packet);
        seen_job_ids.extend(first_ids);
    }
    let mut no_new_count = 0u32;
    let mut stats = RoundStats::default();
    // 休息节奏从用户设的单轮上限派生。拟人化关着时它是个空壳，
    // 停顿仍是改造前那段 3-5 秒
    let mut pacer = GreetPacer::new(&app_runtime_config.humanize_config, budget.max_greets);
    // 撞上平台每日沟通上限之后打招呼会一条接一条地失败，而列表还能一直往下滚。
    // 这个计数是那种「零产出却停不下来」的唯一出口
    let mut consecutive_greet_failures = 0u32;

    let stop_reason = 'outer: loop {
        if is_job_task_stop_requested() {
            break 'outer StopReason::UserStopped;
        }
        if let Some(reason) =
            budget_stop_reason(&budget, &stats, round_started, consecutive_greet_failures)
        {
            break 'outer reason;
        }

        let jeb_card_area_eles = page.eles(".card-area")?;
        logger::info(format!("页面加载到{}条岗位卡片", jeb_card_area_eles.len()))?;
        if jeb_card_area_eles.is_empty() {
            break 'outer StopReason::EmptyList;
        }

        // 记录进入本页时的累计值，页末据此算出“本页”增量
        let round_base = stats.clone();

        for job_card_area_ele in jeb_card_area_eles {
            if is_job_task_stop_requested() {
                break 'outer StopReason::UserStopped;
            }
            if let Some(reason) =
                budget_stop_reason(&budget, &stats, round_started, consecutive_greet_failures)
            {
                break 'outer reason;
            }
            stats.scanned += 1;
            let greet_job =
                match read_job_card(page, &job_card_area_ele, &processed_job_ids, &mut pacer) {
                Ok(CardOutcome::Ready(greet_job)) => *greet_job,
                Ok(CardOutcome::Skipped(reason)) => {
                    stats.record_skip(reason);
                    // 全静默会让用户以为程序卡死，这里按批次给出进度提示
                    let skipped = stats.skipped_known() - round_base.skipped_known();
                    if should_log_skip_progress(skipped) {
                        logger::info(format!("已跳过 {} 条已浏览/已投递岗位，继续扫描", skipped))?;
                    }
                    continue;
                }
                Err(error) => {
                    stats.read_failed += 1;
                    logger::warning(card_failure_message(&error))?;
                    continue;
                }
            };

            logger::info(format!(
                "当前处理岗位:{} 公司:{}",
                greet_job.title, greet_job.company_name
            ))?;

            if let Some(reason) = inactive_recruiter_skip_reason(
                &greet_job.recruiter_active_time,
                &app_runtime_config.platform_filter_config.boss,
            ) {
                stats.skipped_inactive += 1;
                logger::info(format!("岗位招聘者活跃度不满足要求，跳过：{reason}"))?;
                continue;
            }

            let filter_decision = verify::filter_decision(&greet_job, &app_runtime_config);
            if !filter_decision.matched {
                stats.skipped_rule += 1;
                logger::info(format!("岗位不匹配，跳过：{}", filter_decision.reason))?;
                continue;
            }
            if app_runtime_config.job_filter_config.enable_semantic_filter {
                match crate::llm::evaluate_job_match(&app_runtime_config, &greet_job).await {
                    Ok(decision) if decision.matched => logger::info(format!(
                        "AI 岗位复核通过（{}分）：{}",
                        decision.score, decision.reason
                    ))?,
                    Ok(decision) => {
                        stats.skipped_ai += 1;
                        logger::info(format!(
                            "AI 岗位复核未通过，跳过（{}分）：{}",
                            decision.score, decision.reason
                        ))?;
                        continue;
                    }
                    Err(error) => {
                        stats.skipped_ai += 1;
                        logger::warning(format!("AI 岗位复核失败，为避免误投已跳过：{}", error))?;
                        continue;
                    }
                }
            }
            // 「筛选通过即分析」不关心后面招呼发没发出去，所以在进入打招呼之前就登记
            auto_analysis::schedule(
                &build_job_detail(&greet_job.platform_job_id, &greet_job, &app_runtime_config),
                AnalysisTrigger::FilterPassed,
                &app_runtime_config,
            );
            match handle_greet(connection, greet_job.clone(), app_runtime_config.clone()).await {
                Ok(()) => consecutive_greet_failures = 0,
                Err(error) => {
                    stats.greet_failed += 1;
                    consecutive_greet_failures += 1;
                    logger::warning(greet_failure_message(
                        &greet_job.title,
                        &greet_job.company_name,
                        &error,
                    ))?;
                    if should_continue_after_greet_failure() {
                        continue;
                    }
                    // 目前 should_continue_after_greet_failure 恒为 true，
                    // 这里仅作为“单次失败即终止”策略的兜底出口
                    break 'outer StopReason::GreetFailureAborted;
                }
            }
            if is_job_task_stop_requested() {
                break 'outer StopReason::UserStopped;
            }

            stats.greet_success += 1;
            logger::info(format!("{} 初次沟通成功", greet_job.title))?;
            processed_job_ids.insert(greet_job.platform_job_id.clone());
            // 停顿、以及连投若干条之后的休息都在这里面。收到停止请求时立即收尾，
            // 不能让用户等完一段十几分钟的休息
            if !pacer.after_greet(true).await {
                break 'outer StopReason::UserStopped;
            }
        }

        // 本页处理结果汇总：一条都没进入打招呼流程时给出聚合提示，避免界面长时间无输出
        let round = stats.since(&round_base);
        if round.scanned > 0 {
            if round.engaged() == 0 {
                logger::info(format!(
                    "本页 {} 条岗位均已处理过或不匹配，继续向下加载",
                    round.scanned
                ))?;
            } else {
                logger::info(format!("本页处理完成：{}", round.summary()))?;
            }
        }

        // 检查是否已触底
        if page.ele(".loading-wait")?.is_none() {
            break 'outer StopReason::ReachedBottom;
        }

        // 设置监听 → 滚动 → 等待 joblist API 响应，检测是否有新岗位
        let joblist_listener = page.listen_url("wapi/zpgeek/search/joblist.json")?;
        let scroll_probe = scroll_bottom_probe(page)?;

        match joblist_listener.wait(Duration::from_secs(10)) {
            Ok(Some(packet)) => {
                let (api_ids, has_more) = parse_joblist_response(&packet);

                if !has_more {
                    break 'outer StopReason::NoMoreFromApi;
                }

                let new_ids: HashSet<String> = api_ids
                    .into_iter()
                    .filter(|id| !seen_job_ids.contains(id))
                    .collect();

                if new_ids.is_empty() {
                    no_new_count += 1;
                    if no_new_count >= MAX_NO_NEW_RETRY {
                        break 'outer StopReason::NoNewJobs;
                    }
                    logger::info(format!(
                        "第 {}/{} 次滚动未加载到新岗位，重试中",
                        no_new_count, MAX_NO_NEW_RETRY
                    ))?;
                    continue;
                }

                no_new_count = 0;
                seen_job_ids.extend(new_ids);
            }
            _ => {
                no_new_count += 1;
                if no_new_count >= MAX_NO_NEW_RETRY {
                    break 'outer StopReason::ApiTimeout;
                }
                logger::info(format!(
                    "第 {}/{} 次等待岗位接口响应超时，重试中（{}）",
                    no_new_count,
                    MAX_NO_NEW_RETRY,
                    scroll_probe.describe()
                ))?;
                continue;
            }
        }
    };

    logger::info(format!(
        "本轮求职任务结束：{}。{}",
        stop_reason.describe(),
        stats.total_summary()
    ))?;
    if let Some(summary) = pacer.summary() {
        logger::info(summary)?;
    }

    Ok(())
}

/// 岗位列表首屏等待超时时间。BOSS 首屏偶尔要十几秒才渲染完，5 秒过短会误判为失败。
const JOB_LIST_WAIT_TIMEOUT: Duration = Duration::from_secs(20);
/// 连续「无新岗位 / 接口超时」多少次后结束本轮
const MAX_NO_NEW_RETRY: u32 = 3;
/// 每跳过多少条已处理岗位打一次进度日志
const SKIP_PROGRESS_STEP: u32 = 10;

/// 岗位卡片被跳过的原因，用于区分统计
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkipReason {
    /// 卡片带 is-seen 类，页面上已浏览过
    AlreadyViewed,
    /// 岗位 ID 已存在于本地库，之前已投递
    AlreadyProcessed,
    /// 拟人化随机跳过：扫一眼标题就划过去了，没点开
    Humanized,
}

/// 读取一张岗位卡片的结果
#[derive(Debug)]
enum CardOutcome {
    Skipped(SkipReason),
    Ready(Box<GreetJob>),
}

/// 本轮（或本页）岗位处理统计
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RoundStats {
    /// 扫描到的岗位卡片数
    scanned: u32,
    /// 因页面已浏览而跳过
    skipped_viewed: u32,
    /// 因本地库已存在而跳过
    skipped_processed: u32,
    /// 因确定性规则不匹配而跳过
    skipped_rule: u32,
    /// 因 AI 语义复核未通过或失败而跳过
    skipped_ai: u32,
    /// 因招聘者活跃度过低或无法识别而跳过
    skipped_inactive: u32,
    /// 被拟人化随机跳过（「只看不投」）
    skipped_humanize: u32,
    /// 卡片读取失败
    read_failed: u32,
    /// 打招呼成功
    greet_success: u32,
    /// 打招呼失败
    greet_failed: u32,
}

impl RoundStats {
    fn record_skip(&mut self, reason: SkipReason) {
        match reason {
            SkipReason::AlreadyViewed => self.skipped_viewed += 1,
            SkipReason::AlreadyProcessed => self.skipped_processed += 1,
            SkipReason::Humanized => self.skipped_humanize += 1,
        }
    }

    /// 已浏览 + 已投递的跳过总数
    fn skipped_known(&self) -> u32 {
        self.skipped_viewed + self.skipped_processed
    }

    /// 真正进入过打招呼流程的岗位数
    fn engaged(&self) -> u32 {
        self.greet_success + self.greet_failed
    }

    /// 相对基准快照的增量，用于从累计值中还原出「本页」数据
    fn since(&self, base: &Self) -> Self {
        Self {
            scanned: self.scanned.saturating_sub(base.scanned),
            skipped_viewed: self.skipped_viewed.saturating_sub(base.skipped_viewed),
            skipped_processed: self
                .skipped_processed
                .saturating_sub(base.skipped_processed),
            skipped_rule: self.skipped_rule.saturating_sub(base.skipped_rule),
            skipped_ai: self.skipped_ai.saturating_sub(base.skipped_ai),
            skipped_inactive: self.skipped_inactive.saturating_sub(base.skipped_inactive),
            skipped_humanize: self.skipped_humanize.saturating_sub(base.skipped_humanize),
            read_failed: self.read_failed.saturating_sub(base.read_failed),
            greet_success: self.greet_success.saturating_sub(base.greet_success),
            greet_failed: self.greet_failed.saturating_sub(base.greet_failed),
        }
    }

    fn summary(&self) -> String {
        format!(
            "共扫描 {} 条岗位，已浏览跳过 {} 条，已投递跳过 {} 条，活跃度跳过 {} 条，规则过滤跳过 {} 条，AI 复核跳过 {} 条，拟人化跳过 {} 条，读取失败 {} 条，打招呼成功 {} 条，打招呼失败 {} 条",
            self.scanned,
            self.skipped_viewed,
            self.skipped_processed,
            self.skipped_inactive,
            self.skipped_rule,
            self.skipped_ai,
            self.skipped_humanize,
            self.read_failed,
            self.greet_success,
            self.greet_failed,
        )
    }

    /// 整轮结束时的累计摘要。每次滚动加载后外层都会把页面上的全部卡片重新扫一遍，
    /// 因此 scanned 与已浏览/已投递跳过数含重复扫描，措辞上必须与「本页」明确区分，
    /// 否则「共扫描 90 条岗位」会被误读成真的看过 90 个不同岗位。
    fn total_summary(&self) -> String {
        format!(
            "打招呼成功 {} 条，失败 {} 条；活跃度跳过 {} 条，规则过滤跳过 {} 条，AI 复核跳过 {} 条，拟人化跳过 {} 条，读取失败 {} 条；累计扫描岗位卡片 {} 次（含滚动后的重复扫描），其中因已浏览或已投递而跳过 {} 次",
            self.greet_success,
            self.greet_failed,
            self.skipped_inactive,
            self.skipped_rule,
            self.skipped_ai,
            self.skipped_humanize,
            self.read_failed,
            self.scanned,
            self.skipped_known(),
        )
    }
}

/// 本轮任务的结束原因
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopReason {
    /// 岗位列表已触底
    ReachedBottom,
    /// 接口明确返回无更多岗位
    NoMoreFromApi,
    /// 连续多次滚动都没有加载出新岗位
    NoNewJobs,
    /// 连续多次等待岗位接口响应超时
    ApiTimeout,
    /// 用户主动停止
    UserStopped,
    /// 页面上没有任何岗位卡片
    EmptyList,
    /// 打招呼失败且策略要求立即终止
    GreetFailureAborted,
    /// 本轮打招呼条数达到设定上限
    GreetQuotaReached,
    /// 本轮耗时达到设定上限
    TimeLimitReached,
    /// 连续多次打招呼失败，本轮熔断
    TooManyGreetFailures,
}

impl StopReason {
    fn describe(&self) -> &'static str {
        match self {
            StopReason::ReachedBottom => "岗位列表已触底，该搜索条件下的岗位已全部浏览完毕",
            StopReason::NoMoreFromApi => "接口返回无更多岗位，该搜索条件下的岗位已全部浏览完毕",
            StopReason::GreetQuotaReached => {
                "本轮打招呼条数已达设定上限，提前结束本轮；周期投递会在下一轮继续"
            }
            StopReason::TimeLimitReached => {
                "本轮运行时长已达设定上限，提前结束本轮；周期投递会在下一轮继续"
            }
            StopReason::TooManyGreetFailures => {
                "连续多次打招呼失败，可能已达平台每日沟通上限或触发了安全验证，本轮提前结束"
            }
            StopReason::NoNewJobs => {
                "连续多次滚动均未加载到新岗位，判定已到列表末尾，这是正常结束、不是程序崩溃；可更换岗位关键词/城市，或改用周期投递持续跟进"
            }
            StopReason::ApiTimeout => {
                "连续多次等待岗位接口响应超时，已停止继续加载，这是正常结束、不是程序崩溃；可更换岗位关键词/城市，或改用周期投递持续跟进"
            }
            StopReason::UserStopped => "用户已手动停止求职任务",
            StopReason::EmptyList => "当前筛选条件下暂无岗位列表，建议放宽筛选条件后重试",
            StopReason::GreetFailureAborted => "打招呼失败已终止本轮任务",
        }
    }
}

/// 把预算判定翻译成本轮的结束原因。预算还够时返回 None
fn budget_stop_reason(
    budget: &RoundBudget,
    stats: &RoundStats,
    round_started: Instant,
    consecutive_greet_failures: u32,
) -> Option<StopReason> {
    match budget.check(
        stats.greet_success,
        round_started.elapsed(),
        consecutive_greet_failures,
    ) {
        BudgetVerdict::Continue => None,
        BudgetVerdict::GreetLimit => Some(StopReason::GreetQuotaReached),
        BudgetVerdict::TimeLimit => Some(StopReason::TimeLimitReached),
        BudgetVerdict::FailureLimit => Some(StopReason::TooManyGreetFailures),
    }
}

/// 跳过计数达到步长时才打进度日志，避免刷屏
fn should_log_skip_progress(skipped_in_round: u32) -> bool {
    skipped_in_round > 0 && skipped_in_round.is_multiple_of(SKIP_PROGRESS_STEP)
}

/// 判断岗位列表加载超时是否疑似由 BOSS 安全验证 / 登录失效引起
fn looks_like_boss_guard(url: &str, page_text: &str) -> bool {
    const URL_HINTS: [&str; 5] = ["safe", "verify", "captcha", "login", "sign-in"];
    const TEXT_HINTS: [&str; 8] = [
        "安全问题",
        "安全验证",
        "验证码",
        "请完成验证",
        "滑块",
        "请登录",
        "登录已失效",
        "重新登录",
    ];

    let lower_url = url.to_lowercase();
    URL_HINTS.iter().any(|hint| lower_url.contains(hint))
        || TEXT_HINTS.iter().any(|hint| page_text.contains(hint))
}

/// 岗位列表等待超时时给用户的中文提示
fn job_list_wait_error(url: &str, page_text: &str, source: &str) -> String {
    if looks_like_boss_guard(url, page_text) {
        format!(
            "岗位列表加载超时，可能触发了 BOSS 安全验证或登录已失效，请点击『检查BOSS环境』重新登录后重试（当前页面：{url}，原始错误：{source}）"
        )
    } else {
        format!(
            "岗位列表加载超时，请检查网络后重试；若持续失败，请点击『检查BOSS环境』确认登录状态（当前页面：{url}，原始错误：{source}）"
        )
    }
}

/// 抓取页面可见文本片段，仅用于超时诊断，失败时返回空串
fn page_text_snippet(page: &Page) -> String {
    page.ele("body")
        .ok()
        .flatten()
        .and_then(|element| element.text_content().ok())
        .map(|text| {
            let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
            normalized.chars().take(300).collect::<String>()
        })
        .unwrap_or_default()
}

fn card_failure_message(error: &anyhow::Error) -> String {
    format!("处理岗位卡片失败，跳过当前岗位，继续处理：{error}")
}

fn read_job_card(
    page: &Page,
    job_card_area_ele: &Element,
    processed_job_ids: &HashSet<String>,
    pacer: &mut GreetPacer,
) -> Result<CardOutcome, anyhow::Error> {
    if job_card_area_ele.attr("class")?.contains("is-seen") {
        return Ok(CardOutcome::Skipped(SkipReason::AlreadyViewed));
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
            return Ok(CardOutcome::Skipped(SkipReason::AlreadyProcessed));
        }
    }

    // 「每个岗位都点开看」是人做不到的事：真人扫列表时就会凭标题划过去一些。
    //
    // 这一判断必须赶在点击之前。BOSS 会给点开过的卡片打上 is-seen，下一轮扫描
    // 直接跳过——放在点开之后跳过，等于把这个岗位永久放弃掉，而拟人化的本意
    // 只是「这次先不投」
    if pacer.should_skim() {
        return Ok(CardOutcome::Skipped(SkipReason::Humanized));
    }

    human_input::click(page, &job_card_ele)?;
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
    let recruiter_active_time = read_recruiter_active_time(page, &job_card_ele)?;

    let job_detail_url = format!("https://www.zhipin.com{}", job_href);
    let platform_job_id = extract_job_id(&job_detail_url)
        .unwrap_or(&job_detail_url)
        .to_string();

    Ok(CardOutcome::Ready(Box::new(GreetJob {
        platform: PlatformKind::Boss,
        platform_job_id,
        title: job_name,
        company_name: company_text,
        detail: job_detail_text,
        salary: decode_salary(&salary_text),
        location: Some(company_location),
        recruiter_active_time,
        detail_url: job_detail_url,
    })))
}

fn read_recruiter_active_time(
    page: &Page,
    job_card_ele: &Element,
) -> Result<Option<String>, anyhow::Error> {
    for selector in [".boss-online-tag", ".boss-active-time"] {
        if let Some(text) = job_card_ele
            .element(selector)?
            .and_then(|element| element.text_content().ok())
            .and_then(|text| extract_recruiter_active_text(&text))
        {
            return Ok(Some(text));
        }
    }

    for selector in [
        ".job-boss-info .boss-online-tag",
        ".job-boss-info .boss-active-time",
        ".boss-online-tag",
        ".boss-active-time",
    ] {
        if let Some(text) = page
            .ele(selector)?
            .and_then(|element| element.text_content().ok())
            .and_then(|text| extract_recruiter_active_text(&text))
        {
            return Ok(Some(text));
        }
    }

    for selector in [
        ".boss-info",
        ".boss-name",
        ".job-boss-info",
        ".job-detail-op",
        ".job-detail-body",
        ".job-detail-box",
        ".job-detail",
    ] {
        if let Some(text) = match selector {
            ".boss-info" | ".boss-name" => job_card_ele
                .element(selector)?
                .and_then(|element| element.text_content().ok()),
            _ => page
                .ele(selector)?
                .and_then(|element| element.text_content().ok()),
        }
        .and_then(|text| extract_recruiter_active_text(&text))
        {
            return Ok(Some(text));
        }
    }

    let combined = format!(
        "{} {}",
        job_card_ele.text_content().unwrap_or_default(),
        page_text_snippet(page)
    );
    Ok(extract_recruiter_active_text(&combined))
}

fn extract_recruiter_active_text(text: &str) -> Option<String> {
    const PATTERNS: &[&str] = &[
        "当前在线",
        "在线",
        "刚刚活跃",
        "今日活跃",
        "今天活跃",
        "3日内活跃",
        "三日内活跃",
        "3天内活跃",
        "三天内活跃",
        "本周活跃",
        "近一周活跃",
        "7日内活跃",
        "7天内活跃",
        "一周内活跃",
        "本月活跃",
        "2周内活跃",
        "两周内活跃",
        "二周内活跃",
        "近半月活跃",
        "半月内活跃",
        "15日内活跃",
        "十五日内活跃",
        "15天内活跃",
        "十五天内活跃",
        "一个月内活跃",
        "1个月内活跃",
        "30日内活跃",
        "30天内活跃",
        "2月内活跃",
        "2个月内活跃",
        "3月内活跃",
        "3个月内活跃",
        "4月内活跃",
        "4个月内活跃",
        "5月内活跃",
        "5个月内活跃",
        "近半年活跃",
        "半年内活跃",
        "半月前活跃",
        "一个月前活跃",
        "1个月前活跃",
        "两个月前活跃",
        "2个月前活跃",
        "半年前活跃",
    ];
    if let Some(pattern) = PATTERNS.iter().find(|pattern| text.contains(**pattern)) {
        return Some((*pattern).to_string());
    }

    let normalized = text.split_whitespace().collect::<Vec<_>>().join("");
    let Ok(regex) = regex::Regex::new(
        r"([0-9一二两三四五六七八九十半个]+)(分钟|小时|天|日|周|星期|个月|月|年)(?:前|内)活跃",
    ) else {
        return None;
    };
    regex
        .captures(&normalized)
        .and_then(|captures| captures.get(0).map(|m| m.as_str().to_string()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum RecruiterActiveRank {
    Online,
    JustActive,
    ThreeDays,
    ThisWeek,
    TwoWeeks,
    ThisMonth,
    TwoMonths,
    ThreeMonths,
    FourMonths,
    FiveMonths,
    HalfYear,
    Older,
}

fn active_threshold_rank(threshold: BossRecruiterActiveThreshold) -> Option<RecruiterActiveRank> {
    match threshold {
        BossRecruiterActiveThreshold::Online => Some(RecruiterActiveRank::Online),
        BossRecruiterActiveThreshold::JustActive => Some(RecruiterActiveRank::JustActive),
        BossRecruiterActiveThreshold::ThreeDays => Some(RecruiterActiveRank::ThreeDays),
        BossRecruiterActiveThreshold::ThisWeek
        | BossRecruiterActiveThreshold::SevenDays
        | BossRecruiterActiveThreshold::SevenDaysText => Some(RecruiterActiveRank::ThisWeek),
        BossRecruiterActiveThreshold::TwoWeeks => Some(RecruiterActiveRank::TwoWeeks),
        BossRecruiterActiveThreshold::ThisMonth => Some(RecruiterActiveRank::ThisMonth),
        BossRecruiterActiveThreshold::TwoMonths => Some(RecruiterActiveRank::TwoMonths),
        BossRecruiterActiveThreshold::ThreeMonths => Some(RecruiterActiveRank::ThreeMonths),
        BossRecruiterActiveThreshold::FourMonths => Some(RecruiterActiveRank::FourMonths),
        BossRecruiterActiveThreshold::FiveMonths => Some(RecruiterActiveRank::FiveMonths),
        BossRecruiterActiveThreshold::HalfYear | BossRecruiterActiveThreshold::HalfYearAgo => {
            Some(RecruiterActiveRank::HalfYear)
        }
        BossRecruiterActiveThreshold::Disabled => None,
    }
}

fn classify_recruiter_active_text(text: &str) -> RecruiterActiveRank {
    if text.contains("在线") {
        return RecruiterActiveRank::Online;
    }
    if text.contains("刚刚活跃") || text.contains("今日活跃") || text.contains("今天活跃")
    {
        return RecruiterActiveRank::JustActive;
    }
    if text.contains("3日内") || text.contains("三日内") {
        return RecruiterActiveRank::ThreeDays;
    }
    if text.contains("3天内") || text.contains("三天内") {
        return RecruiterActiveRank::ThreeDays;
    }
    if text.contains("本周")
        || text.contains("近一周")
        || text.contains("7日内")
        || text.contains("7天内")
        || text.contains("一周内")
    {
        return RecruiterActiveRank::ThisWeek;
    }
    if text.contains("2周内") || text.contains("两周内") || text.contains("二周内") {
        return RecruiterActiveRank::TwoWeeks;
    }
    if text.contains("本月")
        || text.contains("半月内")
        || text.contains("近半月")
        || text.contains("15日内")
        || text.contains("十五日内")
        || text.contains("15天内")
        || text.contains("十五天内")
        || text.contains("一个月内")
        || text.contains("1个月内")
        || text.contains("30日内")
        || text.contains("30天内")
    {
        return RecruiterActiveRank::ThisMonth;
    }
    if text.contains("2月内") || text.contains("2个月内") || text.contains("两月内") {
        return RecruiterActiveRank::TwoMonths;
    }
    if text.contains("3月内") || text.contains("3个月内") || text.contains("三月内") {
        return RecruiterActiveRank::ThreeMonths;
    }
    if text.contains("4月内") || text.contains("4个月内") || text.contains("四月内") {
        return RecruiterActiveRank::FourMonths;
    }
    if text.contains("5月内") || text.contains("5个月内") || text.contains("五月内") {
        return RecruiterActiveRank::FiveMonths;
    }
    if text.contains("近半年") || text.contains("半年内") || text.contains("半年前") {
        return RecruiterActiveRank::HalfYear;
    }

    let normalized = text.split_whitespace().collect::<Vec<_>>().join("");
    if normalized.contains("分钟前") || normalized.contains("小时前") {
        return RecruiterActiveRank::JustActive;
    }
    if let Some(rank) = classify_relative_active_text(&normalized) {
        return rank;
    }
    if normalized.contains("天前") || normalized.contains("日前") {
        let days = parse_chinese_or_ascii_number(
            normalized
                .split_once('天')
                .or_else(|| normalized.split_once('日'))
                .map(|(value, _)| value)
                .unwrap_or_default(),
        )
        .unwrap_or(31);
        return if days <= 3 {
            RecruiterActiveRank::ThreeDays
        } else if days < 7 {
            RecruiterActiveRank::ThisWeek
        } else if days <= 31 {
            RecruiterActiveRank::ThisMonth
        } else {
            RecruiterActiveRank::Older
        };
    }
    if normalized.contains("周前") || normalized.contains("星期前") {
        return RecruiterActiveRank::Older;
    }
    if normalized.contains("半月前")
        || normalized.contains("个月前")
        || normalized.contains("月前")
        || normalized.contains("年前")
    {
        return RecruiterActiveRank::Older;
    }

    RecruiterActiveRank::Older
}

fn classify_relative_active_text(text: &str) -> Option<RecruiterActiveRank> {
    let regex = regex::Regex::new(
        r"([0-9一二两三四五六七八九十半个]+)(分钟|小时|天|日|周|星期|个月|月|年)(前|内)活跃",
    )
    .ok()?;
    let captures = regex.captures(text)?;
    let amount_text = captures.get(1).map(|m| m.as_str()).unwrap_or_default();
    let unit = captures.get(2).map(|m| m.as_str()).unwrap_or_default();
    let direction = captures.get(3).map(|m| m.as_str()).unwrap_or_default();
    let amount = parse_chinese_or_ascii_number(amount_text).unwrap_or_else(|| {
        if amount_text.contains('半') {
            1
        } else {
            u32::MAX
        }
    });

    match (unit, direction) {
        ("分钟" | "小时", _) => Some(RecruiterActiveRank::JustActive),
        ("天" | "日", "内") if amount <= 3 => Some(RecruiterActiveRank::ThreeDays),
        ("天" | "日", "内") if amount <= 7 => Some(RecruiterActiveRank::ThisWeek),
        ("天" | "日", "内") if amount <= 14 => Some(RecruiterActiveRank::TwoWeeks),
        ("天" | "日", "内") if amount <= 31 => Some(RecruiterActiveRank::ThisMonth),
        ("天" | "日", "前") if amount <= 3 => Some(RecruiterActiveRank::ThreeDays),
        ("天" | "日", "前") if amount < 7 => Some(RecruiterActiveRank::ThisWeek),
        ("天" | "日", "前") if amount <= 31 => Some(RecruiterActiveRank::ThisMonth),
        ("周" | "星期", "内") if amount <= 1 => Some(RecruiterActiveRank::ThisWeek),
        ("周" | "星期", "内") if amount <= 2 => Some(RecruiterActiveRank::TwoWeeks),
        ("周" | "星期", "内") if amount <= 4 => Some(RecruiterActiveRank::ThisMonth),
        ("个月" | "月", "内") if amount <= 1 => Some(RecruiterActiveRank::ThisMonth),
        ("个月" | "月", "内") if amount <= 2 => Some(RecruiterActiveRank::TwoMonths),
        ("个月" | "月", "内") if amount <= 3 => Some(RecruiterActiveRank::ThreeMonths),
        ("个月" | "月", "内") if amount <= 4 => Some(RecruiterActiveRank::FourMonths),
        ("个月" | "月", "内") if amount <= 5 => Some(RecruiterActiveRank::FiveMonths),
        ("个月" | "月", "内") if amount <= 6 => Some(RecruiterActiveRank::HalfYear),
        ("年", "内") if amount_text.contains('半') => Some(RecruiterActiveRank::HalfYear),
        ("年", "前") if amount_text.contains('半') => Some(RecruiterActiveRank::HalfYear),
        _ => Some(RecruiterActiveRank::Older),
    }
}

fn parse_chinese_or_ascii_number(value: &str) -> Option<u32> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Ok(number) = value.parse::<u32>() {
        return Some(number);
    }
    match value {
        "一" | "1" => Some(1),
        "二" | "两" | "2" => Some(2),
        "三" | "3" => Some(3),
        "四" | "4" => Some(4),
        "五" | "5" => Some(5),
        "六" | "6" => Some(6),
        "七" | "7" => Some(7),
        "八" | "8" => Some(8),
        "九" | "9" => Some(9),
        "十" | "10" => Some(10),
        _ => None,
    }
}

fn inactive_recruiter_skip_reason(
    active_text: &Option<String>,
    config: &BossFilterConfig,
) -> Option<String> {
    if !config.active_filter_enabled {
        return None;
    }
    let threshold = active_threshold_rank(config.active_threshold)?;
    let Some(active_text) = active_text
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    else {
        return Some("未识别到招聘者活跃度".to_string());
    };

    let rank = classify_recruiter_active_text(active_text);
    (rank > threshold).then(|| {
        format!(
            "招聘者活跃度「{}」低于阈值「{}」",
            active_text,
            describe_active_threshold(config.active_threshold)
        )
    })
}

fn describe_active_threshold(threshold: BossRecruiterActiveThreshold) -> &'static str {
    match threshold {
        BossRecruiterActiveThreshold::Online => "在线",
        BossRecruiterActiveThreshold::JustActive => "刚刚活跃",
        BossRecruiterActiveThreshold::ThreeDays => "3日内活跃",
        BossRecruiterActiveThreshold::ThisWeek => "本周活跃",
        BossRecruiterActiveThreshold::SevenDays => "7日内活跃",
        BossRecruiterActiveThreshold::SevenDaysText => "7天内活跃",
        BossRecruiterActiveThreshold::TwoWeeks => "2周内活跃",
        BossRecruiterActiveThreshold::ThisMonth => "本月活跃",
        BossRecruiterActiveThreshold::TwoMonths => "2月内活跃",
        BossRecruiterActiveThreshold::ThreeMonths => "3月内活跃",
        BossRecruiterActiveThreshold::FourMonths => "4月内活跃",
        BossRecruiterActiveThreshold::FiveMonths => "5月内活跃",
        BossRecruiterActiveThreshold::HalfYear => "近半年活跃",
        BossRecruiterActiveThreshold::HalfYearAgo => "半年前活跃",
        BossRecruiterActiveThreshold::Disabled => "不筛选",
    }
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

/// 按当前任务上下文拼出岗位记录。
/// 打招呼后落库和「筛选通过即分析」共用它——后者触发时岗位还没入库，只能拿这份内存数据去分析。
fn build_job_detail(job_id: &str, greet_job: &GreetJob, config: &AppRuntimeConfig) -> JobDetail {
    let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let active = config.active_job_profile.as_ref();
    JobDetail {
        id: job_id.to_string(),
        platform: "boss".to_string(),
        source_task_id: crate::rpa::run_flow::current_job_task_id(),
        profile_id: active.map(|profile| profile.id.clone()),
        profile_name: active.map(|profile| profile.name.clone()),
        profile_snapshot_id: active.map(|profile| profile.snapshot_id.clone()),
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
    }
}

fn save_job_detail(job_id: &str, greet_job: &GreetJob, config: &AppRuntimeConfig) -> JobDetail {
    let job_detail = build_job_detail(job_id, greet_job, config);

    if let Err(e) = job_detail_dao::create(job_detail.clone()) {
        let _ = logger::warning(format!("保存岗位数据失败: {}", e));
    }
    job_detail
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
/// 先向上回滚一段，制造真实的滚动位移。
/// 页面已经停在底部时，直接给 scrollTop 赋 scrollHeight 不会产生任何位移，
/// BOSS 的懒加载观察器不会被唤醒，joblist 接口也就永远不会再请求。
const SCROLL_NUDGE_UP_SCRIPT: &str = r#"
(() => {
    const html = document.documentElement;
    const body = document.body;
    const scrollContainer = html.scrollHeight > html.clientHeight ? html : body;
    const scrollHeight = scrollContainer.scrollHeight;
    const target = Math.max(0, scrollContainer.scrollTop - 520);
    scrollContainer.scrollTop = target;
    window.dispatchEvent(new Event('scroll'));
    return { scrollHeight: scrollHeight, scrollTop: scrollContainer.scrollTop };
})();
"#;

/// 回滚之后再滚回底部，触发懒加载
const SCROLL_BOTTOM_SCRIPT: &str = r#"
(() => {
    const html = document.documentElement;
    const body = document.body;
    const scrollContainer = html.scrollHeight > html.clientHeight ? html : body;
    scrollContainer.scrollTop = scrollContainer.scrollHeight;
    window.dispatchEvent(new Event('scroll'));
    return { scrollHeight: scrollContainer.scrollHeight, scrollTop: scrollContainer.scrollTop };
})();
"#;

/// 只读取当前滚动容器高度，用于判断页面是否真的长高了
const SCROLL_HEIGHT_SCRIPT: &str = r#"
(() => {
    const html = document.documentElement;
    const body = document.body;
    const scrollContainer = html.scrollHeight > html.clientHeight ? html : body;
    return { scrollHeight: scrollContainer.scrollHeight, scrollTop: scrollContainer.scrollTop };
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
    human_input::click(work_page, &btn)?;
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
        save_job_detail(&greet_job.platform_job_id, &greet_job, &config);
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
    match build_greet_resources(&config, &greet_job).await? {
        // 整轮取消：既不发文本也不发图片，也不记为已沟通——我们并没有联系过对方
        SendVerdict::Hold(reason) => {
            logger::info(format!(
                "跳过 {}，未发送任何内容：{reason}",
                greet_job.title
            ))?;
            return Ok(());
        }
        SendVerdict::Send(resources) => {
            send_if_any(resources, |resources| send_messages(page, resources))?;
        }
    }

    // 保存该岗位至本地数据库
    let saved = save_job_detail(&greet_job.platform_job_id, &greet_job, &config);
    auto_analysis::schedule(&saved, AnalysisTrigger::GreetSent, &config);

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

/// 一次滚动前后的页面高度，作为“是否加载了新内容”的辅助信号
#[derive(Debug, Clone, Copy, PartialEq)]
struct ScrollProbe {
    height_before: f64,
    height_after: f64,
}

impl ScrollProbe {
    /// 页面高度是否真的增长（留 1px 容差，规避浏览器亚像素抖动）
    fn grew(&self) -> bool {
        self.height_after > self.height_before + 1.0
    }

    fn describe(&self) -> String {
        if self.grew() {
            format!(
                "页面高度已从 {:.0} 增长到 {:.0}，疑似已渲染新内容但未捕获到接口响应",
                self.height_before, self.height_after
            )
        } else {
            format!("页面高度仍为 {:.0}，未检测到新内容", self.height_after)
        }
    }
}

/// 从 JS 返回值里取出 scrollHeight，兼容 `{value: {...}}` 包装与裸数值
fn scroll_height_from_value(value: &Value) -> Option<f64> {
    let raw = value.get("value").unwrap_or(value);
    raw.get("scrollHeight")
        .and_then(Value::as_f64)
        .or_else(|| raw.as_f64())
}

// 滚动到底部（兼容旧签名的包装，忽略高度探测结果）
#[allow(dead_code)]
pub fn scroll_bottom(page: &Page) -> Result<(), anyhow::Error> {
    scroll_bottom_probe(page).map(|_| ())
}

/// 滚动到底部并返回滚动前后的页面高度。
/// 分两步执行 JS（先上滚、再下滚），中间用 `sleep_random_ms` 留出真实间隔，
/// 比在 JS 里写 setTimeout 更可控，也更贴近真人操作。
fn scroll_bottom_probe(page: &Page) -> Result<ScrollProbe, anyhow::Error> {
    let nudged = page.run_js_await(SCROLL_NUDGE_UP_SCRIPT)?;
    let height_before = scroll_height_from_value(&nudged).unwrap_or_default();

    sleep_random_ms(200, 400);
    // 拟人化开着时分几段滚下去：一脚从当前位置踩到 scrollHeight 是滚轮和触控板
    // 都做不出来的位移。没开时仍走原来那一下
    if human_input::scroll_to_bottom(page)?.is_none() {
        page.run_js_await(SCROLL_BOTTOM_SCRIPT)?;
    }

    // 懒加载是异步追加 DOM 的，稍等一下再量高度才有意义
    sleep_random_ms(500, 800);
    let settled = page.run_js_await(SCROLL_HEIGHT_SCRIPT)?;
    let height_after = scroll_height_from_value(&settled).unwrap_or(height_before);

    Ok(ScrollProbe {
        height_before,
        height_after,
    })
}

fn _is_bottom_value(value: &Value) -> Result<bool, anyhow::Error> {
    value
        .get("value")
        .and_then(Value::as_bool)
        .or_else(|| value.as_bool())
        .ok_or_else(|| anyhow::anyhow!("页面滚动状态返回值非布尔值"))
}

// 判断是否到底部
pub fn _is_bottom(page: &Page) -> Result<bool, anyhow::Error> {
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
            recruiter_active_time: Some("本周活跃".to_string()),
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
    fn job_list_timeout_hints_at_boss_security_check() {
        let message = job_list_wait_error(
            "https://www.zhipin.com/web/common/security-check.html?seed=x",
            "请完成验证后继续访问",
            "CDP error: Timed out while waiting for element: .rec-job-list",
        );

        assert!(message.contains("BOSS 安全验证或登录已失效"));
        assert!(message.contains("检查BOSS环境"));
        assert!(message.contains("Timed out while waiting for element"));
    }

    #[test]
    fn job_list_timeout_detects_guard_from_login_url_only() {
        assert!(looks_like_boss_guard(
            "https://login.zhipin.com/?ka=header-login",
            "岗位列表"
        ));
        assert!(looks_like_boss_guard(
            "https://www.zhipin.com/web/geek/jobs",
            "抱歉，请登录后查看岗位"
        ));
        assert!(!looks_like_boss_guard(
            "https://www.zhipin.com/web/geek/jobs?city=101280600&query=Rust",
            "推荐职位 全部职位"
        ));
    }

    #[test]
    fn job_list_timeout_without_guard_signal_suggests_network_retry() {
        let message = job_list_wait_error(
            "https://www.zhipin.com/web/geek/jobs?query=Rust",
            "推荐职位",
            "CDP error: Timed out",
        );

        assert!(message.contains("请检查网络后重试"));
        assert!(!message.contains("可能触发了 BOSS 安全验证"));
    }

    #[test]
    fn scroll_probe_reports_page_growth() {
        let grew = ScrollProbe {
            height_before: 12000.0,
            height_after: 18000.0,
        };
        let flat = ScrollProbe {
            height_before: 12000.0,
            height_after: 12000.0,
        };

        assert!(grew.grew());
        assert!(grew.describe().contains("疑似已渲染新内容"));
        assert!(!flat.grew());
        assert!(flat.describe().contains("未检测到新内容"));
    }

    #[test]
    fn reads_scroll_height_from_wrapped_and_plain_js_results() {
        let wrapped = serde_json::json!({"value": {"scrollHeight": 4200, "scrollTop": 3900}});
        let plain = serde_json::json!({"scrollHeight": 4200.5});
        let bare = serde_json::json!(4200);

        assert_eq!(scroll_height_from_value(&wrapped), Some(4200.0));
        assert_eq!(scroll_height_from_value(&plain), Some(4200.5));
        assert_eq!(scroll_height_from_value(&bare), Some(4200.0));
        assert_eq!(
            scroll_height_from_value(&serde_json::json!({"foo": "bar"})),
            None
        );
    }

    #[test]
    fn scroll_script_moves_up_before_returning_to_the_bottom() {
        // 已经在底部时必须先产生反向位移，否则懒加载观察器不会被唤醒
        assert!(SCROLL_NUDGE_UP_SCRIPT.contains("scrollContainer.scrollTop - 520"));
        assert!(SCROLL_BOTTOM_SCRIPT
            .contains("scrollContainer.scrollTop = scrollContainer.scrollHeight"));
        assert!(SCROLL_NUDGE_UP_SCRIPT.contains("scrollHeight"));
        assert!(SCROLL_HEIGHT_SCRIPT.contains("scrollHeight"));
    }

    #[test]
    fn round_stats_summary_covers_every_bucket() {
        let stats = RoundStats {
            scanned: 30,
            skipped_viewed: 12,
            skipped_processed: 15,
            skipped_rule: 1,
            skipped_ai: 1,
            skipped_inactive: 2,
            skipped_humanize: 2,
            read_failed: 0,
            greet_success: 1,
            greet_failed: 0,
        };

        let summary = stats.summary();

        assert!(summary.contains("共扫描 30 条岗位"));
        assert!(summary.contains("已浏览跳过 12 条"));
        assert!(summary.contains("已投递跳过 15 条"));
        assert!(summary.contains("活跃度跳过 2 条"));
        assert!(summary.contains("规则过滤跳过 1 条"));
        assert!(summary.contains("AI 复核跳过 1 条"));
        assert!(summary.contains("拟人化跳过 2 条"));
        assert!(summary.contains("打招呼成功 1 条"));
        assert!(summary.contains("打招呼失败 0 条"));
    }

    /// 拟人化跳过要和「规则不匹配」分开记：前者是这轮先不投，后者是压根不该投。
    /// 混在一起的话，用户会以为自己的筛选条件写错了
    #[test]
    fn humanized_skips_are_counted_separately_from_rule_skips() {
        let stats = RoundStats {
            scanned: 10,
            skipped_rule: 3,
            skipped_humanize: 2,
            greet_success: 5,
            ..RoundStats::default()
        };

        let summary = stats.total_summary();

        assert!(summary.contains("规则过滤跳过 3 条"));
        assert!(summary.contains("拟人化跳过 2 条"));
        assert_eq!(stats.engaged(), 5);
    }

    #[test]
    fn total_summary_marks_rescans_instead_of_claiming_distinct_jobs() {
        // 三轮滚动重扫同一页 30 张卡片：累计值不能被读成「看过 90 个不同岗位」
        let stats = RoundStats {
            scanned: 90,
            skipped_viewed: 87,
            skipped_processed: 3,
            ..RoundStats::default()
        };

        let summary = stats.total_summary();

        assert!(summary.contains("累计扫描岗位卡片 90 次（含滚动后的重复扫描）"));
        assert!(summary.contains("因已浏览或已投递而跳过 90 次"));
        assert!(summary.contains("打招呼成功 0 条"));
        assert!(!summary.contains("共扫描 90 条岗位"));
    }

    /// 拟人化跳过必须发生在点开卡片之前。
    ///
    /// BOSS 会给点开过的卡片打上 is-seen，下一轮扫描直接跳过——顺序反了，
    /// 「这次先不投」就变成了「永久放弃这个岗位」。这是纯粹的位置关系，
    /// 编译器和运行时都不会报错，只会静默地少投一批岗位
    #[test]
    fn the_humanized_skip_happens_before_the_card_is_opened() {
        let source = include_str!("position_say_hello.rs");

        let skim = source.find("pacer.should_skim()").expect("跳过判断已被移除");
        let open = source
            .find("human_input::click(page, &job_card_ele)")
            .expect("卡片点击已被改写");

        assert!(skim < open, "拟人化跳过跑到了点开卡片之后");
    }

    /// 拟人化跳过不该混进「已浏览/已投递」那两个桶：那两个是「以前处理过」，
    /// 而这个是「这次故意没看」，混在一起进度日志会虚报
    #[test]
    fn a_humanized_skip_is_counted_on_its_own() {
        let mut stats = RoundStats::default();
        stats.record_skip(SkipReason::Humanized);
        stats.record_skip(SkipReason::AlreadyViewed);

        assert_eq!(stats.skipped_humanize, 1);
        assert_eq!(stats.skipped_known(), 1);
        assert_eq!(stats.engaged(), 0);
    }

    #[test]
    fn active_filter_accepts_this_week_but_skips_stale_recruiters() {
        let config = BossFilterConfig {
            active_filter_enabled: true,
            active_threshold: BossRecruiterActiveThreshold::ThisWeek,
        };

        assert!(inactive_recruiter_skip_reason(&Some("在线".to_string()), &config).is_none());
        assert!(inactive_recruiter_skip_reason(&Some("刚刚活跃".to_string()), &config).is_none());
        assert!(inactive_recruiter_skip_reason(&Some("本周活跃".to_string()), &config).is_none());
        assert!(inactive_recruiter_skip_reason(&Some("7天前活跃".to_string()), &config).is_some());
        assert!(inactive_recruiter_skip_reason(&Some("2周内活跃".to_string()), &config).is_some());
        assert!(inactive_recruiter_skip_reason(&Some("半月前活跃".to_string()), &config).is_some());
        assert!(inactive_recruiter_skip_reason(&Some("近半年活跃".to_string()), &config).is_some());
        assert!(inactive_recruiter_skip_reason(&None, &config).is_some());
    }

    #[test]
    fn active_filter_online_requires_explicit_online_status() {
        let config = BossFilterConfig {
            active_filter_enabled: true,
            active_threshold: BossRecruiterActiveThreshold::Online,
        };

        assert!(inactive_recruiter_skip_reason(&Some("在线".to_string()), &config).is_none());
        assert!(inactive_recruiter_skip_reason(&Some("当前在线".to_string()), &config).is_none());
        assert!(inactive_recruiter_skip_reason(&Some("刚刚活跃".to_string()), &config).is_some());
        assert!(inactive_recruiter_skip_reason(&Some("今日活跃".to_string()), &config).is_some());
        assert!(
            inactive_recruiter_skip_reason(&Some("1小时前活跃".to_string()), &config).is_some()
        );
    }

    #[test]
    fn active_filter_accepts_two_weeks_when_threshold_is_this_month() {
        let config = BossFilterConfig {
            active_filter_enabled: true,
            active_threshold: BossRecruiterActiveThreshold::ThisMonth,
        };

        assert!(inactive_recruiter_skip_reason(&Some("2周内活跃".to_string()), &config).is_none());
        assert!(
            inactive_recruiter_skip_reason(&Some("一个月内活跃".to_string()), &config).is_none()
        );
        assert!(inactive_recruiter_skip_reason(&Some("2月内活跃".to_string()), &config).is_some());
    }

    #[test]
    fn active_filter_accepts_real_boss_activity_thresholds() {
        let config = BossFilterConfig {
            active_filter_enabled: true,
            active_threshold: BossRecruiterActiveThreshold::FiveMonths,
        };

        assert!(inactive_recruiter_skip_reason(&Some("5月内活跃".to_string()), &config).is_none());
        assert!(inactive_recruiter_skip_reason(&Some("近半年活跃".to_string()), &config).is_some());
        assert!(inactive_recruiter_skip_reason(&Some("半年前活跃".to_string()), &config).is_some());
    }

    #[test]
    fn active_filter_accepts_half_year_threshold() {
        let config = BossFilterConfig {
            active_filter_enabled: true,
            active_threshold: BossRecruiterActiveThreshold::HalfYear,
        };

        assert!(inactive_recruiter_skip_reason(&Some("近半年活跃".to_string()), &config).is_none());
        assert!(inactive_recruiter_skip_reason(&Some("半年前活跃".to_string()), &config).is_none());
    }

    #[test]
    fn active_filter_can_be_disabled() {
        let config = BossFilterConfig {
            active_filter_enabled: false,
            active_threshold: BossRecruiterActiveThreshold::ThisWeek,
        };

        assert!(inactive_recruiter_skip_reason(&None, &config).is_none());
        assert!(
            inactive_recruiter_skip_reason(&Some("一个月前活跃".to_string()), &config).is_none()
        );
    }

    #[test]
    fn extracts_recruiter_active_text_from_boss_copy() {
        assert_eq!(
            extract_recruiter_active_text("张三 HR 本周活跃").as_deref(),
            Some("本周活跃")
        );
        assert_eq!(
            extract_recruiter_active_text("张三 HR 7天前活跃").as_deref(),
            Some("7天前活跃")
        );
    }

    #[test]
    fn extracts_recruiter_active_text_from_real_boss_dom() {
        let html = r#"
            <span data-v-759013b4="" class="boss-online-tag">在线</span>
            <span data-v-759013b4="" class="boss-active-time">刚刚活跃</span>
            <div data-v-759013b4="" class="job-boss-info">
                <h2 data-v-759013b4="" class="name">
                    邱瑞轩
                    <i data-v-759013b4="" class="icon-vip"></i>
                    <span data-v-759013b4="" class="boss-active-time">刚刚活跃</span>
                </h2>
                <div data-v-759013b4="" class="boss-info-attr"> 中企云软 · 经理 </div>
            </div>
        "#;

        assert_eq!(extract_recruiter_active_text(html).as_deref(), Some("在线"));
        assert_eq!(
            extract_recruiter_active_text(
                r#"<span data-v-759013b4="" class="boss-active-time">刚刚活跃</span>"#
            )
            .as_deref(),
            Some("刚刚活跃")
        );
    }

    #[test]
    fn round_stats_distinguishes_viewed_and_processed_skips() {
        let mut stats = RoundStats::default();
        stats.record_skip(SkipReason::AlreadyViewed);
        stats.record_skip(SkipReason::AlreadyProcessed);
        stats.record_skip(SkipReason::AlreadyProcessed);

        assert_eq!(stats.skipped_viewed, 1);
        assert_eq!(stats.skipped_processed, 2);
        assert_eq!(stats.skipped_known(), 3);
        assert_eq!(stats.engaged(), 0);
    }

    #[test]
    fn round_stats_since_yields_current_page_delta() {
        let base = RoundStats {
            scanned: 30,
            skipped_processed: 30,
            ..RoundStats::default()
        };
        let mut total = base.clone();
        total.scanned += 15;
        total.skipped_processed += 13;
        total.greet_success += 2;

        let round = total.since(&base);

        assert_eq!(round.scanned, 15);
        assert_eq!(round.skipped_processed, 13);
        assert_eq!(round.greet_success, 2);
        assert_eq!(round.engaged(), 2);
    }

    #[test]
    fn skip_progress_is_logged_in_batches_instead_of_every_card() {
        assert!(!should_log_skip_progress(0));
        assert!(!should_log_skip_progress(9));
        assert!(should_log_skip_progress(10));
        assert!(!should_log_skip_progress(11));
        assert!(should_log_skip_progress(30));
    }

    #[test]
    fn stop_reason_explains_normal_endings_with_next_steps() {
        assert!(StopReason::ReachedBottom.describe().contains("已触底"));
        assert!(StopReason::NoMoreFromApi.describe().contains("无更多岗位"));
        assert!(StopReason::UserStopped.describe().contains("手动停止"));
        assert!(StopReason::EmptyList.describe().contains("暂无岗位列表"));

        for reason in [StopReason::NoNewJobs, StopReason::ApiTimeout] {
            let text = reason.describe();
            assert!(text.contains("正常结束、不是程序崩溃"), "{text}");
            assert!(text.contains("周期投递"), "{text}");
        }
    }

    /// 不设上界时行为必须与改造前完全一致，否则「单轮自动求职」会莫名其妙提前收工
    #[test]
    fn an_unlimited_budget_never_ends_the_round_early() {
        let stats = RoundStats {
            greet_success: 500,
            ..RoundStats::default()
        };

        assert_eq!(
            budget_stop_reason(&RoundBudget::unlimited(), &stats, Instant::now(), 99),
            None
        );
    }

    #[test]
    fn budget_limits_map_to_their_own_stop_reasons() {
        let budget = RoundBudget {
            max_greets: 3,
            max_minutes: 0,
            max_consecutive_greet_failures: 5,
        };
        let fresh = RoundStats::default();
        let spent = RoundStats {
            greet_success: 3,
            ..RoundStats::default()
        };

        assert_eq!(budget_stop_reason(&budget, &fresh, Instant::now(), 0), None);
        assert_eq!(
            budget_stop_reason(&budget, &spent, Instant::now(), 0),
            Some(StopReason::GreetQuotaReached)
        );
        assert_eq!(
            budget_stop_reason(&budget, &fresh, Instant::now(), 5),
            Some(StopReason::TooManyGreetFailures)
        );
    }

    /// 提前结束不是故障，文案必须说清楚「下一轮还会继续」，
    /// 否则用户会以为周期投递挂了
    #[test]
    fn budget_stop_reasons_read_as_normal_endings() {
        assert!(StopReason::GreetQuotaReached
            .describe()
            .contains("下一轮继续"));
        assert!(StopReason::TimeLimitReached
            .describe()
            .contains("下一轮继续"));
        assert!(StopReason::TooManyGreetFailures
            .describe()
            .contains("每日沟通上限"));
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
