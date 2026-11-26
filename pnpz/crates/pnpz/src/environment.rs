use crate::{
    lockfile::Lockfile,
    store::Store,
};
use std::{
    env,
    ffi::OsString,
};

/// The `PATH` a command runs with: the `bin` directory of every package in
/// `system`'s environment, ahead of the caller's own `PATH`.
///
/// The entries are `/nix/store` paths, valid where the store is mounted, and
/// only those `store` actually holds a `bin` directory for.
pub fn search_path(lockfile: &Lockfile, system: &str, store: &Store) -> OsString {
    let package_directories = lockfile
        .environment_paths(system)
        .into_iter()
        .filter(|path| {
            store
                .location_of(path)
                .join("bin")
                .is_dir()
        })
        .map(|path| format!("{path}/bin").into());
    let inherited = env::var_os("PATH").unwrap_or_default();
    let directories: Vec<OsString> = package_directories
        .chain(env::split_paths(&inherited).map(Into::into))
        .collect();
    env::join_paths(directories).expect("store paths and inherited PATH entries hold no separator")
}

/// The store paths of `system`'s environment, for display.
#[must_use]
pub fn describe(lockfile: &Lockfile, system: &str) -> Vec<String> {
    lockfile.packages
        .iter()
        .filter(|(_, package)| package.systems.contains_key(system))
        .map(|(name, package)| format!("{name} {}", package.version))
        .collect()
}
