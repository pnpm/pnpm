use command_extra::CommandExtra;
use pnpm_lockfile::EnvLockfile;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use pretty_assertions::assert_eq;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[test]
fn version_flag_prints_the_bare_version() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet.with_arg("--version").output().expect("run pacquet --version");
    dbg!(&output);
    assert!(output.status.success(), "pacquet --version should succeed");
    assert_eq!(String::from_utf8_lossy(&output.stdout), format!("{}\n", pnpm_config::PNPM_VERSION));

    drop(root);
}

#[test]
fn version_flag_accepts_shamefully_hoist() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet
        .with_args(["--shamefully-hoist=true", "--version"])
        .output()
        .expect("run pacquet --shamefully-hoist=true --version");
    dbg!(&output);
    assert!(output.status.success(), "pacquet should accept --shamefully-hoist");
    assert_eq!(String::from_utf8_lossy(&output.stdout), format!("{}\n", pnpm_config::PNPM_VERSION));

    drop(root);
}

#[test]
fn version_flag_rejects_invalid_shamefully_hoist_value() {
    for flag in ["--shamefully-hoist=yes", "--config.shamefully-hoist=yes"] {
        let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
        let output = pacquet
            .with_args([flag, "--version"])
            .output()
            .expect("run pacquet with an invalid shamefully-hoist value");
        dbg!(&output);
        assert!(!output.status.success(), "pacquet should reject {flag}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected argument"));
        drop(root);
    }
}

#[test]
fn short_version_flag_prints_the_bare_version() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet.with_arg("-v").output().expect("run pacquet -v");
    dbg!(&output);
    assert!(output.status.success(), "pacquet -v should succeed");
    assert_eq!(String::from_utf8_lossy(&output.stdout), format!("{}\n", pnpm_config::PNPM_VERSION));

    drop(root);
}

#[test]
fn version_flag_switches_to_project_package_manager_version() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(workspace.join("package.json"), r#"{"packageManager":"pnpm@9.3.0"}"#)
        .expect("write package.json");

    let output = test_command(pacquet, root.path())
        .env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .args(["--version"])
        .output()
        .expect("run pacquet --version");
    dbg!(&output);
    assert!(output.status.success(), "pacquet --version should succeed");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "9.3.0\n");

    drop((root, mock_instance));
}

