//! Port of pnpm's
//! [`checkGlobalBinConflicts`](https://github.com/pnpm/pnpm/blob/1819226b51/global/commands/src/checkGlobalBinConflicts.ts):
//! detect bin-name collisions between the packages about to be installed
//! and the packages already installed globally.

use crate::{
    read_package_json,
    scan::{GlobalPackageInfo, scan_global_packages},
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_cmd_shim::{Host, PackageBinSource, get_bins_from_package_manifest, pkg_owns_bin};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

/// A bin name from a new package would shadow a different already-installed
/// global package's bin, and neither legitimately owns it.
#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    r#"Cannot install: binary "{bin_name}" would conflict with {conflict_display} that is already installed globally"#
)]
#[diagnostic(
    code(ERR_PNPM_GLOBAL_BIN_CONFLICT),
    help("Remove the conflicting package first: pnpm remove -g {alias}")
)]
pub struct GlobalBinConflictError {
    pub bin_name: String,
    /// Pre-rendered `"<alias>"` or `"<alias>" (package "<name>")` text.
    pub conflict_display: String,
    pub alias: String,
}

/// Check for bin-name conflicts between `new_pkgs` and the global packages
/// under `global_dir`. Returns the set of bin names that should be skipped
/// during linking (legitimately owned by a package being kept), or a
/// [`GlobalBinConflictError`] when a true conflict is found.
///
/// `should_skip` selects existing groups to ignore — add passes "any group
/// whose aliases overlap the new ones"; update passes "the group being
/// replaced".
pub fn check_global_bin_conflicts(
    global_dir: &Path,
    global_bin_dir: &Path,
    new_pkgs: &[PackageBinSource],
    should_skip: impl Fn(&GlobalPackageInfo) -> bool,
) -> Result<HashSet<String>, GlobalBinConflictError> {
    let mut bins_to_skip = HashSet::new();

    // Map each new bin name to the packages that provide it.
    let mut new_bin_owners: HashMap<String, Vec<String>> = HashMap::new();
    for pkg in new_pkgs {
        let pkg_name = pkg.manifest.get("name").and_then(Value::as_str).unwrap_or("").to_string();
        for bin in get_bins_from_package_manifest::<Host>(&pkg.manifest, &pkg.location) {
            new_bin_owners.entry(bin.name).or_default().push(pkg_name.clone());
        }
    }
    if new_bin_owners.is_empty() {
        return Ok(bins_to_skip);
    }

    // Only investigate names whose shim already exists in the global bin dir.
    let conflicting: HashSet<String> =
        new_bin_owners.keys().filter(|name| global_bin_dir.join(name).exists()).cloned().collect();
    if conflicting.is_empty() {
        return Ok(bins_to_skip);
    }

    for existing_pkg in scan_global_packages(global_dir) {
        if should_skip(&existing_pkg) {
            continue;
        }
        let modules_dir = existing_pkg.install_dir.join("node_modules");
        for (alias, _) in &existing_pkg.dependencies {
            let dep_dir = modules_dir.join(alias);
            let Some(manifest) = read_package_json(&dep_dir) else { continue };
            let manifest_name =
                manifest.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            for bin in get_bins_from_package_manifest::<Host>(&manifest, &dep_dir) {
                if !conflicting.contains(&bin.name) {
                    continue;
                }
                let new_owns =
                    new_bin_owners[&bin.name].iter().any(|owner| pkg_owns_bin(&bin.name, owner));
                let existing_owns = pkg_owns_bin(&bin.name, &manifest_name);
                // Only the new package owns it → it overrides the old bin.
                if new_owns && !existing_owns {
                    continue;
                }
                // Only the existing package owns it → skip linking the new one.
                if existing_owns && !new_owns {
                    bins_to_skip.insert(bin.name.clone());
                    continue;
                }
                // Both or neither own it → a real conflict.
                let conflict_display = if *alias == manifest_name {
                    format!(r#""{alias}""#)
                } else {
                    format!(r#""{alias}" (package "{manifest_name}")"#)
                };
                return Err(GlobalBinConflictError {
                    bin_name: bin.name,
                    conflict_display,
                    alias: alias.clone(),
                });
            }
        }
    }
    Ok(bins_to_skip)
}
