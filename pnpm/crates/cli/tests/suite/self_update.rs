//! `pacquet self-update` — dispatch-level coverage.
//!
//! `self-update` routes through the `config_self_update` closure, which
//! drops the release-age and trust policies a repo-controlled
//! `pnpm-workspace.yaml` would otherwise set.
//!
//! No test here reaches an activated binary in the global bin directory:
//! the mocked registry's pnpm clears neither half of the engine-identity
//! gate — it ships no platform binaries, and its tarball carries no npm
//! registry signature.
//!
//! Every test points `PNPM_HOME` at a temp dir, so the install and link
//! steps write there instead of clobbering the caller's real pnpm.

use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};
use tempfile::{TempDir, tempdir};

/// A pnpm the mocked registry can serve as `latest`, far enough ahead that
/// it stays newer than the running version through every release.
const NEWER_PNPM: &str = "99.0.0";

/// A pnpm the mocked registry can serve as `latest` that is older than the
/// running version, so an implicit-`latest` run declines the global switch
/// and the project pin is the only thing it writes.
const OLDER_PNPM: &str = "1.0.0";

#[test]
fn self_update_loads_config_and_reaches_the_resolver() {
    let CommandTempCwd { mut pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    // Isolate the global pnpm install: `PNPM_HOME` drives `global_pkg_dir` /
    // `global_bin` / the store, so any link step writes into this temp dir
    // rather than the caller's real pnpm home.
    let global_home = tempdir().expect("global home tempdir");
    pacquet.env("PNPM_HOME", global_home.path());

    // A workspace `minimumReleaseAge` is present to mirror a realistic
    // invocation; it isn't load-bearing for this test's assertions because
    // the resolve fails before any maturity check runs. The policy resolution
    // itself is covered by the config-crate unit tests.
    fs::write(workspace.join("pnpm-workspace.yaml"), "minimumReleaseAge: 1440\n")
        .expect("write pnpm-workspace.yaml");

    let output = pacquet
        .with_args(["self-update", "999.999.999"])
        .output()
        .expect("run pacquet self-update");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("Checking for updates") || stderr.contains("Checking for updates"),
        "the self-update closure should load config and reach the handler; stdout={stdout}, stderr={stderr}",
    );
    assert!(
        !output.status.success(),
        "self-update should fail at the resolve step because `999.999.999` is not on the mock registry",
    );

    drop(root);
    drop(global_home);
}

#[test]
fn self_update_switches_the_global_pnpm_when_the_project_pin_is_already_current() {
    let mut project = PinnedProject::pinned_to(NEWER_PNPM, NEWER_PNPM);

    let output = project.self_update();

    assert_reached_the_global_switch(&output);
}

#[test]
fn self_update_leaves_the_project_pin_alone_when_the_global_switch_fails() {
    let mut project = PinnedProject::pinned_to("1.2.3", NEWER_PNPM);

    let output = project.self_update();

    assert_reached_the_global_switch(&output);
    assert!(
        !output.status.success(),
        "the global switch cannot finish against the fixture registry",
    );
    assert_eq!(project.manifest_text(), r#"{"packageManager":"pnpm@1.2.3"}"#);
}

#[test]
fn self_update_rewrites_the_project_pin_and_reports_the_declined_global_switch() {
    let mut project = PinnedProject::pinned_to("0.1.0", OLDER_PNPM);

    let output = project.self_update();

    assert_pin_updated_and_global_switch_declined(&output);
    assert_eq!(project.manifest_text(), format!(r#"{{"packageManager":"pnpm@{OLDER_PNPM}"}}"#));
}

#[test]
fn self_update_rewrites_a_dev_engines_pin_and_reports_the_declined_global_switch() {
    let mut project = PinnedProject::pinned_through_dev_engines("0.1.0", OLDER_PNPM);

    let output = project.self_update();

    assert_pin_updated_and_global_switch_declined(&output);
    let manifest = project.manifest_text();
    assert!(
        manifest.contains(&format!(r#""version":"{OLDER_PNPM}""#)),
        "the devEngines pin was not rewritten: {manifest}",
    );
}

/// A project that pins pnpm, wired to a mocked registry serving
/// `registry_latest` as pnpm's `latest` and to a throwaway `PNPM_HOME`.
struct PinnedProject {
    pacquet: Command,
    workspace: PathBuf,
    // Held so the temp trees outlive the command that writes into them.
    _root: TempDir,
    _global_home: TempDir,
    _npmrc_info: AddMockedRegistry,
}

impl PinnedProject {
    fn pinned_to(version: &str, registry_latest: &str) -> Self {
        Self::with_manifest(&format!(r#"{{"packageManager":"pnpm@{version}"}}"#), registry_latest)
    }

    fn pinned_through_dev_engines(version: &str, registry_latest: &str) -> Self {
        Self::with_manifest(
            &format!(
                r#"{{"devEngines":{{"packageManager":{{"name":"pnpm","version":"{version}"}}}}}}"#,
            ),
            registry_latest,
        )
    }

    fn with_manifest(manifest: &str, registry_latest: &str) -> Self {
        let CommandTempCwd {
            mut pacquet,
            root,
            workspace,
            npmrc_info,
            ..
        } = CommandTempCwd::init().add_mocked_registry_with_pnpm_version(registry_latest);
        let global_home = tempdir().expect("global home tempdir");
        pacquet.env("PNPM_HOME", global_home.path());
        // Point the trusted package-manager bootstrap registry at the mock
        // too: the project registry never drives an engine download.
        pacquet.env("PNPM_CONFIG_REGISTRY", npmrc_info.mock_instance.url());
        fs::write(workspace.join("package.json"), manifest).expect("write package.json");
        PinnedProject {
            pacquet,
            workspace,
            _root: root,
            _global_home: global_home,
            _npmrc_info: npmrc_info,
        }
    }

    fn self_update(&mut self) -> Output {
        self.pacquet
            .arg("self-update")
            .output()
            .expect("run pacquet self-update")
    }

    fn manifest_text(&self) -> String {
        fs::read_to_string(self.workspace.join("package.json")).expect("read package.json")
    }
}

fn assert_reached_the_global_switch(output: &Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("Switching pnpm from v") && stdout.contains(NEWER_PNPM),
        "self-update stopped at the project pin; stdout={stdout}, stderr={stderr}",
    );
}

fn assert_pin_updated_and_global_switch_declined(output: &Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stdout={stdout}, stderr={stderr}");
    assert!(
        stdout.contains(&format!("The current project has been updated to use pnpm v{OLDER_PNPM}"))
            && stdout.contains(&format!(
                r#"is newer than the "latest" version on the registry (v{OLDER_PNPM})"#,
            )),
        "both the pin update and the declined global switch must be reported; stdout={stdout}, stderr={stderr}",
    );
}
