#[derive(Debug, serde::Serialize, Clone)]
pub struct ServiceStatus {
    pub installed: bool,
    pub running: bool,
    pub enabled: bool,
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
pub use macos::*;
#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(target_os = "windows")]
pub use windows::*;

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
mod platform {
    use super::ServiceStatus;
    use crate::config::RunnerProfile;
    use crate::errors::Error;

    pub fn install(_profile: &RunnerProfile) -> Result<(), Error> {
        Err(Error::Unsupported("service install unsupported on this OS".into()))
    }

    pub fn uninstall(_profile: &RunnerProfile) -> Result<(), Error> {
        Err(Error::Unsupported(
            "service uninstall unsupported on this OS".into(),
        ))
    }

    pub fn enable_on_boot(_profile: &RunnerProfile, _enabled: bool) -> Result<(), Error> {
        Err(Error::Unsupported(
            "service enable unsupported on this OS".into(),
        ))
    }

    pub fn start(_profile: &RunnerProfile) -> Result<(), Error> {
        Err(Error::Unsupported("service start unsupported on this OS".into()))
    }

    pub fn stop(_profile: &RunnerProfile) -> Result<(), Error> {
        Err(Error::Unsupported("service stop unsupported on this OS".into()))
    }

    pub fn status(_profile: &RunnerProfile) -> Result<ServiceStatus, Error> {
        Err(Error::Unsupported("service status unsupported on this OS".into()))
    }

    pub fn external_status(_profile: &RunnerProfile) -> Result<ServiceStatus, Error> {
        Err(Error::Unsupported(
            "external service status unsupported on this OS".into(),
        ))
    }

    pub fn external_disable(_profile: &RunnerProfile) -> Result<(), Error> {
        Err(Error::Unsupported(
            "external service disable unsupported on this OS".into(),
        ))
    }

    pub fn external_remove_artifacts(_profile: &RunnerProfile) -> Result<(), Error> {
        Err(Error::Unsupported(
            "external service removal unsupported on this OS".into(),
        ))
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub use platform::*;
