//! Detect bin-name collisions between the packages about to be installed
//! and the packages already installed globally.

use crate::{
    read_package_json,
    scan::{GlobalPackageInfo, scan_global_packages},
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_cmd_shim::{Host, PackageBinSource, get_bins_from_package_manifest, pkg_owns_bin};
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

/// Failure from [`check_global_bin_conflicts`]: either a real bin-name
/// conflict, or the global packages directory could not be scanned.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum CheckGlobalBinConflictsError {
    #[diagnostic(transparent)]
    Conflict(GlobalBinConflictError),

    #[display("failed to scan the global packages directory: {_0}")]
    #[diagnostic(code(ERR_PNPM_GLOBAL_SCAN))]
    Scan(#[error(source)] std::io::Error),
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
) -> Result<HashSet<String>, CheckGlobalBinConflictsError> {
    let mut bins_to_skip = HashSet::new();

    let new_bin_owners = bins_by_owner(new_pkgs);
    if new_bin_owners.is_empty() {
        return Ok(bins_to_skip);
    }

    // Only investigate names whose shim already exists in the global bin dir.
    let conflicting: HashSet<String> = new_bin_owners
        .keys()
        .filter(|name| bin_slot_exists(global_bin_dir, name))
        .cloned()
        .collect();
    if conflicting.is_empty() {
        return Ok(bins_to_skip);
    }

    let installed = InstalledBins { new_bin_owners: &new_bin_owners, conflicting: &conflicting };
    for existing_pkg in
        scan_global_packages(global_dir).map_err(CheckGlobalBinConflictsError::Scan)?
    {
        if should_skip(&existing_pkg) {
            continue;
        }
        let modules_dir = existing_pkg.install_dir.join("node_modules");
        for (alias, _) in &existing_pkg.dependencies {
            check_installed_dep(alias, &modules_dir.join(alias), &installed, &mut bins_to_skip)?;
        }
    }
    Ok(bins_to_skip)
}

/// Each bin name the packages being installed provide, and which of them
/// provides it.
fn bins_by_owner(new_pkgs: &[PackageBinSource]) -> HashMap<String, Vec<String>> {
    let mut new_bin_owners: HashMap<String, Vec<String>> = HashMap::new();
    for pkg in new_pkgs {
        let pkg_name = pkg.manifest.get("name").and_then(Value::as_str).unwrap_or("").to_string();
        for bin in get_bins_from_package_manifest::<Host>(&pkg.manifest, &pkg.location) {
            new_bin_owners.entry(bin.name).or_default().push(pkg_name.clone());
        }
    }
    new_bin_owners
}

/// The bins the install brings, and which of their names already occupy a slot.
struct InstalledBins<'a> {
    new_bin_owners: &'a HashMap<String, Vec<String>>,
    conflicting: &'a HashSet<String>,
}

/// Judge one already-installed global dependency against the bins being
/// installed.
fn check_installed_dep(
    alias: &str,
    dep_dir: &Path,
    installed: &InstalledBins<'_>,
    bins_to_skip: &mut HashSet<String>,
) -> Result<(), CheckGlobalBinConflictsError> {
    let Some(manifest) = read_package_json(dep_dir) else { return Ok(()) };
    let manifest_name = manifest.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    for bin in get_bins_from_package_manifest::<Host>(&manifest, dep_dir) {
        if !installed.conflicting.contains(&bin.name) {
            continue;
        }
        match bin_ownership(&bin.name, &manifest_name, installed) {
            BinOwnership::NewPackage => continue,
            BinOwnership::ExistingPackage => {
                bins_to_skip.insert(bin.name.clone());
            }
            BinOwnership::Contested => {
                return Err(CheckGlobalBinConflictsError::Conflict(GlobalBinConflictError {
                    bin_name: bin.name,
                    conflict_display: conflict_display(alias, &manifest_name),
                    alias: alias.to_string(),
                }));
            }
        }
    }
    Ok(())
}

/// Which side a bin name belongs to. A package "owns" a bin whose name is its
/// own; when both or neither do, neither may silently take the slot.
enum BinOwnership {
    /// The package being installed owns it, so it overrides the old bin.
    NewPackage,
    /// The installed package owns it, so the new one is not linked.
    ExistingPackage,
    /// A real conflict.
    Contested,
}

fn bin_ownership(
    bin_name: &str,
    manifest_name: &str,
    installed: &InstalledBins<'_>,
) -> BinOwnership {
    let new_owns =
        installed.new_bin_owners[bin_name].iter().any(|owner| pkg_owns_bin(bin_name, owner));
    let existing_owns = pkg_owns_bin(bin_name, manifest_name);
    match (new_owns, existing_owns) {
        (true, false) => BinOwnership::NewPackage,
        (false, true) => BinOwnership::ExistingPackage,
        _ => BinOwnership::Contested,
    }
}

/// How a conflicting dependency is named in the error: by its alias, plus the
/// package behind it when the two differ.
fn conflict_display(alias: &str, manifest_name: &str) -> String {
    if alias == manifest_name {
        format!(r#""{alias}""#)
    } else {
        format!(r#""{alias}" (package "{manifest_name}")"#)
    }
}

/// Whether a bin named `name` already occupies a slot in `global_bin_dir`.
/// On Windows a directly linked `node` runtime and every context-aware shim
/// occupy `<name>.exe` with no bare `<name>` file, so that flavor is checked
/// too — otherwise such an entry would not be detected as a conflict.
#[must_use]
pub fn bin_slot_exists(global_bin_dir: &Path, name: &str) -> bool {
    if global_bin_dir.join(name).exists() {
        return true;
    }
    cfg!(windows) && global_bin_dir.join(format!("{name}.exe")).exists()
}
