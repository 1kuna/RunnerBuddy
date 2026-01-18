mod config;
mod discovery;
mod errors;
mod github_api;
mod logging;
mod logs;
mod runner_mgmt;
mod secrets;
mod service_mgmt;
mod state;
mod util;

use crate::config::{
    default_install_path, default_runner_labels, default_work_dir, now_iso8601, AdoptionDefault,
    Config, InstallMode, OnboardingConfig, RunnerProfile, RunnerScope, SettingsConfig,
};
use crate::errors::{AppError, AppResult, Error};
use crate::service_mgmt::ServiceStatus;
use crate::state::{AppSnapshot, AppState, RunnerStatus, RuntimeState};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{
    menu::MenuBuilder,
    menu::MenuItem,
    tray::TrayIconBuilder,
    AppHandle,
    Emitter,
    Manager,
    State,
};
use tracing::{error, info, warn};

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(serde::Serialize, Clone)]
struct RunnerStatusPayload {
    runner_id: String,
    status: RunnerStatus,
    pid: Option<u32>,
    last_heartbeat: Option<u64>,
}

#[derive(serde::Serialize)]
struct VerifyResult {
    ok: bool,
    reason: Option<String>,
}

#[derive(serde::Serialize)]
struct SettingsSnapshot {
    onboarding: OnboardingConfig,
    settings: SettingsConfig,
}

#[derive(Debug, Deserialize)]
struct SettingsPatch {
    auto_updates_enabled: Option<bool>,
    auto_check_updates_on_launch: Option<bool>,
    adoption_default: Option<AdoptionDefault>,
}

#[derive(Debug, Deserialize)]
struct AdoptOptions {
    strategy: AdoptionDefault,
    replace_service: bool,
    #[serde(default)]
    delete_original_after_verify: bool,
}

fn update_runtime(
    app: &AppHandle,
    state: &State<'_, AppState>,
    runner_id: &str,
    status: RunnerStatus,
    pid: Option<u32>,
    last_error: Option<String>,
) -> RuntimeState {
    let mut runtime_map = state.runtime.lock().expect("runtime mutex poisoned");
    let runtime = runtime_map
        .entry(runner_id.to_string())
        .or_insert_with(RuntimeState::default);
    runtime.status = status;
    runtime.pid = pid;
    runtime.last_heartbeat = Some(now_ts());
    runtime.last_error = last_error;
    let payload = RunnerStatusPayload {
        runner_id: runner_id.to_string(),
        status,
        pid,
        last_heartbeat: runtime.last_heartbeat,
    };
    let _ = app.emit("runner_status", payload);
    runtime.clone()
}

fn get_runner(config: &Config, runner_id: &str) -> Result<RunnerProfile, Error> {
    config
        .runners
        .iter()
        .find(|runner| runner.runner_id == runner_id)
        .cloned()
        .ok_or_else(|| Error::Runner(format!("runner {runner_id} not found")))
}

fn external_conflict_message(profile: &RunnerProfile, status: &ServiceStatus) -> Option<String> {
    if profile.service.provider != crate::config::ServiceProvider::External {
        return None;
    }
    if !status.installed && !status.running && !status.enabled {
        return None;
    }
    let mut details = Vec::new();
    if let Some(id) = profile.service.external_id.as_deref() {
        details.push(format!("id: {id}"));
    }
    if let Some(path) = profile.service.external_path.as_deref() {
        details.push(format!("path: {path}"));
    }
    let detail_str = if details.is_empty() {
        "external service detected".to_string()
    } else {
        format!("external service detected ({})", details.join(", "))
    };
    Some(format!(
        "{detail_str}; replace or remove it before installing RunnerBuddy. Replacing will disable/unload the external service and install a RunnerBuddy-managed service. You can restore the external service using the saved id/path."
    ))
}

fn ensure_no_external_conflict_with_status(
    profile: &RunnerProfile,
    status: ServiceStatus,
) -> Result<(), AppError> {
    if let Some(message) = external_conflict_message(profile, &status) {
        return Err(AppError::new("service", message));
    }
    Ok(())
}

fn ensure_no_external_conflict(profile: &RunnerProfile) -> Result<(), AppError> {
    if profile.service.provider == crate::config::ServiceProvider::External {
        let status = service_mgmt::external_status(profile).map_err(AppError::from)?;
        return ensure_no_external_conflict_with_status(profile, status);
    }
    Ok(())
}

fn handle_tray_menu(app: &AppHandle, menu_id: &str) {
    match menu_id {
        "open" => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        "start" => {
            let app_handle = app.clone();
            tauri::async_runtime::spawn(async move {
                let state = app_handle.state::<AppState>();
                let config = state.config.get();
                let selected = match config.selected_runner_id {
                    Some(id) => id,
                    None => {
                        warn!("tray start requested but no runner selected");
                        return;
                    }
                };
                match runner_mgmt::start_runner(&state.config, &selected, &state.runner_children) {
                    Ok(pid) => {
                        update_runtime(&app_handle, &state, &selected, RunnerStatus::Idle, Some(pid), None);
                        info!("Runner {selected} started from tray");
                    }
                    Err(err) => {
                        error!("Runner start from tray failed: {err}");
                    }
                }
            });
        }
        "stop" => {
            let app_handle = app.clone();
            tauri::async_runtime::spawn(async move {
                let state = app_handle.state::<AppState>();
                let config = state.config.get();
                let selected = match config.selected_runner_id {
                    Some(id) => id,
                    None => {
                        warn!("tray stop requested but no runner selected");
                        return;
                    }
                };
                if let Err(err) = runner_mgmt::stop_runner(&selected, &state.runner_children) {
                    error!("Runner stop from tray failed: {err}");
                    return;
                }
                update_runtime(&app_handle, &state, &selected, RunnerStatus::Offline, None, None);
                info!("Runner {selected} stopped from tray");
            });
        }
        "quit" => {
            app.exit(0);
        }
        _ => {}
    }
}

fn setup_tray(app: &AppHandle) -> Result<(), Error> {
    let open_item = MenuItem::with_id(app, "open", "Open RunnerBuddy", true, None::<&str>)
        .map_err(|err| Error::Service(err.to_string()))?;
    let start_item = MenuItem::with_id(app, "start", "Start runner", true, None::<&str>)
        .map_err(|err| Error::Service(err.to_string()))?;
    let stop_item = MenuItem::with_id(app, "stop", "Stop runner", true, None::<&str>)
        .map_err(|err| Error::Service(err.to_string()))?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit RunnerBuddy", true, None::<&str>)
        .map_err(|err| Error::Service(err.to_string()))?;

    let menu = MenuBuilder::new(app)
        .item(&open_item)
        .item(&start_item)
        .item(&stop_item)
        .separator()
        .item(&quit_item)
        .build()
        .map_err(|err| Error::Service(err.to_string()))?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| Error::Service("default icon missing".into()))?;

    TrayIconBuilder::with_id("runnerbuddy")
        .menu(&menu)
        .icon(icon)
        .tooltip("RunnerBuddy")
        .on_menu_event(|app, event: tauri::menu::MenuEvent| {
            handle_tray_menu(app, event.id().as_ref());
        })
        .build(app)
        .map_err(|err| Error::Service(err.to_string()))?;
    Ok(())
}

