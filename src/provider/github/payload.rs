//! GitHub webhook JSON shapes — minimal subset.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Repository {
    pub full_name: String,
    pub ssh_url: String,
    pub default_branch: String,
}

#[derive(Debug, Deserialize)]
pub struct User {
    pub login: String,
}

#[derive(Debug, Deserialize)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    pub html_url: String,
    #[serde(default)]
    pub assignees: Vec<User>,
}

#[derive(Debug, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub html_url: String,
    pub head: PrRef,
    pub base: PrRef,
}

#[derive(Debug, Deserialize)]
pub struct PrRef {
    #[serde(rename = "ref")]
    pub branch: String,
}

#[derive(Debug, Deserialize)]
pub struct Review {
    pub state: String,
    #[serde(default)]
    pub body: Option<String>,
    pub html_url: String,
}

#[derive(Debug, Deserialize)]
pub struct Comment {
    #[serde(default)]
    pub body: Option<String>,
    pub html_url: String,
}

#[derive(Debug, Deserialize)]
pub struct IssuesEvent {
    pub action: String,
    pub issue: Issue,
    pub repository: Repository,
    pub sender: User,
}

#[derive(Debug, Deserialize)]
pub struct PullRequestEvent {
    pub action: String,
    pub pull_request: PullRequest,
    pub repository: Repository,
    pub sender: User,
}

#[derive(Debug, Deserialize)]
pub struct PullRequestReviewEvent {
    pub action: String,
    pub pull_request: PullRequest,
    pub review: Review,
    pub repository: Repository,
    pub sender: User,
}

#[derive(Debug, Deserialize)]
pub struct IssueCommentEvent {
    pub action: String,
    pub issue: Issue,
    pub comment: Comment,
    pub repository: Repository,
    pub sender: User,
    #[serde(default)]
    pub pull_request: Option<serde_json::Value>, // presence flag only
}
