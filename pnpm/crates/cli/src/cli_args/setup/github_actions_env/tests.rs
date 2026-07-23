use super::{BadValue, persist, validate_persisted_values, write_files};
use pacquet_config::EnvVarOs;
use pacquet_reporter::{LogEvent, LogLevel, PnpmLog, Reporter, SilentReporter};
use pretty_assertions::assert_eq;
use std::{
    ffi::OsString,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

/// Outside a workflow every lookup is unset, so both the gate and the
/// value validation must fall through.
struct NoEnv;
impl EnvVarOs for NoEnv {
    fn var_os(_name: &str) -> Option<OsString> {
        None
    }
}

/// Inside a workflow with only `GITHUB_ENV` provided. The paths never
/// reach the filesystem in the tests that use this fake.
struct InGitHubActions;
impl EnvVarOs for InGitHubActions {
    fn var_os(name: &str) -> Option<OsString> {
        match name {
            "GITHUB_ACTIONS" => Some(OsString::from("true")),
            "GITHUB_ENV" => Some(OsString::from("/github/env")),
            _ => None,
        }
    }
}

#[test]
fn persist_writes_both_targets_named_by_the_environment() {
    static GITHUB_ENV: OnceLock<PathBuf> = OnceLock::new();
    static GITHUB_PATH: OnceLock<PathBuf> = OnceLock::new();
    struct WorkflowEnv;
    impl EnvVarOs for WorkflowEnv {
        fn var_os(name: &str) -> Option<OsString> {
            match name {
                "GITHUB_ACTIONS" => Some(OsString::from("true")),
                "GITHUB_ENV" => Some(GITHUB_ENV.get()?.clone().into_os_string()),
                "GITHUB_PATH" => Some(GITHUB_PATH.get()?.clone().into_os_string()),
                _ => None,
            }
        }
    }

    let dir = tempfile::tempdir().expect("create temp dir");
    let pnpm_home_dir = dir.path().join("pnpm-home");
    let bin_dir = pnpm_home_dir.join("bin");
    let github_env = GITHUB_ENV.get_or_init(|| dir.path().join("github-env"));
    let github_path = GITHUB_PATH.get_or_init(|| dir.path().join("github-path"));
    std::fs::write(github_env, "").expect("create github env");
    std::fs::write(github_path, "").expect("create github path");

    persist::<SilentReporter, WorkflowEnv>(dir.path(), &pnpm_home_dir, &bin_dir);

    assert_eq!(
        std::fs::read_to_string(github_env).expect("read github env"),
        format!("PNPM_HOME={}\n", pnpm_home_dir.display()),
    );
    assert_eq!(
        std::fs::read_to_string(github_path).expect("read github path"),
        format!("{}\n", bin_dir.display()),
    );
}

#[test]
fn persist_writes_nothing_outside_github_actions() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let pnpm_home_dir = dir.path().join("pnpm-home");
    let bin_dir = pnpm_home_dir.join("bin");

    persist::<SilentReporter, NoEnv>(dir.path(), &pnpm_home_dir, &bin_dir);

    assert_eq!(std::fs::read_dir(dir.path()).expect("read temp dir").count(), 0);
}

#[test]
fn files_receive_home_and_bin() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let pnpm_home_dir = dir.path().join("pnpm-home");
    let bin_dir = pnpm_home_dir.join("bin");
    let github_env = dir.path().join("github-env");
    let github_path = dir.path().join("github-path");
    std::fs::write(&github_env, "").expect("create github env");
    std::fs::write(&github_path, "").expect("create github path");

    write_files::<SilentReporter>(
        dir.path(),
        &pnpm_home_dir,
        &bin_dir,
        Some(&github_env),
        Some(&github_path),
    );

    assert_eq!(
        std::fs::read_to_string(github_env).expect("read github env"),
        format!("PNPM_HOME={}\n", pnpm_home_dir.display()),
    );
    assert_eq!(
        std::fs::read_to_string(github_path).expect("read github path"),
        format!("{}\n", bin_dir.display()),
    );
}

#[test]
fn files_start_new_records_after_existing_content() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let pnpm_home_dir = dir.path().join("pnpm-home");
    let bin_dir = pnpm_home_dir.join("bin");
    let github_env = dir.path().join("github-env");
    let github_path = dir.path().join("github-path");
    std::fs::write(&github_env, "EXISTING=value").expect("create github env");
    std::fs::write(&github_path, "/existing/bin").expect("create github path");

    write_files::<SilentReporter>(
        dir.path(),
        &pnpm_home_dir,
        &bin_dir,
        Some(&github_env),
        Some(&github_path),
    );

    assert_eq!(
        std::fs::read_to_string(github_env).expect("read github env"),
        format!("EXISTING=value\nPNPM_HOME={}\n", pnpm_home_dir.display()),
    );
    assert_eq!(
        std::fs::read_to_string(github_path).expect("read github path"),
        format!("/existing/bin\n{}\n", bin_dir.display()),
    );
}

