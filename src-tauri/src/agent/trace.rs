//! Agent 调用轨迹的内存缓冲。
//!
//! 调提示词以前只能靠日志倒推：日志里为了不泄露简历原文和岗位 JD，只留了原因和长度，
//! 于是「模型到底收到了什么、又吐回了什么」永远缺失，改一版提示词要重跑一遍真实投递才能看效果。
//! 这里在 [`crate::agent::run::AgentRunner::execute`] 这个唯一入口上旁路留一份完整轨迹，
//! 测试模式页面据此展示每一轮的提示词、原始输出与判定结果。
//!
//! **只驻内存，绝不落盘**。轨迹里装着日志刻意回避的隐私内容，一旦随应用日志一起躺在磁盘上，
//! 就等于把用户简历长期留在了一个谁都能读的文件里。只有用户显式点导出、
//! 自己选好落点时才由 [`export`] 写文件。进程退出即丢失是有意为之的取舍。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::llm::types::LlmUsage;

/// 一轮模型调用的结局
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RoundVerdict {
    /// 解析与校验都通过
    Passed,
    /// 输出拿到了，但没通过解析或校验
    Rejected { reason: String },
    /// 调用本身失败（网络、鉴权、全链降级耗尽等）
    Failed { reason: String },
}

/// 一轮模型调用的完整记录
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoundTrace {
    /// 从 1 起算
    pub round: u32,
    /// 实际送出去的完整提示词，含返工追加段
    pub prompt: String,
    /// 净化后的模型输出。调用失败时为空串
    pub raw: String,
    pub model: Option<String>,
    pub usage: Option<LlmUsage>,
    pub duration_ms: u64,
    pub verdict: RoundVerdict,
}

/// 一次 Agent 任务运行的完整轨迹
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentTrace {
    pub id: String,
    /// 任务名，取自 `AgentTask::name()`
    pub task_name: String,
    /// RFC3339 本地时间
    pub started_at: String,
    pub duration_ms: u64,
    pub rounds: Vec<RoundTrace>,
    /// 成功时为 "first_try" 或 "recovered"，失败时为 None
    pub stop: Option<String>,
    /// 整体失败原因，成功时为 None
    pub error: Option<String>,
}

/// 环形缓冲容量。
///
/// 200 条按一次运行上限两轮、每轮几 KB 提示词估算，最坏也就是几 MB 常驻内存；
/// 再大就要为「调提示词」这件辅助工作长期占用用户内存，不划算。
pub const TRACE_CAPACITY: usize = 200;

static TRACES: Lazy<Mutex<VecDeque<AgentTrace>>> =
    Lazy::new(|| Mutex::new(VecDeque::with_capacity(TRACE_CAPACITY)));

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// 拿缓冲的锁，遇到中毒直接接管里面的数据继续用。
///
/// 追踪是辅助设施，绝不能反过来搞垮它监控的主链路：某个线程在持锁期间 panic 之后，
/// 如果这里跟着 `unwrap()`，后续每一次大模型调用都会连带 panic，
/// 等于「为了看提示词把投递功能弄挂了」。而本模块持锁期间只做 `VecDeque` 的
/// 增删读，panic 不会让容器处于逻辑上半成品的状态，接管旧数据是安全的。
fn buffer() -> MutexGuard<'static, VecDeque<AgentTrace>> {
    TRACES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 记录一条轨迹，超出容量时挤掉最旧的一条
pub fn record(trace: AgentTrace) {
    let mut buffer = buffer();
    while buffer.len() >= TRACE_CAPACITY {
        buffer.pop_front();
    }
    buffer.push_back(trace);
}

/// 取轨迹，**按时间倒序**（最新的在前）。
/// `ids` 为空表示返回全部；否则只返回 id 命中的那些，顺序同样是时间倒序
pub fn recent(ids: &[String]) -> Vec<AgentTrace> {
    let buffer = buffer();
    buffer
        .iter()
        .rev()
        .filter(|trace| ids.is_empty() || ids.iter().any(|id| id == &trace.id))
        .cloned()
        .collect()
}

/// 清空缓冲
pub fn clear() {
    buffer().clear();
}

/// 导出全部轨迹为 JSON 文件（数组，时间倒序），返回导出条数。
///
/// 这是本模块唯一写盘的地方，且只在用户显式点导出、自己选定落点时才会被调到。
pub fn export(path: &std::path::Path) -> Result<usize, AppError> {
    let traces = recent(&[]);
    let payload = serde_json::to_string_pretty(&traces)
        .map_err(|error| AppError::internal("序列化调用轨迹失败").with_detail(error.to_string()))?;

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|error| {
                AppError::storage("创建轨迹导出目录失败").with_detail(error.to_string())
            })?;
        }
    }
    std::fs::write(path, payload).map_err(|error| {
        AppError::storage("写入轨迹导出文件失败").with_detail(error.to_string())
    })?;

    Ok(traces.len())
}

