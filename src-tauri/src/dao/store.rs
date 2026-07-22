use crate::dao::model::{ChatMessageRecord, InterviewJobAnalysis, JobDetail};
use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

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
            _phantom: PhantomData,
        })
    }

    /// 读取全部数据
    pub fn load_all(&self) -> Result<Vec<T>> {
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
    fn save_all(&self, items: &[T]) -> Result<()> {
        let _permit = crate::storage::read_lock();
        let content = serde_json::to_string_pretty(items).with_context(|| "序列化数据失败")?;
        crate::storage::atomic::atomic_write(&self.file_path, content.as_bytes())
            .map_err(anyhow::Error::from)
            .with_context(|| "写入数据文件失败")
    }

    /// 新增一条记录
    pub fn insert(&self, item: T) -> Result<()> {
        let mut items = self.load_all()?;
        items.push(item);
        self.save_all(&items)
    }

    /// 按 ID 查询
    pub fn get_by_id(&self, id: &str) -> Result<Option<T>> {
        let items = self.load_all()?;
        Ok(items.into_iter().find(|item| item.id() == id))
    }

    /// 更新指定 ID 的记录
    pub fn update_by_id(&self, id: &str, updated: T) -> Result<bool> {
        let mut items = self.load_all()?;
        let index = items.iter().position(|item| item.id() == id);
        match index {
            Some(i) => {
                items[i] = updated;
                self.save_all(&items)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// 删除指定 ID 的记录
    pub fn delete_by_id(&self, id: &str) -> Result<bool> {
        let mut items = self.load_all()?;
        let original_len = items.len();
        items.retain(|item| item.id() != id);
        if items.len() < original_len {
            self.save_all(&items)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// 条件查询
    pub fn query<F>(&self, predicate: F) -> Result<Vec<T>>
    where
        F: Fn(&T) -> bool,
    {
        let items = self.load_all()?;
        Ok(items.into_iter().filter(predicate).collect())
    }

    /// 批量写入（覆盖）
    pub fn replace_all(&self, items: Vec<T>) -> Result<()> {
        self.save_all(&items)
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
}
