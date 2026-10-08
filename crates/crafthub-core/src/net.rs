//! HTTP origin policy and client construction. Every request and every redirect hop
//! is checked against an explicit allowlist of HTTPS hosts.

use std::sync::Arc;
use std::time::Duration;

use url::Url;

use crate::error::{CoreError, Result};

pub const USER_AGENT: &str = concat!("CraftHub/", env!("CARGO_PKG_VERSION"), " (unofficial)");
pub const MAX_REDIRECTS: usize = 5;

#[derive(Debug, Clone)]
pub struct HttpPolicy {
    allowed_hosts: Vec<String>,
    /// Test-only escape hatch: permit plain `http://127.0.0.1:<port>` for local mock servers.
    allow_loopback_http: bool,
}

impl HttpPolicy {
    /// Production policy: GitHub API, github.com download URLs and the asset CDN hosts
    /// GitHub redirects to (observed in RELEASE_AUDIT.md).
    pub fn github() -> Self {
        Self {
            allowed_hosts: [
                "api.github.com",
                "github.com",
                "release-assets.githubusercontent.com",
                "objects.githubusercontent.com",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            allow_loopback_http: false,
        }
    }

    /// Policy for deterministic tests against a local mock server.
    pub fn loopback_for_tests() -> Self {
        Self {
            allowed_hosts: Vec::new(),
            allow_loopback_http: true,
        }
    }

    pub fn check(&self, url: &Url) -> Result<()> {
        let host = url.host_str().unwrap_or_default();
        let ok = match url.scheme() {
            "https" => {
                url.port().is_none()
                    && url.username().is_empty()
                    && url.password().is_none()
                    && self.allowed_hosts.iter().any(|h| h == host)
            }
            "http" => self.allow_loopback_http && host == "127.0.0.1",
            _ => false,
        };
        if ok {
            Ok(())
        } else {
            Err(CoreError::UntrustedUrl(format!(
                "{}://{}",
                url.scheme(),
                host
            )))
        }
    }

    pub fn check_str(&self, url: &str) -> Result<Url> {
        let parsed =
            Url::parse(url).map_err(|_| CoreError::UntrustedUrl("malformed URL".into()))?;
        self.check(&parsed)?;
        Ok(parsed)
    }
}

/// Installs rustls's `ring` crypto provider as the process default (once). Safe to call
/// repeatedly and from several threads; an already-installed provider is kept.
pub fn ensure_crypto_provider() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Builds a client that enforces `policy` on every redirect hop.
pub fn build_client(policy: &HttpPolicy) -> Result<reqwest::Client> {
    ensure_crypto_provider();
    let p = Arc::new(policy.clone());
    let redirect = reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= MAX_REDIRECTS {
            attempt.error("too many redirects")
        } else if p.check(attempt.url()).is_err() {
            attempt.error("redirect to an untrusted origin")
        } else {
            attempt.follow()
        }
    });
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .redirect(redirect)
        .https_only(!policy.allow_loopback_http)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| CoreError::Network(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_policy() {
        let p = HttpPolicy::github();
        for ok in [
            "https://api.github.com/repos/storytold/photocraft/releases",
            "https://github.com/storytold/photocraft/releases/download/v0.3.0/x.zip",
            "https://release-assets.githubusercontent.com/github-production-release-asset/1/2?sig=x",
        ] {
            assert!(p.check_str(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://github.com/x",
            "https://github.com.evil.example/x",
            "https://evil.example/github.com",
            "https://user:pw@github.com/x", // privacy-scan: allow (credential-in-URL test)
            "https://github.com:8443/x",
            "file:///C:/Windows/System32/cmd.exe",
            "https://127.0.0.1/x",
            "http://127.0.0.1:8080/x",
        ] {
            assert!(p.check_str(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn loopback_policy_only_allows_loopback_http() {
        let p = HttpPolicy::loopback_for_tests();
        assert!(p.check_str("http://127.0.0.1:1234/x").is_ok());
        assert!(p.check_str("http://localhost:1234/x").is_err());
        assert!(p.check_str("https://github.com/x").is_err());
    }
}
