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
