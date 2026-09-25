use std::collections::BTreeSet;

use pnpm_lockfile::{Lockfile, PkgName};
use pnpm_package_name::is_valid_old_npm_package_name;

use crate::errors::VerifyError;

/// Add every alias in `aliases` that fails
/// `is_valid_old_npm_package_name` (the `validForOldPackages` rule the
/// dependency-alias check applies) to `invalid`. Only pass maps
/// whose keys become `node_modules/<alias>` directories — not
/// `overrides`, `patched_dependencies`, or peer dependencies.
fn push_invalid_aliases<'alias>(
    aliases: impl Iterator<Item = &'alias PkgName>,
    invalid: &mut BTreeSet<String>,
) {
    for alias in aliases {
        let alias = alias.to_string();
        if !is_valid_old_npm_package_name(&alias) {
            invalid.insert(alias);
        }
    }
}

/// Collect every lockfile-controlled dependency name that isn't a valid
/// npm package name. Three sources, each of which becomes a
/// `node_modules/<name>` path component at install time:
///
/// - every importer's direct-dependency aliases,
/// - every snapshot's own package name (the `snapshots:` map key), and
/// - every snapshot's `dependencies` / `optionalDependencies` aliases.
///
/// A name carrying a `..` traversal, a `/`, or a reserved value such as
/// `node_modules` / `.bin` / `.pnpm` could make an install write outside
/// the intended directory or overwrite pnpm-owned layout.
fn collect_invalid_dependency_names(lockfile: &Lockfile) -> BTreeSet<String> {
    let mut invalid = BTreeSet::new();
    for importer in lockfile.importers.values() {
        for deps in
            [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies]
        {
            push_invalid_aliases(
                deps.iter()
                    .flatten()
                    .map(|(alias, _)| alias),
                &mut invalid,
            );
        }
    }
    let Some(snapshots) = lockfile.snapshots.as_ref() else { return invalid };
    for (key, snapshot) in snapshots {
        push_invalid_aliases(std::iter::once(&key.name), &mut invalid);
        for deps in [&snapshot.dependencies, &snapshot.optional_dependencies] {
            push_invalid_aliases(
                deps.iter()
                    .flatten()
                    .map(|(alias, _)| alias),
                &mut invalid,
            );
        }
    }
    invalid
}

/// Reject a lockfile whose dependency names or aliases are not valid npm
/// package names.
///
/// This is an offline, network-free structural check — it does **not**
/// depend on the resolution-policy verifiers, so it must run on every
/// install, **including under `trustLockfile`**, which disables the
/// policy fan-out. A crafted lockfile alias becomes a filesystem path
/// (`node_modules/<alias>`, a virtual-store slot's inner
/// `node_modules/<name>`, a bin, a hoisted link), and pacquet's
/// [`PkgName`] parser does not validate against npm's package-name
/// rules, so an unchecked `..`/`/`-bearing name escapes the intended
/// directory.
///
/// Surfaces [`VerifyError::InvalidDependencyAlias`]
/// (`ERR_PNPM_INVALID_DEPENDENCY_NAME`) listing every offender.
pub fn verify_lockfile_dependency_names(lockfile: &Lockfile) -> Result<(), VerifyError> {
    let invalid = collect_invalid_dependency_names(lockfile);
    if invalid.is_empty() {
        return Ok(());
    }
    let invalid: Vec<String> = invalid.into_iter().collect();
    Err(VerifyError::invalid_dependency_aliases(&invalid))
}