/// 分配一个新的轨迹 id。
///
/// 自增计数器而不是 uuid：id 只需要在单次进程生命周期内唯一（缓冲本来就不跨进程），
/// 顺序编号还能让人一眼看出先后，排查时比一串随机十六进制有用。
pub fn next_id() -> String {
    format!("trace-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 缓冲是全局的，而同一个测试二进制里的测试默认并行跑。
    /// 项目没有 `serial_test` 依赖，也不值得为几个测试引一个，
    /// 于是照搬 `logger.rs` 里的做法：用一把测试专用的锁把这组测试串起来。
    /// 锁中毒时接管而不是 unwrap，免得一个失败的测试把其余测试全带成 panic。
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn isolated() -> MutexGuard<'static, ()> {
        let guard = TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        clear();
        guard
    }

    fn sample(id: &str, task_name: &str) -> AgentTrace {
        AgentTrace {
            id: id.to_string(),
            task_name: task_name.to_string(),
            started_at: "2026-01-01T00:00:00+08:00".to_string(),
            duration_ms: 12,
            rounds: vec![RoundTrace {
                round: 1,
                prompt: "提示词".to_string(),
                raw: "输出".to_string(),
                model: Some("test-model".to_string()),
                usage: None,
                duration_ms: 10,
                verdict: RoundVerdict::Passed,
            }],
            stop: Some("first_try".to_string()),
            error: None,
        }
    }

    #[test]
    fn overflow_drops_the_oldest_and_never_exceeds_capacity() {
        let _guard = isolated();

        for index in 0..TRACE_CAPACITY + 5 {
            record(sample(&format!("overflow-{index}"), "overflow"));
        }

        let traces = recent(&[]);
        assert_eq!(traces.len(), TRACE_CAPACITY);
        // 最旧的 5 条被挤掉，最新的一条还在
        assert_eq!(
            traces.first().unwrap().id,
            format!("overflow-{}", TRACE_CAPACITY + 4)
        );
        assert_eq!(traces.last().unwrap().id, "overflow-5");
        assert!(traces.iter().all(|trace| trace.id != "overflow-0"));
    }

    #[test]
    fn recent_returns_newest_first() {
        let _guard = isolated();

        record(sample("trace-1", "order"));
        record(sample("trace-2", "order"));
        record(sample("trace-3", "order"));

        let ids: Vec<String> = recent(&[])
            .into_iter()
            .filter(|trace| trace.task_name == "order")
            .map(|trace| trace.id)
            .collect();

        assert_eq!(ids, vec!["trace-3", "trace-2", "trace-1"]);
    }

    #[test]
    fn recent_with_ids_only_returns_the_matching_ones() {
        let _guard = isolated();

        record(sample("trace-1", "filter"));
        record(sample("trace-2", "filter"));
        record(sample("trace-3", "filter"));

        let hit = recent(&["trace-2".to_string()]);

        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].id, "trace-2");
    }

    #[test]
    fn recent_with_ids_keeps_newest_first_order() {
        let _guard = isolated();

        record(sample("trace-1", "filter-order"));
        record(sample("trace-2", "filter-order"));
        record(sample("trace-3", "filter-order"));

        let ids: Vec<String> = recent(&["trace-1".to_string(), "trace-3".to_string()])
            .into_iter()
            .map(|trace| trace.id)
            .collect();

        assert_eq!(ids, vec!["trace-3", "trace-1"]);
    }

    #[test]
    fn clear_empties_the_buffer() {
        let _guard = isolated();

        record(sample("trace-1", "clear"));
        clear();

        assert!(recent(&[]).is_empty());
    }

    #[test]
    fn ids_are_unique_and_prefixed() {
        // 不碰缓冲，因此不需要串行锁
        let first = next_id();
        let second = next_id();

        assert!(first.starts_with("trace-"));
        assert_ne!(first, second);
    }

    #[test]
    fn export_writes_a_newest_first_json_array_and_reports_the_count() {
        let _guard = isolated();
        let dir = tempfile::tempdir().expect("建临时目录");
        let path = dir.path().join("nested").join("traces.json");

        record(sample("trace-1", "export"));
        record(sample("trace-2", "export"));
        let count = export(&path).expect("导出轨迹");

        let restored: Vec<AgentTrace> =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("读回导出文件"))
                .expect("导出的是合法 JSON 数组");

        assert_eq!(count, 2);
        assert_eq!(restored.len(), 2);
        assert_eq!(restored[0].id, "trace-2");
        assert_eq!(restored[1].id, "trace-1");
    }

    #[test]
    fn verdicts_serialize_with_a_tagged_kind() {
        let rejected = serde_json::to_value(RoundVerdict::Rejected {
            reason: "不是合法 JSON".to_string(),
        })
        .expect("序列化判定");

        assert_eq!(rejected["kind"], "rejected");
        assert_eq!(rejected["reason"], "不是合法 JSON");
        assert_eq!(
            serde_json::to_value(RoundVerdict::Passed).expect("序列化判定")["kind"],
            "passed"
        );
    }
}
