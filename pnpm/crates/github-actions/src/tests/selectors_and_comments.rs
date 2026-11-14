use super::{
    FakeGitRunner, RepoVersion, SHA_V4_1_0, SHA_V4_2_0, SHA_V5_0_0, SilentReporter, action,
    find_current, find_outdated_with_runner, is_selector, normalize_selector, refs,
    render_target_value, repo_versions, selector_matcher, update_with_runner, versions_from_refs,
};
use node_semver::Version;
use pnpm_resolving_git_resolver::{GitCommandRunner, GitRunError};
use std::{fs, future::Future, pin::Pin};

const SHA_V3_6_0: &str = "0000000000000000000000000000000000000360";
const SHA_V3_6_1: &str = "0000000000000000000000000000000000000361";

struct MultiMajorGitRunner;

impl GitCommandRunner for MultiMajorGitRunner {
    fn ls_remote<'a>(
        &'a self,
        _repo: &'a str,
        _ref_: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<String, GitRunError>> + Send + 'a>> {
        Box::pin(async {
            Ok(format!(
                concat!(
                    "{}\trefs/tags/v3.6.0\n",
                    "{}\trefs/tags/v3.6.1\n",
                    "{}\trefs/tags/v4.1.0\n",
                    "{}\trefs/tags/v4.2.0\n",
                    "{}\trefs/tags/v5.0.0\n",
                ),
                SHA_V3_6_0, SHA_V3_6_1, SHA_V4_1_0, SHA_V4_2_0, SHA_V5_0_0,
            ))
        })
    }
}

#[test]
fn rejects_purl_and_protocol_prefixed_selectors() {
    for selector in [
        "pkg:npm/pkg-a",
        "!pkg:npm/pkg-a",
        "PKG:CARGO/serde",
        "pkg:pypi/requests",
        "pkg://npm/lodash",
        "jsr:@std/path",
        "npm:@scope/pkg",
        "docker://alpine:3.20",
    ] {
        assert!(!is_selector(selector), "{selector} must not be treated as a GitHub Action");
    }
    assert_eq!(normalize_selector("pkg:npm/pkg-a@1.0.0"), "pkg:npm/pkg-a@1.0.0");
}

#[test]
fn updates_partial_and_non_version_comments_cleanly() {
    let target: &RepoVersion = &repo_versions()[1];
    let cases = [
        (
            format!("actions/checkout@{SHA_V4_1_0} # v4"),
            format!("actions/checkout@{SHA_V4_2_0} # v4.2.0"),
        ),
        (
            format!("actions/checkout@{SHA_V4_1_0} #v4"),
            format!("actions/checkout@{SHA_V4_2_0} # v4.2.0"),
        ),
        (
            format!("actions/checkout@{SHA_V4_1_0} # v4.1"),
            format!("actions/checkout@{SHA_V4_2_0} # v4.2.0"),
        ),
        (
            format!("actions/checkout@{SHA_V4_1_0} # 4"),
            format!("actions/checkout@{SHA_V4_2_0} # v4.2.0"),
        ),
        (
            format!("actions/checkout@{SHA_V4_1_0} # 4.1"),
            format!("actions/checkout@{SHA_V4_2_0} # v4.2.0"),
        ),
        (
            format!("actions/checkout@{SHA_V4_1_0} # 4.1.0"),
            format!("actions/checkout@{SHA_V4_2_0} # v4.2.0"),
        ),
        (
            format!("actions/checkout@{SHA_V4_1_0} # v4 — pin for CI"),
            format!("actions/checkout@{SHA_V4_2_0} # v4.2.0 — pin for CI"),
        ),
        (
            format!("actions/checkout@{SHA_V4_1_0} # pinned manually"),
            format!("actions/checkout@{SHA_V4_2_0} # v4.2.0 pinned manually"),
        ),
        (
            format!("actions/checkout@{SHA_V4_1_0} #"),
            format!("actions/checkout@{SHA_V4_2_0} # v4.2.0"),
        ),
        (
            format!("actions/checkout@{SHA_V4_1_0}\t# v4"),
            format!("actions/checkout@{SHA_V4_2_0}\t# v4.2.0"),
        ),
        (
            format!("owner/setup-v4.1.0@{SHA_V4_1_0} # v4.1.0"),
            format!("owner/setup-v4.1.0@{SHA_V4_2_0} # v4.2.0"),
        ),
    ];

    for (input, expected) in cases {
        assert_eq!(render_target_value(&action(&input), target), expected, "input: {input}");
    }
}

#[test]
fn partial_comment_version_disambiguates_shared_commit_tags() {
    let shared_sha = "4444444444444444444444444444444444444444";
    let versions = versions_from_refs(&refs(&[
        ("refs/tags/v3.9.0", shared_sha),
        ("refs/tags/v4.0.0", shared_sha),
    ]));
    let on_v3 = find_current(&action(&format!("actions/checkout@{shared_sha} # v3")), &versions)
        .expect("v3 match");
    let on_v4 = find_current(&action(&format!("actions/checkout@{shared_sha} # v4")), &versions)
        .expect("v4 match");

    assert_eq!(on_v3.version, Version::parse("3.9.0").unwrap());
    assert_eq!(on_v4.version, Version::parse("4.0.0").unwrap());
}