#[test]
fn each_available_target_is_written_independently() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let pnpm_home_dir = dir.path().join("pnpm-home");
    let bin_dir = pnpm_home_dir.join("bin");
    let github_env = dir.path().join("github-env");
    let github_path = dir.path().join("github-path");
    std::fs::write(&github_env, "").expect("create github env");
    std::fs::write(&github_path, "").expect("create github path");

    write_files::<SilentReporter>(dir.path(), &pnpm_home_dir, &bin_dir, Some(&github_env), None);
    write_files::<SilentReporter>(dir.path(), &pnpm_home_dir, &bin_dir, None, Some(&github_path));

    assert_eq!(
        std::fs::read_to_string(github_env).expect("read github env"),
        format!("PNPM_HOME={}\n", pnpm_home_dir.display()),
    );
    assert_eq!(
        std::fs::read_to_string(github_path).expect("read github path"),
        format!("{}\n", bin_dir.display()),
    );
}

#[test]
fn a_failing_target_does_not_skip_the_others() {
    static WARNINGS: Mutex<Vec<String>> = Mutex::new(Vec::new());
    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            if let LogEvent::Pnpm(PnpmLog { level: LogLevel::Warn, message, .. }) = event {
                WARNINGS.lock().expect("lock warnings").push(message.clone());
            }
        }
    }

    let dir = tempfile::tempdir().expect("create temp dir");
    let pnpm_home_dir = dir.path().join("pnpm-home");
    let bin_dir = pnpm_home_dir.join("bin");
    // A path longer than NAME_MAX fails regardless of the user id, so the
    // failure branch is reached on a root-run runner too.
    let github_env = dir.path().join("a".repeat(300));
    let github_path = dir.path().join("github-path");
    std::fs::write(&github_path, "").expect("create github path");

    write_files::<RecordingReporter>(
        dir.path(),
        &pnpm_home_dir,
        &bin_dir,
        Some(&github_env),
        Some(&github_path),
    );

    assert_eq!(
        std::fs::read_to_string(github_path).expect("read github path"),
        format!("{}\n", bin_dir.display()),
    );
    let warnings = WARNINGS.lock().expect("lock warnings");
    assert_eq!(warnings.len(), 1);
    eprintln!("WARNING:\n{}\n", warnings[0]);
    assert!(warnings[0].contains("GITHUB_ENV"));
    assert!(warnings[0].contains(&github_env.display().to_string()));
}

#[test]
fn non_regular_targets_are_skipped() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let pnpm_home_dir = dir.path().join("pnpm-home");
    let bin_dir = pnpm_home_dir.join("bin");
    let github_env = dir.path().join("github-env-dir");
    let github_path = dir.path().join("github-path");
    std::fs::create_dir(&github_env).expect("create github env dir");
    std::fs::write(&github_path, "").expect("create github path");

    write_files::<SilentReporter>(
        dir.path(),
        &pnpm_home_dir,
        &bin_dir,
        Some(&github_env),
        Some(&github_path),
    );

    assert_eq!(std::fs::read_dir(&github_env).expect("read github env dir").count(), 0);
    assert_eq!(
        std::fs::read_to_string(github_path).expect("read github path"),
        format!("{}\n", bin_dir.display()),
    );
}

#[test]
fn missing_targets_are_not_created() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let pnpm_home_dir = dir.path().join("pnpm-home");
    let bin_dir = pnpm_home_dir.join("bin");
    let github_env = dir.path().join("missing-github-env");

    write_files::<SilentReporter>(dir.path(), &pnpm_home_dir, &bin_dir, Some(&github_env), None);

    assert!(!github_env.exists(), "{} should not have been created", github_env.display());
}

#[test]
fn values_with_line_breaks_are_rejected_inside_github_actions() {
    let pnpm_home_dir = PathBuf::from("/tmp/pnpm-home\nINJECTED=value");
    let bin_dir = pnpm_home_dir.join("bin");

    let err = validate_persisted_values::<InGitHubActions>(&pnpm_home_dir, &bin_dir)
        .expect_err("reject newline");

    assert_eq!(err.to_string(), "PNPM_HOME cannot contain newline or NUL characters");
    assert_eq!(
        err.downcast_ref::<BadValue>().map(BadValue::to_string).as_deref(),
        Some("PNPM_HOME cannot contain newline or NUL characters"),
    );
    assert_eq!(
        err.code().expect("diagnostic code").to_string(),
        "ERR_PNPM_BAD_GITHUB_ACTIONS_ENVIRONMENT_VALUE",
    );
}

#[test]
fn values_are_not_validated_outside_github_actions() {
    let pnpm_home_dir = PathBuf::from("/tmp/pnpm-home\nINJECTED=value");
    let bin_dir = pnpm_home_dir.join("bin");

    validate_persisted_values::<NoEnv>(&pnpm_home_dir, &bin_dir)
        .expect("no value is persisted outside GitHub Actions");
}
