//! Tag parsing and channel classification. Versions are always compared as semver,
//! never lexically (`v0.10.0` > `v0.9.0`).

use semver::Version;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Stable,
    Beta,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Beta => "beta",
        }
    }

    pub fn parse(s: &str) -> Option<Channel> {
        match s {
            "stable" => Some(Channel::Stable),
            "beta" => Some(Channel::Beta),
            _ => None,
        }
    }
}

/// Parses `tag` after stripping the app's literal `prefix`. Returns the version text exactly
/// as it appears in the tag (used to build exact asset names) and the parsed semver.
pub fn parse_tag(tag: &str, prefix: &str) -> Option<(String, Version)> {
    let text = tag.strip_prefix(prefix)?;
    let version = Version::parse(text).ok()?;
    Some((text.to_string(), version))
}

/// GitHub's `prerelease` flag is not reliable upstream (see RELEASE_AUDIT.md), so a
/// semver pre-release component also marks a release as pre-release.
pub fn is_prerelease(api_flag: bool, version: &Version) -> bool {
    api_flag || !version.pre.is_empty()
}

pub fn channel_allows(channel: Channel, prerelease: bool) -> bool {
    match channel {
        Channel::Stable => !prerelease,
        Channel::Beta => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prefixes() {
        assert_eq!(parse_tag("v0.3.0", "v").unwrap().0, "0.3.0");
        assert_eq!(
            parse_tag("artcraft-v0.41.0", "artcraft-v").unwrap().1,
            Version::new(0, 41, 0)
        );
        assert!(parse_tag("0.3.0", "v").is_none());
        assert!(parse_tag("v0.3", "v").is_none());
        assert!(parse_tag("vnext", "v").is_none());
        assert!(parse_tag("artcraft-v0.41.0", "v").is_none());
    }

    #[test]
    fn semver_not_lexical() {
        let a = parse_tag("v0.9.0", "v").unwrap().1;
        let b = parse_tag("v0.10.0", "v").unwrap().1;
        assert!(b > a);
        let rc = parse_tag("v0.1.1-rc.5", "v").unwrap().1;
        let fin = parse_tag("v0.1.1", "v").unwrap().1;
        assert!(fin > rc);
    }

    #[test]
    fn prerelease_detection_ignores_unreliable_flag() {
        let rc = parse_tag("v0.1.1-rc.5", "v").unwrap().1;
        assert!(is_prerelease(false, &rc));
        let fin = parse_tag("v0.1.1", "v").unwrap().1;
        assert!(!is_prerelease(false, &fin));
        assert!(is_prerelease(true, &fin));
        assert!(!channel_allows(Channel::Stable, true));
        assert!(channel_allows(Channel::Beta, true));
    }
}
