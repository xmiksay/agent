use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TriggerReason {
    Issue {
        iid: u64,
        title: String,
        description: String,
        url: String,
    },
    ReviewMR {
        iid: u64,
        title: String,
        source_branch: String,
        target_branch: String,
        url: String,
    },
    FixReview {
        iid: u64,
        title: String,
        source_branch: String,
        url: String,
        #[serde(default)]
        review_body: String,
    },
    MRComment {
        mr_iid: u64,
        comment: String,
        source_branch: String,
        url: String,
    },
    IssueComment {
        issue_iid: u64,
        comment: String,
        url: String,
    },
}

impl TriggerReason {
    pub fn event_id(&self) -> String {
        match self {
            // Hash title+description so editing the issue creates a new task,
            // but a no-op re-fire (same content twice in a row) is deduped.
            Self::Issue {
                iid,
                title,
                description,
                ..
            } => {
                format!(
                    "issue-{iid}-{}",
                    hash_str(&format!("{title}\n{description}"))
                )
            }
            Self::ReviewMR { iid, .. } => format!("review-mr-{iid}"),
            Self::FixReview {
                iid, review_body, ..
            } => {
                format!("fix-review-{iid}-{}", hash_str(review_body))
            }
            Self::MRComment {
                mr_iid, comment, ..
            } => {
                format!("mr-comment-{mr_iid}-{}", hash_str(comment))
            }
            Self::IssueComment {
                issue_iid, comment, ..
            } => {
                format!("issue-comment-{issue_iid}-{}", hash_str(comment))
            }
        }
    }

    pub fn trigger_type(&self) -> &'static str {
        match self {
            Self::Issue { .. } => "issue",
            Self::ReviewMR { .. } => "review_mr",
            Self::FixReview { .. } => "fix_review",
            Self::MRComment { .. } => "mr_comment",
            Self::IssueComment { .. } => "issue_comment",
        }
    }

    pub fn issue_iid(&self) -> Option<u64> {
        match self {
            Self::Issue { iid, .. } | Self::IssueComment { issue_iid: iid, .. } => Some(*iid),
            _ => None,
        }
    }

    pub fn pr_iid(&self) -> Option<u64> {
        match self {
            Self::ReviewMR { iid, .. }
            | Self::FixReview { iid, .. }
            | Self::MRComment { mr_iid: iid, .. } => Some(*iid),
            _ => None,
        }
    }
}

fn hash_str(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

#[derive(Debug, Deserialize)]
pub struct ClaudeOutput {
    pub result: String,
    pub session_id: String,
    pub total_cost_usd: f64,
    pub is_error: bool,
    pub num_turns: u32,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
}
