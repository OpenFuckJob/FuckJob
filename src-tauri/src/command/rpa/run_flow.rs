use std::sync::atomic::{AtomicBool, Ordering};

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

static SYNC_BOSS_RUNNING: AtomicBool = AtomicBool::new(false);
static SYNC_LIEPIN_RUNNING: AtomicBool = AtomicBool::new(false);

struct SyncPlatformGuard(&'static AtomicBool);

impl Drop for SyncPlatformGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn try_acquire_sync_guard(platform: PlatformKind) -> Result<SyncPlatformGuard, String> {
    let (flag, label) = match platform {
        PlatformKind::Boss => (&SYNC_BOSS_RUNNING, "BOSS 直聘"),
        PlatformKind::Liepin => (&SYNC_LIEPIN_RUNNING, "猎聘"),
        PlatformKind::Job51 => return Err("前程无忧暂不支持聊天状态同步".to_string()),
    };
    if flag
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(format!("{label} 正在同步中，请稍后再试"));
    }
    Ok(SyncPlatformGuard(flag))
}

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
        None => match JOB_TASK_MANAGER.stop_all() {
            Ok(count) if count > 0 => Ok(()),
            Ok(_) => run_flow::stop_job_task(),
            Err(error) => Err(error),
        },
    };
    match result {
        Ok(()) => CommandResult::ok(()),
        Err(error) => CommandResult::err(error),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncStatusSummary {
    pub platforms: Vec<String>,
    pub scanned: usize,
    pub updated: usize,
    pub unread_total: u32,
    pub interview_detected: usize,
    pub warnings: Vec<String>,
    pub synced_at: String,
}

/// 一键零点击状态同步（支持单平台或同时同步 BOSS + 猎聘，绝不点击会话卡片、保留未读红点）。
#[tauri::command]
pub async fn sync_platform_status(
    app_handle: tauri::AppHandle,
    platform: Option<PlatformKind>,
) -> CommandResult<SyncStatusSummary> {
    let targets = match platform {
        Some(PlatformKind::Job51) => {
            return CommandResult::err("前程无忧暂不支持聊天状态同步");
        }
        Some(target) => vec![target],
        None => vec![PlatformKind::Boss, PlatformKind::Liepin],
    };

    let mut guards = Vec::with_capacity(targets.len());
    for &target in &targets {
        match try_acquire_sync_guard(target) {
            Ok(guard) => guards.push(guard),
            Err(error) => return CommandResult::err(error),
        }
    }

    let notification_config = crate::config::load_app_config_inner(app_handle)
        .map(|cfg| cfg.notification_config)
        .unwrap_or_default();

    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let _guards = guards;
        let _notify_guard = crate::notification::scoped_notification(notification_config);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build();
        let outcome: CommandResult<SyncStatusSummary> = match rt {
            Ok(runtime) => runtime.block_on(async move {
                let mut summary = SyncStatusSummary {
                    platforms: Vec::new(),
                    scanned: 0,
                    updated: 0,
                    unread_total: 0,
                    interview_detected: 0,
                    warnings: Vec::new(),
                    synced_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                };

                for target in targets {
                    let _log_ctx = logger::scoped_context(None, Some(target));
                    match target {
                        PlatformKind::Boss => {
                            let prev_meta = crate::dao::sync_state_dao::get_platform_meta("boss");
                            let sync_result = crate::browser::with_task_browser(|_, main_tab| {
                                Box::pin(async move {
                                    crate::rpa::boss::handler::sync_chat_history_on_page(main_tab)
                                        .await
                                })
                            })
                            .await;
                            match sync_result {
                                Ok(res) => {
                                    let updated_count = res.jobs_updated + res.jobs_inserted;
                                    summary.platforms.push("boss".to_string());
                                    summary.scanned += res.scanned;
                                    summary.updated += updated_count;
                                    summary.unread_total =
                                        summary.unread_total.saturating_add(res.unread_total);
                                    summary.interview_detected += res.interview_detected;

                                    notify_sync_highlights(
                                        "BOSS 直聘",
                                        prev_meta.last_unread_total,
                                        res.unread_total,
                                        res.interview_detected,
                                        updated_count,
                                    );
                                }
                                Err(err) => {
                                    let err_text = err.to_string();
                                    let mut meta =
                                        crate::dao::sync_state_dao::get_platform_meta("boss");
                                    meta.last_error = Some(err_text.clone());
                                    let _ = crate::dao::sync_state_dao::save_platform_meta(meta);
                                    if run_flow::is_fatal_risk_or_auth_error(&err_text) {
                                        crate::notification::notify_current(
                                            crate::notification::NotificationEvent::RiskAlert,
                                            "BOSS 状态同步检测到风控/登录异常",
                                            err_text.clone(),
                                        );
                                    }
                                    summary.warnings.push(format!("BOSS 同步未完成：{err_text}"));
                                }
                            }
                        }
                        PlatformKind::Liepin => {
                            let prev_meta = crate::dao::sync_state_dao::get_platform_meta("liepin");
                            let sync_result = crate::browser::with_task_browser(|_, main_tab| {
                                Box::pin(async move {
                                    crate::rpa::liepin::handler::sync_chat_status_on_page(main_tab)
                                        .await
                                })
                            })
                            .await;
                            match sync_result {
                                Ok(res) => {
                                    summary.platforms.push("liepin".to_string());
                                    summary.scanned += res.scanned;
                                    summary.updated += res.jobs_updated;
                                    summary.unread_total =
                                        summary.unread_total.saturating_add(res.unread_total);
                                    summary.interview_detected += res.interview_detected;

                                    notify_sync_highlights(
                                        "猎聘",
                                        prev_meta.last_unread_total,
                                        res.unread_total,
                                        res.interview_detected,
                                        res.jobs_updated,
                                    );
                                }
                                Err(err) => {
                                    let err_text = err.to_string();
                                    let mut meta =
                                        crate::dao::sync_state_dao::get_platform_meta("liepin");
                                    meta.last_error = Some(err_text.clone());
                                    let _ = crate::dao::sync_state_dao::save_platform_meta(meta);
                                    if run_flow::is_fatal_risk_or_auth_error(&err_text) {
                                        crate::notification::notify_current(
                                            crate::notification::NotificationEvent::RiskAlert,
                                            "猎聘状态同步检测到风控/登录异常",
                                            err_text.clone(),
                                        );
                                    }
                                    summary.warnings.push(format!("猎聘同步未完成：{err_text}"));
                                }
                            }
                        }
                        PlatformKind::Job51 => {}
                    }
                }

                if summary.platforms.is_empty() && !summary.warnings.is_empty() {
                    CommandResult::err(summary.warnings.join("；"))
                } else {
                    CommandResult::ok(summary)
                }
            }),
            Err(error) => CommandResult::err(format!("创建同步运行时失败：{error}")),
        };
        let _ = tx.send(outcome);
    });

    match rx.await {
        Ok(result) => result,
        Err(_) => CommandResult::err("状态同步线程异常退出"),
    }
}

