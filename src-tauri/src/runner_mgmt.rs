use crate::config::{ConfigStore, InstallMode, RunnerProfile, RunnerScope};
use crate::errors::Error;
use crate::github_api;
use crate::logging::scrub_sensitive;
use crate::secrets;
use crate::discovery;
use crate::util::expand_path;
use futures_util::StreamExt;
use sha2::Digest;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use tauri::{AppHandle, Emitter, Runtime};
use tracing::{info, warn};

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct ReleaseInfo {
    tag_name: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug)]
struct RunnerPlatform {
    os: &'static str,
    arch: &'static str,
    ext: &'static str,
}

impl RunnerPlatform {
    fn asset_prefix(&self) -> String {
        format!("actions-runner-{}-{}", self.os, self.arch)
    }
}

pub async fn download_runner<R: Runtime>(
    app: &AppHandle<R>,
    config_store: &ConfigStore,
    runner_id: &str,
    version: Option<String>,
) -> Result<RunnerProfile, Error> {
    let profile = get_runner_profile(config_store, runner_id)?;
    if matches!(profile.install.mode, InstallMode::Adopted) {
        return Err(Error::Runner(
            "cannot download runner for adopted install".into(),
        ));
    }
    let platform = detect_platform()?;
    let release = fetch_release(version).await?;
    let version = normalize_version(&release.tag_name);
    let asset_name = format!(
        "{}-{}.{}",
        platform.asset_prefix(),
        version,
        platform.ext
    );
    let (asset_url, sha_url) = find_asset_urls(&release.assets, &asset_name)?;
    let download_dir = crate::config::data_dir()?.join("downloads");
    fs::create_dir_all(&download_dir)?;
    let archive_path = download_dir.join(&asset_name);
    info!("Downloading runner {version} for {runner_id}");
    download_with_progress(app, runner_id, &asset_url, &archive_path).await?;
    if let Some(sha_url) = sha_url {
        verify_sha256(&sha_url, &archive_path).await?;
    }
    let install_path = expand_path(&profile.install.install_path);
    if install_path.exists() {
        warn!(
            "install path {:?} exists; clearing before extraction",
            install_path
        );
        fs::remove_dir_all(&install_path)?;
    }
    fs::create_dir_all(&install_path)?;
    extract_archive(&archive_path, &install_path)?;
    let updated = config_store.update(|config| {
        if let Some(runner) = config
            .runners
            .iter_mut()
            .find(|runner| runner.runner_id == runner_id)
        {
            runner.runner_version = Some(version.to_string());
            runner.install.install_path = install_path.to_string_lossy().to_string();
        }
    })?;
    Ok(find_runner_in_config(&updated, runner_id)?)
}

pub async fn configure_runner(
    config_store: &ConfigStore,
    runner_id: &str,
    scope: RunnerScope,
    name: String,
    labels: Vec<String>,
    work_dir: String,
) -> Result<RunnerProfile, Error> {
    let profile = get_runner_profile(config_store, runner_id)?;
    let pat = secrets::load_pat(&profile.pat_alias)?.ok_or_else(|| {
        Error::Runner("no PAT found in credential store; save a token first".into())
    })?;
    let token = github_api::get_registration_token(&scope, &pat).await?;
    let install_path = expand_path(&profile.install.install_path);
    let config_script = runner_config_script(&install_path)?;
    let work_dir_path = expand_path(&work_dir);
    fs::create_dir_all(&work_dir_path)?;
    let labels_csv = labels.join(",");
    let url = scope.url();
    info!("Configuring runner {runner_id} for {url}");
    let mut command = Command::new(config_script);
    command
        .current_dir(&install_path)
        .arg("--unattended")
        .arg("--replace")
        .arg("--url")
        .arg(&url)
        .arg("--token")
        .arg(&token.token)
        .arg("--name")
        .arg(&name)
        .arg("--labels")
        .arg(&labels_csv)
        .arg("--work")
        .arg(&work_dir_path);
    let status = command.status()?;
    if !status.success() {
        return Err(Error::Runner(format!(
            "runner config failed with status {}",
            status
        )));
    }
    let updated = config_store.update(|config| {
        if let Some(runner) = config
            .runners
            .iter_mut()
            .find(|runner| runner.runner_id == runner_id)
        {
            runner.runner_name = name;
            runner.labels = labels;
            runner.work_dir = work_dir;
            runner.scope = Some(scope);
        }
    })?;
    Ok(find_runner_in_config(&updated, runner_id)?)
}

pub fn repair_runner_scope(
    config_store: &ConfigStore,
    runner_id: &str,
) -> Result<RunnerProfile, Error> {
    let profile = get_runner_profile(config_store, runner_id)?;
    if profile.scope.is_some() {
        return Ok(profile);
    }
    let install_path = expand_path(&profile.install.install_path);
    if !install_path.exists() {
        return Err(Error::Runner(format!(
            "runner install path does not exist: {}",
            install_path.to_string_lossy()
        )));
    }
    let scope = discovery::infer_scope_from_install(&install_path).ok_or_else(|| {
        Error::Runner(
            "unable to infer scope from local runner install; ensure the runner is configured and `.runner` exists".into(),
        )
    })?;
    info!(
        "Repaired missing scope for runner {runner_id}: {}",
        scope.url()
    );
    let updated = config_store.update(|config| {
        if let Some(runner) = config
            .runners
            .iter_mut()
            .find(|runner| runner.runner_id == runner_id)
        {
            runner.scope = Some(scope.clone());
        }
    })?;
    Ok(find_runner_in_config(&updated, runner_id)?)
}