#[tauri::command]
async fn app_get_state(state: State<'_, AppState>) -> AppResult<AppSnapshot> {
    Ok(AppSnapshot {
        config: state.config.get(),
        runtime: state.runtime.lock().expect("runtime mutex poisoned").clone(),
    })
}

#[tauri::command]
async fn runners_list(state: State<'_, AppState>) -> AppResult<AppSnapshot> {
    app_get_state(state).await
}

#[tauri::command]
async fn settings_get(state: State<'_, AppState>) -> AppResult<SettingsSnapshot> {
    let config = state.config.get();
    Ok(SettingsSnapshot {
        onboarding: config.onboarding,
        settings: config.settings,
    })
}

#[tauri::command]
async fn settings_update(
    state: State<'_, AppState>,
    patch: SettingsPatch,
) -> AppResult<SettingsSnapshot> {
    let updated = state
        .config
        .update(|config| {
            if let Some(value) = patch.auto_updates_enabled {
                config.settings.auto_updates_enabled = value;
                if !value {
                    config.settings.auto_check_updates_on_launch = false;
                }
            }
            if let Some(value) = patch.auto_check_updates_on_launch {
                if config.settings.auto_updates_enabled {
                    config.settings.auto_check_updates_on_launch = value;
                }
            }
            if let Some(value) = patch.adoption_default {
                config.settings.adoption_default = value;
            }
        })
        .map_err(AppError::from)?;
    info!(
        "Settings updated: auto_updates_enabled={}, auto_check_on_launch={}, adoption_default={:?}",
        updated.settings.auto_updates_enabled,
        updated.settings.auto_check_updates_on_launch,
        updated.settings.adoption_default
    );
    Ok(SettingsSnapshot {
        onboarding: updated.onboarding,
        settings: updated.settings,
    })
}

#[tauri::command]
async fn onboarding_complete(state: State<'_, AppState>) -> AppResult<SettingsSnapshot> {
    let completed_at = now_iso8601();
    let updated = state
        .config
        .update(|config| {
            config.onboarding.completed = true;
            config.onboarding.completed_at = Some(completed_at.clone());
        })
        .map_err(AppError::from)?;
    info!("Onboarding marked complete");
    Ok(SettingsSnapshot {
        onboarding: updated.onboarding,
        settings: updated.settings,
    })
}

#[tauri::command]
async fn onboarding_reset(state: State<'_, AppState>) -> AppResult<SettingsSnapshot> {
    let updated = state
        .config
        .update(|config| {
            config.onboarding.completed = false;
            config.onboarding.completed_at = None;
        })
        .map_err(AppError::from)?;
    info!("Onboarding reset");
    Ok(SettingsSnapshot {
        onboarding: updated.onboarding,
        settings: updated.settings,
    })
}

#[derive(Debug, Deserialize)]
struct CreateRunnerProfileInput {
    display_name: Option<String>,
    runner_name: Option<String>,
    labels: Option<Vec<String>>,
    work_dir: Option<String>,
    scope: Option<RunnerScope>,
    pat_alias: Option<String>,
}

#[tauri::command]
async fn runners_create_profile(
    state: State<'_, AppState>,
    input: CreateRunnerProfileInput,
) -> AppResult<String> {
    let runner_id = crate::config::new_runner_id();
    let config = state.config.get();
    let runner_name = input.runner_name.unwrap_or_else(util::default_runner_name);
    if runner_name.trim().is_empty() {
        return Err(AppError::new("runner", "runner name is required"));
    }
    let display_name = input
        .display_name
        .unwrap_or_else(|| runner_name.clone());
    let labels = input.labels.unwrap_or_else(default_runner_labels);
    let work_dir = input
        .work_dir
        .unwrap_or_else(|| default_work_dir(&runner_id).to_string_lossy().to_string());
    if work_dir.trim().is_empty() {
        return Err(AppError::new("runner", "work directory is required"));
    }
    let install_path = default_install_path(&runner_id)
        .map_err(AppError::from)?
        .to_string_lossy()
        .to_string();
    let profile = RunnerProfile {
        runner_id: runner_id.clone(),
        display_name,
        scope: input.scope,
        runner_name,
        labels,
        work_dir,
        install: crate::config::InstallConfig {
            mode: InstallMode::Managed,
            install_path,
            adopted_from_path: None,
            migration_status: crate::config::MigrationStatus::None,
        },
        runner_version: None,
        pat_alias: input
            .pat_alias
            .unwrap_or_else(|| config.pat_default_alias.clone()),
        service: crate::config::RunnerServiceConfig::default(),
        created_at: now_iso8601(),
        last_seen_at: None,
    };

    state
        .config
        .update(|config| {
            config.runners.push(profile);
            config.selected_runner_id = Some(runner_id.clone());
        })
        .map_err(AppError::from)?;

    Ok(runner_id)
}

#[derive(Debug, Deserialize)]
struct RunnerProfilePatch {
    display_name: Option<String>,
    runner_name: Option<String>,
    labels: Option<Vec<String>>,
    work_dir: Option<String>,
    scope: Option<RunnerScope>,
    pat_alias: Option<String>,
}

