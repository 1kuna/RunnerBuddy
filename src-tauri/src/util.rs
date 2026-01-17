use std::path::PathBuf;

pub fn default_runner_name() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "runnerbuddy".to_string())
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
