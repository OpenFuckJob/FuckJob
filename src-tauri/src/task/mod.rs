//! Bounded background job queue and scheduler.
//!
//! Browser automation itself remains synchronous from the scheduler's point of view: every
//! dispatched job owns a dedicated OS thread and a current-thread Tokio runtime.  Keeping the
//! scheduling state here makes job cancellation and platform-level exclusion independent from
//! the browser implementation.

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    thread,
};

use chrono::{SecondsFormat, Utc};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    config::AppRuntimeConfig,
    rpa::run_flow::{self, FlowMode, PlatformKind},
};

const MAX_QUEUED_TASKS: usize = 32;
const FINISHED_TASK_HISTORY_LIMIT: usize = 100;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobTaskState {
    Queued,
    Starting,
    Running,
    Stopping,
    Succeeded,
    Failed,
    Cancelled,
}

impl JobTaskState {
    fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct JobTaskInfo {
    pub task_id: String,
    pub platform: PlatformKind,
    pub mode: FlowMode,
    pub status: JobTaskState,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub error: Option<String>,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub profile_name: Option<String>,
    #[serde(default)]
    pub profile_snapshot_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct JobTaskOverview {
    pub tasks: Vec<JobTaskInfo>,
    pub running_count: usize,
    pub queued_count: usize,
    pub max_parallel_tasks: usize,
}

struct TaskEntry {
    info: JobTaskInfo,
    interval_minutes: Option<u64>,
    config: Arc<AppRuntimeConfig>,
    cancelled: Arc<AtomicBool>,
}

struct SchedulerState {
    tasks: HashMap<String, TaskEntry>,
    queue: VecDeque<String>,
    running_by_platform: HashMap<PlatformKind, String>,
    running_count: usize,
    max_parallel_tasks: usize,
}

impl SchedulerState {
    fn new(max_parallel_tasks: usize) -> Self {
        Self {
            tasks: HashMap::new(),
            queue: VecDeque::new(),
            running_by_platform: HashMap::new(),
            running_count: 0,
            max_parallel_tasks: normalize_parallelism(max_parallel_tasks),
        }
    }

    fn enqueue(
        &mut self,
        platform: PlatformKind,
        mode: FlowMode,
        interval_minutes: Option<u64>,
        config: Arc<AppRuntimeConfig>,
        profile: Option<JobTaskProfile>,
    ) -> Result<JobTaskInfo, String> {
        if self.queue.len() >= MAX_QUEUED_TASKS {
            return Err(format!(
                "任务队列已满（最多等待 {MAX_QUEUED_TASKS} 个任务）"
            ));
        }

        if mode == FlowMode::PeriodicJobHunting
            && self.tasks.values().any(|entry| {
                entry.info.platform == platform
                    && entry.info.mode == FlowMode::PeriodicJobHunting
                    && !entry.info.status.is_terminal()
            })
        {
            return Err("该平台已有周期投递任务，请先停止后再重新启动".to_string());
        }

        let now = timestamp();
        let task_id = Uuid::new_v4().to_string();
        let info = JobTaskInfo {
            task_id: task_id.clone(),
            platform,
            mode,
            status: JobTaskState::Queued,
            created_at: now,
            started_at: None,
            finished_at: None,
            error: None,
            profile_id: profile.as_ref().and_then(|value| value.profile_id.clone()),
            profile_name: profile
                .as_ref()
                .and_then(|value| value.profile_name.clone()),
            profile_snapshot_id: profile.and_then(|value| value.profile_snapshot_id),
        };
        self.tasks.insert(
            task_id.clone(),
            TaskEntry {
                info: info.clone(),
                interval_minutes,
                config,
                cancelled: Arc::new(AtomicBool::new(false)),
            },
        );
        self.queue.push_back(task_id);
        Ok(info)
    }

    /// Takes the oldest task whose platform is currently available.
    ///
    /// This preserves FIFO order for every platform while allowing a Liepin task to pass a
    /// waiting Boss task when Boss already has a worker (and vice versa).
    fn take_next_runnable(&mut self) -> Option<WorkerInput> {
        if self.running_count >= self.max_parallel_tasks {
            return None;
        }

        let queue_index = self.queue.iter().position(|task_id| {
            self.tasks
                .get(task_id)
                .is_some_and(|entry| !self.running_by_platform.contains_key(&entry.info.platform))
        })?;
        let task_id = self.queue.remove(queue_index)?;
        let entry = self.tasks.get_mut(&task_id)?;
        entry.info.status = JobTaskState::Starting;
        entry.info.started_at = Some(timestamp());
        self.running_count += 1;
        self.running_by_platform
            .insert(entry.info.platform, task_id.clone());

        Some(WorkerInput {
            task_id,
            platform: entry.info.platform,
            mode: entry.info.mode,
            interval_minutes: entry.interval_minutes,
            config: Arc::clone(&entry.config),
            cancelled: Arc::clone(&entry.cancelled),
        })
    }

    fn request_stop(&mut self, task_id: &str) -> Result<(), String> {
        let entry = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| "求职任务不存在".to_string())?;
        match entry.info.status {
            JobTaskState::Queued => {
                entry.cancelled.store(true, Ordering::SeqCst);
                entry.info.status = JobTaskState::Cancelled;
                entry.info.finished_at = Some(timestamp());
                self.queue.retain(|queued_id| queued_id != task_id);
            }
            JobTaskState::Starting | JobTaskState::Running => {
                entry.cancelled.store(true, Ordering::SeqCst);
                entry.info.status = JobTaskState::Stopping;
            }
            JobTaskState::Stopping => {}
            status if status.is_terminal() => return Err("求职任务已结束".to_string()),
            _ => unreachable!("all task states are covered"),
        }
        Ok(())
    }

    fn mark_running(&mut self, task_id: &str) {
        if let Some(entry) = self.tasks.get_mut(task_id) {
            if entry.cancelled.load(Ordering::SeqCst) {
                entry.info.status = JobTaskState::Stopping;
            } else {
                entry.info.status = JobTaskState::Running;
            }
        }
    }

    fn finish(&mut self, task_id: &str, result: Result<(), anyhow::Error>) {
        let Some(entry) = self.tasks.get_mut(task_id) else {
            return;
        };
        let platform = entry.info.platform;
        let cancelled = entry.cancelled.load(Ordering::SeqCst);
        entry.info.finished_at = Some(timestamp());
        match (cancelled, result) {
            (true, _) => {
                entry.info.status = JobTaskState::Cancelled;
                entry.info.error = None;
            }
            (false, Ok(())) => entry.info.status = JobTaskState::Succeeded,
            (false, Err(error)) => {
                entry.info.status = JobTaskState::Failed;
                entry.info.error = Some(error.to_string());
            }
        }
        self.running_count = self.running_count.saturating_sub(1);
        if self
            .running_by_platform
            .get(&platform)
            .is_some_and(|running_id| running_id == task_id)
        {
            self.running_by_platform.remove(&platform);
        }
        self.trim_finished_history();
    }

    fn overview(&self) -> JobTaskOverview {
        let mut tasks = self
            .tasks
            .values()
            .map(|entry| entry.info.clone())
            .collect::<Vec<_>>();
        tasks.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| right.task_id.cmp(&left.task_id))
        });
        JobTaskOverview {
            tasks,
            running_count: self.running_count,
            queued_count: self.queue.len(),
            max_parallel_tasks: self.max_parallel_tasks,
        }
    }

    fn trim_finished_history(&mut self) {
        let mut finished = self
            .tasks
            .iter()
            .filter(|(_, entry)| entry.info.status.is_terminal())
            .map(|(task_id, entry)| (task_id.clone(), entry.info.finished_at.clone()))
            .collect::<Vec<_>>();
        if finished.len() <= FINISHED_TASK_HISTORY_LIMIT {
            return;
        }
        finished.sort_by(|left, right| left.1.cmp(&right.1));
        let remove_count = finished.len() - FINISHED_TASK_HISTORY_LIMIT;
        for (task_id, _) in finished.into_iter().take(remove_count) {
            self.tasks.remove(&task_id);
        }
    }
}

