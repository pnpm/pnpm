//! Pacquet port of pnpm's `@pnpm/installing.env-installer`.
//!
//! Resolves and installs *configurational dependencies* — the packages
//! declared under `configDependencies` in `pnpm-workspace.yaml`. They
//! are installed ahead of regular dependencies, into
//! `node_modules/.pnpm-config/<name>`, and recorded in the env lockfile
//! (the first YAML document of `pnpm-lock.yaml`, see
//! [`pacquet_lockfile::EnvLockfile`]).
//!
//! Mirrors the upstream package at
//! <https://github.com/pnpm/pnpm/blob/31858c544b/installing/env-installer/src>.
//! The install primitive (`import_indexed_dir`) lives in
//! `pacquet-package-manager`, so this crate depends on it; the
//! config-finalization seam in the CLI drives this crate before the
//! main install runs.

mod errors;
mod install_config_deps;
mod manifest_lockfile;
mod options;
mod parse_integrity;
mod prune;
mod resolve_and_install_config_deps;
mod resolve_optional_subdeps;
mod resolve_package_manager_integrities;
mod verify_env_lockfile;

pub use errors::ConfigDepError;
pub use install_config_deps::install_config_deps;
pub use options::ConfigDepsInstallOptions;
pub use parse_integrity::{NormalizedConfigDep, NormalizedSubdep, parse_integrity};
pub use prune::prune_env_lockfile;
pub use resolve_and_install_config_deps::resolve_and_install_config_deps;
pub use resolve_optional_subdeps::resolve_optional_subdeps;
pub use resolve_package_manager_integrities::{
    is_package_manager_resolved, resolve_package_manager_integrities,
};
pub use verify_env_lockfile::{verify_env_lockfile, write_verified_env_lockfile};

#[cfg(test)]
mod tests;