pub fn start_runner(
    config_store: &ConfigStore,
    runner_id: &str,
    child_map: &std::sync::Mutex<HashMap<String, Child>>,
) -> Result<u32, Error> {
    let profile = get_runner_profile(config_store, runner_id)?;
    let install_path = expand_path(&profile.install.install_path);
    let run_script = runner_run_script(&install_path)?;
    let log_dir = crate::config::data_dir()?.join("logs").join(runner_id);
    fs::create_dir_all(&log_dir)?;
    let stdout_path = log_dir.join("runner-stdout.log");
    let stderr_path = log_dir.join("runner-stderr.log");
    let stdout = File::create(stdout_path)?;
    let stderr = File::create(stderr_path)?;
    let mut command = Command::new(run_script);
    command
        .current_dir(&install_path)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    let child = command.spawn()?;
    let pid = child.id();
    let mut guard = child_map.lock().expect("runner child mutex poisoned");
    guard.insert(runner_id.to_string(), child);
    Ok(pid)
}

pub fn stop_runner(
    runner_id: &str,
    child_map: &std::sync::Mutex<HashMap<String, Child>>,
) -> Result<(), Error> {
    let mut guard = child_map.lock().expect("runner child mutex poisoned");
    if let Some(child) = guard.get_mut(runner_id) {
        let _ = child.kill();
        let _ = child.wait();
    }
    guard.remove(runner_id);
    Ok(())
}

pub fn runner_log_dir(profile: &RunnerProfile) -> PathBuf {
    expand_path(&profile.install.install_path).join("_diag")
}

pub fn latest_log_file(log_dir: &Path) -> io::Result<Option<PathBuf>> {
    if !log_dir.exists() {
        return Ok(None);
    }
    let mut entries: Vec<_> = fs::read_dir(log_dir)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|entry| entry.path())
        .collect();
    entries.sort_by_key(|path| path.metadata().and_then(|m| m.modified()).ok());
    Ok(entries.last().cloned())
}

fn detect_platform() -> Result<RunnerPlatform, Error> {
    let os = match std::env::consts::OS {
        "macos" => "osx",
        "linux" => "linux",
        "windows" => "win",
        other => {
            return Err(Error::Unsupported(format!(
                "unsupported OS: {other}"
            )))
        }
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => {
            return Err(Error::Unsupported(format!(
                "unsupported architecture: {other}"
            )))
        }
    };
    let ext = if os == "win" { "zip" } else { "tar.gz" };
    Ok(RunnerPlatform { os, arch, ext })
}

async fn fetch_release(version: Option<String>) -> Result<ReleaseInfo, Error> {
    let client = reqwest::Client::builder()
        .user_agent("RunnerBuddy")
        .build()?;
    let url = if let Some(version) = version {
        let version = normalize_version(&version);
        format!(
            "https://api.github.com/repos/actions/runner/releases/tags/v{version}"
        )
    } else {
        "https://api.github.com/repos/actions/runner/releases/latest".to_string()
    };
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(Error::Runner(format!(
            "release lookup failed: {}",
            resp.status()
        )));
    }
    Ok(resp.json::<ReleaseInfo>().await?)
}

fn normalize_version(tag: &str) -> String {
    tag.trim_start_matches('v').to_string()
}

fn find_asset_urls(
    assets: &[ReleaseAsset],
    name: &str,
) -> Result<(String, Option<String>), Error> {
    let asset = assets
        .iter()
        .find(|asset| asset.name == name)
        .ok_or_else(|| Error::Runner(format!("runner asset not found: {name}")))?;
    let sha_name = format!("{name}.sha256");
    let sha_url = assets
        .iter()
        .find(|asset| asset.name == sha_name)
        .map(|asset| asset.browser_download_url.clone());
    Ok((asset.browser_download_url.clone(), sha_url))
}

async fn download_with_progress<R: Runtime>(
    app: &AppHandle<R>,
    runner_id: &str,
    url: &str,
    dest: &Path,
) -> Result<(), Error> {
    let client = reqwest::Client::builder()
        .user_agent("RunnerBuddy")
        .build()?;
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(Error::Runner(format!(
            "runner download failed: {}",
            resp.status()
        )));
    }
    let total = resp.content_length().unwrap_or(0);
    let mut stream = resp.bytes_stream();
    let mut file = File::create(dest)?;
    let mut downloaded: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let data = chunk?;
        file.write_all(&data)?;
        downloaded += data.len() as u64;
        if total > 0 {
            let percent = ((downloaded as f64 / total as f64) * 100.0) as u64;
            let _ = app.emit(
                "progress",
                ProgressPayload {
                    runner_id: runner_id.to_string(),
                    phase: "download".to_string(),
                    percent,
                },
            );
        }
    }
    Ok(())
}

