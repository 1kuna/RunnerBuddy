use crate::config::Config;
use crate::errors::Error;
use crate::logging::scrub_sensitive;
use crate::runner_mgmt::{latest_log_file, runner_log_dir};
use crate::util::{read_file_tail, LOG_TAIL_BYTES};
use serde::Serialize;
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

fn log_dir_for_runner(runner_id: &str) -> PathBuf {
    crate::config::runner_logs_dir(runner_id)
        .unwrap_or_else(|_| PathBuf::from(".").join("logs").join(runner_id))
}

fn diag_log_latest(runner: &crate::config::RunnerProfile) -> Option<PathBuf> {
    latest_log_file(&runner_log_dir(runner)).ok().flatten()
}

fn diag_log_fallback(runner: Option<&crate::config::RunnerProfile>) -> PathBuf {
    runner
        .map(|profile| runner_log_dir(profile).join("Runner.Listener.log"))
        .unwrap_or_else(|| PathBuf::from("Runner.Listener.log"))
}

pub fn list_sources(
    config: &Config,
    runner_id: &str,
    app_log: &Path,
) -> Result<Vec<LogSource>, Error> {
    let runner = crate::config::find_runner(config, runner_id)?;
    let mut sources = Vec::new();
    sources.push(LogSource {
        id: "app".to_string(),
        label: "App Log".to_string(),
        path: app_log.to_string_lossy().to_string(),
    });
    let log_dir = log_dir_for_runner(runner_id);
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
    if let Some(latest) = diag_log_latest(&runner) {
        sources.push(LogSource {
            id: "runner-diag".to_string(),
            label: "Runner Diag".to_string(),
            path: latest.to_string_lossy().to_string(),
        });
    }
    Ok(sources)
}

pub fn tail(path: &Path, limit: usize) -> Result<Vec<LogLine>, Error> {
    let contents = match read_file_tail(path, LOG_TAIL_BYTES) {
        Ok(Some(contents)) => contents,
        Ok(None) => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };
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
    let log_dir = log_dir_for_runner(runner_id);
    match source {
        "app" => app_log.to_path_buf(),
        "runner-stdout" => log_dir.join("runner-stdout.log"),
        "runner-stderr" => log_dir.join("runner-stderr.log"),
        "runner-diag" => runner
            .and_then(diag_log_latest)
            .unwrap_or_else(|| diag_log_fallback(runner)),
        _ => PathBuf::from(source),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn tail_missing_file_returns_empty() {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("runnerbuddy-missing-log-{ts}.log"));
        assert!(!path.exists());
        let lines = tail(&path, 50).expect("tail should succeed for missing file");
        assert!(lines.is_empty());
    }
}