#[test]
fn child_pnpm_selects_the_version_for_its_own_directory() {
    let CommandTempCwd { pacquet, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let project_a = root.path().join("a");
    let project_b = root.path().join("b");
    fs::create_dir_all(&project_a).expect("create project a");
    fs::create_dir_all(&project_b).expect("create project b");
    fs::write(project_a.join("package.json"), r#"{"packageManager":"pnpm@9.3.0"}"#)
        .expect("write project a package.json");
    fs::write(project_b.join("package.json"), r#"{"packageManager":"pnpm@9.1.3"}"#)
        .expect("write project b package.json");

    let version_script = root.path().join("child-version.cjs");
    fs::write(
        &version_script,
        r"
        const { spawnSync } = require('node:child_process')
        const result = spawnSync('pnpm', ['--version'], {
          cwd: process.argv[2],
          encoding: 'utf8',
        })
        if (result.status !== 0) throw new Error(result.stderr)
        process.stdout.write(result.stdout)
    ",
    )
    .expect("write child version script");
    let output = test_command(pacquet, root.path())
        .current_dir(&project_a)
        .env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .args(["exec", "node"])
        .arg(&version_script)
        .arg(&project_b)
        .output()
        .expect("run child pnpm in project b");
    dbg!(&output);
    assert!(output.status.success(), "child pnpm should succeed");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "9.1.3\n");

    drop((root, mock_instance));
}

/// `--version` runs the pre-command checks — that is what lets it switch to
/// the pinned pnpm above. A pin the running pnpm already satisfies has nothing
/// to switch to, but it is still recorded, so the entry does not depend on
/// which command the project saw first.
#[test]
fn version_flag_records_a_pinned_package_manager_it_does_not_need_to_switch_to() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry_with_pnpm_version(pnpm_config::PNPM_VERSION);
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let pinned = pnpm_config::PNPM_VERSION;
    fs::write(
        workspace.join("package.json"),
        format!(r#"{{"devEngines":{{"packageManager":{{"name":"pnpm","version":"{pinned}"}}}}}}"#),
    )
    .expect("write package.json");

    let output = test_command(pacquet, root.path())
        .env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .args(["--version"])
        .output()
        .expect("run pacquet --version");
    dbg!(&output);
    assert!(output.status.success(), "pacquet --version should succeed");
    assert_eq!(String::from_utf8_lossy(&output.stdout), format!("{pinned}\n"));

    let env_lockfile = EnvLockfile::read(&workspace)
        .expect("read the written env lockfile")
        .expect("the env lockfile should have been written");
    let recorded = env_lockfile.importers[EnvLockfile::ROOT_IMPORTER_KEY]
        .package_manager_dependencies
        .as_ref()
        .expect("packageManagerDependencies should be recorded");
    assert_eq!(recorded["pnpm"].specifier, pinned);
    assert_eq!(recorded["pnpm"].version, pinned);

    drop((root, mock_instance));
}

/// A range pin is resolved once, and the lockfile then names the one pnpm
/// every contributor on the project runs. A running pnpm that satisfies the
/// range as well is not automatically that one.
#[test]
fn version_flag_switches_to_the_version_a_range_pin_resolved_to() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_dev_engine_pin(&workspace, "9.3.0");

    let output = version_output(root.path(), &workspace, &mock_instance.url());
    dbg!(&output);
    assert!(output.status.success(), "pacquet --version should succeed");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "9.3.0\n");

    write_dev_engine_pin(&workspace, ">=0.0.0");

    let output = version_output(root.path(), &workspace, &mock_instance.url());
    dbg!(&output);
    assert!(output.status.success(), "pacquet --version should succeed");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "9.3.0\n");

    drop((root, mock_instance));
}

fn write_dev_engine_pin(workspace: &Path, version: &str) {
    fs::write(
        workspace.join("package.json"),
        format!(
            r#"{{"devEngines":{{"packageManager":{{"name":"pnpm","version":"{version}","onFail":"download"}}}}}}"#,
        ),
    )
    .expect("write package.json");
}

fn version_output(root: &Path, workspace: &Path, registry: &str) -> std::process::Output {
    use assert_cmd::cargo::CommandCargoExt as _;
    use pnpm_testing_utils::command_env::CommandTestExt as _;
    let command = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .without_ambient_pnpm_config();
    test_command(command, root)
        .env("PNPM_CONFIG_REGISTRY", registry)
        .arg("--version")
        .output()
        .expect("run pacquet --version")
}

fn test_command(mut command: Command, root: &Path) -> Command {
    command.env("PNPM_HOME", root.join("pnpm-home"));
    command.env("HOME", root);
    command.env("XDG_CONFIG_HOME", root.join("xdg-config"));
    command.env_remove("COREPACK_ROOT");
    command.env_remove("pnpm_config_pm_on_fail");
    command.env_remove("PNPM_CONFIG_PM_ON_FAIL");
    command
}

// ---------------------------------------------------------------------------
// npm-style `pnpm version <bump|semver>` — ported from the upstream suites
// releasing/commands/test/version/index.test.ts and pnpm/test/version.ts.
// The spawned binary's stdout is a pipe, so reporter styling is plain text.
// ---------------------------------------------------------------------------

fn pacquet_version(workspace: &Path, args: &[&str]) -> std::process::Output {
    use assert_cmd::cargo::CommandCargoExt as _;
    let mut command = Command::cargo_bin("pnpm").expect("find the pnpm binary");
    command.current_dir(workspace).arg("version").args(args);
    command.output().expect("run pacquet version")
}

