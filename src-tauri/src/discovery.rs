use crate::config::{
    default_install_path, default_runner_labels, default_work_dir, now_iso8601, Config, ConfigStore,
    ExternalServiceInfo, InstallConfig, InstallMode, RunnerProfile, RunnerScope,
    RunnerServiceConfig, ServiceProvider,
};
use crate::errors::Error;
use crate::runner_mgmt::latest_log_file;
use crate::service_mgmt;
use crate::util::{default_runner_name, read_file_tail, LOG_TAIL_BYTES};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(target_os = "windows")]
use std::process::Command;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tracing::info;

#[derive(Debug, Serialize, Clone)]
pub struct DiscoveryCandidate {
    pub candidate_id: String,
    pub install_path: String,
    pub runner_name: Option<String>,
    pub labels: Vec<String>,
    pub scope: Option<RunnerScope>,
    pub work_dir: Option<String>,
    pub service_present: bool,
    pub service_id: Option<String>,
    pub service_path: Option<String>,
    pub last_log_time: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ImportOptions {
    pub replace_service: bool,
    pub move_install: bool,
    #[serde(default)]
    pub verify_after_move: bool,
    #[serde(default)]
    pub delete_original_after_verify: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceMigrationStrategy {
    ReplaceWithRunnerbuddy,
}

pub fn scan(config: &Config) -> Result<Vec<DiscoveryCandidate>, Error> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    let mut add_path = |path: PathBuf| {
        if let Ok(canonical) = path.canonicalize() {
            if seen.insert(canonical.clone()) {
                paths.push(canonical);
            }
        } else if seen.insert(path.clone()) {
            paths.push(path);
        }
    };

    if let Ok(managed_dir) = crate::config::managed_runners_dir() {
        if managed_dir.exists() {
            if let Ok(entries) = fs::read_dir(&managed_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        add_path(path);
                    }
                }
            }
        }
    }

    if let Some(user_dirs) = directories::UserDirs::new() {
        let home = user_dirs.home_dir();
        let downloads = home.join("Downloads");
        for root in [home.to_path_buf(), downloads] {
            for path in scan_prefixes(&root, &["actions-runner", "runner"]) {
                add_path(path);
            }
        }
    }

    let mut candidates = Vec::new();
    for path in paths {
        if !looks_like_runner_install(&path) {
            continue;
        }
        if config
            .runners
            .iter()
            .any(|runner| PathBuf::from(&runner.install.install_path) == path)
        {
            continue;
        }
        let metadata = parse_runner_metadata(&path);
        let detected_service = detect_external_service(&path);
        let (service_present, service_id, service_path) = match detected_service {
            Some(service) => (true, service.id, service.path),
            None => (false, None, None),
        };
        let last_log_time = last_log_timestamp(&path);
        candidates.push(DiscoveryCandidate {
            candidate_id: crate::config::new_runner_id(),
            install_path: path.to_string_lossy().to_string(),
            runner_name: metadata.runner_name,
            labels: metadata.labels,
            scope: metadata.scope,
            work_dir: metadata.work_dir,
            service_present,
            service_id,
            service_path,
            last_log_time,
        });
    }

    Ok(candidates)
}

