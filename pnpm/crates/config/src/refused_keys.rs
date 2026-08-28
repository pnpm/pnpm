//! The configuration keys no manifest may set, and where each belongs
//! instead.
//!
//! Mirrors `isRefusedByAProjectManifest` / `whereRefusedKeyBelongs` in
//! pnpm's `config.reader`, so that the two stacks cannot disagree on what
//! was dropped from a config file or on the route a warning offers for it.

use crate::{
    config_types::is_config_file_key,
    naming_cases::{to_camel_case, to_kebab_case},
};
use std::{collections::HashSet, sync::OnceLock};

/// The camelCase keys a project's `pnpm-workspace.yaml` does not contribute,
/// named as in pnpm's `config.reader`: where the machine keeps what it holds
/// across runs, the directories the current command reads and writes in,
/// which credentials pnpm sends to whom, and which scope a `pnpm login`
/// claims for the machine.
const PROJECT_MANIFEST_SKIPPED_KEYS: &[&str] = &[
    "configDir",
    "globalBinDir",
    "globalDir",
    "globalPkgDir",
    "npmrcAuthFile",
    "pnpmHomeDir",
    "stateDir",
    "userconfig",
    "bin",
    "dir",
    "rootProjectManifestDir",
    "workspaceDir",
    "authConfig",
    "userConfig",
    "configByUri",
    "packageManagerNetworkConfig",
    "packageManagerRegistries",
    "scope",
];

/// The reader's own bookkeeping, named as in pnpm's `config.reader`, which
/// there shares one object with the settings but is not settable by anyone.
const CONFIG_CONTEXT_KEYS: &[&str] = &[
    "hooks",
    "finders",
    "allProjects",
    "selectedProjectsGraph",
    "allProjectsGraph",
    "prodAllProjectsGraph",
    "prodOnlySelectedProjectDirs",
    "rootProjectManifest",
    "rootProjectManifestDir",
    "cliOptions",
    "explicitlySetKeys",
    "packageManager",
    "wantedPackageManager",
];

fn refused_keys() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| {
        PROJECT_MANIFEST_SKIPPED_KEYS.iter().chain(CONFIG_CONTEXT_KEYS).copied().collect()
    })
}

/// Whether a project's `pnpm-workspace.yaml` drops `key`, given in either
/// camelCase or kebab-case, whether as a value a project may not contribute
/// or as the reader's own bookkeeping.
#[must_use]
pub fn is_refused_by_a_project_manifest(key: &str) -> bool {
    refused_keys().contains(to_camel_case(key).as_str())
}

/// The global config file key that sets a refused setting, where it is not
/// that setting's own name.
///
/// `userconfig` is accepted under its own name, but never read back: the
/// user-level `.npmrc` comes from `npmrcAuthFile`, so its own name would
/// send the user to a command that changes nothing.
fn global_equivalent_key(camel_key: &str) -> Option<&'static str> {
    match camel_key {
        // Derived from the global bin directory.
        "bin" => Some("global-bin-dir"),
        // Derived from the global directory.
        "globalPkgDir" => Some("global-dir"),
        "userconfig" => Some("npmrc-auth-file"),
        _ => None,
    }
}

/// Where `camel_key` can be set, for a key a project manifest refuses.
#[must_use]
pub fn where_refused_key_belongs(camel_key: &str) -> String {
    if camel_key == "dir" {
        return "Pass --dir on the command line instead".to_string();
    }
    let kebab_key =
        global_equivalent_key(camel_key).map_or_else(|| to_kebab_case(camel_key), str::to_string);
    if is_config_file_key(&kebab_key) {
        format!("Set it for the machine instead: pnpm config set --global {kebab_key}")
    } else {
        "This is not a pnpm setting".to_string()
    }
}

#[cfg(test)]
mod tests;
