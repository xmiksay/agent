use anyhow::Result;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

/// Build a matcher from a list of glob patterns. Each pattern is matched
/// against the whole command string.
///
/// `*` matches across `/` (literal_separator disabled) so patterns can contain
/// path-like arguments without worrying about path semantics.
pub fn build_matcher(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        let glob = GlobBuilder::new(p).literal_separator(false).build()?;
        builder.add(glob);
    }
    Ok(builder.build()?)
}

/// Match the whole command against the allowlist. No pipeline splitting, no
/// inspection of redirection or substitution — if a pattern matches the
/// command string verbatim, it is allowed.
pub fn is_allowed(matcher: &GlobSet, command: &str) -> bool {
    matcher.is_match(command.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn star_crosses_slashes() {
        let m = build_matcher(&["glab issue view*".into()]).unwrap();
        assert!(is_allowed(
            &m,
            "glab issue view 17 --repo f13platform/infrastructure/knowledge-base"
        ));
    }

    #[test]
    fn git_checkout_glob() {
        let m = build_matcher(&["git checkout *".into()]).unwrap();
        assert!(is_allowed(&m, "git checkout -b feature/foo"));
    }

    #[test]
    fn unmatched_pattern_rejected() {
        let m = build_matcher(&["ls *".into()]).unwrap();
        assert!(!is_allowed(&m, "rm -rf /"));
    }

    #[test]
    fn redirection_passes_through_if_pattern_matches() {
        let m = build_matcher(&["ls * 2>/dev/null".into()]).unwrap();
        assert!(is_allowed(&m, "ls src/foo/ src/bar/ 2>/dev/null"));
    }

    #[test]
    fn permissive_wildcard_matches_anything() {
        let m = build_matcher(&["*".into()]).unwrap();
        assert!(is_allowed(&m, "anything goes here && also this"));
    }
}
