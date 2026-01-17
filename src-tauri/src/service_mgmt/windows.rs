use crate::config::{RunnerProfile, ServiceProvider};
use crate::errors::Error;
use crate::service_mgmt::ServiceStatus;
use crate::util::expand_path;
use std::process::Command;

pub fn install(profile: &RunnerProfile) -> Result<(), Error> {
    let install_path = expand_path(&profile.install.install_path);
    let status = Command::new("cmd")
        .arg("/C")
        .arg("svc.cmd")
        .arg("install")
        .current_dir(install_path)
        .status()?;
    if !status.success() {
        return Err(Error::Service("svc.cmd install failed".into()));
    }
    Ok(())
}

pub fn uninstall(profile: &RunnerProfile) -> Result<(), Error> {
    let install_path = expand_path(&profile.install.install_path);
    let status = Command::new("cmd")
        .arg("/C")
        .arg("svc.cmd")
        .arg("uninstall")
        .current_dir(install_path)
        .status()?;
    if !status.success() {
        return Err(Error::Service("svc.cmd uninstall failed".into()));
    }
    Ok(())
}

pub fn enable_on_boot(_profile: &RunnerProfile, _enabled: bool) -> Result<(), Error> {
    Ok(())
}

pub fn start(profile: &RunnerProfile) -> Result<(), Error> {
    let install_path = expand_path(&profile.install.install_path);
    let status = Command::new("cmd")
        .arg("/C")
        .arg("svc.cmd")
        .arg("start")
        .current_dir(install_path)
        .status()?;
    if !status.success() {
        return Err(Error::Service("svc.cmd start failed".into()));
    }
    Ok(())
}

pub fn stop(profile: &RunnerProfile) -> Result<(), Error> {
    let install_path = expand_path(&profile.install.install_path);
    let status = Command::new("cmd")
        .arg("/C")
        .arg("svc.cmd")
        .arg("stop")
        .current_dir(install_path)
        .status()?;
    if !status.success() {
        return Err(Error::Service("svc.cmd stop failed".into()));
    }
    Ok(())
}

pub fn status(profile: &RunnerProfile) -> Result<ServiceStatus, Error> {
    if profile.service.provider == ServiceProvider::External {
        return external_status(profile);
    }
    let install_path = expand_path(&profile.install.install_path);
    let output = Command::new("cmd")
        .arg("/C")
        .arg("svc.cmd")
        .arg("status")
        .current_dir(install_path)
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
    let running = stdout.contains("running");
    let installed = output.status.success();
    Ok(ServiceStatus {
        installed,
        running,
        enabled: installed,
    })
}

pub fn external_status(profile: &RunnerProfile) -> Result<ServiceStatus, Error> {
    let install_path = expand_path(&profile.install.install_path);
    let output = Command::new("cmd")
        .arg("/C")
        .arg("svc.cmd")
        .arg("status")
        .current_dir(install_path)
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
    let running = stdout.contains("running");
    let installed = output.status.success();
    Ok(ServiceStatus {
        installed,
        running,
        enabled: installed,
    })
}

pub fn external_disable(profile: &RunnerProfile) -> Result<(), Error> {
    let install_path = expand_path(&profile.install.install_path);
    let _ = Command::new("cmd")
        .arg("/C")
        .arg("svc.cmd")
        .arg("stop")
        .current_dir(&install_path)
        .status();
    Ok(())
}

pub fn external_remove_artifacts(profile: &RunnerProfile) -> Result<(), Error> {
    let install_path = expand_path(&profile.install.install_path);
    let _ = Command::new("cmd")
        .arg("/C")
        .arg("svc.cmd")
        .arg("stop")
        .current_dir(&install_path)
        .status();
    let status = Command::new("cmd")
        .arg("/C")
        .arg("svc.cmd")
        .arg("uninstall")
        .current_dir(&install_path)
        .status()?;
    if !status.success() {
        return Err(Error::Service("svc.cmd uninstall failed".into()));
    }
    Ok(())
}
