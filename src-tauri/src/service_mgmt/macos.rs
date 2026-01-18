use crate::config::{RunnerProfile, ServiceProvider};
use crate::errors::Error;
use crate::service_mgmt::ServiceStatus;
use crate::util::expand_path;
use plist::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::warn;

pub fn install(profile: &RunnerProfile) -> Result<(), Error> {
    let plist_path = plist_path(&profile.runner_id)?;
    let log_dir = crate::config::runner_logs_dir(&profile.runner_id)?;
    fs::create_dir_all(&log_dir)?;
    let plist = launchd_plist_content(profile, &log_dir);
    fs::write(&plist_path, plist)?;
    let _ = bootout(&profile.runner_id);
    bootstrap(&plist_path)?;
    Ok(())
}

pub fn uninstall(profile: &RunnerProfile) -> Result<(), Error> {
    let plist_path = plist_path(&profile.runner_id)?;
    let _ = bootout(&profile.runner_id);
    if plist_path.exists() {
        fs::remove_file(plist_path)?;
    }
    Ok(())
}

pub fn enable_on_boot(profile: &RunnerProfile, enabled: bool) -> Result<(), Error> {
    let label = label_for(&profile.runner_id);
    let scope = launchctl_scope(&label)?;
    launchctl_status(
        &[if enabled { "enable" } else { "disable" }, &scope],
        "enable/disable",
    )
}

pub fn start(profile: &RunnerProfile) -> Result<(), Error> {
    let label = label_for(&profile.runner_id);
    let scope = launchctl_scope(&label)?;
    launchctl_status(&["kickstart", "-k", &scope], "kickstart")
}

pub fn stop(profile: &RunnerProfile) -> Result<(), Error> {
    let label = label_for(&profile.runner_id);
    let scope = launchctl_scope(&label)?;
    launchctl_status(&["stop", &scope], "stop")
}

pub fn status(profile: &RunnerProfile) -> Result<ServiceStatus, Error> {
    if profile.service.provider == ServiceProvider::External {
        return external_status(profile);
    }
    let label = label_for(&profile.runner_id);
    let scope = launchctl_scope(&label)?;
    let output = Command::new("launchctl").arg("print").arg(&scope).output();
    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let running = stdout.contains("state = running");
            Ok(ServiceStatus {
                installed: true,
                running,
                enabled: true,
            })
        }
        _ => Ok(ServiceStatus {
            installed: false,
            running: false,
            enabled: false,
        }),
    }
}

pub fn external_status(profile: &RunnerProfile) -> Result<ServiceStatus, Error> {
    let plist_path = external_plist_path(profile);
    let installed = plist_path.as_ref().map(|path| path.exists()).unwrap_or(false);
    let label = match external_label(profile) {
        Ok(label) => label,
        Err(err) => {
            warn!(
                "missing external launchd label for runner {}: {}",
                profile.runner_id, err
            );
            return Ok(ServiceStatus {
                installed,
                running: false,
                enabled: installed,
            });
        }
    };
    let scope = launchctl_scope(&label)?;
    let output = Command::new("launchctl").arg("print").arg(&scope).output();
    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let running = stdout.contains("state = running") || stdout.contains("pid =");
            let enabled = plist_path.as_ref().map(|path| path.exists()).unwrap_or(false)
                && !stdout.contains("disabled = true");
            Ok(ServiceStatus {
                installed: true,
                running,
                enabled,
            })
        }
        _ => Ok(ServiceStatus {
            installed,
            running: false,
            enabled: installed,
        }),
    }
}

pub fn external_disable(profile: &RunnerProfile) -> Result<(), Error> {
    let label = external_label(profile)?;
    let scope = launchctl_scope(&label)?;
    launchctl_status(&["bootout", &scope], "bootout")
}

