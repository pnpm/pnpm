use super::{WithError, disable_package_manager_switching, prepend_to_path};
use crate::cli_args::{
    package_manager::PACKAGE_MANAGER_SWITCH_ENV_VARS,
    with::install_pnpm_to_store::slot_from_package_dir,
};
use std::{ffi::OsStr, path::Path, process::Command};

#[test]
fn prepend_to_path_rejects_a_delimiter_in_the_bin_dir() {
    let delimiter = if cfg!(windows) { "a;b" } else { "a:b" };
    let error = prepend_to_path(Path::new(delimiter)).expect_err("must reject the delimiter");
    assert!(matches!(error, WithError::BadPathDir { .. }));
}

#[test]
fn prepend_to_path_accepts_a_normal_bin_dir() {
    let dir = if cfg!(windows) { r"C:\store\bin" } else { "/store/bin" };
    let path = prepend_to_path(Path::new(dir)).expect("a normal dir is accepted");
    assert!(path.to_string_lossy().starts_with(dir));
}

#[test]
fn child_pnpm_disables_all_package_manager_switch_env_variants() {
    let mut command = Command::new("pnpm");

    disable_package_manager_switching(&mut command);

    for name in PACKAGE_MANAGER_SWITCH_ENV_VARS {
        let value = command_env_value(&command, name);
        assert_eq!(value, Some(OsStr::new("false")), "expected {name}=false");
    }
}

fn command_env_value<'command>(command: &'command Command, name: &str) -> Option<&'command OsStr> {
    command.get_envs().find(|(key, _)| env_key_matches(key, name)).and_then(|(_, value)| value)
}

#[cfg(windows)]
fn env_key_matches(key: &OsStr, name: &str) -> bool {
    key.to_str().is_some_and(|key| key.eq_ignore_ascii_case(name))
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
    let package_dir = slot.join("node_modules").join("@pnpm").join("exe");

    assert_eq!(slot_from_package_dir(&package_dir, "@pnpm/exe").as_deref(), Some(slot));
}
