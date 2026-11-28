use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result};
use chrono::Local;
use serde::{Deserialize, Serialize};

/// 单个平台的同步元数据（上次同步时间 + 增量水位线）。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct PlatformSyncMeta {
    pub platform: String,
    #[serde(default)]
    pub last_synced_at: Option<String>,
    /// 已同步到的最新消息毫秒时间戳；下次增量扫描命中此水位线即可提前截断。
    #[serde(default)]
    pub watermark_ms: i64,
    #[serde(default)]
    pub last_scanned: usize,
    #[serde(default)]
    pub last_updated: usize,
    #[serde(default)]
    pub last_unread_total: u32,
    #[serde(default)]
    pub last_error: Option<String>,
}

/// 零点击列表同步采集到的单岗位实时信号（不覆盖 `JobDetail` 主结构，保证完全向后兼容）。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SyncedJobSignal {
    pub job_id: String,
    pub platform: String,
    #[serde(default)]
    pub unread_count: u32,
    #[serde(default)]
    pub last_message_preview: Option<String>,
    #[serde(default)]
    pub last_message_at: Option<i64>,
    #[serde(default)]
    pub last_message_received: Option<bool>,
    #[serde(default)]
    pub platform_status_tag: Option<String>,
    #[serde(default)]
    pub interview_invited: bool,
    #[serde(default)]
    pub rejected_by_platform: bool,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncStateFile {
    #[serde(default)]
    pub platforms: HashMap<String, PlatformSyncMeta>,
    #[serde(default)]
    pub signals: HashMap<String, SyncedJobSignal>,
}

struct SyncStateStore {
    path: PathBuf,
    state: Mutex<SyncStateFile>,
}

static STORE: OnceLock<SyncStateStore> = OnceLock::new();

pub fn init(data_dir: &Path) -> Result<()> {
    let path = data_dir.join("sync_state.json");
    let loaded = if path.exists() {
        let raw = fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str::<SyncStateFile>(&raw).unwrap_or_default()
    } else {
        SyncStateFile::default()
    };
    let _ = STORE.set(SyncStateStore {
        path,
        state: Mutex::new(loaded),
    });
    Ok(())
}

fn persist_locked(path: &Path, state: &SyncStateFile) -> Result<()> {
    let _permit = crate::storage::read_lock();
    let json = serde_json::to_string_pretty(state).context("序列化同步状态失败")?;
    crate::storage::atomic::atomic_write(path, json.as_bytes())
        .map_err(anyhow::Error::from)
        .context("写入同步状态文件失败")
}

pub fn export_state() -> SyncStateFile {
    let Some(store) = STORE.get() else {
        return SyncStateFile::default();
    };
    let guard = store.state.lock().unwrap_or_else(|e| e.into_inner());
    guard.clone()
}

pub fn replace_state(state: SyncStateFile) -> Result<()> {
    let Some(store) = STORE.get() else {
        return Ok(());
    };
    {
        let mut guard = store.state.lock().unwrap_or_else(|e| e.into_inner());
        *guard = state;
        persist_locked(&store.path, &guard)?;
    }
    super::notify_job_data_changed();
    Ok(())
}

