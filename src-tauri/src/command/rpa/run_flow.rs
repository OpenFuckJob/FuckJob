use crate::{
    command::base::CommandResult,
    config::{load_app_config, resolve_job_profile},
    dao::{model::JobProfileSnapshot, profile_snapshot_dao},
    logger,
    rpa::{
        run_flow::{self, EnvCheckResult, FlowMode, PlatformKind, ReadinessReport},
        schedule::{self, PeriodicPlan},
    },
    task::{JobTaskInfo, JobTaskOverview, JobTaskProfile, JOB_TASK_MANAGER},
};

#[tauri::command]
pub async fn check_env(
    app_handle: tauri::AppHandle,
    platform: Option<PlatformKind>,
) -> CommandResult<EnvCheckResult> {
    let platform = platform.unwrap_or(PlatformKind::Boss);
    let app_runtime_config_result = load_app_config(app_handle);
    if app_runtime_config_result.error.is_some() || app_runtime_config_result.data.is_none() {
        return CommandResult::err("浏览器环境检查失败：加载应用配置失败");
    }

    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let _ = logger::set_platform(Some(platform));
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build();
        let result: CommandResult<EnvCheckResult> = match rt {
            Ok(runtime) => match runtime.block_on(run_flow::check_env(platform)) {
                Ok(result) => CommandResult::ok(result),
                Err(error) => CommandResult::err(format!("环境检查失败：{error}")),
            },
            Err(e) => CommandResult::err(format!("创建运行时失败: {e}")),
        };
        let _ = logger::set_platform(None);
        let _ = tx.send(result);
    });

    match rx.await {
        Ok(result) => result,
        Err(_) => CommandResult::err("环境检查线程异常退出"),
    }
}

#[tauri::command]
pub fn get_readiness_report(
    app_handle: tauri::AppHandle,
    platform: PlatformKind,
    mode: FlowMode,
    profile_id: Option<String>,
) -> CommandResult<ReadinessReport> {
    let base_config = match crate::config::load_app_config_inner(app_handle) {
        Ok(config) => config,
        Err(error) => return CommandResult::err(error),
    };
    let config = match mode {
        FlowMode::JobHunting | FlowMode::PeriodicJobHunting => {
            match resolve_job_profile(&base_config, profile_id.as_deref()) {
                Ok(resolved) => resolved.config,
                Err(error) => return CommandResult::err(error),
            }
        }
        _ => base_config,
    };
    CommandResult::ok(run_flow::inspect_readiness(platform, mode, &config))
}

#[tauri::command]
pub async fn preflight_job_task(
    app_handle: tauri::AppHandle,
    platform: PlatformKind,
    mode: FlowMode,
) -> CommandResult<ReadinessReport> {
    let config = match crate::config::load_app_config_inner(app_handle) {
        Ok(config) => config,
        Err(error) => return CommandResult::err(error),
    };
    let mut report = run_flow::inspect_readiness(platform, mode, &config);
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build();
        let result = match runtime {
            Ok(runtime) => runtime.block_on(run_flow::check_env(platform)),
            Err(error) => Err(anyhow::anyhow!(error)),
        };
        let _ = tx.send(result);
    });
    match rx.await {
        Ok(Ok(environment)) if environment.status == run_flow::EnvCheckStatus::Completed => {
            report.items.push(run_flow::ReadinessItem {
                key: "platform_login".to_string(),
                label: "平台登录".to_string(),
                level: run_flow::ReadinessLevel::Ready,
                message: environment.message,
                config_group: None,
            });
        }
        Ok(Ok(environment)) => {
            report.ready = false;
            report.items.push(run_flow::ReadinessItem {
                key: "platform_login".to_string(),
                label: "平台登录".to_string(),
                level: run_flow::ReadinessLevel::Blocked,
                message: environment.message,
                config_group: None,
            });
        }
        Ok(Err(error)) => return CommandResult::err(format!("平台环境检查失败：{error}")),
        Err(_) => return CommandResult::err("平台环境检查线程异常退出"),
    }
    CommandResult::ok(report)
}