async fn verify_sha256(url: &str, archive_path: &Path) -> Result<(), Error> {
    let client = reqwest::Client::builder()
        .user_agent("RunnerBuddy")
        .build()?;
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(Error::Runner(format!(
            "sha256 download failed: {}",
            resp.status()
        )));
    }
    let body = resp.text().await?;
    let expected = body.split_whitespace().next().unwrap_or("").to_string();
    let data = fs::read(archive_path)?;
    let actual = hex::encode(sha2::Sha256::digest(data));
    if expected != actual {
        return Err(Error::Runner("sha256 mismatch for runner download".into()));
    }
    Ok(())
}

fn extract_archive(archive_path: &Path, dest: &Path) -> Result<(), Error> {
    let name = archive_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();
    if name.ends_with(".tar.gz") {
        let file = File::open(archive_path)?;
        let decompressor = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(decompressor);
        archive.unpack(dest)?;
        return Ok(());
    }
    if name.ends_with(".zip") {
        let file = File::open(archive_path)?;
        let mut zip = zip::ZipArchive::new(file)
            .map_err(|err| Error::Runner(format!("zip open failed: {err}")))?;
        for i in 0..zip.len() {
            let mut file = zip
                .by_index(i)
                .map_err(|err| Error::Runner(format!("zip entry failed: {err}")))?;
            let outpath = dest.join(file.mangled_name());
            if file.name().ends_with('/') {
                fs::create_dir_all(&outpath)?;
            } else {
                if let Some(parent) = outpath.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut outfile = File::create(&outpath)?;
                io::copy(&mut file, &mut outfile)?;
            }
        }
        return Ok(());
    }
    Err(Error::Runner(format!(
        "unsupported archive format: {name}"
    )))
}

fn runner_config_script(install_path: &Path) -> Result<PathBuf, Error> {
    let script = if cfg!(target_os = "windows") {
        install_path.join("config.cmd")
    } else {
        install_path.join("config.sh")
    };
    if !script.exists() {
        return Err(Error::Runner(format!(
            "runner config script not found at {:?}",
            script
        )));
    }
    Ok(script)
}

fn runner_run_script(install_path: &Path) -> Result<PathBuf, Error> {
    let script = if cfg!(target_os = "windows") {
        install_path.join("run.cmd")
    } else {
        install_path.join("run.sh")
    };
    if !script.exists() {
        return Err(Error::Runner(format!(
            "runner run script not found at {:?}",
            script
        )));
    }
    Ok(script)
}

#[derive(serde::Serialize, Clone)]
pub struct ProgressPayload {
    pub runner_id: String,
    pub phase: String,
    pub percent: u64,
}

pub fn classify_runner_status(log_dir: &Path) -> Result<crate::state::RunnerStatus, Error> {
    let latest = latest_log_file(log_dir).ok().flatten();
    if latest.is_none() {
        return Ok(crate::state::RunnerStatus::Idle);
    }
    let path = latest.unwrap();
    let content = fs::read_to_string(&path)?;
    let mut last_start = None;
    let mut last_end = None;
    for line in content.lines().rev().take(2000) {
        let line = scrub_sensitive(line);
        if line.contains("Running job:") || line.contains("Job started") {
            last_start = Some(line);
            break;
        }
    }
    for line in content.lines().rev().take(2000) {
        let line = scrub_sensitive(line);
        if line.contains("Job completed") || line.contains("Job finished") {
            last_end = Some(line);
            break;
        }
    }
    match (last_start, last_end) {
        (Some(_), None) => Ok(crate::state::RunnerStatus::Running),
        (Some(_), Some(_)) => Ok(crate::state::RunnerStatus::Idle),
        _ => Ok(crate::state::RunnerStatus::Idle),
    }
}

pub fn has_ready_marker(log_dir: &Path) -> Result<bool, Error> {
    let latest = latest_log_file(log_dir).ok().flatten();
    let Some(path) = latest else {
        return Ok(false);
    };
    let content = fs::read_to_string(&path)?;
    for line in content.lines().rev().take(2000) {
        let line = scrub_sensitive(line);
        if line.contains("Listening for Jobs")
            || line.contains("Listening for jobs")
            || line.contains("Runner listener started")
            || line.contains("Runner started")
            || line.contains("Connected to GitHub")
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn get_runner_profile(config_store: &ConfigStore, runner_id: &str) -> Result<RunnerProfile, Error> {
    let config = config_store.get();
    find_runner_in_config(&config, runner_id)
}

fn find_runner_in_config(config: &crate::config::Config, runner_id: &str) -> Result<RunnerProfile, Error> {
    config
        .runners
        .iter()
        .find(|runner| runner.runner_id == runner_id)
        .cloned()
        .ok_or_else(|| Error::Runner(format!("runner {runner_id} not found")))
}
