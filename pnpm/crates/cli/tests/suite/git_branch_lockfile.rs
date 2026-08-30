use crate::_utils;
pub use _utils::*;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path};

fn set_branch(workspace: &Path, branch: &str) {
    let git_dir = workspace.join(".git");
    fs::create_dir_all(&git_dir).expect("create .git");
    fs::write(git_dir.join("HEAD"), format!("ref: refs/heads/{branch}\n"))
        .expect("write .git/HEAD");
}

fn write_dependencies(workspace: &Path, dependencies: &serde_json::Value) {
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": dependencies }).to_string(),
    )
    .expect("write package.json");
}

#[test]
fn git_branch_lockfile_writes_the_lockfile_of_the_current_branch() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    set_branch(&workspace, "feature/Login");
    append_workspace_yaml_key(&workspace, "gitBranchLockfile", true);
    write_dependencies(&workspace, &serde_json::json!({ "@pnpm.e2e/foo": "1.0.0" }));

    pacquet.with_arg("install").assert().success();

    assert!(
        workspace.join("pnpm-lock.feature!login.yaml").exists(),
        "the branch lockfile is written under the sanitized branch name",
    );
    assert!(
        !workspace.join("pnpm-lock.yaml").exists(),
        "the shared lockfile is left for the branches that have no lockfile of their own",
    );

    drop((root, mock_instance));
}

/// A branch installs for the first time against whatever the shared
/// lockfile already resolved, rather than from nothing.
#[test]
fn a_branch_without_a_lockfile_starts_from_the_shared_one() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    set_branch(&workspace, "main");
    write_dependencies(&workspace, &serde_json::json!({ "@pnpm.e2e/foo": "1.0.0" }));
    pacquet.with_arg("install").assert().success();
    let shared = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(shared.contains("@pnpm.e2e/foo@1.0.0"));

    set_branch(&workspace, "other");
    append_workspace_yaml_key(&workspace, "gitBranchLockfile", true);
    write_dependencies(
        &workspace,
        &serde_json::json!({ "@pnpm.e2e/foo": "1.0.0", "@pnpm.e2e/bar": "100.0.0" }),
    );
    pacquet_in(&workspace).with_arg("install").assert().success();

    let branch_lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.other.yaml")).expect("read branch lockfile");
    assert!(branch_lockfile.contains("@pnpm.e2e/foo@1.0.0"), "{branch_lockfile}");
    assert!(branch_lockfile.contains("@pnpm.e2e/bar@100.0.0"), "{branch_lockfile}");
    assert_eq!(
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("re-read pnpm-lock.yaml"),
        shared,
        "the branch install leaves the shared lockfile alone",
    );

    drop((root, mock_instance));
}

#[test]
fn merging_folds_the_branch_lockfiles_into_the_shared_one_and_deletes_them() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    set_branch(&workspace, "main");
    write_dependencies(&workspace, &serde_json::json!({ "@pnpm.e2e/foo": "1.0.0" }));
    pacquet.with_arg("install").assert().success();

    set_branch(&workspace, "other");
    append_workspace_yaml_key(&workspace, "gitBranchLockfile", true);
    write_dependencies(
        &workspace,
        &serde_json::json!({ "@pnpm.e2e/foo": "1.0.0", "@pnpm.e2e/bar": "100.0.0" }),
    );
    pacquet_in(&workspace).with_arg("install").assert().success();
    assert!(workspace.join("pnpm-lock.other.yaml").exists());

    set_branch(&workspace, "main");
    pacquet_in(&workspace)
        .with_args(["install", "--merge-git-branch-lockfiles"])
        .assert()
        .success();

    assert!(
        !workspace.join("pnpm-lock.other.yaml").exists(),
        "the merged branch lockfiles are deleted",
    );
    let shared = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(shared.contains("@pnpm.e2e/bar@100.0.0"), "{shared}");

    drop((root, mock_instance));
}

/// The branch pattern saves the mainline branches from passing
/// `--merge-git-branch-lockfiles` by hand.
#[test]
fn the_branch_pattern_merges_without_the_flag() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    set_branch(&workspace, "main");
    write_dependencies(&workspace, &serde_json::json!({ "@pnpm.e2e/foo": "1.0.0" }));
    pacquet.with_arg("install").assert().success();

    set_branch(&workspace, "other");
    append_workspace_yaml_key(&workspace, "gitBranchLockfile", true);
    write_dependencies(
        &workspace,
        &serde_json::json!({ "@pnpm.e2e/foo": "1.0.0", "@pnpm.e2e/bar": "100.0.0" }),
    );
    pacquet_in(&workspace).with_arg("install").assert().success();

    set_branch(&workspace, "main");
    append_workspace_yaml_key(&workspace, "mergeGitBranchLockfilesBranchPattern", "[main]");
    pacquet_in(&workspace).with_arg("install").assert().success();

    assert!(!workspace.join("pnpm-lock.other.yaml").exists());
    let shared = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(shared.contains("@pnpm.e2e/bar@100.0.0"), "{shared}");

    drop((root, mock_instance));
}

