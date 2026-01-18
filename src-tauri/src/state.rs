use crate::config::Config;
use crate::discovery::DiscoveryCandidate;
use crate::logging::{LogPaths, LogSetup};
use serde::Serialize;
use std::collections::HashMap;
use std::process::Child;
use std::sync::Mutex;

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RunnerStatus {
    Offline,
    Idle,
    Running,
}

#[derive(Debug, Serialize, Clone)]
pub struct RuntimeState {
    pub status: RunnerStatus,
    pub pid: Option<u32>,
    pub last_heartbeat: Option<u64>,
    pub last_error: Option<String>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            status: RunnerStatus::Offline,
            pid: None,
            last_heartbeat: None,
            last_error: None,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AppSnapshot {
    pub config: Config,
    pub runtime: HashMap<String, RuntimeState>,
}

pub struct AppState {
    pub config: crate::config::ConfigStore,
    pub runtime: Mutex<HashMap<String, RuntimeState>>,
    pub runner_children: Mutex<HashMap<String, Child>>,
    pub discovery_cache: Mutex<HashMap<String, DiscoveryCandidate>>,
    pub last_seen_updates: Mutex<HashMap<String, u64>>,
    pub log_paths: LogPaths,
    _log_guard: tracing_appender::non_blocking::WorkerGuard,
}

impl AppState {
    pub fn new(config: crate::config::ConfigStore, log_setup: LogSetup) -> Self {
        Self {
            config,
            runtime: Mutex::new(HashMap::new()),
            runner_children: Mutex::new(HashMap::new()),
            discovery_cache: Mutex::new(HashMap::new()),
            last_seen_updates: Mutex::new(HashMap::new()),
            log_paths: log_setup.paths,
            _log_guard: log_setup.guard,
        }
    }
}
