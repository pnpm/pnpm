use super::{
    Command, CommandTempCwd, assert_eq, fs, git_commit_all, git_stdout, init_git, manifest_version,
    pacquet_recursive_version, pacquet_version, stderr_of, write_manifest,
    write_two_package_workspace,
};

#[test]
fn git_commit_and_tag_are_created_by_default() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);
    git_commit_all(&workspace, "init");

    let output = pacquet_version(&workspace, &["patch"]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(git_stdout(&workspace, &["tag", "--list"]), "v1.0.1");
    assert_eq!(git_stdout(&workspace, &["log", "-1", "--pretty=%s"]), "1.0.1");
    drop(root);
}

#[test]
fn tag_version_prefix_replaces_the_default_v() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);
    git_commit_all(&workspace, "init");

    let output = pacquet_version(&workspace, &["patch", "--tag-version-prefix", "release-"]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(git_stdout(&workspace, &["tag", "--list"]), "release-1.0.1");
    drop(root);
}

#[test]
fn from_git_sets_the_version_from_the_latest_matching_tag() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);
    git_commit_all(&workspace, "init");

    let status = Command::new("git")
        .args(["tag", "v1.5.0"])
        .current_dir(&workspace)
        .status()
        .expect("tag first version");
    assert!(status.success());

    fs::write(workspace.join("new-file.txt"), "new commit").expect("write new file");
    git_commit_all(&workspace, "new commit");
    let status = Command::new("git")
        .args(["tag", "v2.3.4"])
        .current_dir(&workspace)
        .status()
        .expect("tag latest version");
    assert!(status.success());

    let output = pacquet_version(&workspace, &["from-git", "--no-git-tag-version"]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(manifest_version(&workspace), "2.3.4");
    drop(root);
}

#[test]
fn from_git_fails_when_no_matching_tag_exists() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);
    git_commit_all(&workspace, "init");

    let output = pacquet_version(&workspace, &["from-git", "--no-git-tag-version"]);

    assert!(!output.status.success(), "from-git without a matching tag must fail");
    let stderr = stderr_of(&output);
    assert!(stderr.contains("ERR_PNPM_INVALID_VERSION_FROM_GIT"), "{stderr}");
    let compact_stderr: String = stderr
        .chars()
        .filter(|character| !character.is_whitespace() && *character != '│')
        .collect();
    assert!(compact_stderr.contains(r#"usingtagprefix"v":nomatchingGittagfound"#), "{stderr}");
    drop(root);
}

#[test]
fn from_git_rejects_a_malformed_version_tag() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);
    git_commit_all(&workspace, "init");
    let status = Command::new("git")
        .args(["tag", "v-release-2.3.4"])
        .current_dir(&workspace)
        .status()
        .expect("tag malformed version");
    assert!(status.success());

    let output = pacquet_version(&workspace, &["from-git", "--no-git-tag-version"]);

    assert!(!output.status.success(), "a malformed version tag must fail");
    let stderr = stderr_of(&output);
    assert!(stderr.contains("ERR_PNPM_INVALID_VERSION_FROM_GIT"), "{stderr}");
    let compact_stderr: String = stderr
        .chars()
        .filter(|character| !character.is_whitespace() && *character != '│')
        .collect();
    assert!(
        compact_stderr.contains(r#"usingtagprefix"v":tagisnotavalidversion:"v-release-2.3.4""#),
        "{stderr}",
    );
    drop(root);
}

#[test]
fn from_git_respects_tag_version_prefix() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);
    git_commit_all(&workspace, "init");
    let status = Command::new("git")
        .args(["tag", "release-4.5.6"])
        .current_dir(&workspace)
        .status()
        .expect("tag custom prefix version");
    assert!(status.success());

    let output = pacquet_version(
        &workspace,
        &["from-git", "--tag-version-prefix", "release-", "--no-git-tag-version"],
    );

    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(manifest_version(&workspace), "4.5.6");
    drop(root);
}

#[test]
fn from_git_handles_tag_starting_with_dash() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);
    git_commit_all(&workspace, "init");
    let status = Command::new("git")
        .args(["update-ref", "refs/tags/-1.2.3", "HEAD"])
        .current_dir(&workspace)
        .status()
        .expect("create tag starting with dash");
    assert!(status.success());

    let output = pacquet_version(
        &workspace,
        &["from-git", "--tag-version-prefix", "-", "--no-git-tag-version"],
    );

    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(manifest_version(&workspace), "1.2.3");
    drop(root);
}

#[test]
fn message_substitutes_the_new_version() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);
    git_commit_all(&workspace, "init");

    let output = pacquet_version(&workspace, &["patch", "--message", "chore: release %s"]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(git_stdout(&workspace, &["log", "-1", "--pretty=%s"]), "chore: release 1.0.1");
    drop(root);
}

#[test]
fn no_git_tag_version_skips_the_commit_and_tag() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);
    git_commit_all(&workspace, "init");
    let commits_before = git_stdout(&workspace, &["rev-list", "--count", "HEAD"]);

    let output = pacquet_version(&workspace, &["0.0.0", "--no-git-tag-version"]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(git_stdout(&workspace, &["tag", "--list"]), "");
    assert_eq!(git_stdout(&workspace, &["rev-list", "--count", "HEAD"]), commits_before);
    drop(root);
}

