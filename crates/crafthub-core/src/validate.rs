//! Input validation shared by the catalog, IPC layer and archive extractor.

/// Application identifiers: lowercase ASCII, digits and '-', 2..=32 chars, starting with a letter.
pub fn is_valid_app_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    (2..=32).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// A plain lowercase slug such as a repository name or asset stem.
pub fn is_valid_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !s.starts_with('-')
        && !s.ends_with('-')
}

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "CONIN$",
    "CONOUT$",
];

/// Validates a single path component so it is safe and unambiguous on Windows (and POSIX).
/// Rejects separators, traversal, drive/stream syntax, reserved device names, control
/// characters, trailing dots/spaces (which Windows silently strips) and overlong names.
pub fn is_safe_component(c: &str) -> bool {
    if c.is_empty() || c.len() > 255 || c == "." || c == ".." {
        return false;
    }
    if c.ends_with('.') || c.ends_with(' ') || c.starts_with(' ') {
        return false;
    }
    if c.chars().any(|ch| {
        ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
    }) {
        return false;
    }
    let stem = c.split('.').next().unwrap_or(c).trim_end();
    !RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem))
}

/// Validates a relative path using '/' separators (as stored in ZIP files and manifests).
pub fn is_safe_relative_path(p: &str) -> bool {
    !p.is_empty() && p.len() <= 1024 && !p.starts_with('/') && p.split('/').all(is_safe_component)
}

/// A filename of the form `name.exe` with no directory components.
pub fn is_valid_exe_name(s: &str) -> bool {
    s.len() > 4
        && s.ends_with(".exe")
        && is_safe_component(s)
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// Validates a version directory name derived from a semver string.
pub fn is_valid_version_label(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        && is_safe_component(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_ids() {
        assert!(is_valid_app_id("photocraft"));
        assert!(is_valid_app_id("pdf-craft2"));
        assert!(!is_valid_app_id("PhotoCraft"));
        assert!(!is_valid_app_id("../etc"));
        assert!(!is_valid_app_id("a"));
        assert!(!is_valid_app_id("1abc"));
        assert!(!is_valid_app_id("photo craft"));
        assert!(!is_valid_app_id(&"a".repeat(33)));
    }

    #[test]
    fn components() {
        for ok in [
            "photocraft.exe",
            "README.md",
            "OFL-biz-ud-mincho.txt",
            "a b",
        ] {
            assert!(is_safe_component(ok), "{ok}");
        }
        for bad in [
            "", ".", "..", "a/b", "a\\b", "C:", "con", "CON.txt", "nul.exe", "lpt1", "evil.",
            "evil ", " evil", "a:stream", "a\u{0}b", "a?b", "a*b", "a|b", "COM1.log",
        ] {
            assert!(!is_safe_component(bad), "{bad:?}");
        }
    }

    #[test]
    fn relative_paths() {
        assert!(is_safe_relative_path("root/photocraft.exe"));
        assert!(is_safe_relative_path("root/sub/file.txt"));
        for bad in [
            "/abs",
            "root/../x",
            "root//x",
            "root/./x",
            "C:/x",
            "\\\\server\\share",
            "root\\x",
            "",
        ] {
            assert!(!is_safe_relative_path(bad), "{bad:?}");
        }
    }

    #[test]
    fn exe_names() {
        assert!(is_valid_exe_name("photocraft.exe"));
        assert!(!is_valid_exe_name("photocraft.bat"));
        assert!(!is_valid_exe_name("../photocraft.exe"));
        assert!(!is_valid_exe_name("sub/photocraft.exe"));
        assert!(!is_valid_exe_name(".exe"));
    }
}
