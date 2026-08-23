//! The `updateNotifier` setting end to end: an install asks the
//! package-manager registry for pnpm's `latest` once a day and says so when
//! it is newer than the running pnpm.

use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

/// A pnpm the mocked registry can serve as `latest`, far enough ahead that
/// it stays newer than the running version through every release.
const NEWER_PNPM: &str = "99.0.0";

struct Fixture {
    pacquet: Command,
    state_dir: PathBuf,
    workspace: PathBuf,
    root: tempfile::TempDir,
    npmrc_info: AddMockedRegistry,
}

/// A fixture with the notifier on: the shared command harness turns it off
/// for every other suite, so these tests opt back in.
fn fixture() -> Fixture {
    let CommandTempCwd { mut pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry_with_pnpm_version(NEWER_PNPM);
    pacquet.env("PNPM_CONFIG_UPDATE_NOTIFIER", "true");
    let state_dir = root.path().join("pnpm-state");
    Fixture { pacquet, state_dir, workspace, root, npmrc_info }
}

fn install(pacquet: Command, state_dir: &Path) -> Output {
    run(pacquet, state_dir, "install")
}

fn run(pacquet: Command, state_dir: &Path, command: &str) -> Output {
    pacquet.with_args([command, "--state-dir"]).with_arg(state_dir).output().expect("run pacquet")
}

fn state_file(state_dir: &Path) -> PathBuf {
    state_dir.join("pnpm-state.json")
}

fn output_text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

fn append_to_workspace_yaml(workspace: &Path, line: &str) {
    let path = workspace.join("pnpm-workspace.yaml");
    let mut text = fs::read_to_string(&path).expect("read pnpm-workspace.yaml");
    text.push_str(line);
    fs::write(&path, text).expect("write pnpm-workspace.yaml");
}

#[test]
fn an_install_announces_a_newer_pnpm_and_records_the_check() {
    let Fixture { pacquet, state_dir, root, npmrc_info, .. } = fixture();

    let output = install(pacquet, &state_dir);

    let text = output_text(&output);
    assert!(output.status.success(), "install should succeed: {text}");
    assert!(text.contains("Update available!"), "no update notice: {text}");
    assert!(text.contains(NEWER_PNPM), "the notice should name the newer version: {text}");

    let state: Value = fs::read_to_string(state_file(&state_dir))
        .expect("read pnpm-state.json")
        .parse::<serde_json::Value>()
        .expect("parse pnpm-state.json");
    assert!(state["lastUpdateCheck"].is_string(), "the check should be recorded: {state}");

    drop((root, npmrc_info));
}

#[test]
fn update_notifier_off_skips_the_check_entirely() {
    let Fixture { mut pacquet, state_dir, workspace, root, npmrc_info } = fixture();
    // Drop the fixture's opt-in so the workspace manifest is what decides.
    pacquet.env_remove("PNPM_CONFIG_UPDATE_NOTIFIER");
    append_to_workspace_yaml(&workspace, "updateNotifier: false\n");

    let output = install(pacquet, &state_dir);

    let text = output_text(&output);
    assert!(output.status.success(), "install should succeed: {text}");
    assert!(!text.contains("Update available!"), "the notice should be suppressed: {text}");
    assert!(!state_file(&state_dir).exists(), "no check means nothing to record");

    drop((root, npmrc_info));
}

/// The recorded timestamp is what keeps the check to once a day, so a
/// fresh one silences the very next install.
#[test]
fn a_check_recorded_today_silences_the_next_install() {
    let Fixture { pacquet, state_dir, root, npmrc_info, .. } = fixture();
    fs::create_dir_all(&state_dir).expect("create the state dir");
    let today = chrono::Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string();
    fs::write(state_file(&state_dir), serde_json::json!({ "lastUpdateCheck": today }).to_string())
        .expect("write pnpm-state.json");

    let output = install(pacquet, &state_dir);

    let text = output_text(&output);
    assert!(output.status.success(), "install should succeed: {text}");
    assert!(!text.contains("Update available!"), "the check was already made today: {text}");

    drop((root, npmrc_info));
}

/// `ci` and `install-test` drive the same install pipeline, but they are
/// their own commands: pnpm checks for a newer pnpm on `install` and `add`
/// only.
#[test]
fn the_other_commands_on_the_install_pipeline_do_not_check() {
    for command in ["ci", "install-test"] {
        let Fixture { pacquet, state_dir, root, npmrc_info, .. } = fixture();

        let output = run(pacquet, &state_dir, command);

        let text = output_text(&output);
        assert!(output.status.success(), "{command} should succeed: {text}");
        assert!(!text.contains("Update available!"), "{command} should not check: {text}");
        assert!(!state_file(&state_dir).exists(), "{command} should record no check");

        drop((root, npmrc_info));
    }
}