#[test]
fn allow_same_version_still_tags_via_an_empty_commit() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);
    git_commit_all(&workspace, "init");

    let output = pacquet_version(&workspace, &["1.0.0", "--allow-same-version"]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(git_stdout(&workspace, &["tag", "--list"]), "v1.0.0");
    assert_eq!(git_stdout(&workspace, &["log", "-1", "--pretty=%s"]), "1.0.0");
    drop(root);
}

#[cfg(unix)]
#[test]
fn no_commit_hooks_bypasses_a_failing_pre_commit_hook() {
    use std::os::unix::fs::PermissionsExt as _;

    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);
    git_commit_all(&workspace, "init");
    let hook_path = workspace.join(".git").join("hooks").join("pre-commit");
    fs::write(&hook_path, "#!/bin/sh\nexit 1\n").expect("write pre-commit hook");
    fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755))
        .expect("mark hook executable");

    let output = pacquet_version(&workspace, &["patch", "--no-commit-hooks"]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(git_stdout(&workspace, &["tag", "--list"]), "v1.0.1");
    drop(root);
}

#[test]
fn unclean_working_tree_fails_unless_git_checks_are_disabled() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);
    git_commit_all(&workspace, "init");
    fs::write(workspace.join("dirty.txt"), "x").expect("dirty the tree");

    let output = pacquet_version(&workspace, &["patch"]);
    assert!(!output.status.success(), "an unclean tree must fail");
    let stderr = stderr_of(&output);
    assert!(stderr.contains("ERR_PNPM_UNCLEAN_WORKING_TREE"), "{stderr}");

    let output = pacquet_version(&workspace, &["patch", "--no-git-checks", "--no-git-tag-version"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(manifest_version(&workspace), "1.0.1");
    drop(root);
}

#[test]
fn recursive_mode_skips_the_commit_and_tag() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let (pkg_a, _) = write_two_package_workspace(&workspace);
    init_git(&workspace);
    git_commit_all(&workspace, "init");
    let commits_before = git_stdout(&workspace, &["rev-list", "--count", "HEAD"]);

    let output = pacquet_recursive_version(&workspace, &["-r", "version", "patch"]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(manifest_version(&pkg_a), "1.0.1");
    assert_eq!(git_stdout(&workspace, &["tag", "--list"]), "");
    assert_eq!(git_stdout(&workspace, &["rev-list", "--count", "HEAD"]), commits_before);
    drop(root);
}

#[test]
fn dry_run_skips_the_git_checks_the_lifecycle_scripts_and_the_commit() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let log_script = concat!(
        r#"node -e "require('fs').appendFileSync('lifecycle.log',"#,
        r#" process.env.npm_lifecycle_event + '\n')""#,
    );
    write_manifest(
        &workspace,
        &serde_json::json!({
            "name": "test-pkg",
            "version": "1.0.0",
            "scripts": {
                "preversion": log_script,
                "version": log_script,
                "postversion": log_script,
            },
        })
        .to_string(),
    );
    init_git(&workspace);
    git_commit_all(&workspace, "init");
    let commits_before = git_stdout(&workspace, &["rev-list", "--count", "HEAD"]);
    fs::write(workspace.join("dirty.txt"), "x").expect("dirty the tree");

    let output = pacquet_version(&workspace, &["patch", "--dry-run"]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    assert!(!workspace.join("lifecycle.log").exists(), "lifecycle scripts must not run");
    assert_eq!(git_stdout(&workspace, &["tag", "--list"]), "");
    assert_eq!(git_stdout(&workspace, &["rev-list", "--count", "HEAD"]), commits_before);
    drop(root);
}

#[cfg(unix)]
#[test]
fn a_failing_git_commit_surfaces_the_git_error() {
    use std::os::unix::fs::PermissionsExt as _;

    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);
    git_commit_all(&workspace, "init");
    let hook_path = workspace.join(".git").join("hooks").join("pre-commit");
    fs::write(&hook_path, "#!/bin/sh\necho refused by hook >&2\nexit 1\n")
        .expect("write pre-commit hook");
    fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755))
        .expect("mark hook executable");

    // Without --no-commit-hooks the failing hook fails the commit, and the
    // command reports the git failure instead of swallowing it.
    let output = pacquet_version(&workspace, &["patch"]);

    assert!(!output.status.success(), "a failing git commit must fail the command");
    let stderr = stderr_of(&output);
    assert!(stderr.contains("git commit"), "{stderr}");
    assert!(stderr.contains("refused by hook"), "{stderr}");
    assert_eq!(git_stdout(&workspace, &["tag", "--list"]), "", "no tag after a failed commit");
    drop(root);
}

#[test]
fn an_empty_tag_version_prefix_removes_the_v() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);
    git_commit_all(&workspace, "init");

    let output = pacquet_version(&workspace, &["patch", "--tag-version-prefix", ""]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(git_stdout(&workspace, &["tag", "--list"]), "1.0.1");
    drop(root);
}
