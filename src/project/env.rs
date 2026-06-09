//! A project's `.env` text is a minijinja template: it is rendered with the
//! task's runtime variables (branch, url, …) and then parsed into individual
//! variables, ready to be injected into the agent process at spawn.

use anyhow::{Context, Result};
use minijinja::Environment;
use serde::Serialize;
use tracing::warn;
use uuid::Uuid;

use crate::project::store::ProjectStore;

/// Runtime values exposed to the `.env` template as `{{ branch }}` etc.
#[derive(Debug, Serialize)]
pub struct EnvContext {
    pub branch: String,
    pub default_branch: String,
    /// Remote (SSH) URL of the repository.
    pub url: String,
    /// Project full name, e.g. `group/repo`.
    pub project: String,
    /// Git service slug the project belongs to.
    pub service: String,
    pub task_id: String,
}

/// Render the `.env` template with `ctx`, then parse it into `(key, value)`
/// pairs. Returns an error only if the template itself is malformed.
pub fn build_env_vars(template: &str, ctx: &EnvContext) -> Result<Vec<(String, String)>> {
    let rendered = render(template, ctx)?;
    Ok(parse_env_file(&rendered))
}

fn render(template: &str, ctx: &EnvContext) -> Result<String> {
    let mut env = Environment::new();
    env.add_template("env", template)
        .context("invalid env template")?;
    env.get_template("env")
        .expect("template just added")
        .render(ctx)
        .context("rendering env template")
}

/// Parse `.env` text into `(key, value)` pairs. Blank lines and `#` comments are
/// skipped; an optional leading `export ` and surrounding quotes on the value
/// are stripped. Lines without `=` or with an empty key are ignored.
pub fn parse_env_file(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let line = line.strip_prefix("export ").unwrap_or(line);
            let (key, value) = line.split_once('=')?;
            let key = key.trim();
            if key.is_empty() {
                return None;
            }
            let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
            Some((key.to_string(), value.to_string()))
        })
        .collect()
}

impl ProjectStore {
    /// Render a project's `env_file` template into spawn `(key, value)` pairs. A
    /// missing project or a malformed template yields an empty set (the error is
    /// logged), so a bad env never blocks the run.
    pub async fn spawn_env(&self, project_id: Uuid, ctx: &EnvContext) -> Vec<(String, String)> {
        let Ok(Some(pc)) = self.get_project_by_id(project_id).await else {
            return Vec::new();
        };
        match build_env_vars(&pc.env_file, ctx) {
            Ok(pairs) => pairs,
            Err(e) => {
                warn!(%project_id, error = %e, "skipping project env: template error");
                Vec::new()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> EnvContext {
        EnvContext {
            branch: "42-fix-login".into(),
            default_branch: "master".into(),
            url: "git@host:group/repo.git".into(),
            project: "group/repo".into(),
            service: "gh".into(),
            task_id: "abc-123".into(),
        }
    }

    #[test]
    fn parses_basic_pairs() {
        let pairs = parse_env_file("API_KEY=secret\nPORT=8080");
        assert_eq!(
            pairs,
            vec![
                ("API_KEY".into(), "secret".into()),
                ("PORT".into(), "8080".into()),
            ]
        );
    }

    #[test]
    fn skips_comments_and_blanks() {
        let pairs = parse_env_file("# a comment\n\n  \nFOO=bar\n");
        assert_eq!(pairs, vec![("FOO".into(), "bar".into())]);
    }

    #[test]
    fn strips_export_and_quotes() {
        let pairs = parse_env_file("export TOKEN=\"abc\"\nNAME='john doe'");
        assert_eq!(
            pairs,
            vec![
                ("TOKEN".into(), "abc".into()),
                ("NAME".into(), "john doe".into()),
            ]
        );
    }

    #[test]
    fn keeps_value_with_equals_sign() {
        let pairs = parse_env_file("DSN=postgres://u:p@h/db?x=1");
        assert_eq!(
            pairs,
            vec![("DSN".into(), "postgres://u:p@h/db?x=1".into())]
        );
    }

    #[test]
    fn ignores_malformed_lines() {
        let pairs = parse_env_file("no_equals_here\n=novalue\nGOOD=1");
        assert_eq!(pairs, vec![("GOOD".into(), "1".into())]);
    }

    #[test]
    fn renders_runtime_variables() {
        let pairs = build_env_vars(
            "BRANCH={{ branch }}\nREPO={{ url }}\nDEPLOY={{ project }}-{{ branch }}",
            &ctx(),
        )
        .unwrap();
        assert_eq!(
            pairs,
            vec![
                ("BRANCH".into(), "42-fix-login".into()),
                ("REPO".into(), "git@host:group/repo.git".into()),
                ("DEPLOY".into(), "group/repo-42-fix-login".into()),
            ]
        );
    }

    #[test]
    fn supports_conditionals() {
        let tmpl = "{% if branch != default_branch %}FEATURE=1{% endif %}";
        let pairs = build_env_vars(tmpl, &ctx()).unwrap();
        assert_eq!(pairs, vec![("FEATURE".into(), "1".into())]);
    }

    #[test]
    fn malformed_template_errors() {
        assert!(build_env_vars("{{ unclosed", &ctx()).is_err());
    }
}
