use crate::config::{RunnerProfile, ServiceProvider};
use crate::errors::Error;
use crate::service_mgmt::ServiceStatus;
use crate::util::expand_path;
use std::process::Command;

fn svc_command(profile: &RunnerProfile, action: &str) -> Command {
    let install_path = expand_path(&profile.install.install_path);
    let mut command = Command::new("cmd");
    command
        .arg("/C")
        .arg("svc.cmd")
        .arg(action)
        .current_dir(install_path);
    command
}

fn svc_run(profile: &RunnerProfile, action: &str) -> Result<(), Error> {
    let status = svc_command(profile, action).status()?;
    if !status.success() {
        return Err(Error::Service(format!("svc.cmd {action} failed")));
    }
    Ok(())
}

fn svc_output(profile: &RunnerProfile, action: &str) -> Result<std::process::Output, Error> {
    Ok(svc_command(profile, action).output()?)
}

fn parse_service_status(output: &std::process::Output) -> ServiceStatus {
    let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
    let running = stdout.contains("running");
    let installed = output.status.success();
    ServiceStatus {
        installed,
        running,
        enabled: installed,
    }
}

pub fn install(profile: &RunnerProfile) -> Result<(), Error> {
    svc_run(profile, "install")
}

pub fn uninstall(profile: &RunnerProfile) -> Result<(), Error> {
    svc_run(profile, "uninstall")
}

pub fn enable_on_boot(_profile: &RunnerProfile, _enabled: bool) -> Result<(), Error> {
    Ok(())
}

pub fn start(profile: &RunnerProfile) -> Result<(), Error> {
    svc_run(profile, "start")
}

pub fn stop(profile: &RunnerProfile) -> Result<(), Error> {
    svc_run(profile, "stop")
}

pub fn status(profile: &RunnerProfile) -> Result<ServiceStatus, Error> {
    if profile.service.provider == ServiceProvider::External {
        return external_status(profile);
    }
    let output = svc_output(profile, "status")?;
    Ok(parse_service_status(&output))
}

pub fn external_status(profile: &RunnerProfile) -> Result<ServiceStatus, Error> {
    let output = svc_output(profile, "status")?;
    Ok(parse_service_status(&output))
}

pub fn external_disable(profile: &RunnerProfile) -> Result<(), Error> {
    let _ = svc_run(profile, "stop");
    Ok(())
}

pub fn external_remove_artifacts(profile: &RunnerProfile) -> Result<(), Error> {
    let _ = svc_run(profile, "stop");
    svc_run(profile, "uninstall")
}
