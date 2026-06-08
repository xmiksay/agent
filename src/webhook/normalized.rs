use serde::{Deserialize, Serialize};

use crate::project::ProviderKind;

/// Where the change lives — enough to seed a `projects` row and clone the repo.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectRef {
    pub full_name: String,    // "mygroup/myrepo"
    pub project_slug: String, // "mygroup__myrepo"
    pub ssh_url: String,      // git@host:path.git — internal SSH is configured
    pub default_branch: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NormalizedEvent {
    pub provider: ProviderKind,
    pub project: ProjectRef,
    pub actor: String, // username of whoever triggered the event
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
    Issue {
        iid: u64,
        source_branch: Option<String>,
        #[serde(default)]
        assignees: Vec<String>,
    },
    PullRequest {
        iid: u64,
        source_branch: String,
        #[serde(default)]
        author: Option<String>,
        #[serde(default)]
        reviewers: Vec<String>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    IssueAssigned {
        iid: u64,
        assignees: Vec<String>,
        #[serde(default)]
        labels: Vec<String>,
        title: String,
        body: String,
        url: String,
    },
    IssueUpdated {
        iid: u64,
        assignees: Vec<String>,
        #[serde(default)]
        labels: Vec<String>,
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
        reviewers: Vec<String>,
        author: Option<String>,
    },
    ReviewRequested {
        iid: u64,
        source_branch: String,
        target_branch: String,
        url: String,
        reviewers: Vec<String>,
        title: String,
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
