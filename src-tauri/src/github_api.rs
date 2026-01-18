use crate::config::RunnerScope;
use crate::errors::Error;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, LINK, USER_AGENT};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const API_BASE: &str = "https://api.github.com";

#[derive(Debug, Serialize, Deserialize)]
pub struct RegistrationToken {
    pub token: String,
    pub expires_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RepoPermissions {
    pub admin: bool,
    pub push: bool,
    pub pull: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct RepoInfo {
    pub owner: String,
    pub repo: String,
    pub name_with_owner: String,
    pub url: String,
    pub private: bool,
    #[serde(default)]
    pub permissions: Option<RepoPermissions>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct OrgInfo {
    pub org: String,
    pub url: String,
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
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        let summary = summarize_error_body(&body);
        if summary.is_empty() {
            return Err(Error::Github(format!("token validation failed: {status}")));
        }
        return Err(Error::Github(format!(
            "token validation failed: {status}: {summary}"
        )));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct ApiOwner {
    login: String,
}

#[derive(Debug, Deserialize)]
struct ApiRepo {
    name: String,
    full_name: String,
    html_url: String,
    private: bool,
    owner: ApiOwner,
    #[serde(default)]
    permissions: Option<RepoPermissions>,
}

#[derive(Debug, Deserialize)]
struct ApiOrg {
    login: String,
    html_url: String,
}

fn parse_next_link(header_value: &str) -> Option<String> {
    for part in header_value.split(',') {
        let part = part.trim();
        if !part.contains("rel=\"next\"") {
            continue;
        }
        let start = part.find('<')?;
        let end = part.find('>')?;
        if end <= start + 1 {
            continue;
        }
        return Some(part[start + 1..end].to_string());
    }
    None
}

fn summarize_error_body(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        let message = value
            .get("message")
            .and_then(|val| val.as_str())
            .map(str::to_string);
        let doc_url = value
            .get("documentation_url")
            .and_then(|val| val.as_str())
            .map(str::to_string);
        if let Some(message) = message {
            if let Some(doc_url) = doc_url {
                return format!("{message} ({doc_url})");
            }
            return message;
        }
    }
    let mut snippet = trimmed.to_string();
    if snippet.len() > 600 {
        snippet.truncate(600);
        snippet.push_str("…");
    }
    snippet
}

async fn fetch_all_pages<T>(client: &reqwest::Client, mut url: String) -> Result<Vec<T>, Error>
where
    T: DeserializeOwned,
{
    let mut results = Vec::new();
    let mut iterations = 0;
    loop {
        iterations += 1;
        if iterations > 200 {
            return Err(Error::Github("pagination exceeded 200 pages".into()));
        }
        let resp = client.get(&url).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            let summary = summarize_error_body(&body);
            if summary.is_empty() {
                return Err(Error::Github(format!("request failed: {status}")));
            }
            return Err(Error::Github(format!("request failed: {status}: {summary}")));
        }
        let next_link = resp
            .headers()
            .get(LINK)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_next_link);
        let mut page = resp.json::<Vec<T>>().await?;
        results.append(&mut page);
        match next_link {
            Some(next) => url = next,
            None => break,
        }
    }
    Ok(results)
}

pub async fn list_repos(pat: &str) -> Result<Vec<RepoInfo>, Error> {
    let client = build_client(pat)?;
    let url = format!("{API_BASE}/user/repos?per_page=100&sort=updated&direction=desc");
    let repos = fetch_all_pages::<ApiRepo>(&client, url).await?;
    Ok(repos
        .into_iter()
        .map(|repo| RepoInfo {
            owner: repo.owner.login,
            repo: repo.name,
            name_with_owner: repo.full_name,
            url: repo.html_url,
            private: repo.private,
            permissions: repo.permissions,
        })
        .collect())
}

pub async fn list_orgs(pat: &str) -> Result<Vec<OrgInfo>, Error> {
    let client = build_client(pat)?;
    let url = format!("{API_BASE}/user/orgs?per_page=100");
    let orgs = fetch_all_pages::<ApiOrg>(&client, url).await?;
    Ok(orgs
        .into_iter()
        .map(|org| OrgInfo {
            org: org.login,
            url: org.html_url,
        })
        .collect())
}

pub async fn get_registration_token(scope: &RunnerScope, pat: &str) -> Result<RegistrationToken, Error> {
    let client = build_client(pat)?;
    let endpoint = scope.api_registration_endpoint();
    let resp = client
        .post(format!("{API_BASE}{endpoint}"))
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        let summary = summarize_error_body(&body);
        if summary.is_empty() {
            return Err(Error::Github(format!(
                "registration token request failed: {status}"
            )));
        }
        return Err(Error::Github(format!(
            "registration token request failed: {status}: {summary}"
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
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        let summary = summarize_error_body(&body);
        if summary.is_empty() {
            return Err(Error::Github(format!("remove token request failed: {status}")));
        }
        return Err(Error::Github(format!(
            "remove token request failed: {status}: {summary}"
        )));
    }
    let token = resp.json::<RegistrationToken>().await?;
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::parse_next_link;

    #[test]
    fn parse_next_link_extracts_url() {
        let header = r#"<https://api.github.com/user/repos?page=2&per_page=100>; rel="next", <https://api.github.com/user/repos?page=5&per_page=100>; rel="last""#;
        assert_eq!(
            parse_next_link(header).as_deref(),
            Some("https://api.github.com/user/repos?page=2&per_page=100")
        );
    }

    #[test]
    fn parse_next_link_returns_none_without_next() {
        let header = r#"<https://api.github.com/user/repos?page=5&per_page=100>; rel="last""#;
        assert!(parse_next_link(header).is_none());
    }
}
