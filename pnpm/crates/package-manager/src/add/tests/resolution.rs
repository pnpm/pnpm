use super::{super::normalized_save_specifier, add_jsr_selector, add_npm_selector};

#[tokio::test]
async fn add_keeps_the_range_operator_a_jsr_selector_asks_for() {
    assert_eq!(add_jsr_selector("jsr:@pnpm-e2e/foo@1.0").await, Some("jsr:~1.0.0".to_string()));
}
#[tokio::test]
async fn add_saves_an_npm_selector_as_the_plain_registry_range() {
    assert_eq!(add_npm_selector("npm:foo@^1").await, Some("^1.0.0".to_string()));
}
#[test]
fn normalizes_hosted_git_specifiers_to_shortcut_form() {
    // A bare `owner/repo#committish` shorthand becomes a `github:` shortcut.
    assert_eq!(
        normalized_save_specifier("pnpm/test-git-fetch#8b333f12d5357f4f25a654c305c826294cb073bf"),
        "github:pnpm/test-git-fetch#8b333f12d5357f4f25a654c305c826294cb073bf",
    );
    // A full GitHub URL collapses to the same shortcut.
    assert_eq!(
        normalized_save_specifier("https://github.com/pnpm/test-git-fetch"),
        "github:pnpm/test-git-fetch",
    );
    // An explicit `github:` shorthand is idempotent.
    assert_eq!(
        normalized_save_specifier("github:pnpm/test-git-fetch#abc"),
        "github:pnpm/test-git-fetch#abc",
    );
    // GitLab and Bitbucket shorthands and URLs collapse to their own prefixes.
    assert_eq!(normalized_save_specifier("gitlab:owner/repo#abc"), "gitlab:owner/repo#abc");
    assert_eq!(normalized_save_specifier("https://gitlab.com/owner/repo"), "gitlab:owner/repo");
    assert_eq!(normalized_save_specifier("bitbucket:owner/repo#abc"), "bitbucket:owner/repo#abc");
    assert_eq!(
        normalized_save_specifier("https://bitbucket.org/owner/repo"),
        "bitbucket:owner/repo",
    );
    // An auth-bearing HTTPS URL is kept verbatim — the shortcut form cannot
    // carry the embedded credentials, so shortcutting would drop them.
    assert_eq!(
        normalized_save_specifier("git+https://x-access-token:tkn@github.com/foo/bar.git#abc"),
        "git+https://x-access-token:tkn@github.com/foo/bar.git#abc",
    );
    // Non-git specifiers are kept verbatim.
    assert_eq!(normalized_save_specifier("^1.2.3"), "^1.2.3");
    assert_eq!(normalized_save_specifier("npm:bar@^1"), "npm:bar@^1");
    assert_eq!(normalized_save_specifier("file:../bar"), "file:../bar");
    assert_eq!(normalized_save_specifier("workspace:*"), "workspace:*");
}

#[test]
fn git_dependency_falls_back_to_the_repository_host_identity() {
    use crate::add::aliasless::git_package_name;

    let declared = serde_json::json!({ "name": "@vercel-labs/agent-skills" });
    assert_eq!(
        git_package_name(Some(&declared), "github:vercel-labs/agent-skills").as_deref(),
        Some("@vercel-labs/agent-skills"),
    );
    assert_eq!(
        git_package_name(None, "github:anthropics/skills").as_deref(),
        Some("@anthropics/skills"),
    );
    assert_eq!(
        git_package_name(Some(&serde_json::json!({ "name": "" })), "github:anthropics/skills")
            .as_deref(),
        Some("@anthropics/skills"),
    );
    // A host with no owner and project to read leaves the name unknown.
    assert_eq!(git_package_name(None, "git+file:///tmp/skills"), None);
}

/// A synthesized name keys the manifest entry and names the directory the
/// package is linked into, so `add` refuses it unless npm would.
#[test]
fn a_synthesized_git_dependency_name_is_a_usable_alias() {
    use crate::add::aliasless::git_package_name;

    for specifier in [
        "github:anthropics/skills",
        "gitlab:group/subgroup/project",
        "bitbucket:pnpmjs/git-resolver",
    ] {
        let name = git_package_name(None, specifier)
            .unwrap_or_else(|| panic!("{specifier} synthesized no name"));
        assert!(
            pnpm_package_name::is_valid_dependency_alias(&name),
            "{specifier} synthesized the invalid name {name:?}",
        );
    }
}
