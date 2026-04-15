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
}

#[derive(Debug, Deserialize)]
pub struct MergeRequestEvent {
    pub user: User,
    pub project: Project,
    pub object_attributes: MergeRequestAttributes,
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
    pub id: u64,
    pub path_with_namespace: String,
    pub git_http_url: String,
    pub git_ssh_url: String,
    pub default_branch: String,
}

#[derive(Debug, Deserialize)]
pub struct IssueAttributes {
    pub id: u64,
    pub iid: u64,
    pub title: String,
    pub description: Option<String>,
    pub action: Option<String>,
    #[serde(default)]
    pub assignees: Vec<Assignee>,
    pub labels: Option<Vec<Label>>,
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct MergeRequestAttributes {
    pub id: u64,
    pub iid: u64,
    pub title: String,
    pub description: Option<String>,
    pub action: Option<String>,
    pub source_branch: String,
    pub target_branch: String,
    pub author_id: u64,
    pub url: String,
    #[serde(default)]
    pub reviewers: Vec<Reviewer>,
    #[serde(default)]
    pub assignees: Vec<Assignee>,
}

#[derive(Debug, Deserialize)]
pub struct NoteAttributes {
    pub id: u64,
    pub note: String,
    pub noteable_type: String,
}

#[derive(Debug, Deserialize)]
pub struct Label {
    pub title: String,
}

#[derive(Debug, Deserialize)]
pub struct Reviewer {
    pub username: String,
}

#[derive(Debug, Deserialize)]
pub struct Assignee {
    pub username: String,
}
