use super::{
    ActionReference, RepoVersion, find_current, find_outdated_with_runner, is_selector,
    parse_repo_versions, render_target_ref, render_target_value, split_uses_value,
    update_with_runner,
};
use node_semver::Version;
use pacquet_resolving_git_resolver::{GitCommandRunner, GitRunError};
use std::{fs, future::Future, path::PathBuf, pin::Pin};

const SHA_V4_1_0: &str = "1111111111111111111111111111111111111111";
const SHA_V4_2_0: &str = "2222222222222222222222222222222222222222";
const SHA_V5_0_0: &str = "3333333333333333333333333333333333333333";

fn repo_versions() -> Vec<RepoVersion> {
    parse_repo_versions(&format!(
        "{SHA_V4_1_0}\trefs/tags/v4.1.0\n{SHA_V4_2_0}\trefs/tags/v4.2.0\n{SHA_V5_0_0}\trefs/tags/v5.0.0\n",
    ))
}

fn action(original_value: &str) -> ActionReference {
    let (value, comment) = split_uses_value(original_value);
    let (name, ref_) = value.rsplit_once('@').expect("action reference");
    ActionReference {
        comment_version: comment
            .and_then(|comment| comment.split_whitespace().next())
            .map(str::to_string),
        file: PathBuf::from("workflow.yml"),
        flow_style: false,
        indentation: String::new(),
        name: name.to_string(),
        original_value: original_value.to_string(),
        range: 0..original_value.len(),
        ref_: ref_.to_string(),
        repo: "actions/checkout".to_string(),
    }
}

struct FakeGitRunner;

impl GitCommandRunner for FakeGitRunner {
    fn ls_remote<'a>(
        &'a self,
        _repo: &'a str,
        _ref_: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<String, GitRunError>> + Send + 'a>> {
        Box::pin(async {
            Ok(format!(
                "{SHA_V4_1_0}\trefs/tags/v4.1.0\n{SHA_V4_2_0}\trefs/tags/v4.2.0\n{SHA_V5_0_0}\trefs/tags/v5.0.0\n",
            ))
        })
    }
}

#[test]
fn distinguishes_action_selectors_from_package_selectors() {
    assert_eq!(
        [
            is_selector("actions/checkout"),
            is_selector("@scope/package"),
            is_selector("!@scope/package"),
            is_selector("typescript"),
        ],
        [true, false, false, false],
    );
}

#[test]
fn parses_annotated_semver_tags() {
    let versions = parse_repo_versions(&format!(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\trefs/tags/v4.2.0\n{SHA_V4_2_0}\trefs/tags/v4.2.0^{{}}\nnot-a-version\trefs/tags/latest\n",
    ));
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].commit, SHA_V4_2_0);
    assert_eq!(versions[0].tag, "v4.2.0");
}

#[test]
fn preserves_quoting_and_updates_sha_comment() {
    let action = action(&format!("'actions/checkout@{SHA_V4_1_0}' # v4.1.0 keep pinned"));
    let target = &repo_versions()[1];
    assert_eq!(
        render_target_value(&action, target),
        format!("'actions/checkout@{SHA_V4_2_0}' # v4.2.0 keep pinned"),
    );
}

#[test]
fn floating_major_resolves_to_an_exact_commit() {
    let action = action("actions/checkout@v4");
    let versions = repo_versions();
    let current = find_current(&action, &versions).expect("current version");
    assert_eq!(current.version, Version::parse("4.2.0").unwrap());
    assert_eq!(render_target_ref(&versions[1]), SHA_V4_2_0);
    assert_eq!(render_target_ref(&versions[2]), SHA_V5_0_0);
}

#[test]
fn resolves_prerelease_tags_containing_dots() {
    let versions = parse_repo_versions(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\trefs/tags/v5.0.0-alpha.1\nbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\trefs/tags/v5.0.0-alpha.2\n",
    );
    let current = find_current(&action("actions/checkout@v5.0.0-alpha.1"), &versions)
        .expect("prerelease version");

    assert_eq!(current.version, Version::parse("5.0.0-alpha.1").unwrap());
}