fn write_manifest(dir: &Path, json: &str) {
    fs::write(dir.join("package.json"), json).expect("write package.json");
}

fn manifest_text(dir: &Path) -> String {
    fs::read_to_string(dir.join("package.json")).expect("read manifest")
}

fn manifest_version(dir: &Path) -> String {
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join("package.json")).expect("read manifest"))
            .expect("parse manifest");
    manifest.get("version").and_then(serde_json::Value::as_str).unwrap_or_default().to_string()
}

/// `git init` plus the identity/signing config the commit and tag need.
fn init_git(dir: &Path) {
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.email", "x@y.z"],
        vec!["config", "user.name", "xyz"],
        vec!["config", "commit.gpgSign", "false"],
        vec!["config", "tag.gpgSign", "false"],
    ] {
        let status = Command::new("git").args(&args).current_dir(dir).status().expect("run git");
        assert!(status.success(), "git {args:?} should succeed");
    }
}

fn git_commit_all(dir: &Path, message: &str) {
    for args in [vec!["add", "."], vec!["commit", "-q", "-m", message, "--no-gpg-sign"]] {
        let status = Command::new("git").args(&args).current_dir(dir).status().expect("run git");
        assert!(status.success(), "git {args:?} should succeed");
    }
}

fn git_stdout(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git").args(args).current_dir(dir).output().expect("run git");
    assert!(output.status.success(), "git {args:?} should succeed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn stderr_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn invalid_bump_type_fails() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);

    for argument in ["invalid", "not-a-version"] {
        let output = pacquet_version(&workspace, &[argument]);
        assert!(!output.status.success(), "{argument} must fail");
        let stderr = stderr_of(&output);
        assert!(stderr.contains("ERR_PNPM_INVALID_VERSION_BUMP"), "{argument}: {stderr}");
    }
    drop(root);
}

#[test]
fn missing_bump_without_recursive_fails() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);

    let output = pacquet_version(&workspace, &[]);

    assert!(!output.status.success(), "a bare `pnpm version` must fail");
    let stderr = stderr_of(&output);
    assert!(stderr.contains("ERR_PNPM_INVALID_VERSION_BUMP"), "{stderr}");
    drop(root);
}

#[test]
fn bumps_major_minor_and_patch() {
    for (bump, expected) in [("major", "2.0.0"), ("minor", "1.3.0"), ("patch", "1.2.4")] {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.2.3"}"#);

        let output = pacquet_version(&workspace, &[bump]);

        assert!(output.status.success(), "{bump}: {}", stderr_of(&output));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains(&format!("1.2.3 → {expected}")), "{bump}: {stdout}");
        assert_eq!(manifest_version(&workspace), expected, "{bump}");
        drop(root);
    }
}

#[test]
fn prerelease_bump_uses_the_preid() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);

    let output = pacquet_version(&workspace, &["prerelease", "--preid", "alpha"]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("1.0.0 → 1.0.1-alpha.0"),
        "{:?}",
        String::from_utf8_lossy(&output.stdout),
    );

    // An empty --preid means "no preid" (it is falsy to the TypeScript CLI's
    // semver.inc), so the prerelease starts at a bare `-0`, never `-.0`.
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"2.0.0"}"#);
    let output = pacquet_version(&workspace, &["prerelease", "--preid", ""]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(manifest_version(&workspace), "2.0.1-0");
    drop(root);
}

#[test]
fn json_flag_reports_the_changes_as_json() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);

    let output = pacquet_version(&workspace, &["patch", "--json"]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    let parsed: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim())
            .expect("stdout must be JSON");
    let entry = &parsed.as_array().expect("a JSON array")[0];
    assert_eq!(entry.get("name").and_then(serde_json::Value::as_str), Some("test-pkg"));
    assert_eq!(entry.get("currentVersion").and_then(serde_json::Value::as_str), Some("1.0.0"));
    assert_eq!(entry.get("newVersion").and_then(serde_json::Value::as_str), Some("1.0.1"));
    assert!(entry.get("manifestPath").is_none(), "manifestPath must not be reported");
    drop(root);
}

