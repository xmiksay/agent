use serde::{Deserialize, Serialize};

use crate::project::ProviderKind;

/// Where the change lives — enough to seed a `projects` row and clone the repo.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectRef {
    pub full_name: String,           // "mygroup/myrepo"
    pub project_slug: String,        // "mygroup__myrepo"
    pub ssh_url: String,             // git@host:path.git — internal SSH is configured
    pub default_branch: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NormalizedEvent {
    pub provider: ProviderKind,
    pub project: ProjectRef,
    pub actor: String,               // username of whoever triggered the event
    pub kind: EventKind,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    Approved,
    ChangesRequested,
    Commented,
    Other,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NoteTargetRef {
    Issue { iid: u64, source_branch: Option<String> },
    PullRequest { iid: u64, source_branch: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    IssueAssigned {
        iid: u64,
        assignees: Vec<String>,
        title: String,
        body: String,
        url: String,
    },
    IssueUpdated {
        iid: u64,
        assignees: Vec<String>,
        title: String,
        body: String,
        url: String,
    },
    IssueClosed {
        iid: u64,
        url: String,
    },
    PrReviewSubmitted {
        iid: u64,
        source_branch: String,
        target_branch: String,
        review_body: String,
        state: ReviewState,
        url: String,
    },
    PrClosed {
        iid: u64,
        source_branch: String,
        url: String,
    },
    NoteAdded {
        target: NoteTargetRef,
        body: String,
        url: String,
    },
}

impl EventKind {
    /// Stable string for dedup keys.
    pub fn dedup_key(&self) -> String {
        match self {
            EventKind::IssueAssigned { iid, .. } => format!("issue-assigned-{iid}"),
            EventKind::IssueUpdated { iid, .. } => format!("issue-updated-{iid}"),
            EventKind::IssueClosed { iid, .. } => format!("issue-closed-{iid}"),
            EventKind::PrReviewSubmitted { iid, .. } => format!("pr-review-{iid}"),
            EventKind::PrClosed { iid, .. } => format!("pr-closed-{iid}"),
            EventKind::NoteAdded { target, body, .. } => {
                let h = hash_str(body);
                match target {
                    NoteTargetRef::Issue { iid, .. } => format!("note-issue-{iid}-{h}"),
                    NoteTargetRef::PullRequest { iid, .. } => format!("note-pr-{iid}-{h}"),
                }
            }
        }
    }
}

fn hash_str(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}
