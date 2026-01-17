use crate::config::RunnerScope;
use crate::errors::Error;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::{Deserialize, Serialize};

const API_BASE: &str = "https://api.github.com";

#[derive(Debug, Serialize, Deserialize)]
pub struct RegistrationToken {
    pub token: String,
    pub expires_at: String,
}

fn build_client(pat: &str) -> Result<reqwest::Client, Error> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static("RunnerBuddy"));
    headers.insert(ACCEPT, HeaderValue::from_static("application/vnd.github+json"));
    let auth_value = format!("token {pat}");
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&auth_value).map_err(|err| Error::Github(err.to_string()))?,
    );
    Ok(reqwest::Client::builder()
        .default_headers(headers)
        .build()?)
}

pub async fn validate_pat(pat: &str) -> Result<(), Error> {
    let client = build_client(pat)?;
    let resp = client.get(format!("{API_BASE}/user")).send().await?;
    if !resp.status().is_success() {
        return Err(Error::Github(format!(
            "token validation failed: {}",
            resp.status()
        )));
    }
    Ok(())
}

pub async fn get_registration_token(scope: &RunnerScope, pat: &str) -> Result<RegistrationToken, Error> {
    let client = build_client(pat)?;
    let endpoint = scope.api_registration_endpoint();
    let resp = client
        .post(format!("{API_BASE}{endpoint}"))
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(Error::Github(format!(
            "registration token request failed: {}",
            resp.status()
        )));
    }
    let token = resp.json::<RegistrationToken>().await?;
    Ok(token)
}

pub async fn get_remove_token(scope: &RunnerScope, pat: &str) -> Result<RegistrationToken, Error> {
    let client = build_client(pat)?;
    let endpoint = scope.api_remove_endpoint();
    let resp = client
        .post(format!("{API_BASE}{endpoint}"))
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(Error::Github(format!(
            "remove token request failed: {}",
            resp.status()
        )));
    }
    let token = resp.json::<RegistrationToken>().await?;
    Ok(token)
}