#[test]
fn explicit_versions_are_set_verbatim() {
    for (argument, expected) in [("0.0.0", "0.0.0"), ("2.0.0-beta.1", "2.0.0-beta.1")] {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.2.3"}"#);

        let output = pacquet_version(&workspace, &[argument]);

        assert!(output.status.success(), "{argument}: {}", stderr_of(&output));
        assert_eq!(manifest_version(&workspace), expected, "{argument}");
        drop(root);
    }
}

#[test]
fn same_version_fails_unless_allowed() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);

    let output = pacquet_version(&workspace, &["1.0.0"]);
    assert!(!output.status.success(), "bumping to the same version must fail");
    let stderr = stderr_of(&output);
    assert!(stderr.contains("ERR_PNPM_VERSION_NOT_CHANGED"), "{stderr}");

    let output = pacquet_version(&workspace, &["1.0.0", "--allow-same-version"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("1.0.0 → 1.0.0"),
        "{:?}",
        String::from_utf8_lossy(&output.stdout),
    );
    drop(root);
}

#[test]
fn manifest_without_name_or_version_has_no_packages_to_version() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, "{}");

    let output = pacquet_version(&workspace, &["patch"]);

    assert!(!output.status.success(), "an empty manifest must fail");
    let stderr = stderr_of(&output);
    assert!(stderr.contains("ERR_PNPM_NO_PACKAGES_TO_VERSION"), "{stderr}");
    drop(root);
}

#[test]
fn invalid_manifest_version_fails() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"not-a-version"}"#);

    let output = pacquet_version(&workspace, &["patch"]);

    assert!(!output.status.success(), "an invalid manifest version must fail");
    let stderr = stderr_of(&output);
    assert!(stderr.contains("ERR_PNPM_INVALID_VERSION"), "{stderr}");
    drop(root);
}

