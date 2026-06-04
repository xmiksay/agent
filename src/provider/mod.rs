pub mod github;
pub mod gitlab;
pub mod registry;

use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

pub use registry::ProviderRegistry;

use crate::project::ProviderKind;

/// Where a comment should be posted.
#[derive(Copy, Clone, Debug)]
pub enum NoteTarget {
    /// Comment on an issue (GitLab: issues; GitHub: issues).
    Issue(u64),
    /// Comment on a merge request / pull request.
    MergeRequest(u64),
}

#[async_trait]
pub trait GitProvider: Send + Sync {
    fn kind(&self) -> ProviderKind;

    /// Which `git_services` row this client was built from.
    fn service_id(&self) -> Uuid;

    /// Post a markdown comment on the given target.
    async fn post_note(&self, project_path: &str, target: NoteTarget, body: &str) -> Result<()>;
}
