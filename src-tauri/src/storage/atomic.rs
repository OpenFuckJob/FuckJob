use crate::error::AppError;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    atomic_write_with(path, |file| file.write_all(bytes))
}

pub(crate) fn atomic_write_with<F>(path: &Path, writer: F) -> Result<(), AppError>
where
    F: FnOnce(&mut File) -> io::Result<()>,
{
    let parent = usable_parent(path);
    fs::create_dir_all(parent).map_err(|error| storage_error("无法创建数据目录", path, error))?;

    let temp_path = unique_sibling(path, "tmp")?;
    let mut temp_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .map_err(|error| storage_error("无法创建临时文件", &temp_path, error))?;

    let write_result = writer(&mut temp_file).and_then(|()| temp_file.sync_all());
    drop(temp_file);
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(storage_error("无法安全写入数据", path, error));
    }

    if let Err(error) = replace_destination(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(storage_error("无法替换数据文件", path, error));
    }

    Ok(())
}

fn usable_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn unique_sibling(path: &Path, suffix: &str) -> Result<PathBuf, AppError> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            AppError::storage("数据文件路径无效").with_detail(path.display().to_string())
        })?;
    Ok(usable_parent(path).join(format!(".{file_name}.{}.{}", Uuid::new_v4(), suffix)))
}

#[cfg(not(windows))]
fn replace_destination(temp_path: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(temp_path, destination)
}

#[cfg(windows)]
fn replace_destination(temp_path: &Path, destination: &Path) -> io::Result<()> {
    use std::{iter, os::windows::ffi::OsStrExt};

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

    #[link(name = "Kernel32")]
    extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }

    let existing: Vec<u16> = temp_path
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect();
    let new: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect();
    let result = unsafe {
        MoveFileExW(
            existing.as_ptr(),
            new.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn storage_error(message: &str, path: &Path, error: io::Error) -> AppError {
    AppError::storage(message).with_detail(format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{atomic_write, atomic_write_with};
    use std::{fs, io, io::Write};

    fn temp_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        fs::read_dir(dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(".tmp"))
            })
            .collect()
    }

    #[test]
    fn atomic_write_creates_parent_and_replaces_destination() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("config.yaml");

        atomic_write(&path, b"first").unwrap();
        atomic_write(&path, b"second").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"second");
        assert!(temp_files(path.parent().unwrap()).is_empty());
    }

    #[test]
    fn failed_temp_write_preserves_original_and_cleans_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(&path, b"valid original").unwrap();

        let result = atomic_write_with(&path, |file| {
            file.write_all(b"partial replacement")?;
            Err(io::Error::other("simulated failure"))
        });

        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"valid original");
        assert!(temp_files(dir.path()).is_empty());
    }
}