pub fn import_candidate(
    config_store: &ConfigStore,
    candidate: &DiscoveryCandidate,
    options: &ImportOptions,
) -> Result<RunnerProfile, Error> {
    if options.move_install && candidate.service_present && !options.replace_service {
        return Err(Error::Service(
            "external service detected; replace or remove external service before moving".into(),
        ));
    }
    let runner_id = crate::config::new_runner_id();
    let config = config_store.get();
    let runner_name = candidate
        .runner_name
        .clone()
        .unwrap_or_else(default_runner_name);
    let display_name = candidate
        .runner_name
        .clone()
        .unwrap_or_else(|| runner_name.clone());
    let work_dir = candidate
        .work_dir
        .clone()
        .unwrap_or_else(|| default_work_dir(&runner_id).to_string_lossy().to_string());
    let profile = RunnerProfile {
        runner_id: runner_id.clone(),
        display_name,
        scope: candidate.scope.clone(),
        runner_name,
        labels: if candidate.labels.is_empty() {
            default_runner_labels()
        } else {
            candidate.labels.clone()
        },
        work_dir,
        install: InstallConfig {
            mode: InstallMode::Adopted,
            install_path: candidate.install_path.clone(),
            adopted_from_path: None,
            migration_status: crate::config::MigrationStatus::None,
        },
        runner_version: None,
        pat_alias: config.pat_default_alias.clone(),
        service: RunnerServiceConfig {
            installed: candidate.service_present,
            run_on_boot: candidate.service_present,
            provider: if candidate.service_present {
                ServiceProvider::External
            } else {
                ServiceProvider::Unknown
            },
            external_id: candidate.service_id.clone(),
            external_path: candidate.service_path.clone(),
            external_restore: None,
        },
        created_at: now_iso8601(),
        last_seen_at: candidate.last_log_time.clone(),
    };

    config_store.update(|config| {
        config.runners.push(profile.clone());
        if config.selected_runner_id.is_none() {
            config.selected_runner_id = Some(runner_id.clone());
        }
    })?;

    let mut imported = profile;

    if options.replace_service && imported.service.provider == ServiceProvider::External {
        migrate_external_service(&mut imported, ServiceMigrationStrategy::ReplaceWithRunnerbuddy)?;
        config_store.update_runner(&imported.runner_id, |runner| {
            runner.service = imported.service.clone();
        })?;
    }

    if options.move_install {
        let moved = move_install(config_store, &imported.runner_id, None)?;
        imported = moved;
    }

    Ok(imported)
}

pub fn migrate_external_service(
    profile: &mut RunnerProfile,
    strategy: ServiceMigrationStrategy,
) -> Result<(), Error> {
    match strategy {
        ServiceMigrationStrategy::ReplaceWithRunnerbuddy => {
            if profile.service.provider != ServiceProvider::External {
                return Ok(());
            }
            let restore = ExternalServiceInfo {
                id: profile.service.external_id.clone(),
                path: profile.service.external_path.clone(),
            };
            service_mgmt::external_disable(profile)?;
            service_mgmt::install(profile)?;
            profile.service.installed = true;
            profile.service.run_on_boot = true;
            profile.service.provider = ServiceProvider::Runnerbuddy;
            profile.service.external_restore = Some(restore);
            profile.service.external_id = None;
            profile.service.external_path = None;
            Ok(())
        }
    }
}

pub fn remove_external_artifacts(profile: &mut RunnerProfile) -> Result<(), Error> {
    if profile.service.provider != ServiceProvider::External {
        return Ok(());
    }
    service_mgmt::external_remove_artifacts(profile)?;
    profile.service.installed = false;
    profile.service.run_on_boot = false;
    profile.service.provider = ServiceProvider::Unknown;
    profile.service.external_id = None;
    profile.service.external_path = None;
    Ok(())
}

pub fn move_install(
    config_store: &ConfigStore,
    runner_id: &str,
    destination: Option<String>,
) -> Result<RunnerProfile, Error> {
    let config = config_store.get();
    let profile = crate::config::find_runner(&config, runner_id)?;
    if profile.install.mode == InstallMode::Managed {
        return Err(Error::Runner("runner already managed".into()));
    }
    let src_path = PathBuf::from(&profile.install.install_path);
    let dest_path = match destination {
        Some(path) => PathBuf::from(path),
        None => default_install_path(runner_id)?,
    };
    if dest_path.exists() {
        return Err(Error::Runner("destination already exists".into()));
    }
    info!("Moving runner install {runner_id} -> {:?}", dest_path);
    copy_dir_recursive(&src_path, &dest_path)?;
    verify_copy(&src_path, &dest_path)?;

    let updated_profile = config_store.update_runner(runner_id, |runner| {
        runner.install.mode = InstallMode::Managed;
        runner.install.install_path = dest_path.to_string_lossy().to_string();
        runner.install.adopted_from_path = Some(src_path.to_string_lossy().to_string());
        runner.install.migration_status = crate::config::MigrationStatus::Moved;
    })?;

    if updated_profile.service.provider == ServiceProvider::Runnerbuddy {
        service_mgmt::install(&updated_profile)?;
    }

    Ok(updated_profile)
}

