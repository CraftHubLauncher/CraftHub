pub mod github;
pub mod resolver;
pub mod version;

pub use github::{FetchOutcome, FetchSource, GhAsset, GhRelease, GithubClient, GithubConfig};
pub use resolver::{ReleaseSummary, ResolvedAsset, ResolvedRelease};
pub use version::Channel;
