use super::LinkVirtualStoreBinsError;
use pnpm_cmd_shim::{
    FsCreateDirAll, FsEnsureExecutableBits, FsReadDir, FsReadFile, FsReadHead, FsReadToString,
    FsSetExecutable, FsWalkFiles, FsWrite, LinkBinsError, LinkBinsOptions, PackageBinSource,
    link_bins_of_packages,
};
use pnpm_package_manifest::parse_manifest_bytes;
use rayon::prelude::*;
use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Fallback (non-lockfile) path: enumerate slots via `read_dir`,
/// then walk each slot's `node_modules` to discover children. Used
/// only by the fresh-lockfile installer today; the lockfile
/// path bypasses every directory enumeration in here.
pub(super) fn run_with_readdir<Sys>(
    virtual_store_dir: &Path,
    link_options: &LinkBinsOptions,
) -> Result<(), LinkVirtualStoreBinsError>
where
    Sys: FsReadDir
        + FsReadFile
        + FsReadToString
        + FsReadHead
        + FsCreateDirAll
        + FsWalkFiles
        + FsWrite
        + FsSetExecutable
        + FsEnsureExecutableBits,
{
    let slots = match Sys::read_dir(virtual_store_dir) {
        Ok(slots) => slots,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(LinkVirtualStoreBinsError::ReadVirtualStore {
                dir: virtual_store_dir.to_path_buf(),
                error,
            });
        }
    };
    let slots: Vec<PathBuf> = slots.collect();
    slots.par_iter().try_for_each(|slot_dir| {
        let modules_dir = slot_dir.join("node_modules");
        let Some(self_pkg_dir) = find_slot_own_package_dir(slot_dir, &modules_dir) else {
            return Ok(());
        };
        // Probe the slot's own package directory before walking its
        // children. Without the probe, an incomplete slot whose
        // `node_modules/<pkg>` is missing but whose sibling deps are
        // still present would have `link_bins_excluding` collect the
        // siblings and `create_dir_all` the missing `<pkg>` chain to
        // hold the shims, leaving an orphan package directory on
        // disk. This path runs only for [`crate::InstallWithFreshLockfile`]
        // and visits ~direct-deps slots (small N), so the probe cost
        // is trivial; the lockfile-driven path bypasses this by
        // treating the slot's own pkg dir as an invariant of
        // [`crate::create_virtual_dir_by_snapshot`].
        if Sys::read_dir(&self_pkg_dir).is_err() {
            return Ok(());
        }
        let bins_dir = self_pkg_dir.join("node_modules/.bin");
        link_bins_excluding::<Sys>(&modules_dir, &bins_dir, &self_pkg_dir, link_options)
            .map_err(LinkVirtualStoreBinsError::LinkBins)
    })
}
/// Locate the slot's own package directory inside `<slot>/node_modules`.
///
/// The slot directory's name encodes the package name as
/// `<scope>+<name>@<version>` for the simple case (see
/// [`pnpm_lockfile::PkgNameVerPeer::to_virtual_store_name`]). For
/// peer-resolved slots the version segment itself contains additional
/// `@`-separated peer specs joined by `_`, e.g.
/// `ts-node@10.9.1_@types+node@18.7.19_typescript@5.1.6`. The `@` after
/// `typescript` is part of a peer's version, not the package-name
/// boundary. Parsing from the right (`rfind('@')`) would split there
/// and silently break peer-resolved slots; parse from the left
/// instead, skipping a leading `@` that belongs to a scoped package.
///
/// Returns `None` only when the slot name fails to parse — there's no
/// filesystem probe for the resolved candidate. The slot's own package
/// directory is an invariant of [`crate::create_virtual_dir_by_snapshot`];
/// the downstream [`link_bins_excluding`] handles `NotFound` from its own
/// `read_dir` of `<slot>/node_modules` cleanly when the invariant
/// ever does break, so a probe here would be pure overhead.
pub(super) fn find_slot_own_package_dir(slot_dir: &Path, modules_dir: &Path) -> Option<PathBuf> {
    let slot_name = slot_dir.file_name()?.to_str()?;

    // The package-name half is everything before the **first** `@`,
    // ignoring a single leading `@` that belongs to a scoped name
    // (`@scope+pkg@...` → start the `@` search at offset 1).
    // After `to_virtual_store_name`, `/` in scoped names becomes `+`,
    // so the package-name half can never contain `@` itself.
    let scoped = slot_name.starts_with('@');
    let search_start = usize::from(scoped);
    let at = search_start + slot_name[search_start..].find('@')?;
    let name_part = &slot_name[..at];

    // `+` separates `<scope>+<name>` for scoped packages, and *only*
    // for scoped packages. Gating on `scoped` avoids misparsing a
    // hypothetical unscoped name that contains `+`: `PkgName::parse`
    // does not reject non-URL-safe characters (only npm's
    // `validate-npm-package-name` warns about them), so an unscoped
    // name like `foo+bar` could in principle reach here and would
    // otherwise be split into `foo` / `bar`.
    let pkg_dir = match scoped.then(|| name_part.split_once('+')).flatten() {
        Some((scope, name)) => modules_dir.join(scope).join(name),
        None => modules_dir.join(name_part),
    };
    Some(pkg_dir)
}
/// Like [`pnpm_cmd_shim::link_bins`] but skipping the slot's own package
/// from the candidate set.
pub(super) fn link_bins_excluding<Sys>(
    modules_dir: &Path,
    bins_dir: &Path,
    exclude: &Path,
    link_options: &LinkBinsOptions,
) -> Result<(), LinkBinsError>
where
    Sys: FsReadDir
        + FsReadFile
        + FsReadToString
        + FsReadHead
        + FsCreateDirAll
        + FsWalkFiles
        + FsWrite
        + FsSetExecutable
        + FsEnsureExecutableBits,
{
    let mut packages: Vec<PackageBinSource> = Vec::new();
    let Some(entries) = read_modules_entries::<Sys>(modules_dir)? else {
        return Ok(());
    };

    for path in entries {
        let Some(name) = path.file_name() else {
            continue;
        };
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }
        if name_str.starts_with('@') {
            push_scope_bin_sources::<Sys>(&mut packages, &path, exclude)?;
            continue;
        }
        push_package_bin_source::<Sys>(&mut packages, &path, exclude)?;
    }

    if packages.is_empty() {
        return Ok(());
    }

    link_bins_of_packages::<Sys>(&packages, bins_dir, link_options)
}
/// The entries of a `node_modules`-shaped directory, or `None` when it
/// does not exist. Only `NotFound` is plausibly skippable (a concurrent
/// delete); other errors — permission denied, EIO, `AppArmor` deny — would
/// make the bins under the directory silently disappear, so they
/// surface.
pub(super) fn read_modules_entries<Sys: FsReadDir>(
    dir: &Path,
) -> Result<Option<impl Iterator<Item = PathBuf>>, LinkBinsError> {
    match Sys::read_dir(dir) {
        Ok(entries) => Ok(Some(entries)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(LinkBinsError::ReadModulesDir { dir: dir.to_path_buf(), error }),
    }
}
pub(super) fn push_scope_bin_sources<Sys: FsReadDir + FsReadFile>(
    packages: &mut Vec<PackageBinSource>,
    scope_dir: &Path,
    exclude: &Path,
) -> Result<(), LinkBinsError> {
    let Some(entries) = read_modules_entries::<Sys>(scope_dir)? else {
        return Ok(());
    };
    for path in entries {
        push_package_bin_source::<Sys>(packages, &path, exclude)?;
    }
    Ok(())
}
pub(super) fn push_package_bin_source<Sys: FsReadFile>(
    packages: &mut Vec<PackageBinSource>,
    path: &Path,
    exclude: &Path,
) -> Result<(), LinkBinsError> {
    if paths_eq(path, exclude) {
        return Ok(());
    }
    if let Some(pkg) = read_package::<Sys>(path)? {
        packages.push(pkg);
    }
    Ok(())
}
pub(super) fn read_package<Sys: FsReadFile>(
    location: &Path,
) -> Result<Option<PackageBinSource>, LinkBinsError> {
    let manifest_path = location.join("package.json");
    let bytes = match Sys::read_file(&manifest_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(LinkBinsError::ReadManifest { path: manifest_path, error }),
    };
    let manifest: serde_json::Value = parse_manifest_bytes(&bytes)
        .map_err(|error| LinkBinsError::ParseManifest { path: manifest_path, error })?;
    Ok(Some(PackageBinSource::new(location.to_path_buf(), Arc::new(manifest))))
}
pub(super) fn paths_eq(lhs: &Path, rhs: &Path) -> bool {
    // Lexical comparison is enough; both paths come from the same
    // `node_modules` walk and don't go through canonicalisation.
    lhs == rhs
}