#[test]
fn lifecycle_scripts_run_in_order_around_the_bump() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let log_script = concat!(
        r#"node -e "require('fs').appendFileSync('lifecycle.log',"#,
        r#" process.env.npm_lifecycle_event + ':' + require('./package.json').version + '\n')""#,
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

    let output = pacquet_version(&workspace, &["patch"]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    let log = fs::read_to_string(workspace.join("lifecycle.log")).expect("lifecycle log");
    assert_eq!(log, "preversion:1.0.0\nversion:1.0.1\npostversion:1.0.1\n");
    drop(root);
}

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

fn write_two_package_workspace(workspace: &Path) -> (PathBuf, PathBuf) {
    let pkg_a = workspace.join("packages").join("pkg-a");
    let pkg_b = workspace.join("packages").join("pkg-b");
    fs::create_dir_all(&pkg_a).expect("create pkg-a");
    fs::create_dir_all(&pkg_b).expect("create pkg-b");
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - \"packages/*\"\n")
        .expect("write pnpm-workspace.yaml");
    write_manifest(workspace, r#"{"name":"my-workspace"}"#);
    write_manifest(&pkg_a, r#"{"name":"pkg-a","version":"1.0.0"}"#);
    write_manifest(&pkg_b, r#"{"name":"pkg-b","version":"2.3.0"}"#);
    (pkg_a, pkg_b)
}

fn pacquet_recursive_version(workspace: &Path, args: &[&str]) -> std::process::Output {
    use assert_cmd::cargo::CommandCargoExt as _;
    let mut command = Command::cargo_bin("pnpm").expect("find the pnpm binary");
    command.current_dir(workspace).args(args);
    command.output().expect("run pacquet -r version")
}

#[test]
fn recursive_bumps_every_workspace_package() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let (pkg_a, pkg_b) = write_two_package_workspace(&workspace);

    let output =
        pacquet_recursive_version(&workspace, &["-r", "version", "minor", "--no-git-checks"]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(manifest_version(&pkg_a), "1.1.0");
    assert_eq!(manifest_version(&pkg_b), "2.4.0");
    // The versionless workspace root is skipped, not failed on.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("my-workspace"), "{stdout}");
    drop(root);
}

#[test]
fn recursive_filter_bumps_only_the_selected_package() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let (pkg_a, pkg_b) = write_two_package_workspace(&workspace);

    let output = pacquet_recursive_version(
        &workspace,
        &["-r", "--filter", "pkg-b", "version", "patch", "--no-git-checks"],
    );

    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(manifest_version(&pkg_a), "1.0.0");
    assert_eq!(manifest_version(&pkg_b), "2.3.1");
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
fn dry_run_reports_the_bump_without_writing_the_manifest() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);
    let manifest_before = manifest_text(&workspace);

    let output = pacquet_version(&workspace, &["patch", "--dry-run"]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Version bump plan:"), "{stdout}");
    assert!(stdout.contains("1.0.0 → 1.0.1"), "{stdout}");
    assert_eq!(manifest_text(&workspace), manifest_before);
    drop(root);
}

#[test]
fn dry_run_leaves_every_workspace_manifest_untouched() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let (pkg_a, pkg_b) = write_two_package_workspace(&workspace);
    let manifests_before = [&workspace, &pkg_a, &pkg_b].map(|dir| manifest_text(dir));

    let output = pacquet_recursive_version(&workspace, &["-r", "version", "patch", "--dry-run"]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("pkg-a: 1.0.0 → 1.0.1"), "{stdout}");
    assert!(stdout.contains("pkg-b: 2.3.0 → 2.3.1"), "{stdout}");
    assert_eq!([&workspace, &pkg_a, &pkg_b].map(|dir| manifest_text(dir)), manifests_before);
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

/// The npm-style and change-intents forms share one command: a version
/// argument selects the npm-style bump even inside a workspace, and without
/// `--recursive` it touches only the current package — never the workspace
/// members and never the pending change intents.
#[test]
fn npm_style_bump_in_a_workspace_without_recursive_bumps_only_the_root() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let (pkg_a, pkg_b) = write_two_package_workspace(&workspace);
    write_manifest(&workspace, r#"{"name":"my-workspace","version":"1.0.0"}"#);
    fs::create_dir_all(workspace.join(".changeset")).expect("create .changeset");
    let intent = workspace.join(".changeset").join("calm-cats-smile.md");
    fs::write(&intent, "---\n\"pkg-a\": minor\n---\n\nA pending change intent.\n")
        .expect("write change intent");

    let output = pacquet_version(&workspace, &["patch", "--no-git-checks"]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("my-workspace: 1.0.0 → 1.0.1"), "{stdout}");
    assert_eq!(manifest_version(&workspace), "1.0.1");
    // Workspace members are untouched without --recursive...
    assert_eq!(manifest_version(&pkg_a), "1.0.0");
    assert_eq!(manifest_version(&pkg_b), "2.3.0");
    // ...and the pending change intent is neither consumed nor deleted.
    assert!(intent.exists(), "the change intent must survive an npm-style bump");
    drop(root);
}

#[test]
fn help_describes_the_bump_forms_and_flags() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();

    let output = pacquet_version(&workspace, &["--help"]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    for needle in [
        "major",
        "minor",
        "patch",
        "prerelease",
        "from-git",
        "--preid",
        "--allow-same-version",
        "--message",
        "--no-git-tag-version",
        "--no-commit-hooks",
        "--sign-git-tag",
        "--tag-version-prefix",
        "--json",
    ] {
        assert!(stdout.contains(needle), "help must mention {needle}:\n{stdout}");
    }
    drop(root);
}

#[test]
fn recursive_with_an_empty_selection_bumps_nothing() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let (pkg_a, pkg_b) = write_two_package_workspace(&workspace);

    let output = pacquet_recursive_version(
        &workspace,
        &["-r", "--filter", "no-such-package", "version", "minor", "--no-git-checks"],
    );

    assert!(!output.status.success(), "an empty selection must fail");
    let stderr = stderr_of(&output);
    assert!(stderr.contains("ERR_PNPM_NO_PACKAGES_TO_VERSION"), "{stderr}");
    assert_eq!(manifest_version(&pkg_a), "1.0.0");
    assert_eq!(manifest_version(&pkg_b), "2.3.0");
    drop(root);
}

#[test]
fn recursive_skips_members_without_a_name_or_version() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let (pkg_a, pkg_b) = write_two_package_workspace(&workspace);
    write_manifest(&pkg_b, r#"{"private":true}"#);

    let output =
        pacquet_recursive_version(&workspace, &["-r", "version", "patch", "--no-git-checks"]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("pkg-a"), "{stdout}");
    assert!(!stdout.contains("pkg-b"), "the versionless member must be skipped: {stdout}");
    assert_eq!(manifest_version(&pkg_a), "1.0.1");
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

#[test]
fn version_json_outputs_release_details_in_json() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    init_git(&workspace);
    write_manifest(&workspace, r#"{"name":"test-pkg","version":"1.0.0"}"#);
    git_commit_all(&workspace, "init");

    let output = pacquet_version(&workspace, &["patch", "--json", "--no-git-tag-version"]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    let parsed: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim())
            .expect("stdout must be JSON");
    let arr = parsed.as_array().expect("a JSON array");
    assert_eq!(arr.len(), 1, "expected exactly one release entry");
    let entry = &arr[0];
    assert_eq!(entry.get("name").and_then(serde_json::Value::as_str), Some("test-pkg"));
    assert_eq!(entry.get("currentVersion").and_then(serde_json::Value::as_str), Some("1.0.0"));
    assert_eq!(entry.get("newVersion").and_then(serde_json::Value::as_str), Some("1.0.1"));
    drop(root);
}

#[test]
fn version_recursive_json_prints_empty_array_when_no_pending_changes() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{"name":"root","version":"1.0.0"}"#)
        .expect("write package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - '.'\n")
        .expect("write pnpm-workspace.yaml");

    let output = test_command(pacquet, root.path())
        .current_dir(&workspace)
        .args(["version", "-r", "--json"])
        .output()
        .expect("run pacquet version -r --json");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "[]");

    drop(root);
}