#[tokio::test]
async fn updates_workflow_files_without_reformatting_them() {
    let root = tempfile::tempdir().expect("temp directory");
    let workflows = root.path().join(".github/workflows");
    fs::create_dir_all(&workflows).expect("workflow directory");
    let workflow = workflows.join("ci.yml");
    let source = format!(
        "name: CI\n\njobs:\n  test:\n    strategy: {{ matrix: {{ node: [22, 24] }} }}\n    steps:\n      - run: |\n          uses: actions/checkout@{SHA_V4_1_0} # not a dependency\n      - name: nested input\n        with:\n          uses: actions/checkout@v4\n      - uses: 'actions/checkout@{SHA_V4_1_0}' # v4.1.0 keep pinned\n      - uses: actions/checkout@v4\n  flow:\n    steps: [{{ uses: actions/checkout@v4 }}]\n",
    );
    fs::write(&workflow, &source).expect("workflow");

    update_with_runner(root.path(), false, None, &FakeGitRunner).await.expect("update actions");

    assert_eq!(
        fs::read_to_string(workflow).expect("updated workflow"),
        format!(
            "name: CI\n\njobs:\n  test:\n    strategy: {{ matrix: {{ node: [22, 24] }} }}\n    steps:\n      - run: |\n          uses: actions/checkout@{SHA_V4_1_0} # not a dependency\n      - name: nested input\n        with:\n          uses: actions/checkout@v4\n      - uses: 'actions/checkout@{SHA_V4_2_0}' # v4.2.0 keep pinned\n      - uses: actions/checkout@{SHA_V4_2_0} # v4.2.0\n  flow:\n    steps: [{{ uses: actions/checkout@{SHA_V4_2_0} # v4.2.0\n                    }}]\n",
        ),
    );
}

#[tokio::test]
async fn compatible_outdated_uses_the_current_major() {
    let root = tempfile::tempdir().expect("temp directory");
    let workflows = root.path().join(".github/workflows");
    fs::create_dir_all(&workflows).expect("workflow directory");
    fs::write(
        workflows.join("ci.yml"),
        format!(
            "jobs:\n  test:\n    steps:\n      - uses: actions/checkout@{SHA_V4_1_0} # v4.1.0\n",
        ),
    )
    .expect("workflow");

    let default = find_outdated_with_runner(root.path(), false, None, &FakeGitRunner)
        .await
        .expect("default outdated");
    let compatible = find_outdated_with_runner(root.path(), true, None, &FakeGitRunner)
        .await
        .expect("compatible outdated");

    assert_eq!(default[0].latest, Version::parse("5.0.0").unwrap());
    assert_eq!(compatible[0].latest, Version::parse("4.2.0").unwrap());
}

#[cfg(unix)]
#[tokio::test]
async fn rejects_workflow_symlinks_outside_the_project() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("project directory");
    let outside = tempfile::tempdir().expect("outside directory");
    let original = "jobs:\n  test:\n    steps:\n      - uses: actions/checkout@v4\n";
    fs::write(outside.path().join("ci.yml"), original).expect("outside workflow");
    fs::create_dir_all(root.path().join(".github")).expect("GitHub directory");
    symlink(outside.path(), root.path().join(".github/workflows")).expect("workflow symlink");

    let Err(error) = update_with_runner(root.path(), false, None, &FakeGitRunner).await else {
        panic!("outside workflow must be rejected");
    };

    assert!(error.to_string().contains("outside the project root"));
    assert_eq!(fs::read_to_string(outside.path().join("ci.yml")).unwrap(), original);
}

#[tokio::test]
async fn does_not_mutate_an_external_hardlink_target() {
    let root = tempfile::tempdir().expect("project directory");
    let outside = tempfile::tempdir().expect("outside directory");
    let original = "jobs:\n  test:\n    steps:\n      - uses: actions/checkout@v4\n";
    let outside_workflow = outside.path().join("ci.yml");
    let workflow = root.path().join(".github/workflows/ci.yml");
    fs::write(&outside_workflow, original).expect("outside workflow");
    fs::create_dir_all(workflow.parent().unwrap()).expect("workflow directory");
    fs::hard_link(&outside_workflow, &workflow).expect("workflow hardlink");

    update_with_runner(root.path(), false, None, &FakeGitRunner).await.expect("update actions");

    assert!(
        fs::read_to_string(&workflow)
            .expect("updated workflow")
            .contains(&format!("uses: actions/checkout@{SHA_V4_2_0} # v4.2.0")),
    );
    assert_eq!(fs::read_to_string(outside_workflow).unwrap(), original);
}