pub fn merge_state(incoming: SyncStateFile) -> Result<usize> {
    let Some(store) = STORE.get() else {
        return Ok(0);
    };
    let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let changed = {
        let mut guard = store.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut count = 0usize;

        for (key, mut meta) in incoming.platforms {
            let platform = if meta.platform.trim().is_empty() {
                key.trim().to_string()
            } else {
                meta.platform.trim().to_string()
            };
            if platform.is_empty() {
                continue;
            }
            meta.platform = platform.clone();
            let should_replace = match guard.platforms.get(&platform) {
                None => true,
                Some(existing) => {
                    meta.watermark_ms > existing.watermark_ms
                        || (meta.watermark_ms == existing.watermark_ms
                            && meta.last_synced_at > existing.last_synced_at)
                }
            };
            if should_replace {
                guard.platforms.insert(platform, meta);
                count += 1;
            }
        }

        for (key, mut sig) in incoming.signals {
            if sig.job_id.trim().is_empty() {
                sig.job_id = key.trim().to_string();
            }
            if sig.job_id.trim().is_empty() {
                continue;
            }
            let next = match guard.signals.get(&sig.job_id) {
                Some(existing) => {
                    let merged = merge_signal(existing, sig);
                    if signal_semantically_eq(existing, &merged) {
                        continue;
                    }
                    SyncedJobSignal {
                        updated_at: if merged.updated_at.trim().is_empty() {
                            now.clone()
                        } else {
                            merged.updated_at
                        },
                        ..merged
                    }
                }
                None => {
                    if sig.updated_at.trim().is_empty() {
                        sig.updated_at = now.clone();
                    }
                    sig
                }
            };
            guard.signals.insert(next.job_id.clone(), next);
            count += 1;
        }

        if count > 0 {
            persist_locked(&store.path, &guard)?;
        }
        count
    };
    if changed > 0 {
        super::notify_job_data_changed();
    }
    Ok(changed)
}

pub fn remove_signals(job_ids: &[String]) -> Result<usize> {
    if job_ids.is_empty() {
        return Ok(0);
    }
    let Some(store) = STORE.get() else {
        return Ok(0);
    };
    let removed = {
        let mut guard = store.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut count = 0usize;
        for id in job_ids {
            if guard.signals.remove(id).is_some() {
                count += 1;
            }
        }
        if count > 0 {
            persist_locked(&store.path, &guard)?;
        }
        count
    };
    if removed > 0 {
        super::notify_job_data_changed();
    }
    Ok(removed)
}

pub fn list_platform_meta() -> Vec<PlatformSyncMeta> {
    let Some(store) = STORE.get() else {
        return Vec::new();
    };
    let guard = store.state.lock().unwrap_or_else(|e| e.into_inner());
    let mut items: Vec<PlatformSyncMeta> = guard.platforms.values().cloned().collect();
    items.sort_by(|a, b| a.platform.cmp(&b.platform));
    items
}

pub fn get_platform_meta(platform: &str) -> PlatformSyncMeta {
    let Some(store) = STORE.get() else {
        return PlatformSyncMeta {
            platform: platform.to_string(),
            ..Default::default()
        };
    };
    let guard = store.state.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .platforms
        .get(platform)
        .cloned()
        .unwrap_or_else(|| PlatformSyncMeta {
            platform: platform.to_string(),
            ..Default::default()
        })
}

pub fn save_platform_meta(meta: PlatformSyncMeta) -> Result<()> {
    let Some(store) = STORE.get() else {
        return Ok(());
    };
    {
        let mut guard = store.state.lock().unwrap_or_else(|e| e.into_inner());
        guard.platforms.insert(meta.platform.clone(), meta);
        persist_locked(&store.path, &guard)?;
    }
    super::notify_job_data_changed();
    Ok(())
}

pub fn list_signals() -> HashMap<String, SyncedJobSignal> {
    let Some(store) = STORE.get() else {
        return HashMap::new();
    };
    let guard = store.state.lock().unwrap_or_else(|e| e.into_inner());
    guard.signals.clone()
}

pub(crate) fn signal_semantically_eq(a: &SyncedJobSignal, b: &SyncedJobSignal) -> bool {
    a.job_id == b.job_id
        && a.platform == b.platform
        && a.unread_count == b.unread_count
        && a.last_message_preview == b.last_message_preview
        && a.last_message_at == b.last_message_at
        && a.last_message_received == b.last_message_received
        && a.platform_status_tag == b.platform_status_tag
        && a.interview_invited == b.interview_invited
        && a.rejected_by_platform == b.rejected_by_platform
}