#[test]
fn version_recursive_json_prints_applied_releases_when_pending_changes() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let (_pkg_a, _pkg_b) = write_two_package_workspace(&workspace);
    fs::create_dir_all(workspace.join(".changeset")).expect("create .changeset");
    let intent = workspace.join(".changeset").join("calm-cats-smile.md");
    fs::write(&intent, "---\n\"pkg-a\": minor\n---\n\nA pending change intent.\n")
        .expect("write change intent");

    let output = test_command(pacquet, root.path())
        .env("PACQUET_ASSUME_VERSIONS_PUBLISHED", "1")
        .current_dir(&workspace)
        .args(["version", "-r", "--json", "--no-git-checks"])
        .output()
        .expect("run pacquet version -r --json");
    assert!(output.status.success(), "{}", stderr_of(&output));

    let parsed: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim())
            .expect("stdout must be JSON");
    let arr = parsed.as_array().expect("a JSON array");
    assert_eq!(arr.len(), 1, "expected exactly one release entry");
    let entry = &arr[0];
    assert_eq!(entry.get("name").and_then(serde_json::Value::as_str), Some("pkg-a"));
    assert_eq!(entry.get("currentVersion").and_then(serde_json::Value::as_str), Some("1.0.0"));
    assert_eq!(entry.get("newVersion").and_then(serde_json::Value::as_str), Some("1.1.0"));

    drop(root);
}

