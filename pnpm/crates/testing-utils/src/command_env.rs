//! Keep the pnpm processes a test spawns free of the ambient pnpm
//! configuration.

use std::{ffi::OsString, process::Command};

/// Test-only additions to [`Command`].
pub trait CommandTestExt {
    /// Strip the pnpm and npm configuration variables out of the inherited
    /// environment, so the spawned process is configured only by the
    /// test's own `.npmrc` / workspace YAML and whatever the caller sets
    /// afterwards.
    ///
    /// A spawned command inherits this process's environment, and pnpm
    /// reads settings from `PNPM_CONFIG_*` / `pnpm_config_*` as well as
    /// the npm spellings. One such variable in CI or in a contributor's
    /// shell therefore reconfigures the pnpm under test: `pnpm/setup`
    /// exports `PNPM_CONFIG_GLOBAL_SHIMS` to pin the runtime it installed,
    /// which flipped the shim style the global-shims suites assert on.
    ///
    /// Call this at construction time so a later `env` still wins for
    /// tests that exercise one of these settings deliberately.
    #[must_use]
    fn without_ambient_pnpm_config(self) -> Self;
}

impl CommandTestExt for Command {
    fn without_ambient_pnpm_config(mut self) -> Self {
        for name in ambient_pnpm_config_vars() {
            self.env_remove(name);
        }
        self
    }
}

/// The names in the current environment that [`is_pnpm_config_var`]
/// matches.
fn ambient_pnpm_config_vars() -> impl Iterator<Item = OsString> {
    std::env::vars_os()
        .map(|(name, _)| name)
        .filter(|name| name.to_str().is_some_and(is_pnpm_config_var))
}

/// Whether `name` configures pnpm: either spelling of the `pnpm_config_`
/// / `npm_config_` prefixes, or the context-aware shim kill switch.
/// `PNPM_HOME` is deliberately not one of them — the suites spawn the
/// ambient pnpm for their compatibility checks.
fn is_pnpm_config_var(name: &str) -> bool {
    const PREFIXES: [&str; 2] = ["pnpm_config_", "npm_config_"];
    const SHIM_BYPASS: &str = "PNPM_SHIM_BYPASS";

    name.eq_ignore_ascii_case(SHIM_BYPASS)
        || PREFIXES.iter().any(|prefix| {
            name.len() > prefix.len() && name[..prefix.len()].eq_ignore_ascii_case(prefix)
        })
}

#[cfg(test)]
mod tests;