struct WorkerInput {
    task_id: String,
    platform: PlatformKind,
    mode: FlowMode,
    interval_minutes: Option<u64>,
    config: Arc<AppRuntimeConfig>,
    cancelled: Arc<AtomicBool>,
}

struct TaskManagerInner {
    state: Mutex<SchedulerState>,
    wake_scheduler: Condvar,
}

pub struct TaskManager {
    inner: Arc<TaskManagerInner>,
}

impl TaskManager {
    fn new() -> Self {
        let inner = Arc::new(TaskManagerInner {
            state: Mutex::new(SchedulerState::new(2)),
            wake_scheduler: Condvar::new(),
        });
        spawn_scheduler(Arc::clone(&inner));
        Self { inner }
    }

    pub fn submit(
        &self,
        platform: PlatformKind,
        mode: FlowMode,
        interval_minutes: Option<u64>,
        config: AppRuntimeConfig,
        profile: Option<JobTaskProfile>,
    ) -> Result<JobTaskInfo, String> {
        let max_parallel_tasks = config.browser_config.max_parallel_tasks;
        let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        state.max_parallel_tasks = normalize_parallelism(max_parallel_tasks);
        let info = state.enqueue(platform, mode, interval_minutes, Arc::new(config), profile)?;
        self.inner.wake_scheduler.notify_one();
        Ok(info)
    }