fn scan_prefixes(root: &Path, prefixes: &[&str]) -> Vec<PathBuf> {
    let mut results = Vec::new();
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if prefixes.iter().any(|prefix| name.starts_with(prefix)) {
                results.push(path);
            }
        }
    }
    results
}

pub fn looks_like_runner_install(path: &Path) -> bool {
    let has_scripts = if cfg!(target_os = "windows") {
        path.join("config.cmd").exists() && path.join("run.cmd").exists()
    } else {
        path.join("config.sh").exists() && path.join("run.sh").exists()
    };
    let has_markers = path.join(".runner").exists() || path.join("_diag").exists();
    has_scripts || has_markers
}

struct RunnerMetadata {
    runner_name: Option<String>,
    labels: Vec<String>,
    scope: Option<RunnerScope>,
    work_dir: Option<String>,
}

pub fn infer_scope_from_install(install_path: &Path) -> Option<RunnerScope> {
    let root = parse_runner_metadata(install_path).scope;
    if root.is_some() {
        return root;
    }

    for dir in find_runner_dirs_with_metadata(install_path) {
        let scope = parse_runner_metadata(&dir).scope;
        if scope.is_some() {
            return scope;
        }
    }

    for file in candidate_metadata_files(install_path) {
        let content = read_file_tail(&file, LOG_TAIL_BYTES)
            .unwrap_or(None)
            .unwrap_or_default();
        if let Some(scope) = scope_from_text(&content) {
            return Some(scope);
        }
    }

    let log_dir = install_path.join("_diag");
    if let Ok(Some(path)) = latest_log_file(&log_dir) {
        let content = read_file_tail(&path, LOG_TAIL_BYTES)
            .unwrap_or(None)
            .unwrap_or_default();
        if let Some(scope) = scope_from_text(&content) {
            return Some(scope);
        }
    }

    None
}

