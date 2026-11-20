use super::{
    super::{
        install::same_build,
        normalized_save_specifier,
    },
    add_jsr_selector,
    add_npm_selector,
};

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

#[test]
fn test_is_same_source() {
    use crate::add::source::is_same_source;
    assert!(is_same_source("github:foo-org/skills#main", "github:foo-org/skills#v1", "skills"));
    assert!(!is_same_source("github:foo-org/skills", "github:vercel-labs/skills", "skills"));
    assert!(is_same_source("gist:abc123", "https://gist.github.com/foo/abc123", "abc123"));
    assert!(is_same_source("^1.0.0", "^2.0.0", "express"));
    assert!(!is_same_source("^1.0.0", "github:vercel-labs/skills", "skills"));
    assert!(is_same_source("npm:foo@1.0.0", "npm:foo@2.0.0", "my-foo"));
    assert!(!is_same_source("npm:foo@1.0.0", "npm:bar@1.0.0", "my-foo"));
    assert!(is_same_source("npm:@scope/foo@1.0.0", "npm:@scope/foo@2.0.0", "my-foo"));
    assert!(!is_same_source("npm:@scope/foo@1.0.0", "npm:@scope/bar@1.0.0", "my-foo"));
    assert!(is_same_source(
        "git+https://git.example.com/repo.git#main",
        "git+https://git.example.com/repo.git#v1",
        "repo"
    ));
    assert!(!is_same_source("file:../foo", "file:../bar", "foo"));
    assert!(is_same_source("file:../foo", "file:../foo", "foo"));
    assert!(is_same_source("workspace:*", "workspace:^1.0.0", "my-pkg"));
    assert!(is_same_source("catalog:default", "catalog:default", "my-pkg"));
    assert!(!is_same_source("catalog:foo", "catalog:bar", "my-pkg"));
    assert!(is_same_source("https://example.com/a.tgz#1", "https://example.com/a.tgz#2", "my-pkg"));
    assert!(!is_same_source("https://example.com/a.tgz", "https://example.com/b.tgz", "my-pkg"));
}

#[test]
fn test_collect_dependency_warnings() {
    use crate::add::source::collect_dependency_warnings;
    use pnpm_reporter::{
        LogEvent,
        LogLevel,
        PnpmLog,
    };

    let catalog_warning = LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Warn,
        message: "catalog mismatch".to_string(),
        prefix: "/root".to_string(),
    });

    // Both catalog warning and source warning
    let warnings = collect_dependency_warnings(
        Some(catalog_warning),
        Some("^1.0.0"),
        "github:user/repo",
        "foo",
        "/root",
    );
    assert_eq!(warnings.len(), 2);

    // No source warning when source is same
    let warnings_same = collect_dependency_warnings(None, Some("^1.0.0"), "^2.0.0", "foo", "/root");
    assert_eq!(warnings_same.len(), 0);
}

#[test]
fn a_locked_build_matches_only_the_same_build() {
    let version = |text: &str| text.parse::<node_semver::Version>().unwrap();
    assert!(same_build(&version("1.0.0"), &version("1.0.0")));
    assert!(same_build(&version("1.0.0+foo"), &version("1.0.0+foo")));
    assert!(!same_build(&version("1.0.0+bar"), &version("1.0.0+foo")));
    assert!(!same_build(&version("1.0.0"), &version("1.0.0+foo")));
}

/// `pnpm add file:<publish directory>` saves the name the install resolves:
/// the publishing project's, found within the workspace root, or the lockfile
/// root outside a workspace.
#[tokio::test]
async fn add_names_a_publish_directory_after_its_project() {
    let tmp = tempfile::tempdir().expect("create a temporary directory");
    let workspace = tmp.path();
    let project = workspace.join("lib");
    std::fs::create_dir_all(project.join("dist")).expect("create the publish directory");
    std::fs::write(
        project.join("package.json"),
        r#"{"name":"lib","version":"1.0.0","publishConfig":{"directory":"dist"}}"#,
    )
    .expect("write the project manifest");
    std::fs::write(project.join("dist/package.json"), r#"{"name":"built","version":"1.0.0"}"#)
        .expect("write the built manifest");
    let consumer = workspace.join("app");
    std::fs::create_dir_all(&consumer).expect("create the consumer");
    let manifest = pnpm_package_manifest::PackageManifest::from_value(
        consumer.join("package.json"),
        serde_json::json!({ "name": "app" }),
    );
    let name = async |workspace_dir| {
        crate::add::aliasless::resolve_aliasless_local("file:../lib/dist", &manifest, workspace_dir)
            .await
            .expect("resolve the selector")
            .expect("a local selector")
            .package_name
    };
    assert_eq!(name(Some(workspace)).await, "lib");
    assert_eq!(name(None).await, "built", "without a root the consumer bounds the search");
}

/// Outside a workspace, an explicit lockfile root bounds the search as it
/// bounds the install's.
#[test]
fn add_bounds_the_publish_search_by_the_lockfile_root_outside_a_workspace() {
    let manifest = pnpm_package_manifest::PackageManifest::from_value(
        std::path::PathBuf::from("/repo/app/package.json"),
        serde_json::json!({ "name": "app" }),
    );
    let mut config = pnpm_config::Config::new();
    config.lockfile_dir = Some(std::path::PathBuf::from("/repo"));
    assert_eq!(
        crate::add::aliasless::publish_search_root(&config, &manifest),
        std::path::Path::new("/repo"),
    );
    config.workspace_dir = Some(std::path::PathBuf::from("/ws"));
    assert_eq!(
        crate::add::aliasless::publish_search_root(&config, &manifest),
        std::path::Path::new("/ws"),
    );
}
