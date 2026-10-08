use serde::Serialize;

/// All failures surfaced by the core engine. Messages are written for end users;
/// `kind()` gives the UI a stable machine-readable category.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("Unknown application '{0}'.")]
    UnknownApp(String),
    #[error("{0}")]
    Unsupported(String),
    #[error("GitHub API rate limit reached. {0}")]
    RateLimited(String),
    #[error("Network error: {0}")]
    Network(String),
    #[error("GitHub returned HTTP {status}: {message}")]
    Api { status: u16, message: String },
    #[error("Blocked URL outside the trusted GitHub origins: {0}")]
    UntrustedUrl(String),
    #[error("Integrity check failed: {0}")]
    Integrity(String),
    #[error("Unsafe archive rejected: {0}")]
    UnsafeArchive(String),
    #[error("Archive layout does not match the audited layout: {0}")]
    Layout(String),
    #[error("{0} is running. Close it (and save your work) before continuing.")]
    AppRunning(String),
    #[error("Another operation is already in progress for {0}.")]
    Busy(String),
    #[error("Operation cancelled.")]
    Cancelled,
    #[error("Not enough free disk space: {needed} bytes needed, {available} bytes available.")]
    DiskSpace { needed: u64, available: u64 },
    #[error("{0} is not installed.")]
    NotInstalled(String),
    #[error("{0}")]
    AlreadyInstalled(String),
    #[error("Path safety check failed: {0}")]
    PathSafety(String),
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
    #[error("Database error: {0}")]
    Db(String),
    #[error("Could not start the application: {0}")]
    Launch(String),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("Catalog error: {0}")]
    Catalog(String),
    #[error("Release not found: {0}")]
    ReleaseNotFound(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

impl CoreError {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::UnknownApp(_) => "unknownApp",
            Self::Unsupported(_) => "unsupported",
            Self::RateLimited(_) => "rateLimited",
            Self::Network(_) => "network",
            Self::Api { .. } => "api",
            Self::UntrustedUrl(_) => "untrustedUrl",
            Self::Integrity(_) => "integrity",
            Self::UnsafeArchive(_) => "unsafeArchive",
            Self::Layout(_) => "layout",
            Self::AppRunning(_) => "appRunning",
            Self::Busy(_) => "busy",
            Self::Cancelled => "cancelled",
            Self::DiskSpace { .. } => "diskSpace",
            Self::NotInstalled(_) => "notInstalled",
            Self::AlreadyInstalled(_) => "alreadyInstalled",
            Self::PathSafety(_) => "pathSafety",
            Self::Io { .. } => "io",
            Self::Db(_) => "db",
            Self::Launch(_) => "launch",
            Self::InvalidInput(_) => "invalidInput",
            Self::Catalog(_) => "catalog",
            Self::ReleaseNotFound(_) => "releaseNotFound",
            Self::Internal(_) => "internal",
        }
    }

    pub fn io(context: impl Into<String>) -> impl FnOnce(std::io::Error) -> CoreError {
        let context = context.into();
        move |source| CoreError::Io { context, source }
    }
}

impl From<rusqlite::Error> for CoreError {
    fn from(e: rusqlite::Error) -> Self {
        CoreError::Db(e.to_string())
    }
}

impl From<reqwest::Error> for CoreError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_redirect() {
            return CoreError::UntrustedUrl(format!("redirect rejected ({e})"));
        }
        if e.is_timeout() {
            return CoreError::Network("the request timed out".into());
        }
        // Never include URLs with query strings (signed asset URLs) in messages.
        CoreError::Network(e.without_url().to_string())
    }
}

/// Serializable error shape returned over IPC.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ErrorPayload {
    pub kind: String,
    pub message: String,
}

impl From<&CoreError> for ErrorPayload {
    fn from(e: &CoreError) -> Self {
        ErrorPayload {
            kind: e.kind().to_string(),
            message: e.to_string(),
        }
    }
}

impl Serialize for CoreError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        ErrorPayload::from(self).serialize(s)
    }
}

pub type Result<T, E = CoreError> = std::result::Result<T, E>;