/// Enqueues a browser automation job and returns immediately.
///
/// The scheduler applies a global concurrency limit and a capacity-one lock per platform. The
/// actual flow continues to run on its own OS thread with a current-thread Tokio runtime.
#[tauri::command]
pub fn start_job_task(
    app_handle: tauri::AppHandle,
    platform: PlatformKind,
    mode: FlowMode,
    plan: Option<PeriodicPlan>,
    profile_id: Option<String>,
) -> CommandResult<JobTaskInfo> {
    // 计划在入队时就校验并规整一次，落进任务里的是最终形态。等到 worker 起来才发现
    // 「结束时间已经过了」，用户看到的是任务一闪而过，什么都没发生
    let plan = if mode == FlowMode::PeriodicJobHunting {
        match schedule::resolve_plan(plan, chrono::Local::now()) {
            Ok(plan) => Some(plan),
            Err(error) => return CommandResult::err(error),
        }
    } else {
        None
    };

    let base_config = match crate::config::load_app_config_inner(app_handle) {
        Ok(config) => config,
        Err(error) => return CommandResult::err(error),
    };
    let (config, task_profile) = match mode {
        FlowMode::JobHunting | FlowMode::PeriodicJobHunting => {
            let resolved = match resolve_job_profile(&base_config, profile_id.as_deref()) {
                Ok(resolved) => resolved,
                Err(error) => return CommandResult::err(error),
            };
            let Some(snapshot) = JobProfileSnapshot::from_resolved(&resolved.config) else {
                return CommandResult::err("创建求职方案快照失败：缺少活动方案元信息");
            };
            if let Err(error) = profile_snapshot_dao::upsert(snapshot) {
                return CommandResult::err(format!("保存求职方案快照失败：{error}"));
            }
            let profile = JobTaskProfile {
                profile_id: Some(resolved.profile_id),
                profile_name: Some(resolved.profile_name),
                profile_snapshot_id: Some(resolved.snapshot_id),
            };
            (resolved.config, Some(profile))
        }
        FlowMode::ReplyUnread | FlowMode::PollingReply => {
            // 回复任务会逐会话恢复首次建联快照，不能在整批任务上固定一张方案。
            let config = match resolve_job_profile(&base_config, None) {
                Ok(resolved) => resolved.config,
                Err(error) => return CommandResult::err(error),
            };
            (
                config,
                Some(JobTaskProfile {
                    profile_id: None,
                    profile_name: Some("按会话自动选择".to_string()),
                    profile_snapshot_id: None,
                }),
            )
        }
        FlowMode::SyncChatHistory => (base_config, None),
    };
    let readiness = run_flow::inspect_readiness(platform, mode, &config);
    if !readiness.ready {
        let missing = readiness
            .items
            .iter()
            .filter(|item| item.level == run_flow::ReadinessLevel::Blocked)
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>()
            .join("、");
        return CommandResult::err(format!("任务准备未完成：{missing}"));
    }

    match JOB_TASK_MANAGER.submit(platform, mode, plan, config, task_profile) {
        Ok(task) => CommandResult::ok(task),
        Err(error) => CommandResult::err(error),
    }
}

#[tauri::command]
pub fn get_job_task_status() -> CommandResult<JobTaskOverview> {
    CommandResult::ok(JOB_TASK_MANAGER.overview())
}

#[tauri::command]
pub fn stop_job_task(task_id: Option<String>) -> CommandResult<()> {
    let result = match task_id {
        Some(task_id) => JOB_TASK_MANAGER.stop(&task_id),
        None => run_flow::stop_job_task(),
    };
    match result {
        Ok(()) => CommandResult::ok(()),
        Err(error) => CommandResult::err(error),
    }
}

#[tauri::command]
pub fn read_log_file(lines: Option<usize>) -> CommandResult<String> {
    match logger::read_tail_lines(lines.unwrap_or(500)) {
        Ok(content) => CommandResult::ok(content),
        Err(error) => CommandResult::err(error.to_string()),
    }
}