#[tauri::command]
async fn runners_update_profile(
    state: State<'_, AppState>,
    runner_id: String,
    patch: RunnerProfilePatch,
) -> AppResult<RunnerProfile> {
    let mut found = None;
    let updated = state
        .config
        .update(|config| {
            if let Some(runner) = config
                .runners
                .iter_mut()
                .find(|runner| runner.runner_id == runner_id)
            {
                if let Some(display_name) = patch.display_name {
                    runner.display_name = display_name;
                }
                if let Some(runner_name) = patch.runner_name {
                    runner.runner_name = runner_name;
                }
                if let Some(labels) = patch.labels {
                    runner.labels = labels;
                }
                if let Some(work_dir) = patch.work_dir {
                    runner.work_dir = work_dir;
                }
                if let Some(scope) = patch.scope {
                    runner.scope = Some(scope);
                }
                if let Some(pat_alias) = patch.pat_alias {
                    runner.pat_alias = pat_alias;
                }
                found = Some(runner.runner_id.clone());
            }
        })
        .map_err(AppError::from)?;
    let runner_id = found.ok_or_else(|| AppError::new("runner", "runner not found"))?;
    Ok(updated
        .runners
        .iter()
        .find(|runner| runner.runner_id == runner_id)
        .cloned()
        .ok_or_else(|| AppError::new("runner", "runner missing after update"))?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RunnerDeleteMode {
    ConfigOnly,
    LocalDelete,
    UnregisterAndDelete,
}

#[tauri::command]
async fn runners_delete_profile(
    app: AppHandle,
    state: State<'_, AppState>,
    runner_id: String,
    mode: RunnerDeleteMode,
) -> AppResult<()> {
    info!("Runner delete requested for {runner_id} ({mode:?})");
    let runner_id_log = runner_id.clone();
    let profile = get_runner(&state.config.get(), &runner_id).map_err(AppError::from)?;
    let _ = runner_mgmt::stop_runner(&runner_id, &state.runner_children);
    if profile.service.provider == crate::config::ServiceProvider::Runnerbuddy {
        let _ = service_mgmt::stop(&profile);
        let _ = service_mgmt::uninstall(&profile);
    }
    if matches!(mode, RunnerDeleteMode::UnregisterAndDelete) {
        unregister_runner(&profile).await.map_err(AppError::from)?;
    }
    if matches!(mode, RunnerDeleteMode::LocalDelete | RunnerDeleteMode::UnregisterAndDelete) {
        let install_path = util::expand_path(&profile.install.install_path);
        let work_dir = util::expand_path(&profile.work_dir);
        let logs_dir = crate::config::data_dir()
            .map_err(AppError::from)?
            .join("logs")
            .join(&profile.runner_id);
        let _ = std::fs::remove_dir_all(&install_path);
        let _ = std::fs::remove_dir_all(&work_dir);
        let _ = std::fs::remove_dir_all(&logs_dir);
    }

    let runner_id_clone = runner_id.clone();
    state
        .config
        .update(|config| {
            config.runners.retain(|runner| runner.runner_id != runner_id_clone);
            if config.selected_runner_id.as_deref() == Some(&runner_id_clone) {
                config.selected_runner_id =
                    config.runners.first().map(|runner| runner.runner_id.clone());
            }
        })
        .map_err(AppError::from)?;

    let mut runtime = state.runtime.lock().expect("runtime mutex poisoned");
    runtime.remove(&runner_id);
    let _ = app.emit(
        "runner_status",
        RunnerStatusPayload {
            runner_id,
            status: RunnerStatus::Offline,
            pid: None,
            last_heartbeat: Some(now_ts()),
        },
    );

    info!("Runner delete completed for {runner_id_log}");
    Ok(())
}

#[tauri::command]
async fn runners_select(
    state: State<'_, AppState>,
    runner_id: Option<String>,
) -> AppResult<()> {
    state
        .config
        .update(|config| {
            config.selected_runner_id = runner_id.clone();
        })
        .map_err(AppError::from)?;
    Ok(())
}

fn gh_token_from_cli() -> AppResult<String> {
    let mut candidates = Vec::from(["gh"]);
    if cfg!(target_os = "macos") {
        candidates.extend(["/opt/homebrew/bin/gh", "/usr/local/bin/gh", "/usr/bin/gh"]);
    } else if cfg!(target_os = "linux") {
        candidates.extend(["/usr/bin/gh", "/usr/local/bin/gh", "/snap/bin/gh"]);
    } else if cfg!(target_os = "windows") {
        candidates.push("gh.exe");
    }

    let home = std::env::var("HOME").ok();
    let home_candidates = home
        .as_deref()
        .map(|home| vec![format!("{home}/.local/bin/gh")])
        .unwrap_or_default();

    let mut output = None;
    let mut tried = Vec::new();

    for candidate in candidates
        .into_iter()
        .map(str::to_string)
        .chain(home_candidates.into_iter())
    {
        if tried.contains(&candidate) {
            continue;
        }
        tried.push(candidate.clone());
        match Command::new(&candidate)
            .args(["auth", "token", "--hostname", "github.com"])
            .output()
        {
            Ok(result) => {
                output = Some((candidate, result));
                break;
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(AppError::new(
                    "cli",
                    format!("failed to run GitHub CLI ({candidate}): {err}"),
                ));
            }
        }
    }

    let (gh_path, output) = output.ok_or_else(|| {
        AppError::new(
            "cli",
            format!(
                "GitHub CLI not found. RunnerBuddy looked for: {}. Install `gh`, or launch RunnerBuddy from a terminal after `gh auth login`.",
                tried.join(", ")
            ),
        )
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr.trim();
        let message = if message.is_empty() {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        } else {
            message.to_string()
        };
        let message = message
            .lines()
            .take(20)
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();

        let lowered = message.to_lowercase();
        let hint = if lowered.contains("not logged")
            || lowered.contains("not logged in")
            || lowered.contains("authentication")
        {
            " Run `gh auth login` in a terminal."
        } else {
            ""
        };

        return Err(AppError::new(
            "cli",
            format!("GitHub CLI token request failed ({gh_path}).{hint}\n{message}"),
        ));
    }

    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if token.is_empty() {
        return Err(AppError::new(
            "cli",
            "GitHub CLI returned an empty token. Run `gh auth login` and try again.",
        ));
    }

    Ok(token)
}

#[tauri::command]
async fn auth_save_pat(alias: String, pat: String) -> AppResult<()> {
    secrets::save_pat(&alias, &pat).map_err(AppError::from)?;
    info!("PAT stored in credential store for alias {alias}");
    Ok(())
}

#[tauri::command]
async fn auth_import_gh_token(alias: String) -> AppResult<()> {
    let token = gh_token_from_cli()?;
    secrets::save_pat(&alias, &token).map_err(AppError::from)?;
    info!("GitHub CLI token imported into credential store for alias {alias}");
    Ok(())
}

#[tauri::command]
async fn auth_clear_pat(alias: String) -> AppResult<()> {
    secrets::clear_pat(&alias).map_err(AppError::from)?;
    info!("PAT cleared from credential store for alias {alias}");
    Ok(())
}

#[tauri::command]
async fn auth_check_pat(alias: String) -> AppResult<bool> {
    let pat = secrets::load_pat(&alias).map_err(AppError::from)?;
    if let Some(pat) = pat {
        github_api::validate_pat(&pat).await.map_err(AppError::from)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[tauri::command]
async fn auth_set_default_alias(state: State<'_, AppState>, alias: String) -> AppResult<()> {
    state
        .config
        .update(|config| {
            config.pat_default_alias = alias.clone();
        })
        .map_err(AppError::from)?;
    Ok(())
}

#[tauri::command]
async fn github_get_registration_token(scope: config::RunnerScope, alias: String) -> AppResult<github_api::RegistrationToken> {
    info!("GitHub registration token requested for {} via alias {}", scope.url(), alias);
    let pat = secrets::load_pat(&alias).map_err(AppError::from)?;
    let pat = pat.ok_or_else(|| AppError::new("secrets", "PAT not found in keychain"))?;
    let token = github_api::get_registration_token(&scope, &pat)
        .await
        .map_err(AppError::from)?;
    Ok(token)
}

#[tauri::command]
async fn github_list_repos(alias: String) -> AppResult<Vec<github_api::RepoInfo>> {
    info!("GitHub repo list requested via alias {}", alias);
    let pat = secrets::load_pat(&alias).map_err(AppError::from)?;
    let pat = pat.ok_or_else(|| AppError::new("secrets", "PAT not found in keychain"))?;
    let repos = github_api::list_repos(&pat).await.map_err(AppError::from)?;
    info!("GitHub repo list returned {} repos for alias {}", repos.len(), alias);
    Ok(repos)
}

#[tauri::command]
async fn github_list_orgs(alias: String) -> AppResult<Vec<github_api::OrgInfo>> {
    info!("GitHub org list requested via alias {}", alias);
    let pat = secrets::load_pat(&alias).map_err(AppError::from)?;
    let pat = pat.ok_or_else(|| AppError::new("secrets", "PAT not found in keychain"))?;
    let orgs = github_api::list_orgs(&pat).await.map_err(AppError::from)?;
    info!("GitHub org list returned {} orgs for alias {}", orgs.len(), alias);
    Ok(orgs)
}

#[tauri::command]
async fn runner_repair_scope(
    state: State<'_, AppState>,
    runner_id: String,
) -> AppResult<RunnerProfile> {
    info!("Runner scope repair requested for {runner_id}");
    runner_mgmt::repair_runner_scope(&state.config, &runner_id)
        .map_err(AppError::from)
}

#[tauri::command]
async fn runner_download(
    app: AppHandle,
    state: State<'_, AppState>,
    runner_id: String,
    version: Option<String>,
) -> AppResult<RunnerProfile> {
    info!("Runner download requested for {runner_id}");
    runner_mgmt::download_runner(&app, &state.config, &runner_id, version)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
async fn runner_configure(
    state: State<'_, AppState>,
    runner_id: String,
    scope: config::RunnerScope,
    name: String,
    labels: Vec<String>,
    work_dir: String,
) -> AppResult<RunnerProfile> {
    runner_mgmt::configure_runner(&state.config, &runner_id, scope, name, labels, work_dir)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
async fn runner_start(
    app: AppHandle,
    state: State<'_, AppState>,
    runner_id: String,
) -> AppResult<RuntimeState> {
    let pid = runner_mgmt::start_runner(&state.config, &runner_id, &state.runner_children)
        .map_err(AppError::from)?;
    info!("Runner {runner_id} started with pid {pid}");
    let runtime = update_runtime(&app, &state, &runner_id, RunnerStatus::Idle, Some(pid), None);
    Ok(runtime)
}

#[tauri::command]
async fn runner_stop(
    app: AppHandle,
    state: State<'_, AppState>,
    runner_id: String,
) -> AppResult<RuntimeState> {
    runner_mgmt::stop_runner(&runner_id, &state.runner_children).map_err(AppError::from)?;
    info!("Runner {runner_id} stopped");
    let runtime = update_runtime(&app, &state, &runner_id, RunnerStatus::Offline, None, None);
    Ok(runtime)
}

#[tauri::command]
async fn runner_status(
    app: AppHandle,
    state: State<'_, AppState>,
    runner_id: String,
) -> AppResult<RuntimeState> {
    let config = state.config.get();
    let profile = get_runner(&config, &runner_id).map_err(AppError::from)?;
    let (running, pid) = check_runner_process(&state, &runner_id);

    let service_status = service_mgmt::status(&profile).map_err(AppError::from)?;
    let running = running || service_status.running;

    let status = if running {
        let log_dir = runner_mgmt::runner_log_dir(&profile);
        runner_mgmt::classify_runner_status(&log_dir).map_err(AppError::from)?
    } else {
        RunnerStatus::Offline
    };

    if status != RunnerStatus::Offline {
        let _ = state.config.update(|config| {
            if let Some(runner) = config
                .runners
                .iter_mut()
                .find(|runner| runner.runner_id == runner_id)
            {
                runner.last_seen_at = Some(now_iso8601());
            }
        });
    }

    Ok(update_runtime(&app, &state, &runner_id, status, pid, None))
}

#[tauri::command]
async fn runner_status_all(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<HashMap<String, RuntimeState>> {
    let config = state.config.get();
    let mut results = HashMap::new();
    for runner in config.runners.iter() {
        let runner_id = runner.runner_id.clone();
        let (running, pid) = check_runner_process(&state, &runner_id);
        let service_status = service_mgmt::status(runner).map_err(AppError::from)?;
        let running = running || service_status.running;
        let status = if running {
            runner_mgmt::classify_runner_status(&runner_mgmt::runner_log_dir(runner))
                .map_err(AppError::from)?
        } else {
            RunnerStatus::Offline
        };
        let runtime = update_runtime(&app, &state, &runner_id, status, pid, None);
        results.insert(runner_id, runtime);
    }
    Ok(results)
}

#[tauri::command]
async fn service_install(
    state: State<'_, AppState>,
    runner_id: String,
) -> AppResult<()> {
    let profile = get_runner(&state.config.get(), &runner_id).map_err(AppError::from)?;
    ensure_no_external_conflict(&profile)?;
    service_mgmt::install(&profile).map_err(AppError::from)?;
    info!("Service installed for runner {runner_id}");
    state
        .config
        .update(|config| {
            if let Some(runner) = config
                .runners
                .iter_mut()
                .find(|runner| runner.runner_id == runner_id)
            {
                runner.service.installed = true;
                runner.service.run_on_boot = true;
                runner.service.provider = crate::config::ServiceProvider::Runnerbuddy;
            }
        })
        .map_err(AppError::from)?;
    Ok(())
}

#[tauri::command]
async fn service_uninstall(
    state: State<'_, AppState>,
    runner_id: String,
) -> AppResult<()> {
    let profile = get_runner(&state.config.get(), &runner_id).map_err(AppError::from)?;
    if profile.service.provider == crate::config::ServiceProvider::External {
        return Err(AppError::new(
            "service",
            "external service is managing this runner; remove external artifacts first",
        ));
    }
    service_mgmt::uninstall(&profile).map_err(AppError::from)?;
    info!("Service uninstalled for runner {runner_id}");
    state
        .config
        .update(|config| {
            if let Some(runner) = config
                .runners
                .iter_mut()
                .find(|runner| runner.runner_id == runner_id)
            {
                runner.service.installed = false;
                runner.service.run_on_boot = false;
                if runner.service.provider == crate::config::ServiceProvider::Runnerbuddy {
                    runner.service.provider = crate::config::ServiceProvider::Unknown;
                }
            }
        })
        .map_err(AppError::from)?;
    Ok(())
}

#[tauri::command]
async fn service_enable_on_boot(
    state: State<'_, AppState>,
    runner_id: String,
    enabled: bool,
) -> AppResult<()> {
    let profile = get_runner(&state.config.get(), &runner_id).map_err(AppError::from)?;
    if enabled {
        ensure_no_external_conflict(&profile)?;
    }
    if enabled && !profile.service.installed {
        service_mgmt::install(&profile).map_err(AppError::from)?;
    }
    service_mgmt::enable_on_boot(&profile, enabled).map_err(AppError::from)?;
    info!("Run on boot set to {enabled} for {runner_id}");
    state
        .config
        .update(|config| {
            if let Some(runner) = config
                .runners
                .iter_mut()
                .find(|runner| runner.runner_id == runner_id)
            {
                runner.service.run_on_boot = enabled;
                if enabled {
                    runner.service.installed = true;
                    runner.service.provider = crate::config::ServiceProvider::Runnerbuddy;
                }
            }
        })
        .map_err(AppError::from)?;
    Ok(())
}

#[tauri::command]
async fn service_status(
    state: State<'_, AppState>,
    runner_id: String,
) -> AppResult<service_mgmt::ServiceStatus> {
    let profile = get_runner(&state.config.get(), &runner_id).map_err(AppError::from)?;
    service_mgmt::status(&profile).map_err(AppError::from)
}

#[tauri::command]
async fn service_status_all(
    state: State<'_, AppState>,
) -> AppResult<HashMap<String, service_mgmt::ServiceStatus>> {
    let config = state.config.get();
    let mut results = HashMap::new();
    for runner in config.runners.iter() {
        let status = service_mgmt::status(runner).map_err(AppError::from)?;
        results.insert(runner.runner_id.clone(), status);
    }
    Ok(results)
}

#[tauri::command]
async fn service_start(
    state: State<'_, AppState>,
    runner_id: String,
) -> AppResult<()> {
    let profile = get_runner(&state.config.get(), &runner_id).map_err(AppError::from)?;
    if profile.service.provider == crate::config::ServiceProvider::External {
        return Err(AppError::new(
            "service",
            "external service is managing this runner; start it externally",
        ));
    }
    service_mgmt::start(&profile).map_err(AppError::from)?;
    info!("Service start requested for {runner_id}");
    Ok(())
}

#[tauri::command]
async fn service_stop(
    state: State<'_, AppState>,
    runner_id: String,
) -> AppResult<()> {
    let profile = get_runner(&state.config.get(), &runner_id).map_err(AppError::from)?;
    if profile.service.provider == crate::config::ServiceProvider::External {
        return Err(AppError::new(
            "service",
            "external service is managing this runner; stop it externally",
        ));
    }
    service_mgmt::stop(&profile).map_err(AppError::from)?;
    info!("Service stop requested for {runner_id}");
    Ok(())
}

#[tauri::command]
async fn logs_list_sources(
    state: State<'_, AppState>,
    runner_id: String,
) -> AppResult<Vec<logs::LogSource>> {
    let config = state.config.get();
    logs::list_sources(&config, &runner_id, &state.log_paths.app_log).map_err(AppError::from)
}

#[tauri::command]
async fn logs_tail(
    state: State<'_, AppState>,
    runner_id: String,
    source: String,
    limit: Option<usize>,
) -> AppResult<Vec<logs::LogLine>> {
    let config = state.config.get();
    let path = logs::resolve_source_path(&config, &runner_id, &state.log_paths.app_log, &source);
    let limit = limit.unwrap_or(200);
    logs::tail(&path, limit).map_err(AppError::from)
}

#[tauri::command]
async fn discover_scan(state: State<'_, AppState>) -> AppResult<Vec<discovery::DiscoveryCandidate>> {
    let config = state.config.get();
    let candidates = discovery::scan(&config).map_err(AppError::from)?;
    let mut cache = state.discovery_cache.lock().expect("discovery mutex poisoned");
    cache.clear();
    for candidate in candidates.iter() {
        cache.insert(candidate.candidate_id.clone(), candidate.clone());
    }
    Ok(candidates)
}

#[tauri::command]
async fn discover_import(
    state: State<'_, AppState>,
    candidate_id: String,
    options: discovery::ImportOptions,
) -> AppResult<String> {
    let candidate = {
        let cache = state.discovery_cache.lock().expect("discovery mutex poisoned");
        cache
            .get(&candidate_id)
            .cloned()
            .ok_or_else(|| AppError::new("discover", "candidate not found"))?
    };
    let profile = discovery::import_candidate(&state.config, &candidate, &options)
        .map_err(AppError::from)?;
    if options.move_install && options.verify_after_move {
        let result = verify_runner_install(&state, &profile.runner_id).await?;
        if result.ok && options.delete_original_after_verify {
            warn!(
                "delete_original_after_verify requested for {}; ignoring and requiring explicit deletion",
                profile.runner_id
            );
        }
    }
    Ok(profile.runner_id)
}

#[tauri::command]
async fn discover_adopt(
    state: State<'_, AppState>,
    candidate_id: String,
    options: AdoptOptions,
) -> AppResult<String> {
    let move_install = options.strategy == AdoptionDefault::MoveVerifyDelete;
    let import_options = discovery::ImportOptions {
        replace_service: options.replace_service,
        move_install,
        verify_after_move: move_install,
        delete_original_after_verify: move_install && options.delete_original_after_verify,
    };
    discover_import(state, candidate_id, import_options).await
}

#[tauri::command]
async fn discover_migrate_service(
    state: State<'_, AppState>,
    runner_id: String,
    strategy: discovery::ServiceMigrationStrategy,
) -> AppResult<()> {
    let mut profile = get_runner(&state.config.get(), &runner_id).map_err(AppError::from)?;
    discovery::migrate_external_service(&mut profile, strategy).map_err(AppError::from)?;
    state
        .config
        .update(|config| {
            if let Some(runner) = config
                .runners
                .iter_mut()
                .find(|runner| runner.runner_id == runner_id)
            {
                runner.service = profile.service.clone();
            }
        })
        .map_err(AppError::from)?;
    Ok(())
}

#[tauri::command]
async fn discover_remove_external_artifacts(
    state: State<'_, AppState>,
    runner_id: String,
) -> AppResult<()> {
    let mut profile = get_runner(&state.config.get(), &runner_id).map_err(AppError::from)?;
    discovery::remove_external_artifacts(&mut profile).map_err(AppError::from)?;
    state
        .config
        .update(|config| {
            if let Some(runner) = config
                .runners
                .iter_mut()
                .find(|runner| runner.runner_id == runner_id)
            {
                runner.service = profile.service.clone();
            }
        })
        .map_err(AppError::from)?;
    Ok(())
}

#[tauri::command]
async fn discover_verify_runner(
    state: State<'_, AppState>,
    runner_id: String,
) -> AppResult<VerifyResult> {
    verify_runner_install(&state, &runner_id).await
}

#[tauri::command]
async fn discover_delete_original_install(
    state: State<'_, AppState>,
    runner_id: String,
) -> AppResult<()> {
    delete_original_install(&state, &runner_id)
}

#[tauri::command]
async fn discover_move_install(
    state: State<'_, AppState>,
    runner_id: String,
    destination: Option<String>,
) -> AppResult<RunnerProfile> {
    let profile = get_runner(&state.config.get(), &runner_id).map_err(AppError::from)?;
    if profile.service.provider == crate::config::ServiceProvider::External {
        let status = service_mgmt::external_status(&profile).map_err(AppError::from)?;
        if status.installed || status.running {
            return Err(AppError::new(
                "service",
                "external service detected; replace or remove external service before moving",
            ));
        }
    }
    let _ = runner_mgmt::stop_runner(&runner_id, &state.runner_children);
    if profile.service.provider == crate::config::ServiceProvider::Runnerbuddy {
        let _ = service_mgmt::stop(&profile);
    }
    discovery::move_install(&state.config, &runner_id, destination).map_err(AppError::from)
}

#[tauri::command]
async fn discover_rollback_move(
    state: State<'_, AppState>,
    runner_id: String,
) -> AppResult<RunnerProfile> {
    let profile = get_runner(&state.config.get(), &runner_id).map_err(AppError::from)?;
    let original_path = validate_rollback_move(&profile)?;
    if !original_path.exists() || !discovery::looks_like_runner_install(&original_path) {
        return Err(AppError::new(
            "runner",
            "original install path is missing or does not look like a runner directory",
        ));
    }
    if profile.service.provider == crate::config::ServiceProvider::External {
        return Err(AppError::new(
            "service",
            "external service is managing this runner; replace or remove external artifacts first",
        ));
    }

    info!(
        "Rollback move requested for {} ({} -> {})",
        runner_id,
        profile.install.install_path,
        original_path.to_string_lossy()
    );

    let managed_path = util::expand_path(&profile.install.install_path);
    let _ = runner_mgmt::stop_runner(&runner_id, &state.runner_children);
    if profile.service.provider == crate::config::ServiceProvider::Runnerbuddy
        && profile.service.installed
    {
        let _ = service_mgmt::stop(&profile);
        service_mgmt::uninstall(&profile).map_err(AppError::from)?;
    }

    let original_path_str = original_path.to_string_lossy().to_string();
    let updated = state
        .config
        .update(|config| {
            if let Some(runner) = config
                .runners
                .iter_mut()
                .find(|runner| runner.runner_id == runner_id)
            {
                runner.install.mode = crate::config::InstallMode::Adopted;
                runner.install.install_path = original_path_str.clone();
                runner.install.adopted_from_path = None;
                runner.install.migration_status = crate::config::MigrationStatus::None;
            }
        })
        .map_err(AppError::from)?;

    let updated_profile = get_runner(&updated, &runner_id).map_err(AppError::from)?;
    if profile.service.provider == crate::config::ServiceProvider::Runnerbuddy
        && profile.service.installed
    {
        service_mgmt::install(&updated_profile).map_err(AppError::from)?;
    }

    if managed_path == original_path {
        warn!("Rollback skipped deleting managed copy because paths match: {:?}", managed_path);
    } else if should_delete_managed_copy(&runner_id, &managed_path) {
        if managed_path.exists() && discovery::looks_like_runner_install(&managed_path) {
            if let Err(err) = std::fs::remove_dir_all(&managed_path) {
                warn!(
                    "Rollback left managed copy in place (failed to delete {:?}): {err}",
                    managed_path
                );
            }
        }
    } else {
        warn!(
            "Rollback did not delete managed copy (path outside managed runners dir): {:?}",
            managed_path
        );
    }

    Ok(updated_profile)
}

async fn verify_runner_install(
    state: &State<'_, AppState>,
    runner_id: &str,
) -> AppResult<VerifyResult> {
    let profile = get_runner(&state.config.get(), runner_id).map_err(AppError::from)?;
    if profile.service.provider == crate::config::ServiceProvider::External {
        return Err(AppError::new(
            "service",
            "external service is managing this runner; replace or remove it before verification",
        ));
    }
    let (child_running, _) = check_runner_process(state, runner_id);
    let service_status = service_mgmt::status(&profile).map_err(AppError::from)?;
    if child_running {
        let _ = runner_mgmt::stop_runner(runner_id, &state.runner_children);
    }
    if service_status.running {
        let _ = service_mgmt::stop(&profile);
    }

    let started_via_service =
        profile.service.provider == crate::config::ServiceProvider::Runnerbuddy
            && profile.service.installed;
    if started_via_service {
        service_mgmt::start(&profile).map_err(AppError::from)?;
    } else {
        runner_mgmt::start_runner(&state.config, runner_id, &state.runner_children)
            .map_err(AppError::from)?;
    }

    let log_dir = runner_mgmt::runner_log_dir(&profile);
    let timeout = Duration::from_secs(60);
    let mut ok = false;
    let mut reason = None;
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if runner_mgmt::has_ready_marker(&log_dir).map_err(AppError::from)? {
            ok = true;
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    if !ok {
        reason = Some("runner did not report ready state before timeout".to_string());
    }

    if !service_status.running && started_via_service {
        let _ = service_mgmt::stop(&profile);
    }
    if !child_running && !started_via_service {
        let _ = runner_mgmt::stop_runner(runner_id, &state.runner_children);
    }

    let status = if ok {
        crate::config::MigrationStatus::Verified
    } else {
        crate::config::MigrationStatus::Failed
    };
    let _ = state.config.update(|config| {
        if let Some(runner) = config
            .runners
            .iter_mut()
            .find(|runner| runner.runner_id == runner_id)
        {
            runner.install.migration_status = status.clone();
        }
    });

    Ok(VerifyResult { ok, reason })
}

fn validate_delete_original_install(profile: &RunnerProfile) -> AppResult<PathBuf> {
    if profile.install.migration_status != crate::config::MigrationStatus::Verified {
        return Err(AppError::new(
            "runner",
            "runner has not been verified since migration",
        ));
    }
    if profile.service.provider == crate::config::ServiceProvider::External
        || profile.service.external_id.is_some()
        || profile.service.external_path.is_some()
    {
        return Err(AppError::new(
            "service",
            "external service still references this runner; remove external artifacts first",
        ));
    }
    let original = profile
        .install
        .adopted_from_path
        .clone()
        .ok_or_else(|| AppError::new("runner", "no original install recorded"))?;
    Ok(PathBuf::from(original))
}

fn validate_rollback_move(profile: &RunnerProfile) -> AppResult<PathBuf> {
    if profile.install.mode != crate::config::InstallMode::Managed {
        return Err(AppError::new("runner", "runner is not managed"));
    }
    if profile.install.migration_status == crate::config::MigrationStatus::Verified {
        return Err(AppError::new(
            "runner",
            "runner has been verified since migration; rollback is not allowed",
        ));
    }
    let original = profile
        .install
        .adopted_from_path
        .clone()
        .ok_or_else(|| AppError::new("runner", "no original install recorded"))?;
    let original_path = util::expand_path(&original);
    Ok(original_path)
}

fn should_delete_managed_copy(runner_id: &str, install_path: &PathBuf) -> bool {
    let managed_dir = match crate::config::managed_runners_dir() {
        Ok(path) => path,
        Err(_) => return false,
    };
    if install_path.starts_with(&managed_dir) {
        return true;
    }
    match crate::config::default_install_path(runner_id) {
        Ok(expected) => install_path == &expected,
        Err(_) => false,
    }
}

fn delete_original_install(state: &State<'_, AppState>, runner_id: &str) -> AppResult<()> {
    let profile = get_runner(&state.config.get(), runner_id).map_err(AppError::from)?;
    let original_path = validate_delete_original_install(&profile)?;
    if !discovery::looks_like_runner_install(&original_path) {
        return Err(AppError::new(
            "runner",
            "original install path does not look like a runner directory",
        ));
    }
    std::fs::remove_dir_all(&original_path)
        .map_err(Error::from)
        .map_err(AppError::from)?;
    state
        .config
        .update(|config| {
            if let Some(runner) = config
                .runners
                .iter_mut()
                .find(|runner| runner.runner_id == runner_id)
            {
                runner.install.adopted_from_path = None;
            }
        })
        .map_err(AppError::from)?;
    Ok(())
}

fn check_runner_process(
    state: &State<'_, AppState>,
    runner_id: &str,
) -> (bool, Option<u32>) {
    let mut running = false;
    let mut pid = None;
    let mut guard = state
        .runner_children
        .lock()
        .expect("runner child mutex poisoned");
    if let Some(child) = guard.get_mut(runner_id) {
        match child.try_wait() {
            Ok(Some(_)) => {
                guard.remove(runner_id);
            }
            Ok(None) => {
                pid = Some(child.id());
                running = true;
            }
            Err(err) => {
                error!("runner process check failed: {err}");
            }
        }
    }
    (running, pid)
}

async fn unregister_runner(profile: &RunnerProfile) -> Result<(), Error> {
    let scope = match profile.scope.clone() {
        Some(scope) => scope,
        None => return Ok(()),
    };
    let pat = secrets::load_pat(&profile.pat_alias)?
        .ok_or_else(|| Error::Runner("PAT not found for unregister".into()))?;
    let token = github_api::get_remove_token(&scope, &pat).await?;
    let install_path = util::expand_path(&profile.install.install_path);
    let config_script = if cfg!(target_os = "windows") {
        install_path.join("config.cmd")
    } else {
        install_path.join("config.sh")
    };
    if config_script.exists() {
        let status = std::process::Command::new(config_script)
            .current_dir(&install_path)
            .arg("remove")
            .arg("--token")
            .arg(&token.token)
            .status();
        if let Ok(status) = status {
            if !status.success() {
                warn!("runner remove failed");
            }
        }
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let log_setup = logging::init_logging().expect("failed to init logging");
    let config_store = config::ConfigStore::load().expect("failed to load config");
    let app_state = AppState::new(config_store, log_setup);
    info!("RunnerBuddy starting");
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .manage(app_state)
        .setup(|app| {
            #[cfg(desktop)]
            app.handle().plugin(tauri_plugin_updater::Builder::new().build())?;
            if let Err(err) = setup_tray(app.handle()) {
                error!("tray setup failed: {err}");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_get_state,
            runners_list,
            settings_get,
            settings_update,
            onboarding_complete,
            onboarding_reset,
            runners_create_profile,
            runners_update_profile,
            runners_delete_profile,
            runners_select,
            auth_save_pat,
            auth_import_gh_token,
            auth_clear_pat,
            auth_check_pat,
            auth_set_default_alias,
            github_get_registration_token,
            github_list_repos,
            github_list_orgs,
            runner_repair_scope,
            runner_download,
            runner_configure,
            runner_start,
            runner_stop,
            runner_status,
            runner_status_all,
            service_install,
            service_uninstall,
            service_enable_on_boot,
            service_status,
            service_status_all,
            service_start,
            service_stop,
            logs_list_sources,
            logs_tail,
            discover_scan,
            discover_import,
            discover_adopt,
            discover_migrate_service,
            discover_remove_external_artifacts,
            discover_verify_runner,
            discover_delete_original_install,
            discover_move_install,
            discover_rollback_move,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        default_install_path, now_iso8601, new_runner_id, InstallConfig, InstallMode,
        MigrationStatus, RunnerServiceConfig, RunnerScope, ServiceProvider,
    };
    use crate::{runner_mgmt, secrets};
    use std::collections::HashMap;
    use std::env;
    use std::sync::Mutex;
    use tempfile::tempdir;
    use tauri::test::mock_app;

    fn sample_profile() -> RunnerProfile {
        RunnerProfile {
            runner_id: "abc".to_string(),
            display_name: "Test".to_string(),
            scope: None,
            runner_name: "runner".to_string(),
            labels: vec!["self-hosted".to_string()],
            work_dir: "/tmp".to_string(),
            install: InstallConfig {
                mode: InstallMode::Managed,
                install_path: "/tmp/runner".to_string(),
                adopted_from_path: Some("/tmp/original".to_string()),
                migration_status: MigrationStatus::Verified,
            },
            runner_version: None,
            pat_alias: "default".to_string(),
            service: RunnerServiceConfig {
                installed: true,
                run_on_boot: true,
                provider: ServiceProvider::External,
                external_id: Some("svc.id".to_string()),
                external_path: Some("/tmp/service.plist".to_string()),
                external_restore: None,
            },
            created_at: "2024-01-01T00:00:00Z".to_string(),
            last_seen_at: None,
        }
    }

    #[test]
    fn external_conflict_message_includes_details() {
        let profile = sample_profile();
        let status = ServiceStatus {
            installed: true,
            running: false,
            enabled: true,
        };
        let message = external_conflict_message(&profile, &status).expect("message");
        assert!(message.contains("id: svc.id"));
        assert!(message.contains("path: /tmp/service.plist"));
    }

    #[test]
    fn external_conflict_message_skips_non_external() {
        let mut profile = sample_profile();
        profile.service.provider = ServiceProvider::Runnerbuddy;
        let status = ServiceStatus {
            installed: true,
            running: true,
            enabled: true,
        };
        assert!(external_conflict_message(&profile, &status).is_none());
    }

    #[test]
    fn ensure_no_external_conflict_with_status_blocks_installed() {
        let profile = sample_profile();
        let status = ServiceStatus {
            installed: true,
            running: false,
            enabled: false,
        };
        let err = ensure_no_external_conflict_with_status(&profile, status).expect_err("conflict");
        assert_eq!(err.code, "service");
    }

    #[test]
    fn validate_delete_original_requires_verified() {
        let mut profile = sample_profile();
        profile.install.migration_status = MigrationStatus::Moved;
        let err = validate_delete_original_install(&profile).expect_err("error");
        assert_eq!(err.code, "runner");
    }

    #[test]
    fn validate_delete_original_blocks_external_refs() {
        let mut profile = sample_profile();
        profile.install.migration_status = MigrationStatus::Verified;
        let err = validate_delete_original_install(&profile).expect_err("error");
        assert_eq!(err.code, "service");
    }

    #[test]
    fn validate_delete_original_returns_path() {
        let mut profile = sample_profile();
        profile.service.provider = ServiceProvider::Runnerbuddy;
        profile.service.external_id = None;
        profile.service.external_path = None;
        let path = validate_delete_original_install(&profile).expect("path");
        assert_eq!(path.to_string_lossy(), "/tmp/original");
    }

    #[test]
    fn validate_rollback_move_requires_managed() {
        let mut profile = sample_profile();
        profile.install.mode = InstallMode::Adopted;
        let err = validate_rollback_move(&profile).expect_err("error");
        assert_eq!(err.code, "runner");
    }

    #[test]
    fn validate_rollback_move_blocks_verified() {
        let profile = sample_profile();
        let err = validate_rollback_move(&profile).expect_err("error");
        assert_eq!(err.code, "runner");
    }

    #[test]
    fn validate_rollback_move_returns_path() {
        let mut profile = sample_profile();
        profile.install.migration_status = MigrationStatus::Failed;
        profile.install.mode = InstallMode::Managed;
        let path = validate_rollback_move(&profile).expect("path");
        assert_eq!(path.to_string_lossy(), "/tmp/original");
    }

    #[tokio::test]
    async fn integration_runner_flow_gated() {
        let pat = match env::var("RUNNERBUDDY_TEST_PAT") {
            Ok(value) if !value.trim().is_empty() => value,
            _ => {
                eprintln!("RUNNERBUDDY_TEST_PAT not set; skipping integration test");
                return;
            }
        };
        let scope_raw = match env::var("RUNNERBUDDY_TEST_SCOPE") {
            Ok(value) if !value.trim().is_empty() => value,
            _ => {
                eprintln!("RUNNERBUDDY_TEST_SCOPE not set; skipping integration test");
                return;
            }
        };

        let scope = parse_test_scope(&scope_raw).unwrap_or_else(|| {
            panic!("RUNNERBUDDY_TEST_SCOPE must be repo:owner/repo, org:org, or enterprise:slug")
        });

        let home = tempdir().expect("temp home");
        env::set_var("HOME", home.path());
        env::set_var("XDG_CONFIG_HOME", home.path().join(".config"));
        env::set_var("XDG_DATA_HOME", home.path().join(".local/share"));

        let config_store = crate::config::ConfigStore::load().expect("config store");
        let runner_id = new_runner_id();
        let install_path = default_install_path(&runner_id).expect("install path");
        let work_dir = home.path().join("work").to_string_lossy().to_string();
        let runner_name = format!("runnerbuddy-test-{}", &runner_id[..8]);

        secrets::save_pat("integration", &pat).expect("save pat");

        config_store
            .update(|config| {
                config.runners.push(RunnerProfile {
                    runner_id: runner_id.clone(),
                    display_name: runner_name.clone(),
                    scope: Some(scope.clone()),
                    runner_name: runner_name.clone(),
                    labels: vec!["self-hosted".to_string(), "runnerbuddy".to_string()],
                    work_dir: work_dir.clone(),
                    install: InstallConfig {
                        mode: InstallMode::Managed,
                        install_path: install_path.to_string_lossy().to_string(),
                        adopted_from_path: None,
                        migration_status: MigrationStatus::None,
                    },
                    runner_version: None,
                    pat_alias: "integration".to_string(),
                    service: RunnerServiceConfig::default(),
                    created_at: now_iso8601(),
                    last_seen_at: None,
                });
                config.selected_runner_id = Some(runner_id.clone());
            })
            .expect("config update");

        let app = mock_app();
        let app_handle = app.handle();

        runner_mgmt::download_runner(&app_handle, &config_store, &runner_id, None)
            .await
            .expect("download runner");

        let profile = runner_mgmt::configure_runner(
            &config_store,
            &runner_id,
            scope,
            runner_name,
            vec!["self-hosted".to_string(), "runnerbuddy".to_string()],
            work_dir.clone(),
        )
        .await
        .expect("configure runner");

        let child_map = Mutex::new(HashMap::new());
        runner_mgmt::start_runner(&config_store, &runner_id, &child_map).expect("start runner");

        let log_dir = runner_mgmt::runner_log_dir(&profile);
        let timeout = Duration::from_secs(60);
        let start = std::time::Instant::now();
        let mut ready = false;
        while start.elapsed() < timeout {
            if runner_mgmt::has_ready_marker(&log_dir).unwrap_or(false) {
                ready = true;
                break;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        assert!(ready, "runner did not report ready state");

        let _ = runner_mgmt::stop_runner(&runner_id, &child_map);
        unregister_runner(&profile).await.expect("unregister runner");
        secrets::clear_pat("integration").expect("clear pat");
        let _ = std::fs::remove_dir_all(&install_path);
        let _ = std::fs::remove_dir_all(&work_dir);
    }

    fn parse_test_scope(raw: &str) -> Option<RunnerScope> {
        let trimmed = raw.trim();
        if let Some(value) = trimmed.strip_prefix("repo:") {
            let mut parts = value.splitn(2, '/');
            let owner = parts.next()?.to_string();
            let repo = parts.next()?.to_string();
            if owner.is_empty() || repo.is_empty() {
                return None;
            }
            return Some(RunnerScope::Repo { owner, repo });
        }
        if let Some(value) = trimmed.strip_prefix("org:") {
            let org = value.trim().to_string();
            if org.is_empty() {
                return None;
            }
            return Some(RunnerScope::Org { org });
        }
        if let Some(value) = trimmed.strip_prefix("enterprise:") {
            let enterprise = value.trim().to_string();
            if enterprise.is_empty() {
                return None;
            }
            return Some(RunnerScope::Enterprise { enterprise });
        }
        None
    }
}
