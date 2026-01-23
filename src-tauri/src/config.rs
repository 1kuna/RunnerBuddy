use crate::errors::Error;
use crate::util::{default_runner_name, platform_label};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tracing::warn;
use uuid::Uuid;

const SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AdoptionDefault {
    Adopt,
    MoveVerifyDelete,
}

impl Default for AdoptionDefault {
    fn default() -> Self {
        AdoptionDefault::Adopt
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum AuthMethod {
    Pat,
}

impl Default for AuthMethod {
    fn default() -> Self {
        AuthMethod::Pat
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RunnerScope {
    Repo { owner: String, repo: String },
    Org { org: String },
    Enterprise { enterprise: String },
}

impl RunnerScope {
    pub fn api_registration_endpoint(&self) -> String {
        match self {
            RunnerScope::Repo { owner, repo } => {
                format!("/repos/{owner}/{repo}/actions/runners/registration-token")
            }
            RunnerScope::Org { org } => format!("/orgs/{org}/actions/runners/registration-token"),
            RunnerScope::Enterprise { enterprise } => {
                format!("/enterprises/{enterprise}/actions/runners/registration-token")
            }
        }
    }

    pub fn api_remove_endpoint(&self) -> String {
        match self {
            RunnerScope::Repo { owner, repo } => {
                format!("/repos/{owner}/{repo}/actions/runners/remove-token")
            }
            RunnerScope::Org { org } => format!("/orgs/{org}/actions/runners/remove-token"),
            RunnerScope::Enterprise { enterprise } => {
                format!("/enterprises/{enterprise}/actions/runners/remove-token")
            }
        }
    }

    pub fn url(&self) -> String {
        match self {
            RunnerScope::Repo { owner, repo } => format!("https://github.com/{owner}/{repo}"),
            RunnerScope::Org { org } => format!("https://github.com/{org}"),
            RunnerScope::Enterprise { enterprise } => {
                format!("https://github.com/enterprises/{enterprise}")
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InstallMode {
    Managed,
    Adopted,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MigrationStatus {
    None,
    Moved,
    Verified,
    Failed,
}

impl Default for MigrationStatus {
    fn default() -> Self {
        MigrationStatus::None
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InstallConfig {
    pub mode: InstallMode,
    pub install_path: String,
    #[serde(default)]
    pub adopted_from_path: Option<String>,
    #[serde(default)]
    pub migration_status: MigrationStatus,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ServiceProvider {
    Runnerbuddy,
    External,
    Unknown,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExternalServiceInfo {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RunnerServiceConfig {
    pub installed: bool,
    pub run_on_boot: bool,
    pub provider: ServiceProvider,
    #[serde(default)]
    pub external_id: Option<String>,
    #[serde(default)]
    pub external_path: Option<String>,
    #[serde(default)]
    pub external_restore: Option<ExternalServiceInfo>,
}

impl Default for RunnerServiceConfig {
    fn default() -> Self {
        Self {
            installed: false,
            run_on_boot: false,
            provider: ServiceProvider::Unknown,
            external_id: None,
            external_path: None,
            external_restore: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RunnerProfile {
    pub runner_id: String,
    pub display_name: String,
    pub scope: Option<RunnerScope>,
    pub runner_name: String,
    pub labels: Vec<String>,
    pub work_dir: String,
    pub install: InstallConfig,
    pub runner_version: Option<String>,
    pub pat_alias: String,
    pub service: RunnerServiceConfig,
    pub created_at: String,
    pub last_seen_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OnboardingConfig {
    pub completed: bool,
    #[serde(default)]
    pub completed_at: Option<String>,
}

impl OnboardingConfig {
    fn completed_for_upgrade() -> Self {
        Self {
            completed: true,
            completed_at: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SettingsConfig {
    pub auto_updates_enabled: bool,
    pub auto_check_updates_on_launch: bool,
    pub adoption_default: AdoptionDefault,
}

impl Default for SettingsConfig {
    fn default() -> Self {
        Self {
            auto_updates_enabled: true,
            auto_check_updates_on_launch: true,
            adoption_default: AdoptionDefault::Adopt,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub selected_runner_id: Option<String>,
    #[serde(default = "default_pat_alias")]
    pub pat_default_alias: String,
    #[serde(default = "default_onboarding")]
    pub onboarding: OnboardingConfig,
    #[serde(default = "default_settings")]
    pub settings: SettingsConfig,
    #[serde(default)]
    pub runners: Vec<RunnerProfile>,
}

fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

fn default_pat_alias() -> String {
    "default".to_string()
}

fn default_onboarding() -> OnboardingConfig {
    OnboardingConfig {
        completed: false,
        completed_at: None,
    }
}

fn default_settings() -> SettingsConfig {
    SettingsConfig::default()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            selected_runner_id: None,
            pat_default_alias: default_pat_alias(),
            onboarding: default_onboarding(),
            settings: default_settings(),
            runners: Vec::new(),
        }
    }
}

impl Config {
    fn migrate_from_legacy(legacy: LegacyConfig) -> Result<Self, Error> {
        let runner_id = new_runner_id();
        let install_path = if legacy.install_path.is_empty() {
            default_install_path(&runner_id)?.to_string_lossy().to_string()
        } else {
            legacy.install_path
        };
        let display_name = if legacy.runner.name.trim().is_empty() {
            default_runner_name()
        } else {
            legacy.runner.name.clone()
        };
        let profile = RunnerProfile {
            runner_id: runner_id.clone(),
            display_name,
            scope: legacy.runner.scope,
            runner_name: legacy.runner.name,
            labels: legacy.runner.labels,
            work_dir: legacy.runner.work_dir,
            install: InstallConfig {
                mode: InstallMode::Managed,
                install_path,
                adopted_from_path: None,
                migration_status: MigrationStatus::None,
            },
            runner_version: legacy.runner_version,
            pat_alias: default_pat_alias(),
            service: RunnerServiceConfig {
                installed: legacy.service.installed,
                run_on_boot: legacy.service.run_on_boot,
                provider: if legacy.service.installed || legacy.service.run_on_boot {
                    ServiceProvider::Runnerbuddy
                } else {
                    ServiceProvider::Unknown
                },
                external_id: None,
                external_path: None,
                external_restore: None,
            },
            created_at: now_iso8601(),
            last_seen_at: None,
        };
        Ok(Self {
            schema_version: SCHEMA_VERSION,
            selected_runner_id: Some(runner_id),
            pat_default_alias: default_pat_alias(),
            onboarding: OnboardingConfig::completed_for_upgrade(),
            settings: SettingsConfig::default(),
            runners: vec![profile],
        })
    }
}

pub fn find_runner(config: &Config, runner_id: &str) -> Result<RunnerProfile, Error> {
    config
        .runners
        .iter()
        .find(|runner| runner.runner_id == runner_id)
        .cloned()
        .ok_or_else(|| Error::Runner(format!("runner {runner_id} not found")))
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct LegacyRunnerConfig {
    pub name: String,
    pub labels: Vec<String>,
    pub work_dir: String,
    pub scope: Option<RunnerScope>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct LegacyServiceConfig {
    pub installed: bool,
    pub run_on_boot: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct LegacyGithubConfig {
    pub auth_method: AuthMethod,
    pub token_expires: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct LegacyConfig {
    #[serde(default = "default_legacy_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub runner: LegacyRunnerConfig,
    #[serde(default)]
    pub service: LegacyServiceConfig,
    #[serde(default)]
    pub github: LegacyGithubConfig,
    #[serde(default)]
    pub runner_version: Option<String>,
    #[serde(default)]
    pub install_path: String,
}

fn default_legacy_schema_version() -> u32 {
    1
}

impl Default for LegacyRunnerConfig {
    fn default() -> Self {
        let mut labels = default_runner_labels();
        Self {
            name: default_runner_name(),
            labels: labels.drain(..).collect(),
            work_dir: default_work_dir("default").to_string_lossy().to_string(),
            scope: None,
        }
    }
}

impl Default for LegacyServiceConfig {
    fn default() -> Self {
        Self {
            installed: false,
            run_on_boot: false,
        }
    }
}

impl Default for LegacyGithubConfig {
    fn default() -> Self {
        Self {
            auth_method: AuthMethod::Pat,
            token_expires: None,
        }
    }
}

pub struct ConfigStore {
    path: PathBuf,
    inner: Mutex<Config>,
}

impl ConfigStore {
    pub fn load() -> Result<Self, Error> {
        let path = config_path()?;
        let mut needs_save = false;
        let config = if path.exists() {
            let data = fs::read_to_string(&path)?;
            let value: serde_json::Value = serde_json::from_str(&data)?;
            let schema_version = value
                .get("schema_version")
                .and_then(|val| val.as_u64())
                .unwrap_or(1) as u32;
            let mut config = if schema_version == 1 {
                let legacy: LegacyConfig = serde_json::from_value(value.clone())?;
                needs_save = true;
                Config::migrate_from_legacy(legacy)?
            } else if schema_version < SCHEMA_VERSION {
                let mut config: Config = serde_json::from_value(value.clone())?;
                migrate_external_hint(&mut config, &value);
                config.schema_version = SCHEMA_VERSION;
                needs_save = true;
                config
            } else {
                serde_json::from_value(value.clone())?
            };
            if schema_version != 1 {
                if apply_missing_fields(&mut config, &value) {
                    needs_save = true;
                }
            }
            if sanitize_selected_runner_id(&mut config) {
                needs_save = true;
            }
            config
        } else {
            Config::default()
        };
        let store = Self {
            path,
            inner: Mutex::new(config),
        };
        if needs_save {
            let guard = store.inner.lock().expect("config mutex poisoned");
            store.save_locked(&guard)?;
        }
        Ok(store)
    }

    pub fn get(&self) -> Config {
        self.inner
            .lock()
            .expect("config mutex poisoned")
            .clone()
    }

    pub fn update<F>(&self, updater: F) -> Result<Config, Error>
    where
        F: FnOnce(&mut Config),
    {
        let mut guard = self.inner.lock().expect("config mutex poisoned");
        updater(&mut guard);
        sanitize_selected_runner_id(&mut guard);
        self.save_locked(&guard)?;
        Ok(guard.clone())
    }

    pub fn update_runner<F>(&self, runner_id: &str, updater: F) -> Result<RunnerProfile, Error>
    where
        F: FnOnce(&mut RunnerProfile),
    {
        let updated = self.update(|config| {
            if let Some(runner) = config
                .runners
                .iter_mut()
                .find(|runner| runner.runner_id == runner_id)
            {
                updater(runner);
            }
        })?;
        find_runner(&updated, runner_id)
    }

    fn save_locked(&self, config: &Config) -> Result<(), Error> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(config)?;
        fs::write(&self.path, data)?;
        Ok(())
    }
}

pub fn config_path() -> Result<PathBuf, Error> {
    let dirs = project_dirs()?;
    Ok(dirs.config_dir().join("config.json"))
}

fn migrate_external_hint(config: &mut Config, value: &serde_json::Value) {
    let runners = match value.get("runners").and_then(|val| val.as_array()) {
        Some(runners) => runners,
        None => return,
    };
    for (idx, runner_value) in runners.iter().enumerate() {
        let external_hint = runner_value
            .get("service")
            .and_then(|service| service.get("external_hint"))
            .and_then(|val| val.as_str());
        if let Some(hint) = external_hint {
            if let Some(profile) = config.runners.get_mut(idx) {
                if profile.service.external_id.is_none() {
                    profile.service.external_id = Some(hint.to_string());
                }
            }
        }
    }
}

fn apply_missing_fields(config: &mut Config, value: &serde_json::Value) -> bool {
    let mut updated = false;
    if value.get("onboarding").is_none() {
        config.onboarding = OnboardingConfig::completed_for_upgrade();
        updated = true;
    }
    if value.get("settings").is_none() {
        config.settings = SettingsConfig::default();
        updated = true;
    }
    updated
}

fn sanitize_selected_runner_id(config: &mut Config) -> bool {
    let previous = config.selected_runner_id.clone();
    let next = if config.runners.is_empty() {
        None
    } else {
        match previous.as_deref() {
            Some(selected)
                if config
                    .runners
                    .iter()
                    .any(|runner| runner.runner_id == selected) =>
            {
                previous.clone()
            }
            _ => Some(config.runners[0].runner_id.clone()),
        }
    };

    if previous == next {
        return false;
    }

    warn!(
        "Repairing config selected runner id from {:?} to {:?}",
        previous, next
    );
    config.selected_runner_id = next;
    true
}

pub fn data_dir() -> Result<PathBuf, Error> {
    let dirs = project_dirs()?;
    Ok(dirs.data_dir().to_path_buf())
}

pub fn logs_dir() -> Result<PathBuf, Error> {
    Ok(data_dir()?.join("logs"))
}

pub fn runner_logs_dir(runner_id: &str) -> Result<PathBuf, Error> {
    Ok(logs_dir()?.join(runner_id))
}

pub fn managed_runners_dir() -> Result<PathBuf, Error> {
    Ok(data_dir()?.join("runners"))
}

pub fn default_install_path(runner_id: &str) -> Result<PathBuf, Error> {
    Ok(managed_runners_dir()?.join(runner_id))
}

pub fn default_work_dir(runner_id: &str) -> PathBuf {
    let work_root = directories::UserDirs::new()
        .map(|dirs| dirs.home_dir().join(".runnerbuddy"))
        .unwrap_or_else(|| data_dir().unwrap_or_else(|_| PathBuf::from(".")));
    work_root.join("work").join(runner_id)
}

pub fn default_runner_labels() -> Vec<String> {
    let mut labels = vec!["self-hosted".to_string(), platform_label()];
    labels.push(std::env::consts::ARCH.to_uppercase());
    labels
}

pub fn now_iso8601() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

pub fn new_runner_id() -> String {
    Uuid::new_v4().to_string()
}

fn project_dirs() -> Result<ProjectDirs, Error> {
    ProjectDirs::from("com", "runnerbuddy", "RunnerBuddy")
        .ok_or_else(|| Error::Config("failed to resolve project directories".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_serializes() {
        let config = Config::default();
        let json = serde_json::to_string(&config).expect("serialize config");
        let decoded: Config = serde_json::from_str(&json).expect("deserialize config");
        assert_eq!(decoded.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn default_config_onboarding_incomplete() {
        let config = Config::default();
        assert!(!config.onboarding.completed);
        assert!(config.onboarding.completed_at.is_none());
    }

    #[test]
    fn apply_missing_fields_marks_onboarding_complete() {
        let mut config = Config::default();
        let value = serde_json::json!({
            "schema_version": SCHEMA_VERSION
        });
        assert!(apply_missing_fields(&mut config, &value));
        assert!(config.onboarding.completed);
        assert!(config.settings.auto_updates_enabled);
    }

    #[test]
    fn apply_missing_fields_respects_existing_settings() {
        let mut config = Config::default();
        config.onboarding.completed = false;
        config.settings.auto_updates_enabled = false;
        let value = serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "onboarding": { "completed": false, "completed_at": null },
            "settings": {
                "auto_updates_enabled": false,
                "auto_check_updates_on_launch": false,
                "adoption_default": "adopt"
            }
        });
        assert!(!apply_missing_fields(&mut config, &value));
        assert!(!config.onboarding.completed);
        assert!(!config.settings.auto_updates_enabled);
    }

    #[test]
    fn migrates_legacy_config() {
        let legacy = LegacyConfig {
            schema_version: 1,
            runner: LegacyRunnerConfig {
                name: "runner-1".to_string(),
                labels: vec!["self-hosted".to_string()],
                work_dir: "/tmp/work".to_string(),
                scope: None,
            },
            service: LegacyServiceConfig {
                installed: true,
                run_on_boot: true,
            },
            github: LegacyGithubConfig::default(),
            runner_version: Some("2.0".to_string()),
            install_path: "/tmp/runner".to_string(),
        };
        let migrated = Config::migrate_from_legacy(legacy).expect("migrate");
        assert_eq!(migrated.schema_version, SCHEMA_VERSION);
        assert_eq!(migrated.runners.len(), 1);
        assert_eq!(migrated.runners[0].runner_name, "runner-1");
        assert!(migrated.selected_runner_id.is_some());
    }

    #[test]
    fn remove_endpoint_matches_scope() {
        let repo = RunnerScope::Repo {
            owner: "org".to_string(),
            repo: "repo".to_string(),
        };
        assert_eq!(
            repo.api_remove_endpoint(),
            "/repos/org/repo/actions/runners/remove-token"
        );
        let org = RunnerScope::Org {
            org: "acme".to_string(),
        };
        assert_eq!(org.api_remove_endpoint(), "/orgs/acme/actions/runners/remove-token");
        let enterprise = RunnerScope::Enterprise {
            enterprise: "umbrella".to_string(),
        };
        assert_eq!(
            enterprise.api_remove_endpoint(),
            "/enterprises/umbrella/actions/runners/remove-token"
        );
    }
}