    pub fn overview(&self) -> JobTaskOverview {
        self.inner
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .overview()
    }

    pub fn stop(&self, task_id: &str) -> Result<(), String> {
        let result = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .request_stop(task_id);
        self.inner.wake_scheduler.notify_one();
        result
    }
}

#[derive(Debug, Clone, Default)]
pub struct JobTaskProfile {
    pub profile_id: Option<String>,
    pub profile_name: Option<String>,
    pub profile_snapshot_id: Option<String>,
}

pub static JOB_TASK_MANAGER: Lazy<TaskManager> = Lazy::new(TaskManager::new);

fn spawn_scheduler(inner: Arc<TaskManagerInner>) {
    thread::Builder::new()
        .name("job-task-scheduler".to_string())
        .spawn(move || loop {
            let worker = {
                let mut state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
                loop {
                    if let Some(worker) = state.take_next_runnable() {
                        break worker;
                    }
                    state = inner
                        .wake_scheduler
                        .wait(state)
                        .unwrap_or_else(|e| e.into_inner());
                }
            };
            spawn_worker(Arc::clone(&inner), worker);
        })
        .expect("failed to start job task scheduler");
}

fn spawn_worker(inner: Arc<TaskManagerInner>, worker: WorkerInput) {
    let thread_name = format!("job-task-{}", &worker.task_id[..8]);
    thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            let _log_context =
                crate::logger::scoped_context(Some(worker.task_id.clone()), Some(worker.platform));
            {
                let mut state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
                state.mark_running(&worker.task_id);
            }

            let _task_context = run_flow::enter_job_task_context(
                worker.task_id.clone(),
                Arc::clone(&worker.cancelled),
            );
            let result = if worker.cancelled.load(Ordering::SeqCst) {
                Ok(())
            } else {
                match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime.block_on(run_flow::execute_rpa_flow(
                        worker.platform,
                        worker.mode,
                        worker.interval_minutes,
                        &worker.config,
                    )),
                    Err(error) => Err(anyhow::anyhow!("创建任务运行时失败: {error}")),
                }
            };

            let mut state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
            state.finish(&worker.task_id, result);
            inner.wake_scheduler.notify_one();
        })
        .expect("failed to start job task worker");
}

