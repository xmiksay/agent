use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "object_kind")]
#[serde(rename_all = "snake_case")]
pub enum GitLabEvent {
    Issue(IssueEvent),
    MergeRequest(MergeRequestEvent),
    Note(NoteEvent),
}

#[derive(Debug, Deserialize)]
pub struct IssueEvent {
    pub user: User,
    pub project: Project,
    pub object_attributes: IssueAttributes,
    /// GitLab puts assignees at the top level (object_attributes only has
    /// `assignee_ids`).
    #[serde(default)]
    pub assignees: Vec<Assignee>,
}

#[derive(Debug, Deserialize)]
pub struct MergeRequestEvent {
    pub user: User,
    pub project: Project,
    pub object_attributes: MergeRequestAttributes,
    #[serde(default)]
    pub reviewers: Vec<Reviewer>,
}

#[derive(Debug, Deserialize)]
pub struct NoteEvent {
    pub user: User,
    pub project: Project,
    pub object_attributes: NoteAttributes,
    pub merge_request: Option<MergeRequestAttributes>,
    pub issue: Option<IssueAttributes>,
}

#[derive(Debug, Deserialize)]
pub struct User {
    pub username: String,
}

#[derive(Debug, Deserialize)]
pub struct Project {
    pub path_with_namespace: String,
    pub git_ssh_url: String,
    pub default_branch: String,
}

#[derive(Debug, Deserialize)]
pub struct IssueAttributes {
    pub iid: u64,
    pub title: String,
    pub description: Option<String>,
    pub action: Option<String>,
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct MergeRequestAttributes {
    pub iid: u64,
    pub title: String,
    pub action: Option<String>,
    pub source_branch: String,
    pub target_branch: String,
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct NoteAttributes {
    pub note: String,
    pub noteable_type: String,
}

#[derive(Debug, Deserialize)]
pub struct Reviewer {
    pub username: String,
}

#[derive(Debug, Deserialize)]
pub struct Assignee {
    pub username: String,
}
