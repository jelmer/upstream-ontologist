//! Helpers for accessing the GitHub API and raw repository files.
//!
//! Several providers need to talk to GitHub, either to query the REST API
//! (`api.github.com`) or to download individual files. These helpers
//! centralise that access, including authentication via the `GITHUB_TOKEN`
//! environment variable, so call sites do not each reimplement it.

use crate::{HTTPJSONError, UpstreamDatum};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION};

/// Base URL of the GitHub REST API.
const API_BASE: &str = "https://api.github.com";

/// Base URL for raw file access.
const RAW_BASE: &str = "https://raw.githubusercontent.com";

/// Builds the header map for a GitHub API request, adding an `Authorization`
/// header when `GITHUB_TOKEN` is set in the environment.
fn auth_headers(accept: &'static str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static(accept));
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if let Ok(value) = HeaderValue::from_str(&format!("Bearer {}", token)) {
            headers.insert(AUTHORIZATION, value);
        }
    }
    headers
}

async fn fetch(url: &str, accept: &'static str) -> Result<reqwest::Response, HTTPJSONError> {
    let client = crate::http::build_client()
        .default_headers(auth_headers(accept))
        .build()
        .map_err(HTTPJSONError::HTTPError)?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(HTTPJSONError::HTTPError)?;

    if !response.status().is_success() {
        return Err(HTTPJSONError::Error {
            url: response.url().clone(),
            status: response.status().as_u16(),
            response: Box::new(response),
        });
    }

    Ok(response)
}

/// Fetches a GitHub REST API endpoint and parses the response as JSON.
///
/// `path` is the API path without the host, e.g. `repos/serde-rs/serde` or
/// `repos/serde-rs/serde/tags`. The `GITHUB_TOKEN` environment variable, if
/// set, is sent as a bearer token to raise the rate limit.
pub async fn load_github_json(path: &str) -> Result<serde_json::Value, HTTPJSONError> {
    let url = format!("{}/{}", API_BASE, path.trim_start_matches('/'));
    let response = fetch(&url, "application/vnd.github+json").await?;
    response.json().await.map_err(HTTPJSONError::HTTPError)
}

/// Downloads a raw file from a repository via `raw.githubusercontent.com`.
///
/// This does not consume the API rate limit, but the caller must know the
/// branch or tag (`reference`) and exact `path`; a missing file or wrong
/// reference yields a 404.
pub async fn download_raw_file(
    owner: &str,
    repo: &str,
    reference: &str,
    path: &str,
) -> Result<String, HTTPJSONError> {
    let url = format!(
        "{}/{}/{}/{}/{}",
        RAW_BASE,
        owner,
        repo,
        reference,
        path.trim_start_matches('/')
    );
    let response = fetch(&url, "text/plain").await?;
    response.text().await.map_err(HTTPJSONError::HTTPError)
}

/// Downloads a file via the GitHub contents API.
///
/// Unlike [`download_raw_file`] this resolves the repository's default branch
/// automatically when `reference` is `None`, at the cost of consuming the API
/// rate limit. The base64-encoded content returned by the API is decoded.
pub async fn download_contents(
    owner: &str,
    repo: &str,
    path: &str,
    reference: Option<&str>,
) -> Result<Vec<u8>, crate::ProviderError> {
    let mut url = format!(
        "{}/repos/{}/{}/contents/{}",
        API_BASE,
        owner,
        repo,
        path.trim_start_matches('/')
    );
    if let Some(reference) = reference {
        url.push_str(&format!("?ref={}", reference));
    }

    let response = fetch(&url, "application/vnd.github+json").await?;
    let data: serde_json::Value = response.json().await.map_err(HTTPJSONError::HTTPError)?;

    let encoding = data["encoding"].as_str();
    let content = data["content"].as_str().ok_or_else(|| {
        crate::ProviderError::ParseError("contents API response missing content".to_string())
    })?;

    match encoding {
        Some("base64") => {
            use base64::Engine;
            // The API wraps the base64 payload at column 60 with newlines.
            let stripped: String = content.chars().filter(|c| !c.is_whitespace()).collect();
            base64::engine::general_purpose::STANDARD
                .decode(stripped.as_bytes())
                .map_err(|e| {
                    crate::ProviderError::ParseError(format!("invalid base64 content: {}", e))
                })
        }
        other => Err(crate::ProviderError::ParseError(format!(
            "unexpected contents encoding: {:?}",
            other
        ))),
    }
}