#[tokio::test]
async fn updates_workflow_comments_for_major_and_prose_annotations() {
    let root = tempfile::tempdir().expect("temp directory");
    let workflows = root.path().join(".github/workflows");
    fs::create_dir_all(&workflows).expect("workflow directory");
    let workflow = workflows.join("ci.yml");
    fs::write(
        &workflow,
        format!(
            concat!(
                "jobs:\n  test:\n    steps:\n",
                "      - uses: actions/checkout@{0} # v4\n",
                "      - uses: actions/checkout@{0} # v4 — pin for CI\n",
                "      - uses: actions/checkout@{0} # pinned manually\n",
            ),
            SHA_V4_1_0,
        ),
    )
    .expect("workflow");

    update_with_runner::<SilentReporter, _>(
        root.path(),
        false,
        None,
        "https://github.com",
        &FakeGitRunner,
        None,
    )
    .await
    .expect("update actions");

    assert_eq!(
        fs::read_to_string(&workflow).expect("updated workflow"),
        format!(
            concat!(
                "jobs:\n  test:\n    steps:\n",
                "      - uses: actions/checkout@{0} # v4.2.0\n",
                "      - uses: actions/checkout@{0} # v4.2.0 — pin for CI\n",
                "      - uses: actions/checkout@{0} # v4.2.0 pinned manually\n",
            ),
            SHA_V4_2_0,
        ),
    );
}

#[tokio::test]
async fn filters_subpath_actions_with_positive_negated_and_mixed_selectors() {
    let root = tempfile::tempdir().expect("temp directory");
    let workflows = root.path().join(".github/workflows");
    fs::create_dir_all(&workflows).expect("workflow directory");
    fs::write(
        workflows.join("ci.yml"),
        concat!(
            "jobs:\n  test:\n    steps:\n",
            "      - uses: actions/checkout@v4.1.0\n",
            "      - uses: actions/cache@v4.1.0\n",
            "      - uses: actions/cache/restore@v4.1.0\n",
            "      - uses: actions/cache/save@v4.1.0\n",
        ),
    )
    .expect("workflow");

    let cases: &[(&[&str], &[&str])] = &[
        (&["actions/cache"], &["actions/cache", "actions/cache/restore", "actions/cache/save"]),
        (&["actions/cache/save"], &["actions/cache/save"]),
        (&["!actions/cache"], &["actions/checkout"]),
        (&["!actions/cache/save"], &["actions/cache", "actions/cache/restore", "actions/checkout"]),
        (&["actions/cache", "!actions/cache/save"], &["actions/cache", "actions/cache/restore"]),
        (&["!actions/cache", "actions/cache/save"], &["actions/cache/save"]),
    ];

    for (selectors, expected) in cases {
        let strings = selectors
            .iter()
            .map(|selector| (*selector).to_string())
            .collect::<Vec<_>>();
        let matcher = selector_matcher(&strings);
        let outdated = find_outdated_with_runner::<SilentReporter, _>(
            root.path(),
            false,
            matcher.as_ref(),
            "https://github.com",
            &FakeGitRunner,
            None,
        )
        .await
        .expect("outdated");
        let names = outdated
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(&names, expected, "selectors: {selectors:?}");
    }
}

#[tokio::test]
async fn preserves_distinct_outdated_versions_across_workflows_and_dedupes_identical_ones() {
    let root = tempfile::tempdir().expect("temp directory");
    let workflows = root.path().join(".github/workflows");
    fs::create_dir_all(&workflows).expect("workflow directory");
    fs::write(
        workflows.join("ci.yml"),
        concat!(
            "jobs:\n  test:\n    steps:\n",
            "      - uses: actions/checkout@v4.1.0\n",
            "      - uses: actions/checkout@v4.1.0\n",
        ),
    )
    .expect("ci workflow");
    fs::write(
        workflows.join("release.yml"),
        "jobs:\n  release:\n    steps:\n      - uses: actions/checkout@v3.6.0\n",
    )
    .expect("release workflow");

    let outdated = find_outdated_with_runner::<SilentReporter, _>(
        root.path(),
        false,
        None,
        "https://github.com",
        &MultiMajorGitRunner,
        None,
    )
    .await
    .expect("outdated actions");

    let summary = outdated
        .iter()
        .map(|entry| {
            (
                entry.name.as_str(),
                entry.current.to_string(),
                entry.wanted.to_string(),
                entry.latest.to_string(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        summary,
        vec![
            ("actions/checkout", "3.6.0".to_string(), "3.6.1".to_string(), "5.0.0".to_string()),
            ("actions/checkout", "4.1.0".to_string(), "4.2.0".to_string(), "5.0.0".to_string()),
        ],
    );
}
