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
    path::{Path, PathBuf},
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
fn self_update_refuses_a_pnpm_installed_by_homebrew() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let brewed_pnpm = install_into_homebrew_keg(&pacquet, root.path());

    let output = command_with_env_of(&pacquet, &brewed_pnpm)
        .current_dir(&workspace)
        .arg("self-update")
        .output()
        .expect("run the Homebrew pnpm");

    assert!(!output.status.success(), "self-update must refuse a Homebrew pnpm");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_CANT_SELF_UPDATE_IN_HOMEBREW"), "stderr={stderr}");
    assert!(stderr.contains("brew upgrade pnpm"), "stderr={stderr}");
}

#[test]
fn self_update_rewrites_the_project_pin_of_a_pnpm_installed_by_homebrew() {
    let project = PinnedProject::pinned_to("0.1.0", OLDER_PNPM);
    let brewed_pnpm = install_into_homebrew_keg(&project.pacquet, project.root.path());

    let output = command_with_env_of(&project.pacquet, &brewed_pnpm)
        .current_dir(&project.workspace)
        .arg("self-update")
        .output()
        .expect("run the Homebrew pnpm");

    assert_pin_updated_and_global_switch_skipped(
        &output,
        "The global pnpm was left alone because it is installed by Homebrew. Update it with Homebrew instead: brew upgrade pnpm",
    );
    assert_eq!(project.manifest_text(), format!(r#"{{"packageManager":"pnpm@{OLDER_PNPM}"}}"#));
}

#[test]
fn self_update_rewrites_the_project_pin_under_corepack() {
    let mut project = PinnedProject::pinned_to("0.1.0", OLDER_PNPM);
    project.pacquet.env("COREPACK_ROOT", "/fake/corepack");

    let output = project.self_update();

    assert_pin_updated_and_global_switch_skipped(
        &output,
        "The global pnpm was left alone because it is executed by Corepack.",
    );
    assert_eq!(project.manifest_text(), format!(r#"{{"packageManager":"pnpm@{OLDER_PNPM}"}}"#));
}

#[test]
fn self_update_refuses_a_pnpm_executed_by_corepack_outside_a_pinned_project() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet
        .with_env("COREPACK_ROOT", "/fake/corepack")
        .with_arg("self-update")
        .output()
        .expect("run pacquet self-update");

    assert!(!output.status.success(), "self-update must refuse a Corepack pnpm");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_CANT_SELF_UPDATE_IN_COREPACK"), "stderr={stderr}");
    drop(root);
}

#[test]
fn self_update_refuses_a_pnpm_executed_by_corepack_when_package_json_is_malformed() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{").expect("write package.json");

    let output = pacquet
        .with_env("COREPACK_ROOT", "/fake/corepack")
        .with_arg("self-update")
        .output()
        .expect("run pacquet self-update");

    assert!(!output.status.success(), "self-update must refuse a Corepack pnpm");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_CANT_SELF_UPDATE_IN_COREPACK"), "stderr={stderr}");
    drop(root);
}

/// The executable is copied into a fake keg rather than linked, because
/// pnpm detects Homebrew from its canonical path and the keg's install receipt.
fn install_into_homebrew_keg(pacquet: &Command, root: &Path) -> PathBuf {
    let keg = root
        .join("Cellar")
        .join("pnpm")
        .join("12.0.0");
    fs::create_dir_all(keg.join("bin")).expect("create the keg bin directory");
    fs::write(keg.join("INSTALL_RECEIPT.json"), "{}").expect("write the install receipt");
    let brewed_pnpm = keg
        .join("bin")
        .join(format!("pnpm{}", std::env::consts::EXE_SUFFIX));
    fs::copy(pacquet.get_program(), &brewed_pnpm).expect("copy the pnpm binary");
    brewed_pnpm
}

fn command_with_env_of(template: &Command, program: &Path) -> Command {
    let mut command = Command::new(program);
    command.envs(
        template
            .get_envs()
            .filter_map(|(key, value)| Some((key, value?))),
    );
    command
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

#[test]
fn self_update_rewrites_the_workspace_root_pin_from_a_workspace_package() {
    let mut project = PinnedProject::pinned_to("0.1.0", OLDER_PNPM);
    let package_dir = project.run_from_workspace_package();

    let output = project.self_update();

    assert_pin_updated_and_global_switch_declined(&output);
    assert_eq!(project.manifest_text(), format!(r#"{{"packageManager":"pnpm@{OLDER_PNPM}"}}"#));
    assert_eq!(
        fs::read_to_string(package_dir.join("package.json")).expect("read package.json"),
        r#"{"name":"app"}"#,
    );
}

#[test]
fn self_update_rewrites_the_workspace_root_pin_under_corepack_from_a_workspace_package() {
    let mut project = PinnedProject::pinned_to("0.1.0", OLDER_PNPM);
    project.run_from_workspace_package();
    project.pacquet.env("COREPACK_ROOT", "/fake/corepack");

    let output = project.self_update();

    assert_pin_updated_and_global_switch_skipped(
        &output,
        "The global pnpm was left alone because it is executed by Corepack.",
    );
    assert_eq!(project.manifest_text(), format!(r#"{{"packageManager":"pnpm@{OLDER_PNPM}"}}"#));
}

#[test]
fn self_update_rewrites_a_package_yaml_pin() {
    let mut project = PinnedProject::pinned_to("0.1.0", OLDER_PNPM);
    fs::remove_file(project.workspace.join("package.json")).expect("remove package.json");
    let manifest_path = project.workspace.join("package.yaml");
    fs::write(&manifest_path, "packageManager: pnpm@0.1.0\n").expect("write package.yaml");

    let output = project.self_update();

    assert_pin_updated_and_global_switch_declined(&output);
    assert_eq!(
        fs::read_to_string(&manifest_path).expect("read package.yaml"),
        format!("packageManager: pnpm@{OLDER_PNPM}\n"),
    );
}

/// A project that pins pnpm, wired to a mocked registry serving
/// `registry_latest` as pnpm's `latest` and to a throwaway `PNPM_HOME`.
struct PinnedProject {
    pacquet: Command,
    workspace: PathBuf,
    // Held so the temp trees outlive the command that writes into them.
    root: TempDir,
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
            root,
            _global_home: global_home,
            _npmrc_info: npmrc_info,
        }
    }

    /// Make the project a workspace root and run the command from a
    /// workspace package that pins nothing itself. Returns that package.
    fn run_from_workspace_package(&mut self) -> PathBuf {
        fs::write(self.workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
            .expect("write pnpm-workspace.yaml");
        let package_dir = self.workspace.join("packages/app");
        fs::create_dir_all(&package_dir).expect("create the workspace package");
        fs::write(package_dir.join("package.json"), r#"{"name":"app"}"#)
            .expect("write the workspace package manifest");
        self.pacquet.current_dir(&package_dir);
        package_dir
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

fn assert_pin_updated_and_global_switch_skipped(output: &Output, skipped_message: &str) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stdout={stdout}, stderr={stderr}");
    assert!(
        stdout.contains(&format!("The current project has been updated to use pnpm v{OLDER_PNPM}"))
            && stdout.contains(skipped_message),
        "both the pin update and the skipped global switch must be reported; stdout={stdout}, stderr={stderr}",
    );
}
