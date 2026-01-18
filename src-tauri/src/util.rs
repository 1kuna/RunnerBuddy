use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const LOG_TAIL_BYTES: usize = 1024 * 1024;

pub fn default_runner_name() -> String {
    fn normalize(value: Option<String>) -> Option<String> {
        value
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    normalize(
        hostname::get()
            .ok()
            .and_then(|value| value.into_string().ok()),
    )
    .or_else(|| normalize(std::env::var("HOSTNAME").ok()))
    .or_else(|| normalize(std::env::var("COMPUTERNAME").ok()))
    .unwrap_or_else(|| "runnerbuddy".to_string())
}

pub fn platform_label() -> String {
    match std::env::consts::OS {
        "macos" => "macOS".to_string(),
        "windows" => "Windows".to_string(),
        "linux" => "Linux".to_string(),
        other => other.to_string(),
    }
}

pub fn expand_path(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(user_dirs) = directories::UserDirs::new() {
            return user_dirs.home_dir().join(stripped);
        }
    }
    PathBuf::from(path)
}

pub fn normalize_labels(labels: Vec<String>) -> Vec<String> {
    labels
        .into_iter()
        .map(|label| label.trim().to_string())
        .filter(|label| !label.is_empty())
        .collect()
}

pub fn read_file_tail(path: &Path, max_bytes: usize) -> Result<Option<String>, std::io::Error> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    let len = file.metadata()?.len();
    let start = len.saturating_sub(max_bytes as u64);
    file.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    Ok(Some(String::from_utf8_lossy(&buf).to_string()))
}
