/// Convert a hierarchical identifier (with `/`) into a safe filesystem slug.
///
/// - lowercases ASCII letters
/// - `/` → `__`
/// - any char outside `[a-z0-9_-]` is replaced with `-`
/// - collapses repeated `-`
pub fn slugify(s: &str) -> String {
    let lowered = s.to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    for c in lowered.chars() {
        match c {
            '/' => out.push_str("__"),
            'a'..='z' | '0'..='9' | '_' | '-' => out.push(c),
            _ => {
                if !out.ends_with('-') {
                    out.push('-');
                }
            }
        }
    }
    // Trim trailing single `-` so slug doesn't end ugly.
    while out.ends_with('-') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_slug() {
        assert_eq!(slugify("mygroup/myrepo"), "mygroup__myrepo");
        assert_eq!(slugify("MyGroup/MyRepo"), "mygroup__myrepo");
        assert_eq!(slugify("acme-corp/site"), "acme-corp__site");
    }

    #[test]
    fn branch_slug() {
        assert_eq!(slugify("feature/foo"), "feature__foo");
        assert_eq!(slugify("bugfix/JIRA-123"), "bugfix__jira-123");
        assert_eq!(slugify("release/2026.06"), "release__2026-06");
        assert_eq!(slugify("hotfix!"), "hotfix");
    }
}
