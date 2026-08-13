use crate::{
    command::base::CommandResult,
    config::{load_app_config, resolve_job_profile},
    dao::{model::JobProfileSnapshot, profile_snapshot_dao},
    logger,
    rpa::run_flow::{self, EnvCheckResult, FlowMode, PlatformKind, ReadinessReport},
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
) -> CommandResult<ReadinessReport> {
    match crate::config::load_app_config_inner(app_handle) {
        Ok(config) => CommandResult::ok(run_flow::inspect_readiness(platform, mode, &config)),
        Err(error) => CommandResult::err(error),
    }
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

#[tauri::command]
pub async fn boss_flow(
    app_handle: tauri::AppHandle,
    mode: FlowMode,
    interval_minutes: Option<u64>,
) -> CommandResult<()> {
    rpa_flow(app_handle, PlatformKind::Boss, mode, interval_minutes).await
}

#[tauri::command]
pub async fn rpa_flow(
    app_handle: tauri::AppHandle,
    platform: PlatformKind,
    mode: FlowMode,
    interval_minutes: Option<u64>,
) -> CommandResult<()> {
    let _running_guard = match run_flow::try_start_job_task() {
        Ok(guard) => guard,
        Err(error) => return CommandResult::err(error),
    };

    if let Err(error) = logger::clear() {
        return CommandResult::err(error);
    }
    if let Err(error) = logger::set_platform(Some(platform)) {
        return CommandResult::err(error);
    }

    let app_runtime_config_result = load_app_config(app_handle);
    if app_runtime_config_result.error.is_some() || app_runtime_config_result.data.is_none() {
        let _ = logger::set_platform(None);
        return CommandResult::err("load app config failed");
    }
    let app_runtime_config = app_runtime_config_result.data.unwrap();
    let readiness = run_flow::inspect_readiness(platform, mode, &app_runtime_config);
    if !readiness.ready {
        let missing = readiness
            .items
            .iter()
            .filter(|item| item.level == run_flow::ReadinessLevel::Blocked)
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>()
            .join("、");
        let _ = logger::set_platform(None);
        return CommandResult::err(format!("任务准备未完成：{missing}"));
    }

    let _ = logger::info("求职任务已启动（配置内容已隐藏）");

    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        // Logger context is thread-local. Legacy callers still execute the
        // actual flow on a dedicated worker, so install the platform again on
        // that thread instead of relying on the command thread's context.
        let _log_context = logger::scoped_context(None, Some(platform));
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build();
        let result = match rt {
            Ok(runtime) => runtime.block_on(run_flow::execute_rpa_flow(
                platform,
                mode,
                interval_minutes,
                &app_runtime_config,
            )),
            Err(e) => Err(anyhow::anyhow!("{e}")),
        };
        let _ = tx.send(result);
    });

    let result = rx.await;
    let stopped = run_flow::is_job_task_stop_requested();
    if !stopped {
        if let Some(message) = flow_error_message(&result) {
            let _ = logger::warning(message);
        }
    }
    let command_result = command_result_for_flow(stopped, result);
    let _ = logger::set_platform(None);
    command_result
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
    interval_minutes: Option<u64>,
    profile_id: Option<String>,
) -> CommandResult<JobTaskInfo> {
    if mode == FlowMode::PeriodicJobHunting && interval_minutes.is_none_or(|minutes| minutes == 0) {
        return CommandResult::err("周期性投递间隔必须大于 0 分钟");
    }

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
        FlowMode::ReplyUnread => {
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

    match JOB_TASK_MANAGER.submit(platform, mode, interval_minutes, config, task_profile) {
        Ok(task) => CommandResult::ok(task),
        Err(error) => CommandResult::err(error),
    }
}

fn flow_error_message(
    result: &Result<Result<(), anyhow::Error>, tokio::sync::oneshot::error::RecvError>,
) -> Option<String> {
    match result {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(format!("求职任务执行失败：{error}")),
        Err(_) => Some("求职任务后台线程异常退出，未返回错误详情".to_string()),
    }
}

fn command_result_for_flow(
    stopped: bool,
    result: Result<Result<(), anyhow::Error>, tokio::sync::oneshot::error::RecvError>,
) -> CommandResult<()> {
    if stopped {
        return CommandResult::err(crate::error::AppError::cancelled("任务已由用户停止"));
    }
    match result {
        Ok(Ok(())) => CommandResult::ok(()),
        Ok(Err(e)) => CommandResult::err(crate::error::AppError::from(e)),
        Err(e) => CommandResult::err(crate::error::AppError::from(e)),
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
        // Keep the pre-queue command contract working for callers that still
        // invoke `rpa_flow`/`boss_flow` directly.
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

#[cfg(test)]
mod tests {
    use super::{command_result_for_flow, flow_error_message};
    use crate::error::AppErrorCode;
    use tokio::sync::oneshot::error::RecvError;

    #[test]
    fn user_requested_stop_maps_to_cancelled() {
        let result = command_result_for_flow(true, Ok(Ok(())));
        assert_eq!(result.error.unwrap().code, AppErrorCode::Cancelled);
    }

    #[test]
    fn flow_error_message_exposes_internal_detail_for_logging() {
        let result: Result<Result<(), anyhow::Error>, RecvError> =
            Ok(Err(anyhow::anyhow!("岗位详情面板加载失败")));

        assert_eq!(
            flow_error_message(&result),
            Some("求职任务执行失败：岗位详情面板加载失败".to_string())
        );
    }
}
