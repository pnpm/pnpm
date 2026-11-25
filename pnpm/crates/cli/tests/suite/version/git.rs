use super::{
    Command,
    CommandExtra,
    CommandTempCwd,
    Path,
    assert_eq,
    fs,
    git_commit_all,
    git_stdout,
    init_git,
    manifest_version,
    pacquet_recursive_version,
    pacquet_version,
    stderr_of,
    write_manifest,
    write_two_package_workspace,
};
use pnpm_testing_utils_registry::command_env::CommandTestExt;

#[track_caller]
fn assert_unclean_working_tree(dir: &Path) {
    let output = pacquet_version(dir, &["patch"]);
    let stderr = stderr_of(&output);
    assert!(!output.status.success(), "expected an unclean working tree: {output:?}");
    assert!(stderr.contains("ERR_PNPM_UNCLEAN_WORKING_TREE"), "{stderr}");
}

#[test]
fn version_leaves_excluded_files_unstaged() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0","files":["dist"]}"#);
    fs::write(workspace.join("notes.txt"), "original").expect("write notes");
    git_commit_all(&workspace, "init");
    fs::write(workspace.join("notes.txt"), "edited").expect("edit notes");
    fs::write(workspace.join("scratch.txt"), "local").expect("write scratch file");

    let output = pacquet_version(&workspace, &["patch"]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(git_stdout(&workspace, &["tag", "--list"]), "v1.0.1");
    assert_eq!(
        git_stdout(&workspace, &["show", "--format=", "--name-only", "HEAD"]),
        "package.json",
    );
    assert_eq!(git_stdout(&workspace, &["status", "--porcelain"]), "M notes.txt\n?? scratch.txt");

    git_stdout(&workspace, &["add", "notes.txt"]);
    assert_unclean_working_tree(&workspace);
    assert_eq!(manifest_version(&workspace), "1.0.1");
    drop(root);
}

#[test]
fn version_checks_excluded_files_with_packed_hardlinks() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0","files":["entry.js"]}"#);
    let source = workspace.join("source.txt");
    fs::write(&source, "original").expect("write source");
    fs::write(workspace.join(".gitignore"), "entry.js\n").expect("ignore packed hardlink in git");
    git_commit_all(&workspace, "init");
    fs::hard_link(&source, workspace.join("entry.js")).expect("link packed file");
    fs::write(source, "changed").expect("edit source");

    assert_unclean_working_tree(&workspace);
    assert_eq!(manifest_version(&workspace), "1.0.0");
    drop(root);
}

#[test]
fn version_rejects_untracked_package_files_and_changes_to_pack_rules() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);
    git_commit_all(&workspace, "init");
    for path in ["notes.txt", ".npmignore"] {
        fs::write(workspace.join(path), "*.txt").expect("write untracked file");
        assert_unclean_working_tree(&workspace);
        fs::remove_file(workspace.join(path)).expect("remove fixture file");
    }
    assert_eq!(manifest_version(&workspace), "1.0.0");
    drop(root);
}

#[test]
fn nested_package_honors_npmignore_but_still_checks_required_files() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let package = workspace.join("nested");
    fs::create_dir(&package).expect("create nested package");
    init_git(&workspace);
    write_manifest(&package, r#"{"name":"test-pkg","version":"1.0.0"}"#);
    fs::write(package.join(".npmignore"), "*.txt\n").expect("write package ignore rules");
    git_commit_all(&workspace, "init");
    fs::write(package.join("local notes.txt"), "local").expect("write excluded file");

    let output = pacquet_version(&package, &["patch"]);
    assert!(output.status.success(), "{}", stderr_of(&output));

    fs::write(package.join("README.txt"), "readme").expect("write required file");
    assert_unclean_working_tree(&package);
    assert_eq!(manifest_version(&package), "1.0.1");
    drop(root);
}

#[test]
fn version_checks_files_in_the_publish_directory_with_publish_overrides() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(
        &workspace,
        r#"{"name":"test-pkg","version":"1.0.0","publishConfig":{"directory":"dist"}}"#,
    );
    let dist = workspace.join("dist");
    fs::create_dir(&dist).expect("create publish directory");
    write_manifest(
        &dist,
        r#"{"name":"test-pkg","version":"1.0.0","files":[],"publishConfig":{"main":"entry.js"}}"#,
    );
    git_commit_all(&workspace, "init");

    fs::write(workspace.join("scratch.txt"), "local").expect("write local notes");
    let output = pacquet_version(&workspace, &["patch"]);
    assert!(output.status.success(), "{}", stderr_of(&output));

    fs::write(dist.join("entry.js"), "export {}").expect("write published entry point");
    assert_unclean_working_tree(&workspace);
    assert_eq!(manifest_version(&workspace), "1.0.1");
    drop(root);
}

