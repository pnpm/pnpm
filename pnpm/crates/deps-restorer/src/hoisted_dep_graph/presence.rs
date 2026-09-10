use super::{DependenciesGraph, HoistedDepGraphError, walk::WalkState};
use pnpm_lockfile::{Lockfile, LockfileResolution, PackageKey};
use pnpm_package_is_installable::{
    InstallabilityOptions, InstallabilityVerdict, PackageInstallabilityManifest, WantedEngine,
    package_is_installable,
};
use std::{fs, path::Path};

/// Whether a previous install left this package at `dir`, reached from
/// `modules` without traversing a link: a real directory holding a
/// regular `package.json` whose `version` is `version`.
///
/// Mirrors pnpm's `dirHasPackageJsonWithVersion`, minus its fallback
/// that trusts a directory whose manifest cannot be read, so an
/// interrupted import is repaired rather than skipped.
///
/// The link checks keep the same promise. The linker writes real
/// directories of regular files, so a link on the way to the manifest is
/// not what a previous install left, and what the manifest reports
/// belongs to whatever the link points at. Importing the package writes
/// the lockfile's contents to that path either way, though only the last
/// component of the path is cleared first, so a linked parent survives
/// with the right package behind it.
pub(super) fn package_present_at(modules: &Path, dir: &Path, version: &str) -> bool {
    // `lstat` follows every component but the last, so the directory
    // holding the package needs a check of its own. `dir` is
    // `modules.join(alias)` for a valid npm package name, so that is
    // either `modules` itself or the `@scope` directory.
    let Some(parent) = dir.parent() else { return false };
    if parent != modules && !fs::symlink_metadata(parent).is_ok_and(|entry| entry.is_dir()) {
        return false;
    }
    if !fs::symlink_metadata(dir).is_ok_and(|entry| entry.is_dir()) {
        return false;
    }
    let manifest_path = dir.join("package.json");
    if !fs::symlink_metadata(&manifest_path).is_ok_and(|entry| entry.is_file()) {
        return false;
    }
    let Ok(raw) = fs::read(&manifest_path) else {
        return false;
    };
    serde_json::from_slice::<serde_json::Value>(&raw).is_ok_and(|manifest| {
        manifest.get("version").and_then(serde_json::Value::as_str) == Some(version)
    })
}
/// Whether the current lockfile resolves the package at `dir`
/// differently from the wanted one.
///
/// A dep path carries the package's name and version, so the same key
/// can survive a change of tarball URL, integrity or revision, and the
/// manifest version on disk still matches. The contents are meant to
/// change, so the directory has to be imported again.
///
/// Both install paths reach this: each is handed the current lockfile.
/// `false` when there is no previous graph to compare against, which is
/// an install with no current lockfile or one whose `packages:` map is
/// empty. The recorded location and the manifest version stay the only
/// evidence there, as they are for pnpm's `skipFetch`.
pub(super) fn resolution_changed_at(
    prev_graph: Option<&DependenciesGraph>,
    dir: &Path,
    wanted: &LockfileResolution,
) -> bool {
    prev_graph.is_some_and(|graph| graph.get(dir).is_some_and(|node| &node.resolution != wanted))
}
/// Whether the installability filter rules this package out on this
/// host. Applied only when `!opts.force`. An optional dep on an
/// unsupported platform is silently skipped; a required one is an
/// error.
pub(super) fn installability_skip(
    state: &WalkState<'_>,
    pkg_key: &PackageKey,
    metadata: &pnpm_lockfile::PackageMetadata,
    optional: bool,
) -> Result<bool, HoistedDepGraphError> {
    if state.opts.force {
        return Ok(false);
    }
    let manifest = manifest_for_installability(pkg_key, metadata);
    let install_opts = InstallabilityOptions {
        engine_strict: state.opts.engine_strict,
        optional,
        current_node_version: &state.opts.current_node_version,
        pnpm_version: None,
        current_os: &state.opts.current_os,
        current_cpu: &state.opts.current_cpu,
        current_libc: &state.opts.current_libc,
        supported_architectures: state.opts.supported_architectures.as_ref(),
    };
    match package_is_installable(&pkg_key.to_string(), &manifest, &install_opts) {
        Ok(
            InstallabilityVerdict::Installable | InstallabilityVerdict::ProceedWithWarning { .. },
        ) => Ok(false),
        Ok(InstallabilityVerdict::SkipOptional { .. }) => Ok(true),
        Err(source) => Err(HoistedDepGraphError::Installability(source)),
    }
}
/// Look up the metadata side of a snapshot. Pacquet stores
/// `packages` and `snapshots` separately; the walker needs the
/// metadata for resolution / `has_bin` / bundledDependencies.
pub(super) fn lookup_package_metadata<'a>(
    lockfile: &'a Lockfile,
    key: &PackageKey,
) -> Option<&'a pnpm_lockfile::PackageMetadata> {
    let packages = lockfile.packages.as_ref()?;
    // `packages:` keys are peer-stripped (`react-dom@19.2.7`), while a
    // hoister reference carries the full peer suffix
    // (`react-dom@19.2.7(react@19.2.7)`). Try the exact key first
    // (peerless references — the common case — hit immediately), then
    // fall back to the stripped key so peered snapshots resolve their
    // metadata instead of being silently dropped from the graph along
    // with their whole subtree.
    packages.get(key).or_else(|| {
        if key.suffix.peer().is_empty() {
            return None;
        }
        packages.get(&key.without_peer())
    })
}
/// Project the platform / engines axes from a `PackageMetadata`
/// onto the [`PackageInstallabilityManifest`] shape
/// [`package_is_installable`] consumes. Extracted into its own
/// helper so the walker body stays small.
pub(super) fn manifest_for_installability(
    pkg_key: &PackageKey,
    metadata: &pnpm_lockfile::PackageMetadata,
) -> PackageInstallabilityManifest {
    let engines = metadata.engines.as_ref().map(|engines| WantedEngine {
        node: engines.get("node").cloned(),
        pnpm: engines.get("pnpm").cloned(),
    });
    PackageInstallabilityManifest {
        name: pkg_key.name.to_string(),
        engines,
        cpu: metadata.cpu.clone(),
        os: metadata.os.clone(),
        libc: metadata.libc.as_deref().map(<[String]>::to_vec),
    }
}
/// Lockfile-relative path string (`dir` relative to `lockfile_dir`).
/// Returns an empty string when `dir == lockfile_dir`.
///
/// Backslashes are normalized to forward slashes so the value is
/// portable across platforms — `.modules.yaml.hoistedLocations`
/// is read on whatever OS the next install runs on, and
/// `pnpm-lock.yaml` already uses forward slashes for the same
/// reason. pacquet normalizes here for cross-platform consistency
/// with the rest of pnpm's serialised formats.
pub(super) fn path_relative_to_lockfile_dir(dir: &Path, lockfile_dir: &Path) -> String {
    dir.strip_prefix(lockfile_dir).map_or_else(
        |_| dir.to_string_lossy().replace('\\', "/"),
        |rel| rel.to_string_lossy().replace('\\', "/"),
    )
}