fn parse_runner_metadata(path: &Path) -> RunnerMetadata {
    let runner_file = path.join(".runner");
    if !runner_file.exists() {
        return RunnerMetadata {
            runner_name: None,
            labels: Vec::new(),
            scope: None,
            work_dir: None,
        };
    }
    let data = fs::read_to_string(&runner_file).unwrap_or_default();
    let value: serde_json::Value = serde_json::from_str(&data).unwrap_or_default();
    let runner_name = value
        .get("name")
        .and_then(|val| val.as_str())
        .or_else(|| value.get("agentName").and_then(|val| val.as_str()))
        .or_else(|| value.get("runnerName").and_then(|val| val.as_str()))
        .map(|val| val.to_string());
    let labels = value
        .get("labels")
        .and_then(|val| val.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    if let Some(label) = item.as_str() {
                        return Some(label.to_string());
                    }
                    item.get("name")
                        .and_then(|name| name.as_str())
                        .map(|name| name.to_string())
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let server_url = value
        .get("serverUrl")
        .and_then(|val| val.as_str())
        .or_else(|| value.get("gitHubUrl").and_then(|val| val.as_str()))
        .or_else(|| value.get("githubUrl").and_then(|val| val.as_str()))
        .or_else(|| value.get("url").and_then(|val| val.as_str()));
    let scope = server_url.and_then(scope_from_url);
    let work_dir = value
        .get("workFolder")
        .and_then(|val| val.as_str())
        .or_else(|| value.get("workDir").and_then(|val| val.as_str()))
        .or_else(|| value.get("workDirectory").and_then(|val| val.as_str()))
        .map(|val| resolve_work_dir(path, val));
    RunnerMetadata {
        runner_name,
        labels,
        scope,
        work_dir,
    }
}

fn resolve_work_dir(install_path: &Path, work_folder: &str) -> String {
    if Path::new(work_folder).is_absolute() {
        return work_folder.to_string();
    }
    install_path.join(work_folder).to_string_lossy().to_string()
}

fn scope_from_url(url: &str) -> Option<RunnerScope> {
    let trimmed = url.trim_end_matches('/');
    let path = if let Some(index) = trimmed.find("//") {
        let after_scheme = &trimmed[index + 2..];
        after_scheme.splitn(2, '/').nth(1).unwrap_or("")
    } else {
        trimmed
    };
    let segments: Vec<&str> = path.split('/').filter(|seg| !seg.is_empty()).collect();
    if segments.is_empty() {
        return None;
    }
    if segments.get(0) == Some(&"organizations") && segments.len() >= 2 {
        return Some(RunnerScope::Org {
            org: segments[1].to_string(),
        });
    }
    if segments.get(0) == Some(&"orgs") && segments.len() >= 2 {
        return Some(RunnerScope::Org {
            org: segments[1].to_string(),
        });
    }
    if segments.get(0) == Some(&"enterprises") && segments.len() >= 2 {
        return Some(RunnerScope::Enterprise {
            enterprise: segments[1].to_string(),
        });
    }
    if segments.get(0) == Some(&"repos") && segments.len() >= 3 {
        return Some(RunnerScope::Repo {
            owner: segments[1].to_string(),
            repo: segments[2].to_string(),
        });
    }
    if segments.len() >= 2 {
        return Some(RunnerScope::Repo {
            owner: segments[0].to_string(),
            repo: segments[1].to_string(),
        });
    }
    if segments.len() == 1 {
        return Some(RunnerScope::Org {
            org: segments[0].to_string(),
        });
    }
    None
}

fn find_runner_dirs_with_metadata(root: &Path) -> Vec<PathBuf> {
    if !root.is_dir() {
        return Vec::new();
    }
    let mut results = Vec::new();
    let mut queue = VecDeque::from([(root.to_path_buf(), 0usize)]);
    while let Some((dir, depth)) = queue.pop_front() {
        if depth > 4 {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                queue.push_back((path, depth + 1));
                continue;
            }
            if file_type.is_file() && entry.file_name() == ".runner" {
                if let Some(parent) = path.parent() {
                    results.push(parent.to_path_buf());
                }
            }
        }
    }
    results
}

fn candidate_metadata_files(install_path: &Path) -> Vec<PathBuf> {
    let names = [
        ".runner",
        ".credentials",
        ".credentials_rsaparams",
        ".env",
        ".envs",
    ];
    names
        .iter()
        .map(|name| install_path.join(name))
        .filter(|path| path.exists() && path.is_file())
        .collect()
}

fn scope_from_text(text: &str) -> Option<RunnerScope> {
    let Ok(regex) = Regex::new(r#"https?://[^\s"']+"#) else {
        return None;
    };
    for match_ in regex.find_iter(text) {
        let url = match_.as_str();
        if url.contains("githubusercontent.com") || url.contains("actions.githubusercontent.com") {
            continue;
        }
        if let Some(scope) = scope_from_url(url) {
            return Some(scope);
        }
    }
    None
}

fn last_log_timestamp(install_path: &Path) -> Option<String> {
    let log_dir = install_path.join("_diag");
    let latest = latest_log_file(&log_dir).ok().flatten()?;
    let metadata = fs::metadata(latest).ok()?;
    let modified = metadata.modified().ok()?;
    let timestamp = OffsetDateTime::from(modified);
    timestamp.format(&Rfc3339).ok()
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), Error> {
    if !src.exists() {
        return Err(Error::Runner("source path missing".into()));
    }
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let dest = dst.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir_recursive(&path, &dest)?;
        } else if file_type.is_file() {
            fs::copy(&path, &dest)?;
        }
    }
    Ok(())
}

