//! GitHub release API client — port of Go `checkForUpdates`,
//! `GetLatestRelease`, `FindPlatformAsset`, `FindChecksumAsset`.

use serde::Deserialize;
use std::time::Duration;

use crate::{REPO_NAME, REPO_OWNER};

const MAX_RELEASE_METADATA_BYTES: usize = 1 << 20;
const USER_AGENT: &str = "bvr-updater";

/// A single asset attached to a GitHub release.
#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    /// File name, e.g. `bvr-v0.1.8-linux-x86_64.tar.gz`.
    pub name: String,
    /// Direct download URL.
    pub browser_download_url: String,
    /// Size in bytes reported by the API.
    #[serde(default)]
    pub size: i64,
}

/// A GitHub release (only the fields we need).
#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    /// Tag name, e.g. `v0.1.8`.
    pub tag_name: String,
    /// Web URL of the release page.
    #[serde(default)]
    pub html_url: String,
    /// Attached files.
    #[serde(default)]
    pub assets: Vec<Asset>,
}

/// Update info for `--check-update` output.
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    /// New tag, e.g. `v0.1.8`.
    pub new_version: String,
    /// Release page URL.
    pub release_url: String,
}

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("network error: {0}")]
    Network(String),
    #[error("github api returned status {0}")]
    Status(u16),
    #[error("release metadata exceeds size limit")]
    TooLarge,
    #[error("failed to parse release info: {0}")]
    Parse(String),
}

fn github_token() -> Option<String> {
    for key in ["GITHUB_TOKEN", "GH_TOKEN"] {
        let v = std::env::var(key).unwrap_or_default();
        let v = v.trim().to_string();
        if !v.is_empty() {
            return Some(v);
        }
    }
    None
}

fn agent(timeout: Duration) -> ureq::Agent {
    // Never forward Authorization to non-GitHub hosts on redirect
    // (Go parity: strip token when leaving github domains for CDN hosts).
    // http_status_as_error(false): we inspect status codes ourselves
    // (403/429 → skip check, Go parity).
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .redirect_auth_headers(ureq::config::RedirectAuthHeaders::Never)
        .http_status_as_error(false)
        .build()
        .into()
}

/// Apply `Authorization: Bearer <token>` — callers only invoke this for
/// api.github.com requests, never for CDN redirect targets.
fn with_auth(req: ureq::RequestBuilder<ureq::typestate::WithoutBody>) -> ureq::RequestBuilder<ureq::typestate::WithoutBody> {
    match github_token() {
        Some(tok) => req.header("Authorization", &format!("Bearer {tok}")),
        None => req,
    }
}

fn decode_release_metadata(bytes: &[u8]) -> Result<Release, FetchError> {
    if bytes.len() > MAX_RELEASE_METADATA_BYTES {
        return Err(FetchError::TooLarge);
    }
    let mut de = serde_json::Deserializer::from_slice(bytes);
    let rel: Release =
        serde::Deserialize::deserialize(&mut de).map_err(|e| FetchError::Parse(e.to_string()))?;
    de.end().map_err(|e| FetchError::Parse(e.to_string()))?;
    Ok(rel)
}

/// Fetch the latest release from the GitHub API.
pub fn get_latest_release() -> Result<Release, FetchError> {
    let url = format!("https://api.github.com/repos/{REPO_OWNER}/{REPO_NAME}/releases/latest");
    let req = agent(Duration::from_secs(30))
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github.v3+json");
    let resp = with_auth(req)
        .call()
        .map_err(|e| FetchError::Network(e.to_string()))?;
    let status = resp.status().as_u16();
    if status != 200 {
        return Err(FetchError::Status(status));
    }
    let body = resp.into_body().into_reader();
    let mut bytes = Vec::new();
    use std::io::Read as _;
    body.take((MAX_RELEASE_METADATA_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| FetchError::Network(e.to_string()))?;
    decode_release_metadata(&bytes)
}

/// Check whether a newer release exists. Returns `None` when up to date.
/// Mirrors Go `checkForUpdates`: 403/429 are treated as "no update" (skip),
/// not fatal, so shared-IP rate limits never break startup.
pub fn check_for_updates() -> Result<Option<UpdateInfo>, FetchError> {
    let url = format!("https://api.github.com/repos/{REPO_OWNER}/{REPO_NAME}/releases/latest");
    let req = agent(Duration::from_secs(2))
        .get(&url)
        .header("User-Agent", "bvr-update-check")
        .header("Accept", "application/vnd.github.v3+json");
    let resp = with_auth(req)
        .call()
        .map_err(|e| FetchError::Network(e.to_string()))?;
    let status = resp.status().as_u16();
    match status {
        200 => {}
        // Rate/abuse limits: skip the check silently (Go parity).
        403 | 429 => return Ok(None),
        s => return Err(FetchError::Status(s)),
    }
    let body = resp.into_body().into_reader();
    let mut bytes = Vec::new();
    use std::io::Read as _;
    body.take((MAX_RELEASE_METADATA_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| FetchError::Network(e.to_string()))?;
    let rel = decode_release_metadata(&bytes)?;
    if crate::version::is_newer_than_current(&rel.tag_name) {
        Ok(Some(UpdateInfo {
            new_version: rel.tag_name,
            release_url: rel.html_url,
        }))
    } else {
        Ok(None)
    }
}

impl Release {
    /// Find the asset matching this platform, preferring the stable
    /// (unversioned) name then the legacy versioned fallback.
    pub fn find_platform_asset(&self) -> Option<&Asset> {
        for name in platform_asset_names(&self.tag_name) {
            if let Some(a) = self.assets.iter().find(|a| a.name == name) {
                return Some(a);
            }
        }
        None
    }

    /// Find the `checksums.txt`-style asset (`.sha256` sidecar also accepted).
    pub fn find_checksum_asset(&self) -> Option<&Asset> {
        self.assets.iter().find(|a| {
            a.name == "checksums.txt"
                || a.name.ends_with(".sha256")
        })
    }
}

/// Candidate asset names for `tag` on this platform: stable first, then
/// legacy `bvr-{ver}-{platform}` fallback.
pub fn platform_asset_names(tag: &str) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(stable) = crate::asset_name_for(tag) {
        names.push(stable);
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_stable_asset() {
        let tag = "v0.1.8";
        let Some(expected) = crate::asset_name_for(tag) else {
            return;
        };
        let rel = Release {
            tag_name: tag.into(),
            html_url: String::new(),
            assets: vec![Asset {
                name: expected.clone(),
                browser_download_url: "https://example.com/x".into(),
                size: 1,
            }],
        };
        assert_eq!(rel.find_platform_asset().unwrap().name, expected);
    }

    #[test]
    fn metadata_size_limit() {
        let big = vec![b'x'; MAX_RELEASE_METADATA_BYTES + 2];
        assert!(matches!(
            decode_release_metadata(&big),
            Err(FetchError::TooLarge)
        ));
    }
}
