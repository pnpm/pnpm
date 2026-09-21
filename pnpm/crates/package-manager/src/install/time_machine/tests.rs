#[cfg(unix)]
use super::run_command;
use super::{
    TMUTIL_MAX_PATHS_PER_BATCH, new_directories_to_exclude, tmutil_command, tmutil_failure_message,
};
use pnpm_config::Config;
use pnpm_store_dir::StoreDir;
use std::{ffi::OsStr, fs, io, path::PathBuf};
#[cfg(unix)]
use std::{process::Command, time::Duration};
use tempfile::tempdir;

#[test]
fn selects_only_missing_configured_directories() {
    let temp = tempdir().unwrap();
    let modules_dir = temp.path().join("node_modules");
    let virtual_store_dir = temp.path().join("external virtual store");
    let store_dir = temp.path().join("store");
    fs::create_dir_all(&modules_dir).unwrap();
    let config = Config {
        modules_dir,
        virtual_store_dir: virtual_store_dir.clone(),
        store_dir: StoreDir::new(&store_dir),
        macos_backup: pnpm_config::MacosBackupConfig { modules_dir: false, store_dir: false },
        ..Config::default()
    };

    assert_eq!(
        new_directories_to_exclude(&config, temp.path(), &[]),
        vec![virtual_store_dir, store_dir.join(pnpm_store_dir::STORE_VERSION)],
    );
}

#[test]
fn an_embedded_virtual_store_uses_the_modules_exclusion() {
    let temp = tempdir().unwrap();
    let modules_dir = temp.path().join("node_modules");
    let config = Config {
        modules_dir: modules_dir.clone(),
        virtual_store_dir: modules_dir.join(".pnpm"),
        macos_backup: pnpm_config::MacosBackupConfig { modules_dir: false, ..Default::default() },
        ..Config::default()
    };

    assert_eq!(new_directories_to_exclude(&config, temp.path(), &[]), vec![modules_dir]);
}

#[test]
fn selects_a_missing_embedded_virtual_store_under_existing_modules() {
    let temp = tempdir().unwrap();
    let modules_dir = temp.path().join("node_modules");
    fs::create_dir(&modules_dir).unwrap();
    let virtual_store_dir = modules_dir.join(".pnpm");
    let config = Config {
        modules_dir,
        virtual_store_dir: virtual_store_dir.clone(),
        macos_backup: pnpm_config::MacosBackupConfig { modules_dir: false, ..Default::default() },
        ..Config::default()
    };

    assert_eq!(new_directories_to_exclude(&config, temp.path(), &[]), vec![virtual_store_dir]);
}

#[test]
fn selects_missing_workspace_modules_directories() {
    let temp = tempdir().unwrap();
    let modules_dir = temp.path().join("node_modules");
    fs::create_dir(&modules_dir).unwrap();
    fs::create_dir(modules_dir.join(".pnpm")).unwrap();
    let project_dir = temp.path().join("packages/child");
    fs::create_dir_all(&project_dir).unwrap();
    let project_modules_dir = project_dir.join("node_modules");
    let config = Config {
        modules_dir: modules_dir.clone(),
        virtual_store_dir: modules_dir.join(".pnpm"),
        macos_backup: pnpm_config::MacosBackupConfig { modules_dir: false, ..Default::default() },
        ..Config::default()
    };

    assert_eq!(
        new_directories_to_exclude(&config, temp.path(), &[project_dir]),
        vec![project_modules_dir],
    );
}

#[test]
fn deduplicates_workspace_modules_directories() {
    let temp = tempdir().unwrap();
    let modules_dir = temp.path().join("node_modules");
    fs::create_dir_all(modules_dir.join(".pnpm")).unwrap();
    let project_dir = temp.path().join("packages/child");
    fs::create_dir_all(&project_dir).unwrap();
    let config = Config {
        virtual_store_dir: modules_dir.join(".pnpm"),
        modules_dir,
        macos_backup: pnpm_config::MacosBackupConfig { modules_dir: false, ..Default::default() },
        ..Config::default()
    };

    assert_eq!(
        new_directories_to_exclude(
            &config,
            temp.path(),
            &[project_dir.clone(), project_dir.clone()],
        ),
        vec![project_dir.join("node_modules")],
    );
}

