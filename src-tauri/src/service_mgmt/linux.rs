use crate::config::{RunnerProfile, ServiceProvider};
use crate::errors::Error;
use crate::service_mgmt::ServiceStatus;
use crate::util::expand_path;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

pub fn install(profile: &RunnerProfile) -> Result<(), Error> {
    let unit_path = unit_path(&profile.runner_id)?;
    if let Some(parent) = unit_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let unit = systemd_unit_content(profile);
    fs::write(&unit_path, unit)?;
    systemctl(&["--user", "daemon-reload"])?;
    systemctl(&["--user", "enable", &unit_name(&profile.runner_id)])?;
    systemctl(&["--user", "start", &unit_name(&profile.runner_id)])?;
    Ok(())
}

pub fn uninstall(profile: &RunnerProfile) -> Result<(), Error> {
    let unit_path = unit_path(&profile.runner_id)?;
    let unit_name = unit_name(&profile.runner_id);
    let _ = systemctl(&["--user", "stop", &unit_name]);
    let _ = systemctl(&["--user", "disable", &unit_name]);
    if unit_path.exists() {
        fs::remove_file(unit_path)?;
    }
    systemctl(&["--user", "daemon-reload"])?;
    Ok(())
}

pub fn enable_on_boot(profile: &RunnerProfile, enabled: bool) -> Result<(), Error> {
    let unit_name = unit_name(&profile.runner_id);
    if enabled {
        systemctl(&["--user", "enable", &unit_name])?;
    } else {
        systemctl(&["--user", "disable", &unit_name])?;
    }
    Ok(())
}

pub fn start(profile: &RunnerProfile) -> Result<(), Error> {
    systemctl(&["--user", "start", &unit_name(&profile.runner_id)])?;
    Ok(())
}

pub fn stop(profile: &RunnerProfile) -> Result<(), Error> {
    systemctl(&["--user", "stop", &unit_name(&profile.runner_id)])?;
    Ok(())
}

pub fn status(profile: &RunnerProfile) -> Result<ServiceStatus, Error> {
    if profile.service.provider == ServiceProvider::External {
        return external_status(profile);
    }
    let unit = unit_name(&profile.runner_id);
    let installed = unit_path(&profile.runner_id)?.exists();
    let running = systemctl_status(&["--user", "is-active", &unit]).unwrap_or(false);
    let enabled = systemctl_status(&["--user", "is-enabled", &unit]).unwrap_or(false);
    Ok(ServiceStatus {
        installed,
        running,
        enabled,
    })
}

pub fn external_status(profile: &RunnerProfile) -> Result<ServiceStatus, Error> {
    let unit = external_unit_name(profile)?;
    let installed = external_unit_path(profile).map(|path| path.exists()).unwrap_or(false);
    let running = systemctl_status(&["--user", "is-active", &unit]).unwrap_or(false);
    let enabled = systemctl_status(&["--user", "is-enabled", &unit]).unwrap_or(false);
    Ok(ServiceStatus {
        installed,
        running,
        enabled,
    })
}

pub fn external_disable(profile: &RunnerProfile) -> Result<(), Error> {
    let unit = external_unit_name(profile)?;
    let _ = systemctl(&["--user", "stop", &unit]);
    systemctl(&["--user", "disable", &unit])?;
    Ok(())
}

pub fn external_remove_artifacts(profile: &RunnerProfile) -> Result<(), Error> {
    let _ = external_disable(profile);
    if let Some(path) = external_unit_path(profile) {
        if path.exists() {
            fs::remove_file(path)?;
        }
    }
    systemctl(&["--user", "daemon-reload"])?;
    Ok(())
}

pub fn systemd_unit_content(profile: &RunnerProfile) -> String {
    let install_path = expand_path(&profile.install.install_path);
    let run_script = install_path.join("run.sh");
    format!(
        r#"[Unit]
Description=RunnerBuddy GitHub Actions Runner ({runner_id})
After=network.target

[Service]
Type=simple
WorkingDirectory={install_path}
ExecStart={run_script}
Restart=always
RestartSec=5

[Install]
WantedBy=default.target
"#,
        runner_id = profile.runner_id,
        install_path = install_path.to_string_lossy(),
        run_script = run_script.to_string_lossy()
    )
}

fn unit_name(runner_id: &str) -> String {
    format!("runnerbuddy-{runner_id}.service")
}

fn unit_path(runner_id: &str) -> Result<PathBuf, Error> {
    let user_dirs = directories::UserDirs::new()
        .ok_or_else(|| Error::Service("unable to resolve user home".into()))?;
    Ok(user_dirs
        .home_dir()
        .join(".config")
        .join("systemd")
        .join("user")
        .join(unit_name(runner_id)))
}

fn external_unit_name(profile: &RunnerProfile) -> Result<String, Error> {
    if let Some(unit) = profile.service.external_id.clone() {
        return Ok(unit);
    }
    if let Some(path) = external_unit_path(profile) {
        if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
            return Ok(name.to_string());
        }
    }
    Err(Error::Service("missing external systemd unit".into()))
}

fn external_unit_path(profile: &RunnerProfile) -> Option<PathBuf> {
    profile
        .service
        .external_path
        .as_ref()
        .map(|path| PathBuf::from(path))
}

fn systemctl(args: &[&str]) -> Result<(), Error> {
    let status = Command::new("systemctl").args(args).status()?;
    if !status.success() {
        return Err(Error::Service("systemctl command failed".into()));
    }
    Ok(())
}

fn systemctl_status(args: &[&str]) -> Option<bool> {
    Command::new("systemctl")
        .args(args)
        .status()
        .ok()
        .map(|status| status.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{InstallConfig, InstallMode, RunnerProfile, RunnerServiceConfig};

    #[test]
    fn unit_contains_exec_start() {
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
        let unit = systemd_unit_content(&profile);
        assert!(unit.contains("ExecStart="));
        assert!(unit.contains("runnerbuddy-abc.service"));
    }
}