/// 批量写入零点击采集的岗位信号；同一岗位仅在语义字段发生变化时写盘。
pub fn upsert_signals(items: Vec<SyncedJobSignal>) -> Result<usize> {
    if items.is_empty() {
        return Ok(0);
    }
    let Some(store) = STORE.get() else {
        return Ok(0);
    };
    let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let changed = {
        let mut guard = store.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut count = 0usize;

        for mut incoming in items {
            if incoming.job_id.trim().is_empty() {
                continue;
            }
            let next = match guard.signals.get(&incoming.job_id) {
                Some(existing) => {
                    let merged = merge_signal(existing, incoming);
                    if signal_semantically_eq(existing, &merged) {
                        continue;
                    }
                    SyncedJobSignal {
                        updated_at: now.clone(),
                        ..merged
                    }
                }
                None => {
                    incoming.updated_at = now.clone();
                    incoming
                }
            };
            guard.signals.insert(next.job_id.clone(), next);
            count += 1;
        }

        if count > 0 {
            persist_locked(&store.path, &guard)?;
        }
        count
    };

    if changed > 0 {
        super::notify_job_data_changed();
    }
    Ok(changed)
}

/// 当某个岗位已经被点开处理未读后，清零本地记录的未读角标。
pub fn clear_unread_for_job(job_id: &str) {
    let Some(store) = STORE.get() else {
        return;
    };
    let changed = {
        let mut guard = store.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(signal) = guard.signals.get_mut(job_id) {
            if signal.unread_count > 0 {
                signal.unread_count = 0;
                let _ = persist_locked(&store.path, &guard);
                true
            } else {
                false
            }
        } else {
            false
        }
    };
    if changed {
        super::notify_job_data_changed();
    }
}

