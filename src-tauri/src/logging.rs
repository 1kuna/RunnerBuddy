use crate::config::logs_dir;
use crate::errors::Error;
use regex::Regex;
use std::path::PathBuf;
use std::sync::OnceLock;
use tracing_subscriber::EnvFilter;

#[derive(Debug)]
pub struct LogSetup {
    pub paths: LogPaths,
    pub guard: tracing_appender::non_blocking::WorkerGuard,
}

#[derive(Debug, Clone)]
pub struct LogPaths {
    pub app_log: PathBuf,
}

pub fn init_logging() -> Result<LogSetup, Error> {
    let log_dir = logs_dir()?;
    std::fs::create_dir_all(&log_dir)?;
    let file_appender = tracing_appender::rolling::daily(&log_dir, "runnerbuddy.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(non_blocking)
        .with_ansi(false)
        .init();
    Ok(LogSetup {
        paths: LogPaths {
            app_log: log_dir.join("runnerbuddy.log"),
        },
        guard,
    })
}

pub fn scrub_sensitive(line: &str) -> String {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        [
            r"ghp_[A-Za-z0-9]{10,}",
            r"ghs_[A-Za-z0-9]{10,}",
            r"ghu_[A-Za-z0-9]{10,}",
            r"github_pat_[A-Za-z0-9_]{10,}",
            r"Bearer\s+[A-Za-z0-9._-]{10,}",
            r"token\s+[A-Za-z0-9._-]{10,}",
        ]
        .iter()
        .filter_map(|pattern| Regex::new(pattern).ok())
        .collect()
    });
    let mut redacted = line.to_string();
    for regex in patterns.iter() {
        redacted = regex.replace_all(&redacted, "[REDACTED]").to_string();
    }
    redacted
}

#[cfg(test)]
mod tests {
    use super::scrub_sensitive;

    #[test]
    fn scrubs_github_pat() {
        let line = "Authorization: token ghp_1234567890abcdef";
        let scrubbed = scrub_sensitive(line);
        assert!(scrubbed.contains("[REDACTED]"));
    }
}
