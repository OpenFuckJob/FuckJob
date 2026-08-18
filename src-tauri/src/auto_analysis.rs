//! 岗位自动分析的后台调度。
//!
//! 一次分析是一次完整的大模型调用，比 RPA 流程里的任何一步都慢，所以永远不在主流程里等它：
//! 触发点只负责登记，真正的调用放到后台串行执行。分析失败只写运行日志，不影响求职任务本身——
//! 岗位有没有分析报告，和这个岗位该不该打招呼、该不该回复是两件事。

use crate::config::{AnalysisTrigger, AppRuntimeConfig};
use crate::dao::model::JobDetail;
use crate::dao::{analysis_dao, job_detail_dao};
use crate::logger;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;
use tokio::sync::Semaphore;

/// 串行闸门：同一时刻只跑一个自动分析，避免和 RPA 自身的模型调用抢配额
fn gate() -> &'static Semaphore {
    static GATE: OnceLock<Semaphore> = OnceLock::new();
    GATE.get_or_init(|| Semaphore::new(1))
}

/// 本轮求职任务已经自动分析的岗位数，用于 `max_per_task` 限额
static ANALYZED_IN_TASK: AtomicUsize = AtomicUsize::new(0);

/// 求职任务开始时清零限额计数
pub fn reset_task_counter() {
    ANALYZED_IN_TASK.store(0, Ordering::SeqCst);
}

pub fn analyzed_in_task() -> usize {
    ANALYZED_IN_TASK.load(Ordering::SeqCst)
}

/// 只看策略本身，不碰存储：时机是否命中、有没有可用模型、限额是否用尽。
fn strategy_allows(trigger: AnalysisTrigger, config: &AppRuntimeConfig) -> bool {
    let analysis = &config.analysis_config;
    if !analysis.triggers_on(trigger) {
        return false;
    }
    // 分析走的是和其他 AI 功能同一条降级链，主用服务没配就没有可用链路
    if config.llm_chain().is_empty() {
        return false;
    }
    if analysis.max_per_task > 0 && analyzed_in_task() >= analysis.max_per_task {
        return false;
    }
    true
}

/// 判断这次触发是否该真的去分析。同步、便宜，放在主流程里执行。
fn should_analyze(job_id: &str, trigger: AnalysisTrigger, config: &AppRuntimeConfig) -> bool {
    if !strategy_allows(trigger, config) {
        return false;
    }
    // 解析失败的旧记录不算数，那种报告没有可用内容，值得重跑一次
    if config.analysis_config.skip_analyzed
        && matches!(analysis_dao::get_by_job_id(job_id), Ok(Some(existing)) if existing.parse_error.is_none())
    {
        return false;
    }
    true
}

/// 在某个时机登记一次自动分析，立即返回。
///
/// 调用方直接给出岗位数据而不是 id：筛选刚通过时岗位还没入库，这条路径必须也能用。
/// 不满足策略条件时这里什么都不做，调用方不需要自己判断。
pub fn schedule(job: &JobDetail, trigger: AnalysisTrigger, config: &AppRuntimeConfig) {
    if !should_analyze(&job.id, trigger, config) {
        return;
    }

    // 拿不到运行时说明调用点不在异步上下文里，宁可跳过也不要把 RPA 主流程拖住
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        let _ = logger::warning(format!(
            "岗位「{}」的自动分析已跳过：当前不在异步运行时中",
            job.title
        ));
        return;
    };

    // 计数在登记时就加，后台任务排队期间不会被同一批岗位穿透限额
    ANALYZED_IN_TASK.fetch_add(1, Ordering::SeqCst);
    let job = job.clone();
    let config = config.clone();
    handle.spawn(async move {
        let _permit = match gate().acquire().await {
            Ok(permit) => permit,
            Err(_) => return,
        };
        run_once(&job, &config).await;
    });
}

/// 只拿得到岗位 id 的触发点（例如自动回复）用这个入口，岗位数据从本地库读。
pub fn schedule_by_id(job_id: &str, trigger: AnalysisTrigger, config: &AppRuntimeConfig) {
    if !should_analyze(job_id, trigger, config) {
        return;
    }
    match job_detail_dao::get_by_id(job_id) {
        Ok(Some(job)) => schedule(&job, trigger, config),
        // 岗位不在本地库里，说明这条会话还没建立岗位记录，下一轮同步后再说
        Ok(None) => {}
        Err(error) => {
            let _ = logger::warning(format!("岗位 {job_id} 自动分析失败，读取岗位出错：{error}"));
        }
    }
}

async fn run_once(job: &JobDetail, config: &AppRuntimeConfig) {
    let title = job.title.clone();

    match crate::command::job::analyze_job(job, config).await {
        Ok(analysis) if analysis.parse_error.is_none() => {
            let _ = logger::info(format!(
                "已自动分析岗位「{title}」，匹配度 {} 分",
                analysis.match_score
            ));
        }
        Ok(_) => {
            let _ = logger::warning(format!("岗位「{title}」自动分析完成，但模型输出解析不完整"));
        }
        Err(error) => {
            let _ = logger::warning(format!("岗位「{title}」自动分析失败：{error}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{default_app_config, AnalysisTrigger, LlmConfig, LlmProviderPreset};

    fn config_with(trigger: AnalysisTrigger, llm: bool) -> AppRuntimeConfig {
        let mut config = default_app_config();
        config.analysis_config.trigger = trigger;
        if llm {
            config.llm_config = Some(LlmConfig {
                provider: LlmProviderPreset::DeepSeek,
                base_url: "https://api.deepseek.com".to_string(),
                model: "deepseek-chat".to_string(),
            });
        }
        config
    }

    #[test]
    fn off_never_triggers() {
        let config = config_with(AnalysisTrigger::Off, true);
        assert!(!strategy_allows(AnalysisTrigger::GreetSent, &config));
        assert!(!strategy_allows(AnalysisTrigger::Off, &config));
    }

    #[test]
    fn only_the_configured_moment_triggers() {
        let config = config_with(AnalysisTrigger::GreetSent, true);
        assert!(strategy_allows(AnalysisTrigger::GreetSent, &config));
        assert!(!strategy_allows(AnalysisTrigger::FilterPassed, &config));
        assert!(!strategy_allows(AnalysisTrigger::ReplyReceived, &config));
    }

    /// 没有可用模型链路时开着策略也不该触发，否则每个岗位都会白跑一次失败
    #[test]
    fn without_a_model_chain_nothing_is_analyzed() {
        let config = config_with(AnalysisTrigger::GreetSent, false);
        assert!(!strategy_allows(AnalysisTrigger::GreetSent, &config));
    }

    #[test]
    fn per_task_limit_stops_further_analysis() {
        let mut config = config_with(AnalysisTrigger::GreetSent, true);
        config.analysis_config.max_per_task = 2;
        ANALYZED_IN_TASK.store(2, Ordering::SeqCst);
        assert!(!strategy_allows(AnalysisTrigger::GreetSent, &config));

        // 0 表示不限额，计数再高也照跑
        config.analysis_config.max_per_task = 0;
        assert!(strategy_allows(AnalysisTrigger::GreetSent, &config));
        reset_task_counter();
    }
}