fn notify_sync_highlights(
    platform_label: &str,
    prev_unread_total: u32,
    new_unread_total: u32,
    interview_detected: usize,
    updated_count: usize,
) {
    let has_new_unread = new_unread_total > prev_unread_total;
    let has_new_interview = interview_detected > 0 && updated_count > 0;
    if !has_new_unread && !has_new_interview {
        return;
    }
    let mut parts = Vec::new();
    if has_new_unread {
        parts.push(format!(
            "新增 {} 条未读消息（当前共 {} 条未读）",
            new_unread_total - prev_unread_total,
            new_unread_total
        ));
    }
    if interview_detected > 0 {
        parts.push(format!("识别到 {} 条约面信号", interview_detected));
    }
    crate::notification::notify_current(
        crate::notification::NotificationEvent::HrReply,
        format!("{platform_label} 状态同步发现新动态"),
        parts.join("，"),
    );
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
    use super::*;

    #[test]
    fn sync_guard_enforces_single_flight_and_releases_on_drop() {
        let boss_guard = try_acquire_sync_guard(PlatformKind::Boss).expect("first boss sync ok");
        assert!(try_acquire_sync_guard(PlatformKind::Boss).is_err());

        // Different platform can still run concurrently
        let liepin_guard =
            try_acquire_sync_guard(PlatformKind::Liepin).expect("liepin sync independent");
        assert!(try_acquire_sync_guard(PlatformKind::Liepin).is_err());

        drop(boss_guard);
        assert!(try_acquire_sync_guard(PlatformKind::Boss).is_ok());

        drop(liepin_guard);
        assert!(try_acquire_sync_guard(PlatformKind::Liepin).is_ok());
    }
}
