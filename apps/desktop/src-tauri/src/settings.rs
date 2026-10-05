use crate::{
    mapping,
    profile::{DesktopProfile, PROVIDERS},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use usage_contracts as api;
use usage_core::{CoreError, SourceConfig};

#[cfg(test)]
const PROVIDER: &str = "ccusage.claude-code";
struct PendingDirectory {
    provider_id: String,
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
    #[cfg(test)]
    pub async fn remember_directory(
        &self,
        path: PathBuf,
    ) -> Result<api::SourceDirectory, api::ApiError> {
        self.remember_directory_for(PROVIDER, path).await
    }
    pub async fn remember_directory_for(
        &self,
        provider_id: &str,
        path: PathBuf,
    ) -> Result<api::SourceDirectory, api::ApiError> {
        if !PROVIDERS.contains(&provider_id) {
            return Err(mapping::error(CoreError::ProviderNotFound));
        }
        let provider = provider_id.to_owned();
        let path = tokio::task::spawn_blocking(move || validate_directory_for(&provider, &path))
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
                provider_id: provider_id.into(),
                path,
                expires: Instant::now() + Duration::from_secs(300),
            },
        );
        Ok(api::SourceDirectory {
            directory_ref: reference,
            label: "已选择自定义用量目录".into(),
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
        let unique: std::collections::BTreeSet<_> = request
            .providers
            .iter()
            .map(|value| &value.provider_id)
            .collect();
        if revision.to_string() != request.expected_revision
            || request.providers.len() > 3
            || unique.len() != request.providers.len()
        {
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
            let mut provider = next.provider(&update.provider_id).map_err(mapping::error)?;
            let current = provider.directory_ref.as_deref().unwrap_or("configured");
            match update.directory_ref {
                None => {
                    provider.root_path = None;
                    provider.directory_ref = None;
                }
                Some(reference) if provider.root_path.is_some() && reference == current => {}
                Some(reference) => {
                    let pending = state
                        .pending
                        .get(&reference)
                        .filter(|entry| {
                            entry.expires > Instant::now()
                                && entry.provider_id == update.provider_id
                        })
                        .ok_or_else(|| {
                            settings_error(
                                api::ErrorCode::InvalidDirectoryRef,
                                "目录引用无效或已过期，请重新选择。",
                            )
                        })?;
                    provider.root_path = Some(pending.path.clone());
                    provider.directory_ref = Some(reference);
                }
            }
            provider.enabled = update.enabled;
            next.set_provider(&update.provider_id, provider)
                .map_err(mapping::error)?;
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
        let configs = state.profile.configs();
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
        providers: PROVIDERS.iter().filter_map(|id| profile.provider(id).ok().map(|provider| api::ProviderSettings {
            provider_id: (*id).into(),
            enabled: provider.enabled,
            directory: provider
                .root_path
                .as_ref()
                .map(|_| api::SourceDirectory {
                    directory_ref: provider
                        .directory_ref
                        .clone()
                        .unwrap_or_else(|| "configured".into()),
                    label: "已配置自定义用量目录".into(),
                }),
        })).collect(),
        collection_notice: "启用并扫描后，从所选来源的本机日志/数据库提取用量；不读取认证文件，不保存或上传聊天正文。"
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
fn validate_directory_for(provider: &str, path: &Path) -> Result<String, CoreError> {
    let kind = match provider {
        "ccusage.codex" => usage_adapters::AgentKind::Codex,
        "ccusage.antigravity" => usage_adapters::AgentKind::Antigravity,
        _ => return validate_directory(path),
    };
    usage_adapters::validate_agent_directory(kind, path)?
        .to_str()
        .map(str::to_owned)
        .ok_or(CoreError::InvalidQuery)
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
