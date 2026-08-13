use crate::dao::model::{ChatMessageRecord, InterviewJobAnalysis, JobDetail};
use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 批量操作结果
#[derive(Debug, Default, Clone, Copy)]
pub struct BatchResult {
    pub added: usize,
    pub updated: usize,
}

/// 可标识 trait，统一主键访问
pub trait Identifiable {
    fn id(&self) -> &str;
}

impl Identifiable for JobDetail {
    fn id(&self) -> &str {
        &self.id
    }
}

impl Identifiable for InterviewJobAnalysis {
    fn id(&self) -> &str {
        &self.job_id
    }
}

impl Identifiable for ChatMessageRecord {
    fn id(&self) -> &str {
        &self.id
    }
}

/// 通用 JSON 文件存储引擎
pub struct JsonStore<T> {
    file_path: PathBuf,
    /// Serializes a complete read-modify-write transaction for this store.
    /// Read-only operations intentionally do not take this lock.
    mutation_lock: Mutex<()>,
    _phantom: PhantomData<T>,
}

impl<T: Serialize + DeserializeOwned + Identifiable> JsonStore<T> {
    /// 创建存储实例
    /// `data_dir` 为 app_data_dir，文件将存放在 `{data_dir}/data/{file_name}` 下
    pub fn new(data_dir: &Path, file_name: &str) -> Result<Self> {
        let dir = data_dir.join("data");
        fs::create_dir_all(&dir).with_context(|| format!("创建数据目录失败: {}", dir.display()))?;
        Ok(Self {
            file_path: dir.join(file_name),
            mutation_lock: Mutex::new(()),
            _phantom: PhantomData,
        })
    }

    /// 读取全部数据
    pub fn load_all(&self) -> Result<Vec<T>> {
        self.load_all_unlocked()
    }

    fn load_all_unlocked(&self) -> Result<Vec<T>> {
        if !self.file_path.exists() {
            return Ok(Vec::new());
        }
        let content = fs::read_to_string(&self.file_path).with_context(|| "读取数据文件失败")?;
        if content.trim().is_empty() {
            return Ok(Vec::new());
        }
        serde_json::from_str(&content).with_context(|| "解析 JSON 数据失败")
    }

    /// 写入全部数据
    fn save_all_unlocked(&self, items: &[T]) -> Result<()> {
        let _permit = crate::storage::read_lock();
        let content = serde_json::to_string_pretty(items).with_context(|| "序列化数据失败")?;
        crate::storage::atomic::atomic_write(&self.file_path, content.as_bytes())
            .map_err(anyhow::Error::from)
            .with_context(|| "写入数据文件失败")
    }

    /// 新增一条记录
    pub fn insert(&self, item: T) -> Result<()> {
        self.transaction(|items| {
            items.push(item);
            Ok(((), true))
        })
    }

    /// 按 ID 查询
    pub fn get_by_id(&self, id: &str) -> Result<Option<T>> {
        let items = self.load_all()?;
        Ok(items.into_iter().find(|item| item.id() == id))
    }

    /// 更新指定 ID 的记录
    pub fn update_by_id(&self, id: &str, updated: T) -> Result<bool> {
        self.transaction(
            |items| match items.iter().position(|item| item.id() == id) {
                Some(i) => {
                    items[i] = updated;
                    Ok((true, true))
                }
                None => Ok((false, false)),
            },
        )
    }

    /// 删除指定 ID 的记录
    pub fn delete_by_id(&self, id: &str) -> Result<bool> {
        self.transaction(|items| {
            let original_len = items.len();
            items.retain(|item| item.id() != id);
            let changed = items.len() < original_len;
            Ok((changed, changed))
        })
    }

    /// 条件查询
    pub fn query<F>(&self, predicate: F) -> Result<Vec<T>>
    where
        F: Fn(&T) -> bool,
    {
        let items = self.load_all()?;
        Ok(items.into_iter().filter(predicate).collect())
    }

    /// 批量 upsert：一次读、内存合并、一次写
    /// `should_update` 决定当 incoming 与 existing 的 id 相同时，是否用 incoming 替换 existing
    pub fn batch_upsert<F>(&self, incoming: Vec<T>, should_update: F) -> Result<BatchResult>
    where
        F: Fn(&T, &T) -> bool,
    {
        self.transaction(|items| {
            let mut index_map: HashMap<String, usize> = items
                .iter()
                .enumerate()
                .map(|(i, item)| (item.id().to_string(), i))
                .collect();

            let mut result = BatchResult::default();

            for incoming_item in incoming {
                let id = incoming_item.id().to_string();
                if let Some(&idx) = index_map.get(&id) {
                    if should_update(&items[idx], &incoming_item) {
                        items[idx] = incoming_item;
                        result.updated += 1;
                    }
                } else {
                    index_map.insert(id, items.len());
                    items.push(incoming_item);
                    result.added += 1;
                }
            }

            let changed = result.added > 0 || result.updated > 0;
            Ok((result, changed))
        })
    }

    /// 批量插入不存在的记录（跳过已存在的 ID）
    pub fn batch_insert_new(&self, incoming: Vec<T>) -> Result<usize> {
        self.transaction(|items| {
            let mut existing_ids: std::collections::HashSet<String> =
                items.iter().map(|item| item.id().to_string()).collect();

            let mut added = 0usize;
            for item in incoming {
                if existing_ids.insert(item.id().to_string()) {
                    items.push(item);
                    added += 1;
                }
            }

            Ok((added, added > 0))
        })
    }