fn verify_copy(src: &Path, dst: &Path) -> Result<(), Error> {
    let checks = if cfg!(target_os = "windows") {
        vec!["config.cmd", "run.cmd", ".runner"]
    } else {
        vec!["config.sh", "run.sh", ".runner"]
    };
    for file in checks {
        let src_file = src.join(file);
        if !src_file.exists() {
            continue;
        }
        let dst_file = dst.join(file);
        if !dst_file.exists() {
            return Err(Error::Runner(format!("missing {file} after copy")));
        }
        let src_size = fs::metadata(&src_file)?.len();
        let dst_size = fs::metadata(&dst_file)?.len();
        if src_size != dst_size {
            return Err(Error::Runner(format!("size mismatch for {file}")));
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn parse_launchd_label_for_run_script(plist_path: &Path, run_script: &str) -> Option<String> {
    let plist = plist::Value::from_file(plist_path).ok()?;
    let dict = plist.as_dictionary()?;
    let label = dict
        .get("Label")
        .and_then(|value| value.as_string())
        .map(|value| value.to_string())?;
    let program = dict
        .get("Program")
        .and_then(|value| value.as_string())
        .map(|value| value.to_string());
    let program_args = dict
        .get("ProgramArguments")
        .and_then(|value| value.as_array())
        .map(|array| {
            array
                .iter()
                .filter_map(|item| item.as_string().map(|val| val.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let matches = program.as_deref() == Some(run_script)
        || program_args.iter().any(|arg| arg == run_script);
    if matches {
        Some(label)
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn unit_references_run_script(contents: &str, run_script: &str) -> bool {
    contents
        .lines()
        .filter_map(|line| line.split_once('='))
        .any(|(key, value)| key.trim() == "ExecStart" && value.contains(run_script))
}

#[cfg(target_os = "macos")]
fn detect_external_service(install_path: &Path) -> Option<ExternalServiceInfo> {
    let run_script = install_path.join("run.sh").to_string_lossy().to_string();
    let user_dirs = match directories::UserDirs::new() {
        Some(dirs) => dirs,
        None => return None,
    };
    let launch_agents = user_dirs
        .home_dir()
        .join("Library")
        .join("LaunchAgents");
    if let Ok(entries) = fs::read_dir(launch_agents) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("plist") {
                continue;
            }
            if let Some(label) = parse_launchd_label_for_run_script(&path, &run_script) {
                return Some(ExternalServiceInfo {
                    id: Some(label),
                    path: Some(path.to_string_lossy().to_string()),
                });
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn detect_external_service(install_path: &Path) -> Option<ExternalServiceInfo> {
    let run_script = install_path.join("run.sh").to_string_lossy().to_string();
    let user_dirs = match directories::UserDirs::new() {
        Some(dirs) => dirs,
        None => return None,
    };
    let systemd_dir = user_dirs
        .home_dir()
        .join(".config")
        .join("systemd")
        .join("user");
    if let Ok(entries) = fs::read_dir(systemd_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("service") {
                continue;
            }
            let content = fs::read_to_string(&path).unwrap_or_default();
            if unit_references_run_script(&content, &run_script) {
                let unit = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.to_string());
                return Some(ExternalServiceInfo {
                    id: unit,
                    path: Some(path.to_string_lossy().to_string()),
                });
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn detect_external_service(install_path: &Path) -> Option<ExternalServiceInfo> {
    let output = Command::new("cmd")
        .arg("/C")
        .arg("svc.cmd")
        .arg("status")
        .current_dir(install_path)
        .output();
    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let id = parse_windows_service_name(&stdout).unwrap_or_else(|| "svc.cmd".to_string());
            Some(ExternalServiceInfo {
            id: Some(id),
            path: Some(install_path.to_string_lossy().to_string()),
            })
        }
        _ => None,
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn detect_external_service(_install_path: &Path) -> Option<ExternalServiceInfo> {
    None
}

#[cfg(any(target_os = "windows", test))]
fn parse_windows_service_name(output: &str) -> Option<String> {
    for line in output.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        for prefix in ["service_name:", "service name:", "service:", "name:"] {
            if !lower.starts_with(prefix) {
                continue;
            }
            let value = trimmed
                .splitn(2, ':')
                .nth(1)
                .unwrap_or_default()
                .trim()
                .trim_matches('"');
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn scope_from_repo_url() {
        let scope = scope_from_url("https://github.com/org/repo").expect("scope");
        match scope {
            RunnerScope::Repo { owner, repo } => {
                assert_eq!(owner, "org");
                assert_eq!(repo, "repo");
            }
            _ => panic!("unexpected scope"),
        }
    }

    #[test]
    fn scope_from_api_repo_url() {
        let scope = scope_from_url("https://api.github.com/repos/org/repo").expect("scope");
        match scope {
            RunnerScope::Repo { owner, repo } => {
                assert_eq!(owner, "org");
                assert_eq!(repo, "repo");
            }
            _ => panic!("unexpected scope"),
        }
    }

    #[test]
    fn scope_from_org_settings_url() {
        let scope =
            scope_from_url("https://github.com/organizations/acme/settings/actions/runners")
                .expect("scope");
        match scope {
            RunnerScope::Org { org } => {
                assert_eq!(org, "acme");
            }
            _ => panic!("unexpected scope"),
        }
    }

    #[test]
    fn parses_runner_metadata_file() {
        let dir = tempdir().expect("tempdir");
        let runner_path = dir.path().join(".runner");
        let data = r#"{
  "agentName": "runner-1",
  "labels": ["self-hosted", "macOS"],
  "serverUrl": "https://github.com/org/repo",
  "workFolder": "_work"
}"#;
        fs::write(&runner_path, data).expect("write runner file");
        let metadata = parse_runner_metadata(dir.path());
        assert_eq!(metadata.runner_name.as_deref(), Some("runner-1"));
        assert!(metadata.labels.contains(&"self-hosted".to_string()));
        match metadata.scope {
            Some(RunnerScope::Repo { owner, repo }) => {
                assert_eq!(owner, "org");
                assert_eq!(repo, "repo");
            }
            _ => panic!("unexpected scope"),
        }
        assert!(metadata.work_dir.unwrap_or_default().ends_with("_work"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parses_systemd_exec_start() {
        let run_script = "/opt/actions-runner/run.sh";
        let unit = format!(
            "[Unit]\nDescription=Runner\n[Service]\nExecStart={run_script}\n",
            run_script = run_script
        );
        assert!(unit_references_run_script(&unit, run_script));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn parses_launchd_label() {
        let dir = tempdir().expect("tempdir");
        let plist_path = dir.path().join("com.example.runner.plist");
        let run_script = "/Users/test/actions-runner/run.sh";
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.example.runner</string>
  <key>ProgramArguments</key>
  <array>
    <string>{run_script}</string>
  </array>
</dict>
</plist>
"#,
            run_script = run_script
        );
        fs::write(&plist_path, plist).expect("write plist");
        let label = parse_launchd_label_for_run_script(&plist_path, run_script);
        assert_eq!(label.as_deref(), Some("com.example.runner"));
    }

    #[test]
    fn parses_windows_service_name() {
        let output = "SERVICE_NAME: actions.runner.acme.repo.runner1\nSTATE              : 4  RUNNING\n";
        assert_eq!(
            parse_windows_service_name(output).as_deref(),
            Some("actions.runner.acme.repo.runner1")
        );
        let output = "Service Name: \"actions.runner.org.repo.runner2\"\n";
        assert_eq!(
            parse_windows_service_name(output).as_deref(),
            Some("actions.runner.org.repo.runner2")
        );
    }
}
