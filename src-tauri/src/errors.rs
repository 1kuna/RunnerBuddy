use serde::Serialize;
use std::fmt;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("config error: {0}")]
    Config(String),
    #[error("secrets error: {0}")]
    Secrets(String),
    #[error("github api error: {0}")]
    Github(String),
    #[error("runner error: {0}")]
    Runner(String),
    #[error("service error: {0}")]
    Service(String),
    #[error("unsupported platform: {0}")]
    Unsupported(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Serialize)]
pub struct AppError {
    pub code: String,
    pub message: String,
}

impl AppError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.code.is_empty() {
            write!(f, "{}", self.message)
        } else {
            write!(f, "{}: {}", self.code, self.message)
        }
    }
}

impl From<Error> for AppError {
    fn from(err: Error) -> Self {
        let code = match &err {
            Error::Config(_) => "config",
            Error::Secrets(_) => "secrets",
            Error::Github(_) => "github",
            Error::Runner(_) => "runner",
            Error::Service(_) => "service",
            Error::Unsupported(_) => "unsupported",
            Error::Io(_) => "io",
            Error::Http(_) => "http",
            Error::Json(_) => "json",
        };
        AppError::new(code, err.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