pub(crate) fn merge_signal(
    existing: &SyncedJobSignal,
    incoming: SyncedJobSignal,
) -> SyncedJobSignal {
    let existing_ts = existing.last_message_at.unwrap_or(0);
    let incoming_ts = incoming.last_message_at.unwrap_or(0);
    let existing_has_ts = existing.last_message_at.is_some_and(|ts| ts > 0);
    let incoming_has_ts = incoming.last_message_at.is_some_and(|ts| ts > 0);
    let incoming_is_timeless_over_dated = existing_has_ts && !incoming_has_ts;
    let incoming_is_newer = !incoming_is_timeless_over_dated && incoming_ts >= existing_ts;

    let unread_count = if incoming_is_timeless_over_dated {
        // 无时间戳的 DOM 兜底信号不得清零已记录的未读数，但可叠加更高的未读观测
        existing.unread_count.max(incoming.unread_count)
    } else if incoming_is_newer || !existing_has_ts {
        // 未读数以本次最新列表观测为准（列表上未读清零代表用户已在手机/网页端读过）
        incoming.unread_count
    } else {
        existing.unread_count
    };

    SyncedJobSignal {
        job_id: incoming.job_id,
        platform: incoming.platform,
        unread_count,
        last_message_preview: if incoming_is_newer {
            incoming
                .last_message_preview
                .or_else(|| existing.last_message_preview.clone())
        } else {
            existing
                .last_message_preview
                .clone()
                .or(incoming.last_message_preview)
        },
        last_message_at: Some(existing_ts.max(incoming_ts)).filter(|ts| *ts > 0),
        last_message_received: if incoming_is_newer {
            incoming
                .last_message_received
                .or(existing.last_message_received)
        } else {
            existing
                .last_message_received
                .or(incoming.last_message_received)
        },
        platform_status_tag: if incoming_is_newer {
            incoming
                .platform_status_tag
                .or_else(|| existing.platform_status_tag.clone())
        } else {
            existing
                .platform_status_tag
                .clone()
                .or(incoming.platform_status_tag)
        },
        // 面试邀约与平台拒绝属于正向累积信号，一旦确认不会因旧列表回退
        interview_invited: existing.interview_invited || incoming.interview_invited,
        rejected_by_platform: existing.rejected_by_platform || incoming.rejected_by_platform,
        updated_at: if incoming.updated_at.is_empty() {
            existing.updated_at.clone()
        } else {
            incoming.updated_at
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_signal_keeps_newer_preview_and_updates_unread_count() {
        let old = SyncedJobSignal {
            job_id: "job-1".into(),
            platform: "boss".into(),
            unread_count: 2,
            last_message_preview: Some("你好，可以发简历吗".into()),
            last_message_at: Some(1_000),
            last_message_received: Some(true),
            platform_status_tag: None,
            interview_invited: false,
            rejected_by_platform: false,
            updated_at: "t1".into(),
        };
        let next = SyncedJobSignal {
            job_id: "job-1".into(),
            platform: "boss".into(),
            unread_count: 0,
            last_message_preview: Some("约明天下午 3 点面试方便吗？".into()),
            last_message_at: Some(2_000),
            last_message_received: Some(true),
            platform_status_tag: Some("约面试".into()),
            interview_invited: true,
            rejected_by_platform: false,
            updated_at: "t2".into(),
        };

        let merged = merge_signal(&old, next);
        assert_eq!(merged.unread_count, 0);
        assert_eq!(
            merged.last_message_preview.as_deref(),
            Some("约明天下午 3 点面试方便吗？")
        );
        assert!(merged.interview_invited);
        assert_eq!(merged.platform_status_tag.as_deref(), Some("约面试"));
    }

    #[test]
    fn signal_semantically_eq_ignores_updated_at() {
        let a = SyncedJobSignal {
            job_id: "job-1".into(),
            platform: "boss".into(),
            unread_count: 1,
            last_message_preview: Some("您好".into()),
            last_message_at: Some(1_000),
            last_message_received: Some(true),
            platform_status_tag: Some("1 条未读".into()),
            interview_invited: false,
            rejected_by_platform: false,
            updated_at: "2026-11-28 10:00:00".into(),
        };
        let mut b = a.clone();
        b.updated_at = "2026-11-28 10:05:00".into();
        assert!(signal_semantically_eq(&a, &b));

        b.unread_count = 0;
        assert!(!signal_semantically_eq(&a, &b));
    }

    #[test]
    fn timeless_dom_signal_preserves_dated_status_tag_and_unread_count() {
        let dated = SyncedJobSignal {
            job_id: "job-1".into(),
            platform: "boss".into(),
            unread_count: 2,
            last_message_preview: Some("约明天下午面试".into()),
            last_message_at: Some(5_000),
            last_message_received: Some(true),
            platform_status_tag: Some("收到约面".into()),
            interview_invited: true,
            rejected_by_platform: false,
            updated_at: "t1".into(),
        };
        let dom_zero = SyncedJobSignal {
            job_id: "job-1".into(),
            platform: "boss".into(),
            unread_count: 0,
            last_message_preview: Some("较弱的DOM摘要".into()),
            last_message_at: None,
            last_message_received: None,
            platform_status_tag: Some("沟通中".into()),
            interview_invited: false,
            rejected_by_platform: false,
            updated_at: "t2".into(),
        };

        let merged = merge_signal(&dated, dom_zero);
        assert_eq!(merged.unread_count, 2);
        assert_eq!(merged.last_message_at, Some(5_000));
        assert_eq!(merged.last_message_preview.as_deref(), Some("约明天下午面试"));
        assert_eq!(merged.platform_status_tag.as_deref(), Some("收到约面"));
        assert!(merged.interview_invited);

        // If timeless DOM sees higher unread count, keep the max without clobbering status tag
        let dom_higher = SyncedJobSignal {
            job_id: "job-1".into(),
            platform: "boss".into(),
            unread_count: 3,
            last_message_preview: None,
            last_message_at: None,
            last_message_received: Some(true),
            platform_status_tag: Some("3 条未读".into()),
            interview_invited: false,
            rejected_by_platform: false,
            updated_at: "t3".into(),
        };
        let merged_higher = merge_signal(&dated, dom_higher);
        assert_eq!(merged_higher.unread_count, 3);
        assert_eq!(merged_higher.platform_status_tag.as_deref(), Some("收到约面"));
    }
}
