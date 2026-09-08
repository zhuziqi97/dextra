//! Cerebro 专属凭据来源；文件模式不访问系统钱包。

use std::{io::Write, path::{Path, PathBuf}};
use serde::{Deserialize, Serialize};
use crate::app_error::AppCommandError;
use super::identity::{CredentialStore, RunnerCredential, SystemCredentialStore};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StorageMode {
    #[default]
    File,
    Keyring,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageSettings {
    pub mode: StorageMode,
    pub keyring_available: bool,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct StorageConfig {
    mode: StorageMode,
}

pub(super) struct SelectedCredentialStore {
    mode: StorageMode,
    root: PathBuf,
}

fn data_root() -> PathBuf {
    let fallback = dirs::data_dir().unwrap_or_else(|| PathBuf::from(".codeg-data")).join("codeg");
    crate::paths::resolve_effective_data_dir(&fallback)
}

fn read_optional<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>, AppCommandError> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|error| {
            AppCommandError::configuration_invalid(format!("{}: {error}", path.display()))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(AppCommandError::io(error)),
    }
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), AppCommandError> {
    let parent = path.parent().expect("Cerebro 文件始终位于应用数据目录");
    std::fs::create_dir_all(parent).map_err(AppCommandError::io)?;
    // tempfile 复用系统私有临时文件与原子替换能力，避免自行维护写入协议。
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(AppCommandError::io)?;
    serde_json::to_writer_pretty(&mut file, value)
        .map_err(|error| AppCommandError::io_error(error.to_string()))?;
    file.flush().map_err(AppCommandError::io)?;
    file.as_file().sync_all().map_err(AppCommandError::io)?;
    file.persist(path).map_err(|error| AppCommandError::io(error.error))?;
    Ok(())
}

impl SelectedCredentialStore {
    pub(super) fn current() -> Result<Self, AppCommandError> {
        Self::at(data_root())
    }

    fn at(root: PathBuf) -> Result<Self, AppCommandError> {
        let config: StorageConfig = read_optional(&root.join("cerebro-storage.json"))?.unwrap_or_default();
        Ok(Self { mode: config.mode, root })
    }

    fn credential_path(&self) -> PathBuf {
        self.root.join("cerebro-credential.json")
    }
}

impl CredentialStore for SelectedCredentialStore {
    fn load(&self) -> Result<Option<RunnerCredential>, AppCommandError> {
        match self.mode {
            StorageMode::File => read_optional(&self.credential_path()),
            StorageMode::Keyring => SystemCredentialStore.load(),
        }
    }

    fn save(&self, credential: &RunnerCredential) -> Result<(), AppCommandError> {
        match self.mode {
            StorageMode::File => write_json(&self.credential_path(), credential),
            StorageMode::Keyring => SystemCredentialStore.save(credential),
        }
    }

    fn clear(&self) -> Result<(), AppCommandError> {
        match self.mode {
            StorageMode::File => match std::fs::remove_file(self.credential_path()) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(AppCommandError::io(error)),
            },
            StorageMode::Keyring => SystemCredentialStore.clear(),
        }
    }
}

pub fn get_settings() -> Result<StorageSettings, AppCommandError> {
    let store = SelectedCredentialStore::current()?;
    Ok(StorageSettings {
        mode: store.mode,
        keyring_available: cfg!(feature = "tauri-runtime"),
    })
}

/// 只修改来源，不读取钱包或删除旧凭据；显式导入由独立操作完成。
pub fn select(mode: StorageMode) -> Result<StorageSettings, AppCommandError> {
    if mode == StorageMode::Keyring && !cfg!(feature = "tauri-runtime") {
        return Err(AppCommandError::configuration_invalid("系统凭据库仅在桌面版本可用"));
    }
    write_json(&data_root().join("cerebro-storage.json"), &StorageConfig { mode })?;
    get_settings()
}

/// 用户显式把旧存储复制到当前存储；保留源条目，不静默覆盖目标身份。
pub fn import_existing() -> Result<(), AppCommandError> {
    let target = SelectedCredentialStore::current()?;
    if target.load()?.is_some() {
        return Err(AppCommandError::already_exists("当前存储已有 Runner 身份，请先明确断开"));
    }
    let credential = match target.mode {
        StorageMode::File => SystemCredentialStore.load()?,
        StorageMode::Keyring => read_optional(&target.credential_path())?,
    }.ok_or_else(|| AppCommandError::configuration_missing("旧存储中没有 Cerebro Runner 凭据"))?;
    target.save(&credential)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_identity_survives_reopen_and_clear_without_touching_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::write(root.join("tokens.json"), "other credentials").unwrap();
        let store = SelectedCredentialStore::at(root.clone()).unwrap();
        assert_eq!(store.mode, StorageMode::File);
        assert!(store.load().unwrap().is_none());
        let credential: RunnerCredential = serde_json::from_value(serde_json::json!({
            "cerebro_base_url": "http://localhost/", "runner_id": "file-runner", "refresh_credential": "test-refresh"
        })).unwrap();
        store.save(&credential).unwrap();
        let reopened = SelectedCredentialStore::at(root.clone()).unwrap();
        assert_eq!(reopened.load().unwrap(), Some(credential));
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(store.credential_path()).unwrap().permissions().mode() & 0o777, 0o600);
        }
        reopened.clear().unwrap();
        assert!(reopened.load().unwrap().is_none());
        assert_eq!(std::fs::read_to_string(root.join("tokens.json")).unwrap(), "other credentials");
    }
}
