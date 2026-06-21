//! Decide whether a [`NormalizedEvent`] should start a fresh task, and with what
//! [`TriggerReason`]. The reattach-to-existing-task path lives in `dispatch`; this
//! module owns only the start-gates for new tasks.

use std::collections::BTreeMap;

use tracing::info;

use crate::jobs::types::TriggerReason;
use crate::service::{TriggerConfig, TriggerMode};
use crate::webhook::normalized::{EventKind, NormalizedEvent, NoteTargetRef};

/// The effective config for `trigger_type`: the per-type override row if present,
/// else "enabled with the service-level defaults".
fn resolve_cfg(
    cfgs: &BTreeMap<String, TriggerConfig>,
    trigger_type: &str,
    default_mode: TriggerMode,
    default_label: &str,
) -> TriggerConfig {
    cfgs.get(trigger_type)
        .cloned()
        .unwrap_or_else(|| TriggerConfig {
            enabled: true,
            mode: default_mode,
            label: default_label.to_string(),
        })
}

pub(super) fn build_trigger(
    ev: &NormalizedEvent,
    my_username: &str,
    trigger_mode: TriggerMode,
    trigger_label: &str,
    trigger_cfgs: &BTreeMap<String, TriggerConfig>,
) -> Option<TriggerReason> {
    match &ev.kind {
        EventKind::IssueAssigned {
            iid,
            assignees,
            labels,
            title,
            body,
            url,
        }
        | EventKind::IssueUpdated {
            iid,
            assignees,
            labels,
            title,
            body,
            url,
        } => {
            let kind = match &ev.kind {
                EventKind::IssueAssigned { .. } => "assigned",
                _ => "updated",
            };
            // Per-type override of the service-level mode/label default.
            let cfg = resolve_cfg(trigger_cfgs, "issue", trigger_mode, trigger_label);
            if !cfg.enabled {
                info!(iid, kind, "issue trigger disabled for this service");
                return None;
            }
            let by_assignee = matches!(cfg.mode, TriggerMode::Assignee | TriggerMode::Both)
                && assignees.iter().any(|a| a == my_username);
            let by_label = matches!(cfg.mode, TriggerMode::Label | TriggerMode::Both)
                && !cfg.label.is_empty()
                && labels.iter().any(|l| l == &cfg.label);
            let matched = by_assignee || by_label;
            info!(
                iid,
                kind,
                bot = %my_username,
                mode = cfg.mode.as_str(),
                label = %cfg.label,
                assignees = ?assignees,
                labels = ?labels,
                matched,
                title = %title,
                "issue event"
            );
            if !matched {
                return None;
            }
            Some(TriggerReason::Issue {
                iid: *iid,
                title: title.clone(),
                description: body.clone(),
                url: url.clone(),
            })
        }
        EventKind::IssueClosed { .. } => None, // lifecycle in Phase 7
        EventKind::PrReviewSubmitted {
            iid,
            source_branch,
            state,
            review_body,
            url,
            author,
            ..
        } => {
            use crate::webhook::normalized::ReviewState;
            if !resolve_cfg(trigger_cfgs, "fix_review", trigger_mode, trigger_label).enabled {
                info!(iid, "fix_review trigger disabled for this service");
                return None;
            }
            // FixReview is the "fix comments on my own MR" workflow, so the gate
            // is "bot authored the MR". GitHub gives us the author directly.
            // GitLab webhooks only expose `author_id` (numeric); the bot doesn't
            // submit reviews via post_note, so there's no echo to guard against
            // and we trust an unknown author as the bot's MR.
            let is_my_mr = match author {
                Some(a) => a == my_username,
                None => true,
            };
            if !is_my_mr {
                info!(iid, bot = %my_username, author = ?author, actor = %ev.actor, "review event on PR not authored by bot, skipping");
                return None;
            }
            // A bare approval with no body has nothing to address.
            if matches!(state, ReviewState::Approved) && review_body.trim().is_empty() {
                return None;
            }
            Some(TriggerReason::FixReview {
                iid: *iid,
                title: String::new(),
                source_branch: source_branch.clone(),
                url: url.clone(),
                review_body: review_body.clone(),
            })
        }
        EventKind::ReviewRequested {
            iid,
            source_branch,
            target_branch,
            url,
            reviewers,
            title,
        } => {
            if !resolve_cfg(trigger_cfgs, "review_mr", trigger_mode, trigger_label).enabled {
                info!(iid, "review_mr trigger disabled for this service");
                return None;
            }
            if !reviewers.iter().any(|r| r == my_username) {
                return None;
            }
            Some(TriggerReason::ReviewMR {
                iid: *iid,
                title: title.clone(),
                source_branch: source_branch.clone(),
                target_branch: target_branch.clone(),
                url: url.clone(),
            })
        }
        EventKind::PrClosed { .. } => None, // lifecycle in Phase 7
        EventKind::NoteAdded { target, body, url } => {
            let mention = format!("@{my_username}");
            let mentioned = body.contains(&mention);
            match target {
                NoteTargetRef::PullRequest {
                    iid,
                    source_branch,
                    author,
                    reviewers,
                } => {
                    // Mentions always count. Otherwise only act if the bot owns
                    // (authored) the MR or is a reviewer on it. The bot's own
                    // posts are stamped with BOT_NOTE_MARKER and filtered at
                    // source in normalize_note, so we don't need an actor-based
                    // loop guard here — and we trust an unknown author as the
                    // bot's MR (matters for shared-account GitLab setups).
                    let is_my_mr = match author {
                        Some(a) => a == my_username,
                        None => true,
                    };
                    let is_reviewer = reviewers.iter().any(|r| r == my_username);
                    if !mentioned && !is_my_mr && !is_reviewer {
                        return None;
                    }
                    if !resolve_cfg(trigger_cfgs, "mr_comment", trigger_mode, trigger_label).enabled
                    {
                        info!(iid, "mr_comment trigger disabled for this service");
                        return None;
                    }
                    Some(TriggerReason::MRComment {
                        mr_iid: *iid,
                        comment: body.clone(),
                        source_branch: source_branch.clone(),
                        url: url.clone(),
                    })
                }
                NoteTargetRef::Issue { iid, assignees, .. } => {
                    // Bot reacts to comments on issues it is assigned to.
                    // GitLab can't expose assignee usernames in the note payload;
                    // there we still require an @-mention of the bot.
                    let is_my_issue = assignees.iter().any(|a| a == my_username);
                    if !mentioned && !is_my_issue {
                        return None;
                    }
                    if !resolve_cfg(trigger_cfgs, "issue_comment", trigger_mode, trigger_label)
                        .enabled
                    {
                        info!(iid, "issue_comment trigger disabled for this service");
                        return None;
                    }
                    Some(TriggerReason::IssueComment {
                        issue_iid: *iid,
                        comment: body.clone(),
                        url: url.clone(),
                    })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProviderKind;
    use crate::webhook::normalized::ProjectRef;

    fn issue_event(assignees: Vec<&str>, labels: Vec<&str>) -> NormalizedEvent {
        NormalizedEvent {
            provider: ProviderKind::Github,
            project: ProjectRef {
                full_name: "acme/repo".into(),
                project_slug: "acme__repo".into(),
                remote_url: "git@host:acme/repo.git".into(),
                default_branch: "main".into(),
            },
            actor: "someone".into(),
            kind: EventKind::IssueAssigned {
                iid: 7,
                assignees: assignees.into_iter().map(String::from).collect(),
                labels: labels.into_iter().map(String::from).collect(),
                title: "T".into(),
                body: "B".into(),
                url: "u".into(),
            },
        }
    }

    /// No per-type overrides — every trigger uses the service-level defaults.
    fn no_cfgs() -> BTreeMap<String, TriggerConfig> {
        BTreeMap::new()
    }

    #[test]
    fn label_mode_matches_via_label_not_assignee() {
        let ev = issue_event(vec!["other"], vec!["agent"]);
        // Label mode ignores assignees entirely; the watched label fires it.
        assert!(build_trigger(&ev, "bot", TriggerMode::Label, "agent", &no_cfgs()).is_some());
    }

    #[test]
    fn label_mode_ignores_assignee_match() {
        let ev = issue_event(vec!["bot"], vec!["something-else"]);
        // Bot is assigned but mode is label-only and the label doesn't match.
        assert!(build_trigger(&ev, "bot", TriggerMode::Label, "agent", &no_cfgs()).is_none());
    }

    #[test]
    fn label_mode_with_empty_label_never_matches() {
        let ev = issue_event(vec!["bot"], vec!["agent"]);
        assert!(build_trigger(&ev, "bot", TriggerMode::Label, "", &no_cfgs()).is_none());
    }

    #[test]
    fn assignee_mode_matches_via_assignee_not_label() {
        let ev = issue_event(vec!["bot"], vec!["agent"]);
        // Assignee mode ignores labels; assignment fires it.
        assert!(build_trigger(&ev, "bot", TriggerMode::Assignee, "agent", &no_cfgs()).is_some());
    }

    #[test]
    fn assignee_mode_ignores_label_match() {
        let ev = issue_event(vec!["other"], vec!["agent"]);
        // The watched label is present but mode is assignee-only.
        assert!(build_trigger(&ev, "bot", TriggerMode::Assignee, "agent", &no_cfgs()).is_none());
    }

    #[test]
    fn both_mode_matches_on_either() {
        let by_label = issue_event(vec!["other"], vec!["agent"]);
        let by_assignee = issue_event(vec!["bot"], vec!["x"]);
        let neither = issue_event(vec!["other"], vec!["x"]);
        assert!(build_trigger(&by_label, "bot", TriggerMode::Both, "agent", &no_cfgs()).is_some());
        assert!(
            build_trigger(&by_assignee, "bot", TriggerMode::Both, "agent", &no_cfgs()).is_some()
        );
        assert!(build_trigger(&neither, "bot", TriggerMode::Both, "agent", &no_cfgs()).is_none());
    }

    #[test]
    fn disabled_issue_trigger_returns_none() {
        // Bot is assigned and mode would match, but the per-type row disables it.
        let ev = issue_event(vec!["bot"], vec!["agent"]);
        let mut cfgs = BTreeMap::new();
        cfgs.insert(
            "issue".to_string(),
            TriggerConfig {
                enabled: false,
                mode: TriggerMode::Assignee,
                label: String::new(),
            },
        );
        assert!(build_trigger(&ev, "bot", TriggerMode::Assignee, "agent", &cfgs).is_none());
    }

    #[test]
    fn per_type_override_mode_matches() {
        // Service default is assignee, but the per-type row switches to label;
        // the watched label fires even though the bot isn't assigned.
        let ev = issue_event(vec!["other"], vec!["agent"]);
        let mut cfgs = BTreeMap::new();
        cfgs.insert(
            "issue".to_string(),
            TriggerConfig {
                enabled: true,
                mode: TriggerMode::Label,
                label: "agent".to_string(),
            },
        );
        assert!(build_trigger(&ev, "bot", TriggerMode::Assignee, "", &cfgs).is_some());
    }

    // --- Full ingestion path: raw provider payload → TriggerReason ---------
    //
    // These exercise the real normalizers (github::parse / gitlab::normalize)
    // feeding build_trigger, so a regression in payload field mapping is caught
    // end-to-end rather than only against hand-built NormalizedEvents.

    use crate::webhook::types::GitLabEvent;

    /// GitHub: raw webhook body of `event_type` → TriggerReason for `bot`.
    fn gh(
        event_type: &str,
        payload: serde_json::Value,
        mode: TriggerMode,
    ) -> Option<TriggerReason> {
        let ev = crate::webhook::github::parse(event_type, payload.to_string().as_bytes())
            .unwrap()
            .unwrap();
        build_trigger(&ev, "bot", mode, "agent", &no_cfgs())
    }

    /// GitLab: raw webhook body → TriggerReason for `bot`.
    fn gl(payload: serde_json::Value, mode: TriggerMode) -> Option<TriggerReason> {
        let event: GitLabEvent = serde_json::from_value(payload).unwrap();
        let ev = crate::webhook::gitlab::normalize(&event).unwrap();
        build_trigger(&ev, "bot", mode, "agent", &no_cfgs())
    }

    fn gh_repo() -> serde_json::Value {
        serde_json::json!({
            "full_name": "acme/repo",
            "ssh_url": "git@github.com:acme/repo.git",
            "default_branch": "main"
        })
    }

    fn gl_project() -> serde_json::Value {
        serde_json::json!({
            "path_with_namespace": "acme/repo",
            "git_ssh_url": "git@gitlab.com:acme/repo.git",
            "default_branch": "main"
        })
    }

    #[test]
    fn github_issue_opened_normalizes_to_issue_trigger() {
        let payload = serde_json::json!({
            "action": "opened",
            "issue": {
                "number": 42,
                "title": "Fix the bug",
                "body": "It is broken",
                "html_url": "https://github.com/acme/repo/issues/42",
                "assignees": [{ "login": "bot" }],
                "labels": []
            },
            "repository": gh_repo(),
            "sender": { "login": "operator" }
        });
        match gh("issues", payload, TriggerMode::Assignee) {
            Some(TriggerReason::Issue {
                iid,
                title,
                description,
                url,
            }) => {
                assert_eq!(iid, 42);
                assert_eq!(title, "Fix the bug");
                assert_eq!(description, "It is broken");
                assert_eq!(url, "https://github.com/acme/repo/issues/42");
            }
            other => panic!("expected Issue, got {other:?}"),
        }
    }

    #[test]
    fn github_review_requested_normalizes_to_review_mr() {
        let payload = serde_json::json!({
            "action": "review_requested",
            "pull_request": {
                "number": 7,
                "title": "Add feature",
                "html_url": "https://github.com/acme/repo/pull/7",
                "head": { "ref": "feature-x" },
                "base": { "ref": "main" },
                "user": { "login": "operator" },
                "requested_reviewers": [{ "login": "bot" }]
            },
            "repository": gh_repo(),
            "sender": { "login": "operator" }
        });
        match gh("pull_request", payload, TriggerMode::Assignee) {
            Some(TriggerReason::ReviewMR {
                iid,
                source_branch,
                target_branch,
                ..
            }) => {
                assert_eq!(iid, 7);
                assert_eq!(source_branch, "feature-x");
                assert_eq!(target_branch, "main");
            }
            other => panic!("expected ReviewMR, got {other:?}"),
        }
    }

    #[test]
    fn github_pr_comment_normalizes_to_mr_comment() {
        let payload = serde_json::json!({
            "action": "created",
            "issue": {
                "number": 7,
                "title": "Add feature",
                "html_url": "https://github.com/acme/repo/pull/7",
                "assignees": [],
                "labels": []
            },
            "comment": {
                "body": "hey @bot please rebase",
                "html_url": "https://github.com/acme/repo/pull/7#issuecomment-1"
            },
            "repository": gh_repo(),
            "sender": { "login": "operator" },
            "pull_request": { "url": "https://api.github.com/.../pulls/7" }
        });
        match gh("issue_comment", payload, TriggerMode::Assignee) {
            Some(TriggerReason::MRComment {
                mr_iid, comment, ..
            }) => {
                assert_eq!(mr_iid, 7);
                assert_eq!(comment, "hey @bot please rebase");
            }
            other => panic!("expected MRComment, got {other:?}"),
        }
    }

    #[test]
    fn github_issue_comment_normalizes_to_issue_comment() {
        let payload = serde_json::json!({
            "action": "created",
            "issue": {
                "number": 13,
                "title": "Question",
                "html_url": "https://github.com/acme/repo/issues/13",
                "assignees": [],
                "labels": []
            },
            "comment": {
                "body": "@bot can you take a look?",
                "html_url": "https://github.com/acme/repo/issues/13#issuecomment-2"
            },
            "repository": gh_repo(),
            "sender": { "login": "operator" }
        });
        match gh("issue_comment", payload, TriggerMode::Assignee) {
            Some(TriggerReason::IssueComment {
                issue_iid, comment, ..
            }) => {
                assert_eq!(issue_iid, 13);
                assert_eq!(comment, "@bot can you take a look?");
            }
            other => panic!("expected IssueComment, got {other:?}"),
        }
    }

    #[test]
    fn gitlab_issue_opened_normalizes_to_issue_trigger() {
        let payload = serde_json::json!({
            "object_kind": "issue",
            "user": { "username": "operator" },
            "project": gl_project(),
            "object_attributes": {
                "iid": 5,
                "title": "Fix the bug",
                "description": "It is broken",
                "action": "open",
                "url": "https://gitlab.com/acme/repo/-/issues/5"
            },
            "assignees": [{ "username": "bot" }],
            "labels": []
        });
        match gl(payload, TriggerMode::Assignee) {
            Some(TriggerReason::Issue {
                iid,
                title,
                description,
                url,
            }) => {
                assert_eq!(iid, 5);
                assert_eq!(title, "Fix the bug");
                assert_eq!(description, "It is broken");
                assert_eq!(url, "https://gitlab.com/acme/repo/-/issues/5");
            }
            other => panic!("expected Issue, got {other:?}"),
        }
    }

    #[test]
    fn gitlab_merge_request_opened_normalizes_to_review_mr() {
        let payload = serde_json::json!({
            "object_kind": "merge_request",
            "user": { "username": "operator" },
            "project": gl_project(),
            "object_attributes": {
                "iid": 9,
                "title": "Add feature",
                "action": "open",
                "source_branch": "feature-x",
                "target_branch": "main",
                "url": "https://gitlab.com/acme/repo/-/merge_requests/9"
            },
            "reviewers": [{ "username": "bot" }]
        });
        match gl(payload, TriggerMode::Assignee) {
            Some(TriggerReason::ReviewMR {
                iid,
                source_branch,
                target_branch,
                ..
            }) => {
                assert_eq!(iid, 9);
                assert_eq!(source_branch, "feature-x");
                assert_eq!(target_branch, "main");
            }
            other => panic!("expected ReviewMR, got {other:?}"),
        }
    }

    #[test]
    fn gitlab_mr_note_normalizes_to_mr_comment() {
        let payload = serde_json::json!({
            "object_kind": "note",
            "user": { "username": "operator" },
            "project": gl_project(),
            "object_attributes": {
                "note": "please rebase",
                "noteable_type": "MergeRequest"
            },
            "merge_request": {
                "iid": 9,
                "title": "Add feature",
                "source_branch": "feature-x",
                "target_branch": "main",
                "url": "https://gitlab.com/acme/repo/-/merge_requests/9"
            }
        });
        match gl(payload, TriggerMode::Assignee) {
            Some(TriggerReason::MRComment {
                mr_iid,
                comment,
                source_branch,
                ..
            }) => {
                assert_eq!(mr_iid, 9);
                assert_eq!(comment, "please rebase");
                assert_eq!(source_branch, "feature-x");
            }
            other => panic!("expected MRComment, got {other:?}"),
        }
    }

    #[test]
    fn gitlab_issue_note_normalizes_to_issue_comment() {
        // GitLab issue notes carry no assignee usernames, so an @-mention is
        // what gates the trigger.
        let payload = serde_json::json!({
            "object_kind": "note",
            "user": { "username": "operator" },
            "project": gl_project(),
            "object_attributes": {
                "note": "@bot can you take a look?",
                "noteable_type": "Issue"
            },
            "issue": {
                "iid": 5,
                "title": "Question",
                "url": "https://gitlab.com/acme/repo/-/issues/5"
            }
        });
        match gl(payload, TriggerMode::Assignee) {
            Some(TriggerReason::IssueComment {
                issue_iid, comment, ..
            }) => {
                assert_eq!(issue_iid, 5);
                assert_eq!(comment, "@bot can you take a look?");
            }
            other => panic!("expected IssueComment, got {other:?}"),
        }
    }
}
