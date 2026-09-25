use super::{finish_legacy_migration, legacy_global_add_specs, migrate_legacy_global_packages};
use pnpm_local_spec::LocalSpec;
use pretty_assertions::assert_eq;
use std::path::{Path, PathBuf};

#[test]
fn legacy_global_add_specs_skips_pnpm_and_packages_already_installed() {
    let dependencies = serde_json::json!({
        "pnpm": "10.15.0",
        "@pnpm/exe": "10.15.0",
        "typescript": "^5.4.0",
        "prettier": "3.0.0",
        "empty": "",
        "broken": 1,
    });
    let dependencies = dependencies.as_object().expect("object");
    let already_installed = std::iter::once("prettier".to_string()).collect();
    assert_eq!(
        legacy_global_add_specs(dependencies, &already_installed, None),
        ["typescript@^5.4.0"],
    );
}

#[test]
fn legacy_global_add_specs_resolves_relative_file_dependencies_against_the_manifest_dir() {
    let dependencies = serde_json::json!({
        "my-cli": "file:../packages/cli",
        "dotted": "file:/./pkg",
        "bare": "../packages/other",
        "abs": "file:/tmp/pkg",
        "typescript": "^5.4.0",
        "hosted": "user/repo",
    });
    let dependencies = dependencies.as_object().expect("object");
    let manifest_dir = PathBuf::from("/home/user/.local/share/pnpm/global/5");
    let specs = legacy_global_add_specs(
        dependencies,
        &std::collections::BTreeSet::new(),
        Some(&manifest_dir),
    );
    let anchored = |spec: &str| {
        LocalSpec::parse_filesystem(spec, &manifest_dir).expect("local spec").render(None)
    };
    let mut expected = vec![
        "abs@file:/tmp/pkg".to_string(),
        format!("bare@{}", anchored("../packages/other")),
        format!("dotted@{}", anchored("file:/./pkg")),
        "hosted@user/repo".to_string(),
        format!("my-cli@{}", anchored("file:../packages/cli")),
        "typescript@^5.4.0".to_string(),
    ];
    expected.sort();
    assert_eq!(specs, expected);
}

#[test]
fn a_failed_legacy_migration_is_reported_and_does_not_stop_setup() {
    use std::sync::Mutex;

    use pnpm_reporter::{LogEvent, LogLevel, Reporter};

    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS
                .lock()
                .expect("lock")
                .push(event.clone());
        }
    }

    EVENTS.lock().expect("lock").clear();
    let dir = PathBuf::from("/proj");
    finish_legacy_migration::<RecordingReporter>(&dir, Ok(()));
    assert!(EVENTS.lock().expect("lock").is_empty());

    finish_legacy_migration::<RecordingReporter>(
        &dir,
        Err(miette::miette!("Failed to migrate global packages (exit code 1)")),
    );
    let events = EVENTS.lock().expect("lock").clone();
    let LogEvent::Pnpm(log) = &events[0] else {
        panic!("expected a pnpm log, got {events:?}");
    };
    assert_eq!(log.level, LogLevel::Warn);
    assert_eq!(log.prefix, "/proj");
    assert!(log.message.contains("exit code 1"), "{}", log.message);
}

fn write_legacy_manifest(home: &Path, body: &str) {
    let dir = home.join("global").join("5");
    std::fs::create_dir_all(&dir).expect("create legacy global dir");
    std::fs::write(dir.join("package.json"), body).expect("write legacy manifest");
}

fn migrate(home: &Path, exec_path: &Path) -> miette::Result<()> {
    migrate_legacy_global_packages::<pnpm_reporter::SilentReporter>(exec_path, home, home)
}

