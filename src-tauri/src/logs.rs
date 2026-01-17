use crate::config::Config;
use crate::errors::Error;
use crate::logging::scrub_sensitive;
use crate::runner_mgmt::{latest_log_file, runner_log_dir};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
pub struct LogSource {
    pub id: String,
    pub label: String,
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct LogLine {
    pub line: String,
}

pub fn list_sources(
    config: &Config,
    runner_id: &str,
    app_log: &Path,
) -> Result<Vec<LogSource>, Error> {
    let runner = config
        .runners
        .iter()
        .find(|runner| runner.runner_id == runner_id)
        .ok_or_else(|| Error::Runner(format!("runner {runner_id} not found")))?;
    let mut sources = Vec::new();
    sources.push(LogSource {
        id: "app".to_string(),
        label: "App Log".to_string(),
        path: app_log.to_string_lossy().to_string(),
    });
    let log_dir = crate::config::data_dir()?.join("logs").join(runner_id);
    let stdout = log_dir.join("runner-stdout.log");
    let stderr = log_dir.join("runner-stderr.log");
    sources.push(LogSource {
        id: "runner-stdout".to_string(),
        label: "Runner Stdout".to_string(),
        path: stdout.to_string_lossy().to_string(),
    });
    sources.push(LogSource {
        id: "runner-stderr".to_string(),
        label: "Runner Stderr".to_string(),
        path: stderr.to_string_lossy().to_string(),
    });
    let diag_dir = runner_log_dir(runner);
    if let Ok(Some(latest)) = latest_log_file(&diag_dir) {
        sources.push(LogSource {
            id: "runner-diag".to_string(),
            label: "Runner Diag".to_string(),
            path: latest.to_string_lossy().to_string(),
        });
    }
    Ok(sources)
}

pub fn tail(path: &Path, limit: usize) -> Result<Vec<LogLine>, Error> {
    let contents = fs::read_to_string(path)?;
    let lines: Vec<LogLine> = contents
        .lines()
        .rev()
        .take(limit)
        .map(|line| LogLine {
            line: scrub_sensitive(line),
        })
        .collect();
    Ok(lines.into_iter().rev().collect())
}

pub fn resolve_source_path(
    config: &Config,
    runner_id: &str,
    app_log: &Path,
    source: &str,
) -> PathBuf {
    let runner = config
        .runners
        .iter()
        .find(|runner| runner.runner_id == runner_id);
    let log_dir = crate::config::data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("logs")
        .join(runner_id);
    match source {
        "app" => app_log.to_path_buf(),
        "runner-stdout" => log_dir.join("runner-stdout.log"),
        "runner-stderr" => log_dir.join("runner-stderr.log"),
        "runner-diag" => runner
            .and_then(|profile| latest_log_file(&runner_log_dir(profile)).ok().flatten())
            .unwrap_or_else(|| {
                runner
                    .map(|profile| runner_log_dir(profile).join("Runner.Listener.log"))
                    .unwrap_or_else(|| PathBuf::from("Runner.Listener.log"))
            }),
        _ => PathBuf::from(source),
    }
}
