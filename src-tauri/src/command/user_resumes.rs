use crate::command::base::CommandResult;
use crate::{error::AppError, storage::atomic::atomic_write};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};
use tauri::Manager;

const USER_RESUMES_FILE_NAME: &str = "user_resumes.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeEntry {
    pub content: String,
    pub thumbnail: Option<String>,
}

pub type UserResumes = HashMap<String, ResumeEntry>;

#[tauri::command]
pub fn load_user_resumes(app_handle: tauri::AppHandle) -> CommandResult<UserResumes> {
    match user_resumes_path(&app_handle).and_then(|path| load_user_resumes_from_path(&path)) {
        Ok(resumes) => CommandResult::ok(resumes),
        Err(error) => CommandResult::err(error),
    }
}

#[tauri::command]
pub fn save_user_resumes(app_handle: tauri::AppHandle, resumes: UserResumes) -> CommandResult<()> {
    match user_resumes_path(&app_handle).and_then(|path| save_user_resumes_to_path(&path, &resumes))
    {
        Ok(()) => CommandResult::ok(()),
        Err(error) => CommandResult::err(error),
    }
}

fn user_resumes_path(app_handle: &tauri::AppHandle) -> Result<PathBuf, AppError> {
    let data_dir = app_handle.path().app_data_dir().map_err(|error| {
        AppError::storage("无法定位应用数据目录").with_detail(error.to_string())
    })?;
    Ok(user_resumes_path_from_data_dir(&data_dir))
}

fn user_resumes_path_from_data_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("data").join(USER_RESUMES_FILE_NAME)
}

fn load_user_resumes_from_path(path: &Path) -> Result<UserResumes, AppError> {
    if !path.exists() {
        return Ok(UserResumes::new());
    }

    let content = fs::read_to_string(path).map_err(|error| {
        AppError::storage("无法读取简历数据").with_detail(format!("{}: {error}", path.display()))
    })?;
    if content.trim().is_empty() {
        return Ok(UserResumes::new());
    }

    serde_json::from_str(&content).map_err(|error| {
        AppError::storage("简历数据格式损坏").with_detail(format!("{}: {error}", path.display()))
    })
}

fn save_user_resumes_to_path(path: &Path, resumes: &UserResumes) -> Result<(), AppError> {
    let _permit = crate::storage::read_lock();
    save_user_resumes_to_path_unlocked(path, resumes)
}

pub(crate) fn save_user_resumes_to_path_unlocked(
    path: &Path,
    resumes: &UserResumes,
) -> Result<(), AppError> {
    let content = serde_json::to_vec_pretty(resumes)
        .map_err(|error| AppError::storage("无法序列化简历数据").with_detail(error.to_string()))?;
    atomic_write(path, &content)
}

#[cfg(test)]
mod tests {
    use super::{
        load_user_resumes_from_path, save_user_resumes_to_path, user_resumes_path_from_data_dir,
        ResumeEntry, UserResumes,
    };

    #[test]
    fn user_resumes_live_in_the_shared_data_directory() {
        let data_dir = std::path::Path::new("/app-data");

        assert_eq!(
            user_resumes_path_from_data_dir(data_dir),
            data_dir.join("data").join("user_resumes.json")
        );
    }

    #[test]
    fn missing_user_resumes_file_loads_empty_map() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.json");

        let resumes = load_user_resumes_from_path(&path).unwrap();

        assert!(resumes.is_empty());
    }

    #[test]
    fn saved_user_resumes_can_be_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("user_resumes.json");
        let mut resumes = UserResumes::new();
        resumes.insert(
            "后端工程师".to_string(),
            ResumeEntry {
                content: "## 项目经历\n- 负责网关".to_string(),
                thumbnail: None,
            },
        );

        save_user_resumes_to_path(&path, &resumes).unwrap();
        let loaded = load_user_resumes_from_path(&path).unwrap();

        assert_eq!(
            loaded
                .get("后端工程师")
                .map(|resume| resume.content.as_str()),
            Some("## 项目经历\n- 负责网关")
        );
    }
}