#[test]
fn selects_relocated_workspace_modules_directories() {
    let temp = tempdir().unwrap();
    let workspace_root = temp.path().join("workspace");
    let project_dir = workspace_root.join("packages/child");
    fs::create_dir_all(&project_dir).unwrap();
    let modules_dir = workspace_root.join("nested/node_modules");
    let config = Config {
        modules_dir: modules_dir.clone(),
        virtual_store_dir: modules_dir.join(".pnpm"),
        macos_backup: pnpm_config::MacosBackupConfig { modules_dir: false, ..Default::default() },
        ..Config::default()
    };

    assert_eq!(
        new_directories_to_exclude(&config, &workspace_root, std::slice::from_ref(&project_dir)),
        vec![
            modules_dir,
            project_dir.join("node_modules"),
            workspace_root.join("nested/packages/child/node_modules"),
        ],
    );
}

#[test]
fn normalizes_virtual_store_containment() {
    let temp = tempdir().unwrap();
    let modules_dir = temp.path().join("node_modules");
    let virtual_store_dir = modules_dir.join("../virtual-store");
    let config = Config {
        modules_dir: modules_dir.clone(),
        virtual_store_dir,
        macos_backup: pnpm_config::MacosBackupConfig { modules_dir: false, ..Default::default() },
        ..Config::default()
    };

    assert_eq!(
        new_directories_to_exclude(&config, temp.path(), &[]),
        vec![modules_dir, temp.path().join("virtual-store")],
    );
}

#[test]
fn selects_the_effective_global_virtual_store() {
    let temp = tempdir().unwrap();
    let modules_dir = temp.path().join("node_modules");
    let global_virtual_store_dir = temp.path().join("global virtual store");
    let config = Config {
        modules_dir: modules_dir.clone(),
        virtual_store_dir: modules_dir.join(".pnpm"),
        global_virtual_store_dir: global_virtual_store_dir.clone(),
        enable_global_virtual_store: true,
        macos_backup: pnpm_config::MacosBackupConfig { modules_dir: false, ..Default::default() },
        ..Config::default()
    };

    assert_eq!(
        new_directories_to_exclude(&config, temp.path(), &[]),
        vec![modules_dir, global_virtual_store_dir],
    );
}

#[test]
fn passes_paths_as_arguments_without_a_shell() {
    let path = PathBuf::from("/tmp/project; touch injected");
    let command = tmutil_command(std::slice::from_ref(&path));

    assert_eq!(command.get_program(), OsStr::new("/usr/bin/tmutil"));
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        vec![OsStr::new("addexclusion"), path.as_os_str()],
    );
}

#[test]
fn bounds_the_number_of_paths_per_tmutil_invocation() {
    let paths = (0..=TMUTIL_MAX_PATHS_PER_BATCH)
        .map(|index| PathBuf::from(format!("/tmp/project-{index}")))
        .collect::<Vec<_>>();
    let commands = paths
        .chunks(TMUTIL_MAX_PATHS_PER_BATCH)
        .map(tmutil_command)
        .collect::<Vec<_>>();

    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0].get_args().count(), TMUTIL_MAX_PATHS_PER_BATCH + 1);
    assert_eq!(commands[1].get_args().count(), 2);
}

#[test]
fn reports_spawn_and_timeout_failures() {
    let spawn_error =
        tmutil_failure_message(Ok(Err(io::Error::new(io::ErrorKind::NotFound, "tmutil missing"))))
            .unwrap();
    let timeout = tmutil_failure_message(Ok(Ok(None))).unwrap();

    assert!(spawn_error.contains("tmutil missing"));
    assert!(timeout.contains("Timed out after 30 seconds"));
}

#[cfg(unix)]
#[test]
fn drains_output_and_enforces_the_command_timeout() {
    let mut noisy = Command::new("/bin/sh");
    noisy.args(["-c", "yes diagnostic | head -c 200000 >&2; exit 7"]);
    let output = run_command(noisy, Duration::from_secs(2)).unwrap().unwrap();
    assert!(!output.status.success());
    assert_eq!(output.stderr.len(), 200_000);

    let mut stalled = Command::new("/bin/sh");
    stalled.args(["-c", "while :; do :; done"]);
    assert!(run_command(stalled, Duration::from_millis(10)).unwrap().is_none());
}

#[cfg(unix)]
#[test]
fn sanitizes_nonzero_tmutil_diagnostics() {
    use std::{os::unix::process::ExitStatusExt, process::Output};

    let message = tmutil_failure_message(Ok(Ok(Some(Output {
        status: std::process::ExitStatus::from_raw(1),
        stdout: Vec::new(),
        stderr: b"bad\x1b[31m output".to_vec(),
    }))))
    .unwrap();

    assert!(message.contains("bad[31m output"));
    assert!(!message.contains('\x1b'));
}