/// Every spelling of the recursive dry run reported in
/// <https://github.com/pnpm/pnpm/issues/13271> reaches the release-plan
/// preview, and the preview is what the run without `--dry-run` applies.
#[test]
fn recursive_dry_run_previews_the_plan_without_applying_it() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let (pkg_a, pkg_b) = write_two_package_workspace(&workspace);
    fs::create_dir_all(workspace.join(".changeset")).expect("create .changeset");
    for (file, intent) in [
        ("calm-cats-smile.md", "---\n\"pkg-a\": minor\n---\n\nA feature.\n"),
        ("brave-bats-clap.md", "---\n\"pkg-b\": patch\n---\n\nA fix.\n"),
    ] {
        fs::write(workspace.join(".changeset").join(file), intent).expect("write change intent");
    }
    let before = release_inputs(&workspace);

    for args in [
        ["version", "-r", "--dry-run"].as_slice(),
        ["version", "-r", "--no-git-checks", "--dry-run"].as_slice(),
        ["-r", "version", "--dry-run"].as_slice(),
        ["version", "-r", "--dry-run", "--filter", "pkg-a"].as_slice(),
    ] {
        let output = pacquet_version_assuming_published(&workspace, args);
        assert!(output.status.success(), "pnpm {args:?}:\n{}", stderr_of(&output));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Release plan:"), "pnpm {args:?}: {stdout}");
        assert!(stdout.contains("pkg-a: 1.0.0 → 1.1.0"), "pnpm {args:?}: {stdout}");
        assert_eq!(
            stdout.contains("pkg-b: 2.3.0 → 2.3.1"),
            !args.contains(&"--filter"),
            "only the filtered plan leaves pkg-b out — pnpm {args:?}: {stdout}",
        );
        assert_eq!(release_inputs(&workspace), before, "pnpm {args:?} wrote to the workspace");
    }

    let applied =
        pacquet_version_assuming_published(&workspace, &["version", "-r", "--no-git-checks"]);
    assert!(applied.status.success(), "{}", stderr_of(&applied));
    let stdout = String::from_utf8_lossy(&applied.stdout);
    assert!(stdout.contains("pkg-a: 1.0.0 → 1.1.0"), "{stdout}");
    assert!(stdout.contains("pkg-b: 2.3.0 → 2.3.1"), "{stdout}");
    assert_eq!(manifest_version(&pkg_a), "1.1.0");
    assert_eq!(manifest_version(&pkg_b), "2.3.1");
    drop(root);
}

/// Everything `pnpm version -r` reads and rewrites — the workspace manifests
/// and the change intents — so a dry run can be held to leaving all of it
/// byte-identical.
fn release_inputs(workspace: &Path) -> BTreeMap<PathBuf, String> {
    let manifests = ["pkg-a", "pkg-b"]
        .map(|pkg| workspace.join("packages").join(pkg).join("package.json"))
        .into_iter();
    let intents = fs::read_dir(workspace.join(".changeset"))
        .expect("read .changeset")
        .map(|entry| entry.expect("read a .changeset entry").path());
    manifests
        .chain(intents)
        .map(|path| {
            let contents = fs::read_to_string(&path).expect("read a release input");
            (path, contents)
        })
        .collect()
}

/// `pnpm version -r` with the first-release probe stubbed out, so the release
/// plan is assembled without a registry round trip. The probe's own effect on
/// the plan is covered against the mock registry by the `first_release_probe_*`
/// tests in `change.rs`.
fn pacquet_version_assuming_published(workspace: &Path, args: &[&str]) -> std::process::Output {
    use assert_cmd::cargo::CommandCargoExt as _;
    let mut command = Command::cargo_bin("pnpm").expect("find the pnpm binary");
    command.current_dir(workspace).env("PACQUET_ASSUME_VERSIONS_PUBLISHED", "1").args(args);
    command.output().expect("run pacquet version")
}