pub fn external_remove_artifacts(profile: &RunnerProfile) -> Result<(), Error> {
    let _ = external_disable(profile);
    if let Some(path) = external_plist_path(profile) {
        if path.exists() {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

pub fn launchd_plist_content(profile: &RunnerProfile, log_dir: &Path) -> String {
    let install_path = expand_path(&profile.install.install_path);
    let run_script = install_path.join("run.sh");
    let stdout = log_dir.join("runner-stdout.log");
    let stderr = log_dir.join("runner-stderr.log");
    let label = label_for(&profile.runner_id);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{script}</string>
  </array>
  <key>WorkingDirectory</key>
  <string>{workdir}</string>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{stdout}</string>
  <key>StandardErrorPath</key>
  <string>{stderr}</string>
</dict>
</plist>
"#,
        label = label,
        script = run_script.to_string_lossy(),
        workdir = install_path.to_string_lossy(),
        stdout = stdout.to_string_lossy(),
        stderr = stderr.to_string_lossy(),
    )
}

fn launchctl_scope(label: &str) -> Result<String, Error> {
    let uid = user_uid()?;
    Ok(format!("gui/{uid}/{label}"))
}

fn launchctl_user_scope() -> Result<String, Error> {
    let uid = user_uid()?;
    Ok(format!("gui/{uid}"))
}

fn launchctl_status(args: &[&str], context: &str) -> Result<(), Error> {
    let status = Command::new("launchctl").args(args).status()?;
    if !status.success() {
        return Err(Error::Service(format!("launchctl {context} failed")));
    }
    Ok(())
}

fn label_for(runner_id: &str) -> String {
    format!("com.runnerbuddy.runner.{runner_id}")
}

fn external_plist_path(profile: &RunnerProfile) -> Option<PathBuf> {
    profile
        .service
        .external_path
        .as_ref()
        .map(|path| PathBuf::from(path))
}

fn external_label(profile: &RunnerProfile) -> Result<String, Error> {
    if let Some(label) = profile.service.external_id.clone() {
        return Ok(label);
    }
    if let Some(path) = external_plist_path(profile) {
        if let Some(label) = parse_launchd_label(&path) {
            return Ok(label);
        }
    }
    Err(Error::Service("missing external launchd label".into()))
}

fn parse_launchd_label(path: &Path) -> Option<String> {
    let plist = Value::from_file(path).ok()?;
    let dict = plist.as_dictionary()?;
    dict.get("Label")
        .and_then(|value| value.as_string())
        .map(|value| value.to_string())
}

fn plist_path(runner_id: &str) -> Result<PathBuf, Error> {
    let user_dirs = directories::UserDirs::new()
        .ok_or_else(|| Error::Service("unable to resolve user home".into()))?;
    Ok(user_dirs
        .home_dir()
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{}.plist", label_for(runner_id))))
}

fn user_uid() -> Result<String, Error> {
    let output = Command::new("id").arg("-u").output()?;
    if !output.status.success() {
        return Err(Error::Service("failed to read uid".into()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn bootstrap(plist_path: &Path) -> Result<(), Error> {
    let scope = launchctl_user_scope()?;
    let plist = plist_path.to_string_lossy();
    launchctl_status(&["bootstrap", &scope, plist.as_ref()], "bootstrap")
}

fn bootout(runner_id: &str) -> Result<(), Error> {
    let label = label_for(runner_id);
    let scope = launchctl_scope(&label)?;
    launchctl_status(&["bootout", &scope], "bootout")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{InstallConfig, InstallMode, RunnerProfile, RunnerServiceConfig};

    #[test]
    fn plist_contains_label() {
        let profile = RunnerProfile {
            runner_id: "abc".to_string(),
            display_name: "Test".to_string(),
            scope: None,
            runner_name: "runner".to_string(),
            labels: vec!["self-hosted".to_string()],
            work_dir: "/tmp".to_string(),
            install: InstallConfig {
                mode: InstallMode::Managed,
                install_path: "/tmp/runner".to_string(),
                adopted_from_path: None,
                migration_status: crate::config::MigrationStatus::None,
            },
            runner_version: None,
            pat_alias: "default".to_string(),
            service: RunnerServiceConfig::default(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            last_seen_at: None,
        };
        let log_dir = PathBuf::from("/tmp");
        let plist = launchd_plist_content(&profile, &log_dir);
        assert!(plist.contains("com.runnerbuddy.runner.abc"));
    }
}
