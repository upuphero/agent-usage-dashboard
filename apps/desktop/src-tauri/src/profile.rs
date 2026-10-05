//! Host-private persistence for stable local identities and initial source configuration.
//! This file is not an API DTO or a database row. Source configuration changes never create a new identity implicitly.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};
use usage_core::CoreError;

pub const PROVIDERS: [&str; 3] = [
    "ccusage.claude-code",
    "ccusage.codex",
    "ccusage.antigravity",
];
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderProfile {
    pub dataset_id: String,
    pub enabled: bool,
    pub root_path: Option<String>,
    pub directory_ref: Option<String>,
}
impl ProviderProfile {
    fn fresh() -> Self {
        Self {
            dataset_id: uuid::Uuid::new_v4().to_string(),
            enabled: false,
            root_path: None,
            directory_ref: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopProfile {
    pub profile_version: u32,
    pub device_id: String,
    pub claude_dataset_id: String,
    pub claude_enabled: bool,
    pub claude_root_path: Option<String>,
    pub timezone: String,
    #[serde(default = "initial_revision")]
    pub settings_revision: u64,
    #[serde(default)]
    pub claude_directory_ref: Option<String>,
    #[serde(default)]
    pub additional_providers: BTreeMap<String, ProviderProfile>,
}
fn initial_revision() -> u64 {
    1
}
impl DesktopProfile {
    fn fresh() -> Self {
        Self {
            profile_version: 2,
            device_id: uuid::Uuid::new_v4().to_string(),
            claude_dataset_id: uuid::Uuid::new_v4().to_string(),
            claude_enabled: false,
            claude_root_path: None,
            timezone: "UTC".into(),
            settings_revision: 1,
            claude_directory_ref: None,
            additional_providers: PROVIDERS[1..]
                .iter()
                .map(|id| ((*id).into(), ProviderProfile::fresh()))
                .collect(),
        }
    }
    fn validate(&self) -> Result<(), CoreError> {
        if self.profile_version > 2 {
            return Err(CoreError::StorageSchemaNewer);
        }
        if !matches!(self.profile_version, 1 | 2) || self.settings_revision == 0 {
            return Err(CoreError::InvalidData);
        }
        for id in [&self.device_id, &self.claude_dataset_id] {
            let canonical = uuid::Uuid::parse_str(id)
                .map_err(|_| CoreError::InvalidData)?
                .to_string();
            if canonical.as_str() != id.as_str() {
                return Err(CoreError::InvalidData);
            }
        }
        usage_core::application::validate_timezone(&self.timezone)?;
        if self.profile_version == 2
            && (self.additional_providers.len() != 2
                || !PROVIDERS[1..]
                    .iter()
                    .all(|id| self.additional_providers.contains_key(*id)))
        {
            return Err(CoreError::InvalidData);
        }
        for (id, provider) in &self.additional_providers {
            if !PROVIDERS[1..].contains(&id.as_str())
                || uuid::Uuid::parse_str(&provider.dataset_id)
                    .map_err(|_| CoreError::InvalidData)?
                    .to_string()
                    != provider.dataset_id
            {
                return Err(CoreError::InvalidData);
            }
            if provider.root_path.is_none() && provider.directory_ref.is_some() {
                return Err(CoreError::InvalidData);
            }
            if provider.root_path.as_ref().is_some_and(|path| {
                !Path::new(path).is_absolute() || path.trim() != path || path.contains(',')
            }) {
                return Err(CoreError::InvalidQuery);
            }
        }
        if self.claude_root_path.is_none() && self.claude_directory_ref.is_some() {
            return Err(CoreError::InvalidData);
        }
        if self.claude_root_path.as_ref().is_some_and(|root| {
            !Path::new(root).is_absolute() || root.trim() != root.as_str() || root.contains(',')
        }) {
            return Err(CoreError::InvalidQuery);
        }
        Ok(())
    }
    fn read(path: &Path) -> Result<Self, CoreError> {
        if fs::metadata(path).map_err(|_| CoreError::Storage)?.len() > 16 * 1024 {
            return Err(CoreError::InvalidData);
        }
        let profile: Self =
            serde_json::from_slice(&fs::read(path).map_err(|_| CoreError::Storage)?)
                .map_err(|_| CoreError::InvalidData)?;
        profile.validate()?;
        Ok(profile)
    }
    pub fn load_or_create(directory: &Path) -> Result<Self, CoreError> {
        fs::create_dir_all(directory).map_err(|_| CoreError::Storage)?;
        let path = directory.join("profile.json");
        match fs::metadata(&path) {
            Ok(_) => {
                let mut profile = Self::read(&path)?;
                if profile.profile_version == 1 {
                    profile.profile_version = 2;
                    for id in &PROVIDERS[1..] {
                        profile
                            .additional_providers
                            .entry((*id).into())
                            .or_insert_with(ProviderProfile::fresh);
                    }
                    profile.persist(directory)?;
                }
                return Ok(profile);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => return Err(CoreError::Storage),
        }
        if directory.join("usage.db").exists() {
            // Losing the identity file must not create a second dataset over retained usage.
            return Err(CoreError::InvalidData);
        }
        let profile = Self::fresh();
        let staging = directory.join(format!(".profile-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&staging)
                .map_err(|_| CoreError::Storage)?;
            file.write_all(
                &serde_json::to_vec_pretty(&profile).map_err(|_| CoreError::InvalidData)?,
            )
            .map_err(|_| CoreError::Storage)?;
            file.sync_all().map_err(|_| CoreError::Storage)?;
            drop(file);
            // Atomic create-if-absent: a concurrent first launch cannot overwrite the winning IDs.
            match fs::hard_link(&staging, &path) {
                Ok(()) => Ok(profile),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    Self::read(&path)
                }
                Err(_) => Err(CoreError::Storage),
            }
        })();
        let _ = fs::remove_file(staging);
        result
    }
    pub fn persist(&self, directory: &Path) -> Result<(), CoreError> {
        self.validate()?;
        let encoded = serde_json::to_vec_pretty(self).map_err(|_| CoreError::InvalidData)?;
        if encoded.len() > 16 * 1024 {
            return Err(CoreError::InvalidData);
        }
        let staging = directory.join(format!(".profile-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&staging)
                .map_err(|_| CoreError::Storage)?;
            file.write_all(&encoded).map_err(|_| CoreError::Storage)?;
            file.sync_all().map_err(|_| CoreError::Storage)?;
            drop(file);
            replace_profile(&staging, &directory.join("profile.json"))?;
            #[cfg(unix)]
            std::fs::File::open(directory)
                .and_then(|file| file.sync_all())
                .map_err(|_| CoreError::Storage)?;
            Ok(())
        })();
        let _ = fs::remove_file(staging);
        result
    }
    pub fn provider(&self, id: &str) -> Result<ProviderProfile, CoreError> {
        if id == PROVIDERS[0] {
            return Ok(ProviderProfile {
                dataset_id: self.claude_dataset_id.clone(),
                enabled: self.claude_enabled,
                root_path: self.claude_root_path.clone(),
                directory_ref: self.claude_directory_ref.clone(),
            });
        }
        self.additional_providers
            .get(id)
            .cloned()
            .ok_or(CoreError::ProviderNotFound)
    }
    pub fn set_provider(&mut self, id: &str, value: ProviderProfile) -> Result<(), CoreError> {
        if id == PROVIDERS[0] {
            self.claude_enabled = value.enabled;
            self.claude_root_path = value.root_path;
            self.claude_directory_ref = value.directory_ref;
        } else if let Some(provider) = self.additional_providers.get_mut(id) {
            *provider = value;
        } else {
            return Err(CoreError::ProviderNotFound);
        }
        Ok(())
    }
    pub fn configs(&self) -> BTreeMap<String, usage_core::SourceConfig> {
        PROVIDERS
            .into_iter()
            .filter_map(|id| {
                self.provider(id).ok().map(|provider| {
                    (
                        id.into(),
                        usage_core::SourceConfig {
                            enabled: provider.enabled,
                            root_path: provider.root_path,
                        },
                    )
                })
            })
            .collect()
    }
}
#[cfg(windows)]
fn replace_profile(from: &Path, to: &Path) -> Result<(), CoreError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let from: Vec<_> = from
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let to: Vec<_> = to
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    if unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(CoreError::Storage)
    } else {
        Ok(())
    }
}
#[cfg(not(windows))]
fn replace_profile(from: &Path, to: &Path) -> Result<(), CoreError> {
    fs::rename(from, to).map_err(|_| CoreError::Storage)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identities_survive_restart_and_configuration_edits() {
        let dir = tempfile::tempdir().unwrap();
        let first = DesktopProfile::load_or_create(dir.path()).unwrap();
        assert!(!first.claude_enabled);
        let mut changed = first.clone();
        changed.claude_enabled = true;
        changed.timezone = "America/Phoenix".into();
        changed.claude_root_path = Some(dir.path().join("fixture logs").to_string_lossy().into());
        fs::write(
            dir.path().join("profile.json"),
            serde_json::to_vec(&changed).unwrap(),
        )
        .unwrap();
        let second = DesktopProfile::load_or_create(dir.path()).unwrap();
        assert_eq!(second.device_id, first.device_id);
        assert_eq!(second.claude_dataset_id, first.claude_dataset_id);
        assert!(second.claude_enabled);
    }
    #[test]
    fn newer_or_corrupt_profiles_are_never_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = DesktopProfile::load_or_create(dir.path()).unwrap();
        profile.profile_version = 3;
        let bytes = serde_json::to_vec(&profile).unwrap();
        fs::write(dir.path().join("profile.json"), &bytes).unwrap();
        assert!(matches!(
            DesktopProfile::load_or_create(dir.path()),
            Err(CoreError::StorageSchemaNewer)
        ));
        assert_eq!(fs::read(dir.path().join("profile.json")).unwrap(), bytes);
    }
    #[test]
    fn retained_database_without_profile_never_gets_new_dataset_ids() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("usage.db"), b"retained data").unwrap();
        assert!(matches!(
            DesktopProfile::load_or_create(dir.path()),
            Err(CoreError::InvalidData)
        ));
        assert!(!dir.path().join("profile.json").exists());
    }
    #[test]
    fn legacy_profile_gets_settings_defaults_without_new_identities() {
        let original = DesktopProfile::fresh();
        let mut legacy = serde_json::to_value(&original).unwrap();
        legacy.as_object_mut().unwrap().remove("settingsRevision");
        legacy.as_object_mut().unwrap().remove("claudeDirectoryRef");
        let read: DesktopProfile = serde_json::from_value(legacy).unwrap();
        read.validate().unwrap();
        assert_eq!(read.settings_revision, 1);
        assert_eq!(read.device_id, original.device_id);
        assert_eq!(read.claude_dataset_id, original.claude_dataset_id);
    }
    #[test]
    fn version_one_migration_creates_stable_separate_provider_datasets() {
        let dir = tempfile::tempdir().unwrap();
        let mut old = DesktopProfile::fresh();
        old.profile_version = 1;
        old.additional_providers.clear();
        fs::write(
            dir.path().join("profile.json"),
            serde_json::to_vec(&old).unwrap(),
        )
        .unwrap();
        let migrated = DesktopProfile::load_or_create(dir.path()).unwrap();
        let reopened = DesktopProfile::load_or_create(dir.path()).unwrap();
        assert_eq!(migrated.device_id, old.device_id);
        assert_eq!(migrated.claude_dataset_id, old.claude_dataset_id);
        assert_eq!(
            migrated.provider("ccusage.codex").unwrap().dataset_id,
            reopened.provider("ccusage.codex").unwrap().dataset_id
        );
        assert_ne!(
            migrated.provider("ccusage.codex").unwrap().dataset_id,
            migrated.provider("ccusage.antigravity").unwrap().dataset_id
        );
    }
}
