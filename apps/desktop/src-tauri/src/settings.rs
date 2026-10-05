use crate::{mapping, profile::DesktopProfile};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use usage_contracts as api;
use usage_core::{CoreError, SourceConfig};

const PROVIDER: &str = "ccusage.claude-code";
struct PendingDirectory {
    path: String,
    expires: Instant,
}
struct State {
    profile: DesktopProfile,
    pending: BTreeMap<String, PendingDirectory>,
}
pub struct SettingsStore {
    directory: PathBuf,
    state: Mutex<State>,
}
impl SettingsStore {
    pub fn new(directory: &Path, profile: DesktopProfile) -> Self {
        Self {
            directory: directory.into(),
            state: Mutex::new(State {
                profile,
                pending: BTreeMap::new(),
            }),
        }
    }
    pub async fn get(&self) -> api::SettingsResult {
        view(&self.state.lock().await.profile)
    }
    pub async fn remember_directory(
        &self,
        path: PathBuf,
    ) -> Result<api::SourceDirectory, api::ApiError> {
        let path = tokio::task::spawn_blocking(move || validate_directory(&path))
            .await
            .map_err(|_| mapping::error(CoreError::Storage))?
            .map_err(mapping::error)?;
        let mut state = self.state.lock().await;
        state
            .pending
            .retain(|_, entry| entry.expires > Instant::now());
        if state.pending.len() >= 16 {
            state.pending.clear();
        }
        let reference = uuid::Uuid::new_v4().to_string();
        state.pending.insert(
            reference.clone(),
            PendingDirectory {
                path,
                expires: Instant::now() + Duration::from_secs(300),
            },
        );
        Ok(api::SourceDirectory {
            directory_ref: reference,
            label: "已选择自定义 Claude 日志目录".into(),
        })
    }
    pub async fn update(
        &self,
        request: api::UpdateSettingsRequest,
    ) -> Result<(api::SettingsResult, BTreeMap<String, SourceConfig>), api::ApiError> {
        usage_core::application::validate_timezone(&request.timezone).map_err(mapping::error)?;
        let revision = request
            .expected_revision
            .parse::<u64>()
            .map_err(|_| mapping::error(CoreError::InvalidQuery))?;
        if revision.to_string() != request.expected_revision || request.providers.len() > 1 {
            return Err(mapping::error(CoreError::InvalidQuery));
        }
        let mut state = self.state.lock().await;
        if revision != state.profile.settings_revision {
            return Err(settings_error(
                api::ErrorCode::SettingsConflict,
                "设置已被更新，请重新读取后保存。",
            ));
        }
        let mut next = state.profile.clone();
        next.timezone = request.timezone;
        for update in request.providers {
            if update.provider_id != PROVIDER {
                return Err(mapping::error(CoreError::ProviderNotFound));
            }
            let current = state
                .profile
                .claude_directory_ref
                .as_deref()
                .unwrap_or("configured");
            match update.directory_ref {
                None => {
                    next.claude_root_path = None;
                    next.claude_directory_ref = None;
                }
                Some(reference)
                    if state.profile.claude_root_path.is_some() && reference == current =>
                {
                    ()
                }
                Some(reference) => {
                    let pending = state
                        .pending
                        .get(&reference)
                        .filter(|entry| entry.expires > Instant::now())
                        .ok_or_else(|| {
                            settings_error(
                                api::ErrorCode::InvalidDirectoryRef,
                                "目录引用无效或已过期，请重新选择。",
                            )
                        })?;
                    next.claude_root_path = Some(pending.path.clone());
                    next.claude_directory_ref = Some(reference);
                }
            }
            next.claude_enabled = update.enabled;
        }
        next.settings_revision = next
            .settings_revision
            .checked_add(1)
            .ok_or_else(|| mapping::error(CoreError::Overflow))?;
        let directory = self.directory.clone();
        let write = next.clone();
        tokio::task::spawn_blocking(move || write.persist(&directory))
            .await
            .map_err(|_| mapping::error(CoreError::Storage))?
            .map_err(mapping::error)?;
        state.profile = next;
        let configs = BTreeMap::from([(
            PROVIDER.into(),
            SourceConfig {
                enabled: state.profile.claude_enabled,
                root_path: state.profile.claude_root_path.clone(),
            },
        )]);
        Ok((view(&state.profile), configs))
    }
}
fn settings_error(code: api::ErrorCode, message: &str) -> api::ApiError {
    api::ApiError {
        api_version: api::API_VERSION.into(),
        code,
        message: message.into(),
        retryable: false,
    }
}
fn view(profile: &DesktopProfile) -> api::SettingsResult {
    api::SettingsResult {
        api_version: api::API_VERSION.into(),
        revision: profile.settings_revision.to_string(),
        timezone: profile.timezone.clone(),
        providers: vec![api::ProviderSettings {
            provider_id: PROVIDER.into(),
            enabled: profile.claude_enabled,
            directory: profile
                .claude_root_path
                .as_ref()
                .map(|_| api::SourceDirectory {
                    directory_ref: profile
                        .claude_directory_ref
                        .clone()
                        .unwrap_or_else(|| "configured".into()),
                    label: "已配置自定义 Claude 日志目录".into(),
                }),
        }],
        collection_notice: "启用后，仅从所选 Claude 日志目录提取用量；正文不持久化、不上传。"
            .into(),
        directory_change_policy: "preserve-dataset".into(),
    }
}
fn validate_directory(path: &Path) -> Result<String, CoreError> {
    if !path.is_absolute() {
        return Err(CoreError::InvalidQuery);
    }
    let root = if path.file_name().is_some_and(|name| name == "projects") {
        path.parent().ok_or(CoreError::InvalidQuery)?
    } else {
        path
    };
    let root = root.canonicalize().map_err(io_error)?;
    std::fs::read_dir(root.join("projects")).map_err(io_error)?;
    let value = root.to_str().ok_or(CoreError::InvalidQuery)?;
    if value.contains(',') || value.trim() != value {
        return Err(CoreError::InvalidQuery);
    }
    Ok(value.into())
}
fn io_error(error: std::io::Error) -> CoreError {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        CoreError::PermissionDenied
    } else {
        CoreError::SourceNotDetected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn save_is_revision_checked_and_keeps_identity_and_opaque_paths() {
        let directory = tempfile::tempdir().unwrap();
        let original = DesktopProfile::load_or_create(directory.path()).unwrap();
        let store = SettingsStore::new(directory.path(), original.clone());
        let logs = directory.path().join("logs/projects");
        std::fs::create_dir_all(&logs).unwrap();
        let selected = store.remember_directory(logs).await.unwrap();
        let request = || api::UpdateSettingsRequest {
            expected_revision: "1".into(),
            timezone: "America/Phoenix".into(),
            providers: vec![api::ProviderSettingsUpdate {
                provider_id: PROVIDER.into(),
                enabled: true,
                directory_ref: Some(selected.directory_ref.clone()),
            }],
        };
        let (result, _) = store.update(request()).await.unwrap();
        assert_eq!(result.revision, "2");
        assert_eq!(
            store.update(request()).await.unwrap_err().code,
            api::ErrorCode::SettingsConflict
        );
        let persisted = DesktopProfile::load_or_create(directory.path()).unwrap();
        assert_eq!(persisted.device_id, original.device_id);
        assert_eq!(persisted.claude_dataset_id, original.claude_dataset_id);
        assert!(persisted.claude_enabled);
        assert!(!serde_json::to_string(&result)
            .unwrap()
            .contains(directory.path().to_str().unwrap()));
    }
    #[tokio::test]
    async fn arbitrary_directory_refs_are_rejected_without_writing() {
        let directory = tempfile::tempdir().unwrap();
        let profile = DesktopProfile::load_or_create(directory.path()).unwrap();
        let store = SettingsStore::new(directory.path(), profile);
        let result = store
            .update(api::UpdateSettingsRequest {
                expected_revision: "1".into(),
                timezone: "UTC".into(),
                providers: vec![api::ProviderSettingsUpdate {
                    provider_id: PROVIDER.into(),
                    enabled: true,
                    directory_ref: Some("C:/secret".into()),
                }],
            })
            .await;
        assert_eq!(
            result.unwrap_err().code,
            api::ErrorCode::InvalidDirectoryRef
        );
        assert_eq!(store.get().await.revision, "1");
    }
}
