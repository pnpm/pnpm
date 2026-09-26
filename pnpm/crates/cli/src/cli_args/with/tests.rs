use super::{PackageManagerCheck, configure_pnpm_environment};
use crate::{
    cli_args::package_manager::PACKAGE_MANAGER_SWITCH_ENV_VARS,
    engine_pm::install::slot_from_package_dir,
};
use std::{ffi::OsStr, path::Path, process::Command};

#[test]
fn child_pnpm_disables_all_package_manager_switch_env_variants() {
    let mut command = Command::new("pnpm");
    let bin_dir = Path::new("downloaded-pnpm").to_path_buf();

    configure_pnpm_environment(&mut command, &[bin_dir], PackageManagerCheck::Disabled)
        .expect("configure pnpm environment");

    for name in PACKAGE_MANAGER_SWITCH_ENV_VARS {
        let value = command_env_value(&command, name);
        assert_eq!(value, Some(OsStr::new("false")), "expected {name}=false");
    }
}

#[test]
fn automatically_switched_pnpm_inherits_the_parent_environment() {
    let mut command = Command::new("pnpm");
    let bin_dir = Path::new("downloaded-pnpm").to_path_buf();

    configure_pnpm_environment(&mut command, &[bin_dir], PackageManagerCheck::Enabled)
        .expect("configure pnpm environment");

    assert_eq!(command.get_envs().count(), 0);
}

fn command_env_value<'command>(command: &'command Command, name: &str) -> Option<&'command OsStr> {
    command
        .get_envs()
        .find(|(key, _)| env_key_matches(key, name))
        .and_then(|(_, value)| value)
}

#[cfg(windows)]
fn env_key_matches(key: &OsStr, name: &str) -> bool {
    key.to_str()
        .is_some_and(|key| key.eq_ignore_ascii_case(name))
}

#[cfg(not(windows))]
fn env_key_matches(key: &OsStr, name: &str) -> bool {
    key == OsStr::new(name)
}

#[test]
fn resolves_unscoped_package_dir_to_global_virtual_store_slot() {
    let slot = Path::new("/store/links/hash");
    let package_dir = slot.join("node_modules").join("pnpm");

    assert_eq!(slot_from_package_dir(&package_dir, "pnpm").as_deref(), Some(slot));
}

#[test]
fn resolves_scoped_package_dir_to_global_virtual_store_slot() {
    let slot = Path::new("/store/links/hash");
    let package_dir = slot
        .join("node_modules")
        .join("@pnpm")
        .join("exe");

    assert_eq!(slot_from_package_dir(&package_dir, "@pnpm/exe").as_deref(), Some(slot));
}

/// A `SIGTERM` sent to pnpm while it runs the pnpm it switched to must reach
/// that pnpm. Without the relay, pnpm dies from the signal and leaves the
/// other one running with nobody to wait for it.
#[cfg(unix)]
#[test]
fn a_termination_of_pnpm_reaches_the_pnpm_it_switched_to() {
    use super::spawn_pnpm;
    use std::{fs, os::unix::fs::PermissionsExt, thread, time::Duration};

    let dir = tempfile::tempdir().expect("create a temp dir");
    let bin_dir = dir.path().join("bin");
    fs::create_dir(&bin_dir).expect("create the engine bin dir");
    let fake_pnpm = bin_dir.join("pnpm");
    fs::write(
        &fake_pnpm,
        "#!/bin/sh\n\
         trap 'echo terminated > \"$1/got-sigterm\"; exit 0' TERM\n\
         echo started > \"$1/started\"\n\
         while :; do sleep 0.1; done\n",
    )
    .expect("write the fake pnpm");
    fs::set_permissions(&fake_pnpm, fs::Permissions::from_mode(0o755))
        .expect("make the fake pnpm executable");

    let workdir = dir.path().to_path_buf();
    let switched = thread::spawn({
        let workdir = workdir.clone();
        move || spawn_pnpm(&[bin_dir], [workdir], PackageManagerCheck::Enabled)
    });
    let started = workdir.join("started");
    for _ in 0..600 {
        if started.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert!(started.exists(), "the switched pnpm should have started");

    // SAFETY: signalling this test's own process, which is what `kill` aimed
    // at pnpm alone does.
    let signalled = unsafe { libc::kill(libc::getpid(), libc::SIGTERM) };
    assert_eq!(signalled, 0, "the signal should reach pnpm");

    let status = switched
        .join()
        .expect("the spawning thread should not panic")
        .expect("the switched pnpm should be spawned");
    assert!(status.success(), "the switched pnpm should exit on its own terms: {status:?}");
    assert!(
        workdir.join("got-sigterm").exists(),
        "the switched pnpm should have received the SIGTERM",
    );
}
