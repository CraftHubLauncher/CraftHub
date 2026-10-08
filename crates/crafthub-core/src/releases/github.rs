//! GitHub REST API client for release metadata, with ETag caching and honest
//! rate-limit/offline behaviour (stale cache is reported as stale, never as fresh).

use futures_util::StreamExt;
use reqwest::StatusCode;
use reqwest::header::{ACCEPT, ETAG, HeaderMap, IF_NONE_MATCH};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{CoreError, Result};
use crate::net::{HttpPolicy, build_client};
use crate::registry::Registry;
use crate::util::now_unix;

const MAX_API_BODY: usize = 8 * 1024 * 1024;
const RELEASES_PER_PAGE: u32 = 30;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GhRelease {
    pub id: u64,
    pub tag_name: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub published_at: Option<String>,
    pub html_url: String,
    #[serde(default)]
    pub assets: Vec<GhAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GhAsset {
    pub id: u64,
    pub name: String,
    pub size: u64,
    #[serde(default)]
    pub digest: Option<String>,
    pub browser_download_url: String,
    #[serde(default)]
    pub state: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FetchSource {
    /// Fresh response from GitHub.
    Network,
    /// GitHub confirmed the cached copy is current (HTTP 304).
    NotModified,
    /// GitHub could not be reached or refused; cached data may be out of date.
    StaleCache,
}

#[derive(Debug, Clone)]
pub struct FetchOutcome {
    pub releases: Vec<GhRelease>,
    pub fetched_at: i64,
    pub source: FetchSource,
    pub warning: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GithubConfig {
    pub api_base: String,
    /// Base for `browser_download_url` validation (`https://github.com` in production).
    pub download_base: String,
    pub policy: HttpPolicy,
}

impl GithubConfig {
    pub fn production() -> Self {
        Self {
            api_base: "https://api.github.com".into(),
            download_base: "https://github.com".into(),
            policy: HttpPolicy::github(),
        }
    }
}

#[derive(Clone)]
pub struct GithubClient {
    client: reqwest::Client,
    config: GithubConfig,
}

impl GithubClient {
    pub fn new(config: GithubConfig) -> Result<Self> {
        config.policy.check_str(&config.api_base)?;
        Ok(Self {
            client: build_client(&config.policy)?,
            config,
        })
    }

    pub fn config(&self) -> &GithubConfig {
        &self.config
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.client
    }

    fn releases_url(&self, repo: &str) -> Result<Url> {
        let url = format!(
            "{}/repos/{}/releases?per_page={}",
            self.config.api_base.trim_end_matches('/'),
            repo,
            RELEASES_PER_PAGE
        );
        self.config.policy.check_str(&url)
    }

    /// Cached-only read used for fast/offline startup.
    pub fn cached_releases(&self, repo: &str, registry: &Registry) -> Result<Option<FetchOutcome>> {
        let url = self.releases_url(repo)?;
        let Some(entry) = registry.http_cache_get(url.as_str())? else {
            return Ok(None);
        };
        let releases = parse_releases(&entry.body)?;
        Ok(Some(FetchOutcome {
            releases,
            fetched_at: entry.fetched_at,
            source: FetchSource::StaleCache,
            warning: None,
        }))
    }

    /// Fetches the release list, using ETag revalidation and falling back to the cache
    /// (marked stale) on network failure or rate limiting.
    pub async fn fetch_releases(&self, repo: &str, registry: &Registry) -> Result<FetchOutcome> {
        let url = self.releases_url(repo)?;
        let cached = registry.http_cache_get(url.as_str())?;

        let mut req = self
            .client
            .get(url.clone())
            .header(ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(etag) = cached.as_ref().and_then(|c| c.etag.clone()) {
            req = req.header(IF_NONE_MATCH, etag);
        }

        let stale = |warning: String| -> Result<FetchOutcome> {
            match &cached {
                Some(c) => Ok(FetchOutcome {
                    releases: parse_releases(&c.body)?,
                    fetched_at: c.fetched_at,
                    source: FetchSource::StaleCache,
                    warning: Some(warning),
                }),
                None => Err(CoreError::Network(warning)),
            }
        };

        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                let err = CoreError::from(e);
                return match err {
                    CoreError::Network(m) => stale(format!("GitHub could not be reached ({m}).")),
                    other => Err(other),
                };
            }
        };

        let status = resp.status();
        if status == StatusCode::NOT_MODIFIED {
            if let Some(c) = &cached {
                let now = now_unix();
                registry.http_cache_touch(url.as_str(), now)?;
                return Ok(FetchOutcome {
                    releases: parse_releases(&c.body)?,
                    fetched_at: now,
                    source: FetchSource::NotModified,
                    warning: None,
                });
            }
            return Err(CoreError::Api {
                status: 304,
                message: "unexpected Not Modified without a cached copy".into(),
            });
        }

        if let Some(msg) = rate_limit_message(status, resp.headers()) {
            return match &cached {
                Some(_) => stale(msg),
                None => Err(CoreError::RateLimited(msg)),
            };
        }

        if status == StatusCode::NOT_FOUND {
            return Err(CoreError::Api {
                status: 404,
                message: format!("repository {repo} was not found"),
            });
        }
        if !status.is_success() {
            let msg = format!("GitHub returned HTTP {}.", status.as_u16());
            return if status.is_server_error() {
                stale(msg)
            } else {
                Err(CoreError::Api {
                    status: status.as_u16(),
                    message: msg,
                })
            };
        }

        let etag = resp
            .headers()
            .get(ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let body = read_limited(resp, MAX_API_BODY).await?;
        let text = String::from_utf8(body).map_err(|_| CoreError::Api {
            status: 200,
            message: "response was not UTF-8".into(),
        })?;
        let releases = parse_releases(&text)?;
        let now = now_unix();
        registry.http_cache_put(url.as_str(), etag.as_deref(), &text, now)?;
        Ok(FetchOutcome {
            releases,
            fetched_at: now,
            source: FetchSource::Network,
            warning: None,
        })
    }

    /// Downloads a small text file (e.g. `SHA256SUMS.txt`) into memory with a hard size cap.
    pub async fn fetch_small_text(&self, url: &str, max: usize) -> Result<String> {
        let url = self.config.policy.check_str(url)?;
        let resp = self.client.get(url).send().await?;
        if !resp.status().is_success() {
            return Err(CoreError::Api {
                status: resp.status().as_u16(),
                message: "could not download checksum file".into(),
            });
        }
        let body = read_limited(resp, max).await?;
        String::from_utf8(body)
            .map_err(|_| CoreError::Integrity("checksum file is not UTF-8".into()))
    }
}

fn parse_releases(body: &str) -> Result<Vec<GhRelease>> {
    serde_json::from_str(body).map_err(|e| CoreError::Api {
        status: 200,
        message: format!("unexpected release JSON: {e}"),
    })
}

async fn read_limited(resp: reqwest::Response, max: usize) -> Result<Vec<u8>> {
    if resp.content_length().is_some_and(|l| l > max as u64) {
        return Err(CoreError::Network("response is larger than allowed".into()));
    }
    let mut out = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if out.len() + chunk.len() > max {
            return Err(CoreError::Network("response is larger than allowed".into()));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

fn rate_limit_message(status: StatusCode, headers: &HeaderMap) -> Option<String> {
    if status != StatusCode::FORBIDDEN && status != StatusCode::TOO_MANY_REQUESTS {
        return None;
    }
    let header = |n: &str| headers.get(n).and_then(|v| v.to_str().ok());
    let exhausted = header("x-ratelimit-remaining") == Some("0");
    let retry_after = header("retry-after").and_then(|v| v.parse::<i64>().ok());
    if !exhausted && retry_after.is_none() && status != StatusCode::TOO_MANY_REQUESTS {
        return None;
    }
    let wait = retry_after.or_else(|| {
        header("x-ratelimit-reset")
            .and_then(|v| v.parse::<i64>().ok())
            .map(|reset| (reset - now_unix()).max(0))
    });
    Some(match wait {
        Some(secs) => format!(
            "Try again in about {} minute(s).",
            (secs.max(0) as u64).div_ceil(60).max(1)
        ),
        None => "Try again later.".into(),
    })
}

/// Parses a `sha256:<hex>` digest into lowercase hex.
pub fn parse_sha256_digest(digest: &str) -> Option<String> {
    let hex = digest.strip_prefix("sha256:")?;
    (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| hex.to_ascii_lowercase())
}

/// Finds the hash for `asset_name` in a `sha256sum`-style file (`<hex>  <name>` or `<hex> *<name>`).
pub fn find_in_checksums(text: &str, asset_name: &str) -> Option<String> {
    let mut found = None;
    for line in text.lines() {
        let line = line.trim();
        let Some((hash, rest)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let name = rest.trim_start().trim_start_matches('*');
        if name == asset_name {
            if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            if found.is_some() {
                return None; // ambiguous: listed twice
            }
            found = Some(hash.to_ascii_lowercase());
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_parsing() {
        let h = "9997f7df3a6f0717b46bb0f503b3d920c3beedeb9e9c4e1ff2ef3b8671acd47e";
        assert_eq!(
            parse_sha256_digest(&format!("sha256:{h}")).as_deref(),
            Some(h)
        );
        assert!(parse_sha256_digest(&format!("sha512:{h}")).is_none());
        assert!(parse_sha256_digest("sha256:abc").is_none());
        assert!(parse_sha256_digest(&format!("sha256:{}", "z".repeat(64))).is_none());
    }

    #[test]
    fn checksum_file_parsing() {
        let text = "aaaa  other.zip\n9997f7df3a6f0717b46bb0f503b3d920c3beedeb9e9c4e1ff2ef3b8671acd47e  photocraft-0.3.0-windows-x64-portable.zip\n";
        assert_eq!(
            find_in_checksums(text, "photocraft-0.3.0-windows-x64-portable.zip").as_deref(),
            Some("9997f7df3a6f0717b46bb0f503b3d920c3beedeb9e9c4e1ff2ef3b8671acd47e")
        );
        assert!(find_in_checksums(text, "photocraft").is_none());
        let dup = format!("{0}  a.zip\n{0} *a.zip\n", "b".repeat(64));
        assert!(find_in_checksums(&dup, "a.zip").is_none());
    }

    #[test]
    fn rate_limit_detection() {
        let mut h = HeaderMap::new();
        h.insert("x-ratelimit-remaining", "0".parse().unwrap());
        assert!(rate_limit_message(StatusCode::FORBIDDEN, &h).is_some());
        assert!(rate_limit_message(StatusCode::FORBIDDEN, &HeaderMap::new()).is_none());
        assert!(rate_limit_message(StatusCode::TOO_MANY_REQUESTS, &HeaderMap::new()).is_some());
        assert!(rate_limit_message(StatusCode::OK, &h).is_none());
    }
}