/// Repository metadata returned by the GitHub repos API.
///
/// This covers the fields useful as upstream metadata; the API returns many
/// more that are ignored here.
#[derive(serde::Deserialize, Debug, Clone)]
pub struct RepoMetadata {
    /// Repository description.
    pub description: Option<String>,
    /// Homepage URL declared on the repository.
    pub homepage: Option<String>,
    /// Canonical repository URL.
    pub html_url: Option<String>,
    /// License information, if GitHub detected one.
    pub license: Option<License>,
    /// Whether the repository is archived.
    #[serde(default)]
    pub archived: bool,
}

/// License information from the GitHub repos API.
#[derive(serde::Deserialize, Debug, Clone)]
pub struct License {
    /// SPDX identifier, e.g. `Apache-2.0`. May be `NOASSERTION` for licenses
    /// GitHub could not map to SPDX.
    pub spdx_id: Option<String>,
}

/// Fetches repository metadata from the GitHub repos API.
///
/// Returns `None` if the repository does not exist.
pub async fn repo_metadata(
    owner: &str,
    repo: &str,
) -> Result<Option<RepoMetadata>, crate::ProviderError> {
    let path = format!("repos/{}/{}", owner, repo);
    match load_github_json(&path).await {
        Ok(value) => serde_json::from_value(value).map(Some).map_err(|e| {
            crate::ProviderError::ParseError(format!("Failed to parse repo metadata: {}", e))
        }),
        Err(HTTPJSONError::Error { status: 404, .. }) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

impl RepoMetadata {
    /// Converts the repository metadata into upstream data items.
    pub fn to_upstream_data(&self) -> Vec<UpstreamDatum> {
        let mut results = Vec::new();
        if let Some(description) = self.description.as_deref() {
            if !description.is_empty() {
                results.push(UpstreamDatum::Summary(description.to_string()));
            }
        }
        if let Some(homepage) = self.homepage.as_deref() {
            if !homepage.is_empty() {
                results.push(UpstreamDatum::Homepage(homepage.to_string()));
            }
        }
        if let Some(html_url) = self.html_url.as_deref() {
            results.push(UpstreamDatum::Repository(html_url.to_string()));
        }
        if let Some(spdx) = self.license.as_ref().and_then(|l| l.spdx_id.as_deref()) {
            if !spdx.is_empty() && spdx != "NOASSERTION" {
                results.push(UpstreamDatum::License(spdx.to_string()));
            }
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_repo_metadata() {
        let data = r#"{
            "description": "Serialization framework for Rust",
            "homepage": "https://serde.rs/",
            "html_url": "https://github.com/serde-rs/serde",
            "license": {"spdx_id": "Apache-2.0"},
            "archived": false
        }"#;
        let meta: RepoMetadata = serde_json::from_str(data).unwrap();
        assert_eq!(
            meta.description.as_deref(),
            Some("Serialization framework for Rust")
        );
        assert_eq!(
            meta.license.as_ref().unwrap().spdx_id.as_deref(),
            Some("Apache-2.0")
        );

        let data = meta.to_upstream_data();
        assert_eq!(
            data,
            vec![
                UpstreamDatum::Summary("Serialization framework for Rust".to_string()),
                UpstreamDatum::Homepage("https://serde.rs/".to_string()),
                UpstreamDatum::Repository("https://github.com/serde-rs/serde".to_string()),
                UpstreamDatum::License("Apache-2.0".to_string()),
            ]
        );
    }

    #[test]
    fn test_noassertion_license_dropped() {
        let data = r#"{"license": {"spdx_id": "NOASSERTION"}}"#;
        let meta: RepoMetadata = serde_json::from_str(data).unwrap();
        assert_eq!(meta.to_upstream_data(), vec![]);
    }
}