    /// 批量写入（覆盖）
    pub fn replace_all(&self, items: Vec<T>) -> Result<()> {
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|error| anyhow::anyhow!("获取数据写锁失败: {error}"))?;
        self.save_all_unlocked(&items)
    }

    /// 在同一个互斥区内完成读取、修改和按需写回。
    ///
    /// 闭包返回值中的 `bool` 表示数据是否发生变化；仅在为 `true` 时写入文件。
    /// 传给闭包的集合已在锁内读取，闭包中不要再次调用本 store 的写方法，
    /// 否则会尝试重复获取同一把非重入锁。
    pub fn transaction<R, F>(&self, mutation: F) -> Result<R>
    where
        F: FnOnce(&mut Vec<T>) -> Result<(R, bool)>,
    {
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|error| anyhow::anyhow!("获取数据事务锁失败: {error}"))?;
        let mut items = self.load_all_unlocked()?;
        let (result, changed) = mutation(&mut items)?;
        if changed {
            self.save_all_unlocked(&items)?;
        }
        Ok(result)
    }

    /// 统计记录数
    pub fn count(&self) -> Result<usize> {
        Ok(self.load_all()?.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::sync::{Arc, Barrier};
    use std::thread;

    #[derive(Serialize, Deserialize, Debug, Clone)]
    struct TestItem {
        id: String,
        name: String,
    }

    impl Identifiable for TestItem {
        fn id(&self) -> &str {
            &self.id
        }
    }

    fn setup(tmp: &Path) -> JsonStore<TestItem> {
        JsonStore::new(tmp, "test_items.json").unwrap()
    }

    #[test]
    fn test_insert_and_load() {
        let tmp = tempfile::tempdir().unwrap();
        let store = setup(tmp.path());

        store
            .insert(TestItem {
                id: "1".into(),
                name: "Alice".into(),
            })
            .unwrap();

        let items = store.load_all().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "Alice");
    }

    #[test]
    fn test_get_by_id() {
        let tmp = tempfile::tempdir().unwrap();
        let store = setup(tmp.path());

        store
            .insert(TestItem {
                id: "1".into(),
                name: "Alice".into(),
            })
            .unwrap();

        let found = store.get_by_id("1").unwrap().unwrap();
        assert_eq!(found.name, "Alice");

        assert!(store.get_by_id("999").unwrap().is_none());
    }

    #[test]
    fn test_update_by_id() {
        let tmp = tempfile::tempdir().unwrap();
        let store = setup(tmp.path());

        store
            .insert(TestItem {
                id: "1".into(),
                name: "Alice".into(),
            })
            .unwrap();

        let updated = store
            .update_by_id(
                "1",
                TestItem {
                    id: "1".into(),
                    name: "Bob".into(),
                },
            )
            .unwrap();
        assert!(updated);

        let items = store.load_all().unwrap();
        assert_eq!(items[0].name, "Bob");
    }

    #[test]
    fn test_delete_by_id() {
        let tmp = tempfile::tempdir().unwrap();
        let store = setup(tmp.path());

        store
            .insert(TestItem {
                id: "1".into(),
                name: "Alice".into(),
            })
            .unwrap();
        store
            .insert(TestItem {
                id: "2".into(),
                name: "Bob".into(),
            })
            .unwrap();

        assert!(store.delete_by_id("1").unwrap());
        assert_eq!(store.count().unwrap(), 1);
        assert!(!store.delete_by_id("999").unwrap());
    }

    #[test]
    fn test_query() {
        let tmp = tempfile::tempdir().unwrap();
        let store = setup(tmp.path());

        store
            .insert(TestItem {
                id: "1".into(),
                name: "Alice".into(),
            })
            .unwrap();
        store
            .insert(TestItem {
                id: "2".into(),
                name: "Bob".into(),
            })
            .unwrap();

        let result = store.query(|item| item.name.starts_with('A')).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "Alice");
    }

    #[test]
    fn test_load_empty_file() {
        let tmp = tempfile::tempdir().unwrap();
        let store = setup(tmp.path());

        let items = store.load_all().unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn concurrent_inserts_do_not_lose_updates() {
        const THREADS: usize = 8;
        const INSERTS_PER_THREAD: usize = 25;

        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(setup(tmp.path()));
        let start = Arc::new(Barrier::new(THREADS));
        let mut workers = Vec::new();

        for worker_id in 0..THREADS {
            let store = Arc::clone(&store);
            let start = Arc::clone(&start);
            workers.push(thread::spawn(move || {
                start.wait();
                for item_id in 0..INSERTS_PER_THREAD {
                    store
                        .insert(TestItem {
                            id: format!("{worker_id}:{item_id}"),
                            name: format!("worker-{worker_id}"),
                        })
                        .unwrap();
                }
            }));
        }

        for worker in workers {
            worker.join().unwrap();
        }

        let items = store.load_all().unwrap();
        assert_eq!(items.len(), THREADS * INSERTS_PER_THREAD);
        let unique_ids: std::collections::HashSet<_> =
            items.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(unique_ids.len(), THREADS * INSERTS_PER_THREAD);
    }

    #[test]
    fn concurrent_batch_inserts_do_not_overwrite_each_other() {
        const THREADS: usize = 4;

        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(setup(tmp.path()));
        let start = Arc::new(Barrier::new(THREADS));
        let workers: Vec<_> = (0..THREADS)
            .map(|worker_id| {
                let store = Arc::clone(&store);
                let start = Arc::clone(&start);
                thread::spawn(move || {
                    let batch: Vec<_> = (0..20)
                        .map(|item_id| TestItem {
                            id: format!("batch-{worker_id}:{item_id}"),
                            name: format!("batch-{worker_id}"),
                        })
                        .collect();
                    start.wait();
                    assert_eq!(store.batch_insert_new(batch).unwrap(), 20);
                })
            })
            .collect();

        for worker in workers {
            worker.join().unwrap();
        }

        assert_eq!(store.count().unwrap(), THREADS * 20);
    }
}