#[test]
fn migrate_legacy_global_packages_ignores_a_missing_or_unusable_manifest() {
    let home = tempfile::tempdir().expect("create temp dir");
    let exec_path = home.path().join("missing-pnpm");
    migrate(home.path(), &exec_path).expect("missing manifest is a no-op");

    write_legacy_manifest(home.path(), "not json");
    migrate(home.path(), &exec_path).expect("unreadable json is a no-op");

    write_legacy_manifest(home.path(), r#"{"dependencies":[]}"#);
    migrate(home.path(), &exec_path).expect("non-object dependencies are a no-op");

    write_legacy_manifest(home.path(), r#"{"dependencies":{"pnpm":"10.15.0"}}"#);
    migrate(home.path(), &exec_path).expect("pnpm itself is not reinstalled");
}

#[cfg(unix)]
fn write_fake_pnpm(dir: &Path, exit_code: i32) -> PathBuf {
    let path = dir.join("pnpm");
    let record = dir.join("record.txt");
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf '%s\\n' \"$PNPM_HOME\" >> '{}'\nprintf '%s\\n' \"$PATH\" >> '{}'\nexit {exit_code}\n",
            record.display(),
            record.display(),
            record.display(),
        ),
    )
    .expect("write fake pnpm");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

#[cfg(windows)]
fn write_fake_pnpm(dir: &Path, exit_code: i32) -> PathBuf {
    let path = dir.join("pnpm.cmd");
    let record = dir.join("record.txt");
    std::fs::write(
        &path,
        format!(
            "@echo off\r\necho %* > \"{}\"\r\necho %PNPM_HOME% >> \"{}\"\r\necho %PATH% >> \"{}\"\r\nexit /b {exit_code}\r\n",
            record.display(),
            record.display(),
            record.display(),
        ),
    )
    .expect("write fake pnpm");
    path
}

#[test]
fn migrate_legacy_global_packages_reinstalls_recorded_packages() {
    let home = tempfile::tempdir().expect("create temp dir");
    write_legacy_manifest(
        home.path(),
        r#"{"dependencies":{"pnpm":"10.15.0","typescript":"^5.4.0"}}"#,
    );
    let exec_path = write_fake_pnpm(home.path(), 0);

    migrate(home.path(), &exec_path).expect("migration succeeds");

    let record = std::fs::read_to_string(home.path().join("record.txt")).expect("read record");
    assert!(record.contains("typescript@^5.4.0"), "{record}");
    assert!(record.contains(&home.path().display().to_string()), "{record}");
}

fn link_install_group(global: &Path, install: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(install, global.join("abc")).expect("link install group");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(install, global.join("abc")).expect("link install group");
}

#[test]
fn migrate_legacy_global_packages_skips_packages_already_in_the_current_layout() {
    let home = tempfile::tempdir().expect("create temp dir");
    let install = home.path().join("install");
    std::fs::create_dir_all(&install).expect("create install dir");
    std::fs::write(install.join("package.json"), r#"{"dependencies":{"prettier":"3.0.0"}}"#)
        .expect("write current manifest");
    let global = home
        .path()
        .join("global")
        .join(pnpm_config::GLOBAL_LAYOUT_VERSION);
    std::fs::create_dir_all(&global).expect("create current global dir");
    link_install_group(&global, &install);
    write_legacy_manifest(home.path(), r#"{"dependencies":{"prettier":"3.0.0"}}"#);
    let exec_path = home.path().join("missing-pnpm");

    migrate(home.path(), &exec_path).expect("already installed package is skipped");
    assert!(!home.path().join("record.txt").exists());
}

#[test]
fn migrate_legacy_global_packages_fails_when_reinstall_exits_nonzero() {
    let home = tempfile::tempdir().expect("create temp dir");
    write_legacy_manifest(home.path(), r#"{"dependencies":{"typescript":"^5.4.0"}}"#);
    let exec_path = write_fake_pnpm(home.path(), 1);

    let error = migrate(home.path(), &exec_path).expect_err("nonzero exit fails setup");
    assert!(error.to_string().contains("exit code 1"), "{error}");
}

#[test]
fn migrate_legacy_global_packages_fails_when_reinstall_cannot_start() {
    let home = tempfile::tempdir().expect("create temp dir");
    write_legacy_manifest(home.path(), r#"{"dependencies":{"typescript":"^5.4.0"}}"#);

    let error = migrate(home.path(), &home.path().join("missing-pnpm"))
        .expect_err("missing executable fails setup");
    assert!(error.to_string().contains("run the global package migration"), "{error}");
}