#[test]
fn version_requires_a_clean_tree_when_publication_inputs_are_unknown() {
    for manifest in [
        r#"{"name":"test-pkg","version":"1.0.0","publishConfig":{"directory":"dist"}}"#,
        r#"{"name":"test-pkg","version":"1.0.0","files":["dist"],"scripts":{"prepublishOnly":"build"}}"#,
    ] {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        init_git(&workspace);
        write_manifest(&workspace, manifest);
        git_commit_all(&workspace, "init");
        fs::write(workspace.join("local.txt"), "edited").expect("write local file");
        assert_unclean_working_tree(&workspace);
        assert_eq!(manifest_version(&workspace), "1.0.0");
        drop(root);
    }
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
        .with_args(["tag", "v1.5.0"])
        .with_current_dir(&workspace)
        .status()
        .expect("tag first version");
    assert!(status.success());

    fs::write(workspace.join("new-file.txt"), "new commit").expect("write new file");
    git_commit_all(&workspace, "new commit");
    let status = Command::new("git")
        .with_args(["tag", "v2.3.4"])
        .with_current_dir(&workspace)
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
        .filter(|character| !character.is_whitespace())
        .filter(|character| *character != '│')
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
        .with_args(["tag", "v-release-2.3.4"])
        .with_current_dir(&workspace)
        .status()
        .expect("tag malformed version");
    assert!(status.success());

    let output = pacquet_version(&workspace, &["from-git", "--no-git-tag-version"]);

    assert!(!output.status.success(), "a malformed version tag must fail");
    let stderr = stderr_of(&output);
    assert!(stderr.contains("ERR_PNPM_INVALID_VERSION_FROM_GIT"), "{stderr}");
    let compact_stderr: String = stderr
        .chars()
        .filter(|character| !character.is_whitespace())
        .filter(|character| *character != '│')
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
        .with_args(["tag", "release-4.5.6"])
        .with_current_dir(&workspace)
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
        .with_args(["update-ref", "refs/tags/-1.2.3", "HEAD"])
        .with_current_dir(&workspace)
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
fn git_tag_version_false_skips_the_commit_and_tag() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);
    fs::write(workspace.join("pnpm-workspace.yaml"), "gitTagVersion: false\n")
        .expect("write workspace settings");
    git_commit_all(&workspace, "init");
    let commits_before = git_stdout(&workspace, &["rev-list", "--count", "HEAD"]);

    let output = pacquet_version(&workspace, &["patch"]);

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
    let hook_path = workspace
        .join(".git")
        .join("hooks")
        .join("pre-commit");
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
    fs::write(workspace.join("dirty.txt"), "x").expect("write file");
    git_stdout(&workspace, &["add", "dirty.txt"]);

    assert_unclean_working_tree(&workspace);

    git_commit_all(&workspace, "track file");
    fs::write(workspace.join("dirty.txt"), "edited").expect("modify tracked file");
    assert_unclean_working_tree(&workspace);

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
    fs::write(workspace.join("dirty.txt"), "x").expect("write file");
    git_stdout(&workspace, &["add", "dirty.txt"]);

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
    let hook_path = workspace
        .join(".git")
        .join("hooks")
        .join("pre-commit");
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

fn pacquet_version_with_env(
    workspace: &Path,
    args: &[&str],
    env: &[(&str, &str)],
) -> std::process::Output {
    use assert_cmd::cargo::CommandCargoExt as _;
    let mut command = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .without_ambient_pnpm_config()
        .with_arg("version")
        .with_args(args);
    for (key, value) in env {
        command = command.with_env(key, value);
    }
    command.output().expect("run pacquet version")
}

#[test]
fn tag_version_prefix_from_workspace_yaml_is_honored() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);
    fs::write(workspace.join("pnpm-workspace.yaml"), "tagVersionPrefix: \"\"\n")
        .expect("write pnpm-workspace.yaml");
    git_commit_all(&workspace, "init");

    let output = pacquet_version(&workspace, &["patch"]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(git_stdout(&workspace, &["tag", "--list"]), "1.0.1");
    drop(root);
}

#[test]
fn tag_version_prefix_from_workspace_yaml_with_a_value_is_honored() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);
    fs::write(workspace.join("pnpm-workspace.yaml"), "tagVersionPrefix: \"release-\"\n")
        .expect("write pnpm-workspace.yaml");
    git_commit_all(&workspace, "init");

    let output = pacquet_version(&workspace, &["patch"]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(git_stdout(&workspace, &["tag", "--list"]), "release-1.0.1");
    drop(root);
}

#[test]
fn tag_version_prefix_flag_overrides_workspace_yaml() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);
    fs::write(workspace.join("pnpm-workspace.yaml"), "tagVersionPrefix: \"\"\n")
        .expect("write pnpm-workspace.yaml");
    git_commit_all(&workspace, "init");

    let output = pacquet_version(&workspace, &["patch", "--tag-version-prefix", "release-"]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(git_stdout(&workspace, &["tag", "--list"]), "release-1.0.1");
    drop(root);
}

#[test]
fn tag_version_prefix_env_var_is_honored() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);
    git_commit_all(&workspace, "init");

    let output = pacquet_version_with_env(
        &workspace,
        &["patch"],
        &[("PNPM_CONFIG_TAG_VERSION_PREFIX", "rel-")],
    );

    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(git_stdout(&workspace, &["tag", "--list"]), "rel-1.0.1");
    drop(root);
}

#[test]
fn empty_tag_version_prefix_env_var_removes_the_v() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);
    git_commit_all(&workspace, "init");

    let output =
        pacquet_version_with_env(&workspace, &["patch"], &[("PNPM_CONFIG_TAG_VERSION_PREFIX", "")]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(git_stdout(&workspace, &["tag", "--list"]), "1.0.1");
    drop(root);
}

#[test]
fn from_git_uses_tag_version_prefix_from_workspace_yaml() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);
    fs::write(workspace.join("pnpm-workspace.yaml"), "tagVersionPrefix: \"release-\"\n")
        .expect("write pnpm-workspace.yaml");
    git_commit_all(&workspace, "init");

    let status = Command::new("git")
        .with_args(["tag", "release-1.5.0"])
        .with_current_dir(&workspace)
        .status()
        .expect("tag first version");
    assert!(status.success());

    let output = pacquet_version(&workspace, &["from-git", "--no-git-tag-version"]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(manifest_version(&workspace), "1.5.0");
    drop(root);
}
