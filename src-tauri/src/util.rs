use std::path::PathBuf;

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