/// Merging is what makes the branch lockfiles disposable, and it needs a
/// lockfile to merge into. With lockfile handling off nothing reads them,
/// so deleting them would drop resolutions no file is left holding.
#[test]
fn merging_keeps_the_branch_lockfiles_when_lockfiles_are_disabled() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    set_branch(&workspace, "other");
    append_workspace_yaml_key(&workspace, "gitBranchLockfile", true);
    write_dependencies(&workspace, &serde_json::json!({ "@pnpm.e2e/foo": "1.0.0" }));
    pacquet.with_arg("install").assert().success();
    let branch_lockfile = workspace.join("pnpm-lock.other.yaml");
    let before = fs::read_to_string(&branch_lockfile).expect("read branch lockfile");

    set_branch(&workspace, "main");
    append_workspace_yaml_key(&workspace, "lockfile", false);
    pacquet_in(&workspace)
        .with_args(["install", "--merge-git-branch-lockfiles"])
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(&branch_lockfile).expect("re-read branch lockfile"),
        before,
        "an install that never reads a lockfile must not delete the branch lockfiles",
    );

    drop((root, mock_instance));
}

/// `dedupe --check` reports what a dedupe would change and takes its
/// lockfile back afterwards, so the merge it ran never reaches disk — and
/// the branch lockfiles it merged from have to survive it.
#[test]
fn merging_keeps_the_branch_lockfiles_on_a_check_only_dedupe() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    set_branch(&workspace, "main");
    write_dependencies(&workspace, &serde_json::json!({ "@pnpm.e2e/foo": "1.0.0" }));
    pacquet.with_arg("install").assert().success();

    let branch_lockfile = workspace.join("pnpm-lock.other.yaml");
    fs::write(&branch_lockfile, "lockfileVersion: '9.0'\n").unwrap();
    append_workspace_yaml_key(&workspace, "mergeGitBranchLockfiles", true);

    pacquet_in(&workspace).with_args(["dedupe", "--check"]).assert().success();

    assert!(
        branch_lockfile.exists(),
        "a check-only run takes its lockfile back, so it cannot have merged them",
    );

    drop((root, mock_instance));
}

/// The merge unions the two lockfiles' keys, so a dependency the main
/// branch dropped after the branch lockfile was written comes back in
/// the merged result. `--frozen-lockfile` never resolves, so nothing
/// else takes it out again before the freshness check sees it.
#[test]
fn merging_drops_a_dependency_the_manifest_no_longer_declares() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let write_manifest = |dependencies: serde_json::Value| {
        fs::write(
            workspace.join("package.json"),
            serde_json::json!({
                "dependencies": dependencies,
                "peerDependencies": { "@pnpm.e2e/bar": "100.0.0" },
            })
            .to_string(),
        )
        .expect("write package.json");
    };

    set_branch(&workspace, "main");
    write_manifest(serde_json::json!({ "@pnpm.e2e/foo": "1.0.0", "@pnpm.e2e/qar": "100.0.0" }));
    pacquet.with_arg("install").assert().success();

    // What the other branch last resolved, taken before `qar` was dropped.
    let branch_lockfile = workspace.join("pnpm-lock.other.yaml");
    fs::copy(workspace.join("pnpm-lock.yaml"), &branch_lockfile).expect("seed a branch lockfile");

    write_manifest(serde_json::json!({ "@pnpm.e2e/foo": "1.0.0" }));
    pacquet_in(&workspace).with_arg("install").assert().success();

    pacquet_in(&workspace)
        .with_args(["install", "--merge-git-branch-lockfiles", "--frozen-lockfile"])
        .assert()
        .success();

    assert!(!branch_lockfile.exists(), "the merged branch lockfile is deleted");
    let shared = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(!shared.contains("@pnpm.e2e/qar"), "{shared}");
    assert!(
        shared.contains("@pnpm.e2e/bar@100.0.0"),
        "the auto-installed peer is still declared, so it survives: {shared}",
    );

    drop((root, mock_instance));
}

/// Reconciling the fold against the manifests must not double as a repair
/// for an ordinary stale lockfile: `mergeGitBranchLockfilesBranchPattern`
/// leaves merge mode on for every install on a matched branch, so a frozen
/// install there still has to report drift the fold did not introduce.
///
/// A branch lockfile that is absent, empty, or carries no lockfile
/// document all fold nothing in, and each has to reach the same verdict.
#[test]
fn merging_with_nothing_to_merge_still_rejects_an_outdated_lockfile() {
    for branch_lockfile_content in [None, Some(""), Some("lockfileVersion: '9.0'\n")] {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        set_branch(&workspace, "main");
        write_dependencies(&workspace, &serde_json::json!({ "@pnpm.e2e/foo": "1.0.0" }));
        pacquet.with_arg("install").assert().success();

        // The manifest drops the dependency without the lockfile being
        // updated, and nothing gets folded in to explain the leftover entry.
        write_dependencies(&workspace, &serde_json::json!({}));
        if let Some(content) = branch_lockfile_content {
            fs::write(workspace.join("pnpm-lock.other.yaml"), content)
                .expect("write branch lockfile");
        }

        let assert = pacquet_in(&workspace)
            .with_args(["install", "--merge-git-branch-lockfiles", "--frozen-lockfile"])
            .assert()
            .failure();
        let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
        eprintln!("STDERR:\n{stderr}\n");
        assert!(
            stderr.contains("ERR_PNPM_OUTDATED_LOCKFILE"),
            "branch lockfile {branch_lockfile_content:?} folds nothing in, \
             so the drift must still be reported; got:\n{stderr}",
        );

        drop((root, mock_instance));
    }
}
