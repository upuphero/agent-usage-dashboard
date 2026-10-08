use crate::{
    mapping,
    profile::{DesktopProfile, TimezoneMode, PROVIDERS},
    timezone,
};
use std::{
    collections::{BTreeMap, BTreeSet},
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
    /// Enabled sources already rebuilt in the current effective zone from their current directory;
    /// cleared when the zone changes, and per source when its directory changes.
    rebuilt: BTreeSet<String>,
}
pub struct TimezoneView {
    pub revision: String,
    pub mode: TimezoneMode,
    pub effective: String,
    pub needs_rescan: bool,
}
type Saved = (api::SettingsResult, BTreeMap<String, SourceConfig>, bool);
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
                rebuilt: BTreeSet::new(),
            }),
        }
    }
    pub async fn get(&self) -> api::SettingsResult {
        view(&self.state.lock().await.profile)
    }
    pub async fn auto_config(&self) -> (String, String, api::AutoCollectionConfig) {
        let state = self.state.lock().await;
        (
            state.profile.settings_revision.to_string(),
            state.profile.timezone.clone(),
            api::AutoCollectionConfig {
                enabled: state.profile.auto_collection.enabled,
                interval_minutes: state.profile.auto_collection.interval_minutes,
            },
        )
    }
    /// Independent write: never accepts source paths, provider switches or timezone.
    pub async fn update_auto(
        &self,
        request: api::UpdateAutoCollectionRequest,
    ) -> Result<(), api::ApiError> {
        let revision = request
            .expected_revision
            .parse::<u64>()
            .map_err(|_| mapping::error(CoreError::InvalidQuery))?;
        if revision.to_string() != request.expected_revision {
            return Err(mapping::error(CoreError::InvalidQuery));
        }
        if !matches!(request.config.interval_minutes, 1 | 5 | 15) {
            return Err(mapping::error(CoreError::InvalidQuery));
        }
        let mut state = self.state.lock().await;
        if request.expected_revision != state.profile.settings_revision.to_string() {
            return Err(settings_error(
                api::ErrorCode::SettingsConflict,
                "设置已被更新，请重新读取后保存。",
            ));
        }
        let mut next = state.profile.clone();
        next.auto_collection.enabled = request.config.enabled;
        next.auto_collection.interval_minutes = request.config.interval_minutes;
        next.settings_revision = next
            .settings_revision
            .checked_add(1)
            .ok_or_else(|| mapping::error(CoreError::Overflow))?;
        let write = next.clone();
        let directory = self.directory.clone();
        tokio::task::spawn_blocking(move || write.persist(&directory))
            .await
            .map_err(|_| mapping::error(CoreError::Storage))?
            .map_err(mapping::error)?;
        state.profile = next;
        Ok(())
    }
    pub async fn timezone_view(&self) -> TimezoneView {
        let state = self.state.lock().await;
        TimezoneView {
            revision: state.profile.settings_revision.to_string(),
            mode: state.profile.timezone_mode,
            effective: state.profile.timezone.clone(),
            needs_rescan: state.profile.timezone_needs_rescan,
        }
    }
    /// The zone still being rebuilt from source logs, if any.
    pub async fn rebuild_target(&self) -> Option<String> {
        let state = self.state.lock().await;
        if state.profile.timezone_needs_rescan {
            Some(state.profile.timezone.clone())
        } else {
            None
        }
    }
    pub async fn rebuilt(&self) -> BTreeSet<String> {
        self.state.lock().await.rebuilt.clone()
    }
    /// Only a success in the exact current target counts; an older zone never acknowledges it.
    pub async fn record_rebuild_success(&self, provider: &str, zone: &str) -> bool {
        let mut state = self.state.lock().await;
        if !state.profile.timezone_needs_rescan || state.profile.timezone != zone {
            return false;
        }
        state.rebuilt.insert(provider.into());
        true
    }
    /// Clears the persisted retry marker once every currently enabled source is rebuilt.
    pub async fn complete_rebuild_if_done(&self, enabled: &[String]) -> Result<bool, CoreError> {
        let mut state = self.state.lock().await;
        if !state.profile.timezone_needs_rescan
            || !enabled.iter().all(|id| state.rebuilt.contains(id))
        {
            return Ok(false);
        }
        let mut next = state.profile.clone();
        next.timezone_needs_rescan = false;
        self.commit(&mut state, next).await?;
        state.rebuilt.clear();
        Ok(true)
    }
    /// Applies a detected zone at the caller's scan gate; returns whether the zone changed.
    pub async fn follow_system_timezone(&self, target: &str) -> Result<bool, CoreError> {
        let mut state = self.state.lock().await;
        if state.profile.timezone_mode != TimezoneMode::FollowSystem
            || timezone::same_zone(&state.profile.timezone, target)
        {
            return Ok(false);
        }
        let mut next = state.profile.clone();
        next.timezone = timezone::canonical(target).ok_or(CoreError::InvalidQuery)?;
        next.timezone_needs_rescan = true;
        next.settings_revision = next
            .settings_revision
            .checked_add(1)
            .ok_or(CoreError::Overflow)?;
        self.commit(&mut state, next).await?;
        Ok(true)
    }
    /// Independent mode write; the caller holds the scan gate. A fixed change of zone waits for
    /// idle scans, while following always saves and a differing system zone applies later.
    pub async fn update_timezone(
        &self,
        request: &api::UpdateTimezoneRequest,
        system: Option<&str>,
        scans_active: bool,
    ) -> Result<bool, api::ApiError> {
        let revision = parse_revision(&request.expected_revision)?;
        let invalid = || mapping::error(CoreError::InvalidQuery);
        let fixed = match request.timezone.as_deref() {
            Some(zone) => Some(timezone::canonical(zone).ok_or_else(invalid)?),
            None => None,
        };
        let mode = match (request.mode, &fixed) {
            (api::TimezoneMode::Fixed, Some(_)) => TimezoneMode::Fixed,
            (api::TimezoneMode::FollowSystem, None) => TimezoneMode::FollowSystem,
            _ => return Err(invalid()),
        };
        let mut state = self.state.lock().await;
        if revision != state.profile.settings_revision {
            return Err(conflict());
        }
        let current = state.profile.timezone.clone();
        let target = match (fixed, system) {
            (Some(zone), _) => zone,
            (None, Some(zone)) if !scans_active => zone.to_owned(),
            (None, _) => current.clone(),
        };
        let changed = !timezone::same_zone(&target, &current);
        if changed && scans_active {
            return Err(mapping::error(CoreError::ScanBusy));
        }
        let mut next = state.profile.clone();
        next.timezone_mode = mode;
        if changed {
            next.timezone = target;
            next.timezone_needs_rescan = true;
        }
        next.settings_revision = next
            .settings_revision
            .checked_add(1)
            .ok_or_else(|| mapping::error(CoreError::Overflow))?;
        let committed = self.commit(&mut state, next).await;
        committed.map_err(mapping::error)?;
        Ok(changed)
    }
    /// Atomic persistence of the next profile; any effective-zone change restarts rebuild progress,
    /// and a source whose directory changes must be rebuilt again from the new one.
    async fn commit(&self, state: &mut State, next: DesktopProfile) -> Result<(), CoreError> {
        let write = next.clone();
        let directory = self.directory.clone();
        tokio::task::spawn_blocking(move || write.persist(&directory))
            .await
            .map_err(|_| CoreError::Storage)??;
        if next.timezone != state.profile.timezone {
            state.rebuilt.clear();
        }
        // The directory is the scope a scan captures; unaffected sources keep their progress.
        let (before, after) = (state.profile.configs(), next.configs());
        let root = |configs: &BTreeMap<String, SourceConfig>, id: &str| {
            configs.get(id).map(|config| config.root_path.clone())
        };
        state
            .rebuilt
            .retain(|id| root(&before, id) == root(&after, id));
        state.profile = next;
        Ok(())
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
    /// Returns the view, source configuration and whether the effective zone changed.
    pub async fn update(
        &self,
        request: api::UpdateSettingsRequest,
    ) -> Result<Saved, api::ApiError> {
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
            return Err(conflict());
        }
        let mut next = state.profile.clone();
        // Older clients only send an explicit statistical zone: a different one is a fixed choice.
        let timezone_changed = !timezone::same_zone(&request.timezone, &next.timezone);
        if timezone_changed {
            let invalid = || mapping::error(CoreError::InvalidQuery);
            next.timezone = timezone::canonical(&request.timezone).ok_or_else(invalid)?;
            next.timezone_mode = TimezoneMode::Fixed;
            next.timezone_needs_rescan = true;
        }
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
        let committed = self.commit(&mut state, next).await;
        committed.map_err(mapping::error)?;
        let configs = state.profile.configs();
        Ok((view(&state.profile), configs, timezone_changed))
    }
}
fn parse_revision(value: &str) -> Result<u64, api::ApiError> {
    let revision = value
        .parse::<u64>()
        .map_err(|_| mapping::error(CoreError::InvalidQuery))?;
    if revision.to_string() != value {
        return Err(mapping::error(CoreError::InvalidQuery));
    }
    Ok(revision)
}
fn conflict() -> api::ApiError {
    settings_error(
        api::ErrorCode::SettingsConflict,
        "设置已被更新，请重新读取后保存。",
    )
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
    async fn automatic_configuration_validates_intervals_revision_and_persists_independently() {
        let dir = tempfile::tempdir().unwrap();
        let original = DesktopProfile::load_or_create(dir.path()).unwrap();
        let store = SettingsStore::new(dir.path(), original.clone());
        let update = |revision: &str, interval| api::UpdateAutoCollectionRequest {
            expected_revision: revision.into(),
            config: api::AutoCollectionConfig {
                enabled: true,
                interval_minutes: interval,
            },
        };
        assert_eq!(
            store.update_auto(update("1", 2)).await.unwrap_err().code,
            api::ErrorCode::InvalidQuery
        );
        for (revision, interval) in [("1", 1), ("2", 5), ("3", 15)] {
            store.update_auto(update(revision, interval)).await.unwrap();
        }
        assert_eq!(
            store.update_auto(update("3", 5)).await.unwrap_err().code,
            api::ErrorCode::SettingsConflict
        );
        let reopened = DesktopProfile::load_or_create(dir.path()).unwrap();
        assert!(reopened.auto_collection.enabled);
        assert_eq!(reopened.auto_collection.interval_minutes, 15);
        assert_eq!(reopened.timezone, original.timezone);
        assert_eq!(reopened.device_id, original.device_id);
        assert_eq!(reopened.claude_dataset_id, original.claude_dataset_id);
        assert!(!reopened.claude_enabled);
    }
    fn code<T: std::fmt::Debug>(result: Result<T, api::ApiError>) -> api::ErrorCode {
        result.unwrap_err().code
    }
    #[tokio::test]
    async fn rebuild_is_acknowledged_only_by_all_enabled_sources_in_the_target_zone() {
        let directory = tempfile::tempdir().unwrap();
        let mut profile = DesktopProfile::load_or_create(directory.path()).unwrap();
        let zone = "America/Phoenix";
        profile.timezone = zone.into();
        profile.timezone_needs_rescan = true;
        profile.persist(directory.path()).unwrap();
        let store = SettingsStore::new(directory.path(), profile);
        let codex = "ccusage.codex";
        let enabled = [PROVIDER.to_owned(), codex.to_owned()];
        assert_eq!(store.rebuild_target().await.as_deref(), Some(zone));
        assert!(!store.record_rebuild_success(codex, "UTC").await);
        assert!(store.record_rebuild_success(PROVIDER, zone).await);
        let done = store.complete_rebuild_if_done(&enabled).await;
        assert!(!done.unwrap());
        // A restart before completion keeps the persisted retry marker.
        let reopened = DesktopProfile::load_or_create(directory.path()).unwrap();
        assert!(reopened.timezone_needs_rescan);
        assert!(store.record_rebuild_success(codex, zone).await);
        let done = store.complete_rebuild_if_done(&enabled).await;
        assert!(done.unwrap());
        let completed = DesktopProfile::load_or_create(directory.path()).unwrap();
        assert!(!completed.timezone_needs_rescan);
        assert_eq!(completed.settings_revision, 1);
        assert!(store.rebuild_target().await.is_none());
        assert!(!store.record_rebuild_success(PROVIDER, zone).await);
    }
    #[tokio::test]
    async fn a_new_directory_withdraws_only_that_sources_rebuild_acknowledgement() {
        let directory = tempfile::tempdir().unwrap();
        let mut profile = DesktopProfile::load_or_create(directory.path()).unwrap();
        let zone = "Asia/Tokyo";
        profile.timezone = zone.into();
        profile.timezone_needs_rescan = true;
        let dataset = profile.claude_dataset_id.clone();
        let store = SettingsStore::new(directory.path(), profile);
        let codex = "ccusage.codex";
        let enabled = [PROVIDER.to_owned(), codex.to_owned()];
        let logs = |name: &str| {
            let path = directory.path().join(name).join("projects");
            std::fs::create_dir_all(&path).unwrap();
            path
        };
        let save = |revision: &str, directory_ref: &str| api::UpdateSettingsRequest {
            expected_revision: revision.into(),
            timezone: zone.into(),
            providers: vec![api::ProviderSettingsUpdate {
                provider_id: PROVIDER.into(),
                enabled: true,
                directory_ref: Some(directory_ref.into()),
            }],
        };
        let first = store.remember_directory(logs("first")).await.unwrap();
        store.update(save("1", &first.directory_ref)).await.unwrap();
        assert!(store.record_rebuild_success(PROVIDER, zone).await);
        // Saving the same directory, or choosing the same folder again, keeps its progress.
        store.update(save("2", &first.directory_ref)).await.unwrap();
        let again = store.remember_directory(logs("first")).await.unwrap();
        store.update(save("3", &again.directory_ref)).await.unwrap();
        assert!(store.record_rebuild_success(codex, zone).await);
        let both = BTreeSet::from([PROVIDER.to_owned(), codex.to_owned()]);
        assert_eq!(store.rebuilt().await, both);
        // Another directory withdraws only that source; the other keeps its valid progress.
        let moved = store.remember_directory(logs("moved")).await.unwrap();
        store.update(save("4", &moved.directory_ref)).await.unwrap();
        assert_eq!(store.rebuilt().await, BTreeSet::from([codex.to_owned()]));
        assert!(!store.complete_rebuild_if_done(&enabled).await.unwrap());
        let reopened = DesktopProfile::load_or_create(directory.path()).unwrap();
        assert!(reopened.timezone_needs_rescan);
        // Completion needs a success after the change.
        assert!(store.record_rebuild_success(PROVIDER, zone).await);
        assert!(store.complete_rebuild_if_done(&enabled).await.unwrap());
        let completed = DesktopProfile::load_or_create(directory.path()).unwrap();
        assert!(!completed.timezone_needs_rescan);
        assert_eq!(completed.timezone, zone);
        assert_eq!(completed.claude_dataset_id, dataset);
    }
    #[tokio::test]
    async fn no_enabled_sources_complete_a_rebuild_without_collection() {
        let directory = tempfile::tempdir().unwrap();
        let mut profile = DesktopProfile::load_or_create(directory.path()).unwrap();
        profile.timezone_needs_rescan = true;
        let store = SettingsStore::new(directory.path(), profile);
        assert!(store.complete_rebuild_if_done(&[]).await.unwrap());
        assert!(!store.timezone_view().await.needs_rescan);
    }
    #[tokio::test]
    async fn detected_zones_apply_only_in_follow_mode_and_restart_rebuild_progress() {
        let directory = tempfile::tempdir().unwrap();
        let mut profile = DesktopProfile::load_or_create(directory.path()).unwrap();
        profile.timezone = "Asia/Calcutta".into();
        let store = SettingsStore::new(directory.path(), profile);
        let (kolkata, paris, arizona) = ("Asia/Kolkata", "Europe/Paris", "US/Arizona");
        // The same identity in another spelling is not a change.
        assert!(!store.follow_system_timezone(kolkata).await.unwrap());
        assert!(store.follow_system_timezone(paris).await.unwrap());
        let view = store.timezone_view().await;
        assert_eq!(view.effective, paris);
        assert_eq!(view.revision, "2");
        assert!(view.needs_rescan);
        assert!(store.record_rebuild_success(PROVIDER, paris).await);
        assert!(store.follow_system_timezone(arizona).await.unwrap());
        assert!(store.rebuilt().await.is_empty());
        assert_eq!(store.timezone_view().await.effective, "America/Phoenix");
        let fixed = api::UpdateTimezoneRequest {
            expected_revision: "3".into(),
            mode: api::TimezoneMode::Fixed,
            timezone: Some("Asia/Tokyo".into()),
        };
        let changed = store.update_timezone(&fixed, None, false).await;
        assert!(changed.unwrap());
        assert!(!store.follow_system_timezone("UTC").await.unwrap());
        let reopened = DesktopProfile::load_or_create(directory.path()).unwrap();
        assert_eq!(reopened.timezone, "Asia/Tokyo");
        assert_eq!(reopened.timezone_mode, TimezoneMode::Fixed);
    }
    #[tokio::test]
    async fn timezone_writes_validate_mode_revision_and_the_scan_gate() {
        let directory = tempfile::tempdir().unwrap();
        let mut profile = DesktopProfile::load_or_create(directory.path()).unwrap();
        profile.timezone = "America/Phoenix".into();
        let store = SettingsStore::new(directory.path(), profile);
        let request = |revision: &str, mode, zone: Option<&str>| api::UpdateTimezoneRequest {
            expected_revision: revision.into(),
            mode,
            timezone: zone.map(Into::into),
        };
        let fixed = api::TimezoneMode::Fixed;
        let follow = api::TimezoneMode::FollowSystem;
        for invalid in [
            request("1", fixed, None),
            request("1", follow, Some("UTC")),
            request("1", fixed, Some("Mars/Base")),
            request("01", follow, None),
        ] {
            let result = store.update_timezone(&invalid, None, false).await;
            assert_eq!(code(result), api::ErrorCode::InvalidQuery);
        }
        // Following during a scan saves the mode; the monitor applies the zone at a safe boundary.
        let tokyo = Some("Asia/Tokyo");
        let busy = request("1", follow, None);
        let changed = store.update_timezone(&busy, tokyo, true).await;
        assert!(!changed.unwrap());
        let view = store.timezone_view().await;
        assert_eq!(view.mode, TimezoneMode::FollowSystem);
        assert_eq!(view.effective, "America/Phoenix");
        // A fixed change of zone never replaces the scope of a running scan.
        let fixed_tokyo = request("2", fixed, tokyo);
        let result = store.update_timezone(&fixed_tokyo, None, true).await;
        assert_eq!(code(result), api::ErrorCode::ScanBusy);
        let alias = request("2", fixed, Some("Japan"));
        let changed = store.update_timezone(&alias, None, false).await;
        assert!(changed.unwrap());
        let reopened = DesktopProfile::load_or_create(directory.path()).unwrap();
        assert_eq!(reopened.timezone, "Asia/Tokyo");
        assert_eq!(reopened.timezone_mode, TimezoneMode::Fixed);
        assert!(reopened.timezone_needs_rescan);
        assert_eq!(reopened.settings_revision, 3);
        let stale = request("2", follow, None);
        let result = store.update_timezone(&stale, Some("UTC"), false).await;
        assert_eq!(code(result), api::ErrorCode::SettingsConflict);
        let idle = request("3", follow, None);
        let changed = store.update_timezone(&idle, Some("UTC"), false).await;
        assert!(changed.unwrap());
        assert_eq!(store.timezone_view().await.effective, "UTC");
    }
    #[tokio::test]
    async fn legacy_saves_keep_the_mode_unless_they_choose_a_different_zone() {
        let directory = tempfile::tempdir().unwrap();
        let mut profile = DesktopProfile::load_or_create(directory.path()).unwrap();
        profile.timezone = "Asia/Calcutta".into();
        let store = SettingsStore::new(directory.path(), profile);
        let save = |revision: &str, zone: &str| api::UpdateSettingsRequest {
            expected_revision: revision.into(),
            timezone: zone.into(),
            providers: vec![],
        };
        let (_, _, changed) = store.update(save("1", "Asia/Kolkata")).await.unwrap();
        assert!(!changed);
        let view = store.timezone_view().await;
        assert_eq!(view.mode, TimezoneMode::FollowSystem);
        assert_eq!(view.effective, "Asia/Calcutta");
        assert!(!view.needs_rescan);
        let (_, _, changed) = store.update(save("2", "US/Arizona")).await.unwrap();
        assert!(changed);
        let view = store.timezone_view().await;
        assert_eq!(view.mode, TimezoneMode::Fixed);
        assert_eq!(view.effective, "America/Phoenix");
        assert!(view.needs_rescan);
        let result = store.update(save("3", "Mars/Base")).await;
        assert_eq!(code(result), api::ErrorCode::InvalidQuery);
    }
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
        let (result, _, _) = store.update(request()).await.unwrap();
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