fn normalize_parallelism(value: usize) -> usize {
    value.clamp(1, 2)
}

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_app_config;

    fn enqueue(state: &mut SchedulerState, platform: PlatformKind) -> JobTaskInfo {
        state
            .enqueue(
                platform,
                FlowMode::JobHunting,
                None,
                Arc::new(default_app_config()),
                None,
            )
            .unwrap()
    }

    #[test]
    fn scheduler_runs_different_platforms_in_parallel_and_queues_same_platform() {
        let mut state = SchedulerState::new(2);
        let boss_first = enqueue(&mut state, PlatformKind::Boss);
        let boss_second = enqueue(&mut state, PlatformKind::Boss);
        let liepin = enqueue(&mut state, PlatformKind::Liepin);

        assert_eq!(
            state.take_next_runnable().unwrap().task_id,
            boss_first.task_id
        );
        assert_eq!(state.take_next_runnable().unwrap().task_id, liepin.task_id);
        assert!(state.take_next_runnable().is_none());
        assert_eq!(state.queue.front(), Some(&boss_second.task_id));
    }

    #[test]
    fn same_platform_tasks_keep_fifo_order() {
        let mut state = SchedulerState::new(2);
        let _first = enqueue(&mut state, PlatformKind::Boss);
        let second = enqueue(&mut state, PlatformKind::Boss);
        let first_worker = state.take_next_runnable().unwrap();
        state.finish(&first_worker.task_id, Ok(()));

        assert_eq!(state.take_next_runnable().unwrap().task_id, second.task_id);
    }

    #[test]
    fn cancelling_one_task_does_not_cancel_another() {
        let mut state = SchedulerState::new(2);
        let boss = enqueue(&mut state, PlatformKind::Boss);
        let liepin = enqueue(&mut state, PlatformKind::Liepin);
        let boss_worker = state.take_next_runnable().unwrap();
        let liepin_worker = state.take_next_runnable().unwrap();

        state.request_stop(&boss.task_id).unwrap();

        assert!(boss_worker.cancelled.load(Ordering::SeqCst));
        assert!(!liepin_worker.cancelled.load(Ordering::SeqCst));
        assert_eq!(
            state.tasks[&boss.task_id].info.status,
            JobTaskState::Stopping
        );
        assert_eq!(
            state.tasks[&liepin.task_id].info.status,
            JobTaskState::Starting
        );
    }

    #[test]
    fn cancelling_queued_task_removes_it_from_queue() {
        let mut state = SchedulerState::new(1);
        let first = enqueue(&mut state, PlatformKind::Boss);
        let queued = enqueue(&mut state, PlatformKind::Liepin);
        let _ = state.take_next_runnable().unwrap();

        state.request_stop(&queued.task_id).unwrap();

        assert!(state.queue.is_empty());
        assert_eq!(
            state.tasks[&queued.task_id].info.status,
            JobTaskState::Cancelled
        );
        assert_eq!(state.running_count, 1);
        assert_eq!(
            state.tasks[&first.task_id].info.status,
            JobTaskState::Starting
        );
    }

    #[test]
    fn duplicate_periodic_task_for_the_same_platform_is_rejected() {
        let mut state = SchedulerState::new(2);
        state
            .enqueue(
                PlatformKind::Boss,
                FlowMode::PeriodicJobHunting,
                Some(30),
                Arc::new(default_app_config()),
                None,
            )
            .unwrap();

        let error = state
            .enqueue(
                PlatformKind::Boss,
                FlowMode::PeriodicJobHunting,
                Some(60),
                Arc::new(default_app_config()),
                None,
            )
            .unwrap_err();

        assert!(error.contains("已有周期投递任务"));
    }

    #[test]
    fn task_info_exposes_bound_profile_metadata() {
        let mut state = SchedulerState::new(1);
        let info = state
            .enqueue(
                PlatformKind::Boss,
                FlowMode::JobHunting,
                None,
                Arc::new(default_app_config()),
                Some(JobTaskProfile {
                    profile_id: Some("rust".into()),
                    profile_name: Some("Rust 后端".into()),
                    profile_snapshot_id: Some("fnv-1".into()),
                }),
            )
            .unwrap();

        assert_eq!(info.profile_id.as_deref(), Some("rust"));
        assert_eq!(info.profile_name.as_deref(), Some("Rust 后端"));
        assert_eq!(info.profile_snapshot_id.as_deref(), Some("fnv-1"));
    }
}
