use crate::{
    Lockfile, PackageKey, PackageMetadata, PkgName, ResolvedDependencyMap, SnapshotDepRef,
};
use pnpm_deps_path::index_of_dep_path_suffix;
use pnpm_patching::{
    PatchGroup, PatchGroupRecord, PatchInput, get_patch_info, group_patched_dependencies,
};
use std::collections::HashMap;

/// The version a patch was matched against, when the lockfile records one.
///
/// The version slot of a key only holds a bare semver for a registry package;
/// a git / tarball / `file:` one holds its resolution there instead, and the
/// version it resolved to is what `packages:` records — so that entry wins
/// when there is one. A registry-qualified key
/// (`<name>@<registryName>:<version>`) gets no such entry, because its slot
/// already carries the semver behind the registry alias.
fn recorded_version(
    key: &PackageKey,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
) -> Option<String> {
    let metadata_key = key.without_peer();
    packages
        .and_then(|packages| packages.get(&metadata_key))
        .and_then(|metadata| metadata.version.clone())
        .or_else(|| match metadata_key.suffix.registry_qualified() {
            Some((_, version)) => Some(version.to_string()),
            None => metadata_key.suffix.version_semver().map(ToString::to_string),
        })
}

/// The package identity a `snapshots:` key stands for, as every consumer that
/// matches a lockfile entry against `patchedDependencies` must derive it.
///
/// Falls back to the key's version slot verbatim when the lockfile records no
/// version of its own, which is the identity a resolution-shaped lookup gets.
///
/// Mirrors pnpm's `nameVerFromPkgSnapshot` composed with its `parse`, which is
/// what the same lookups on the TypeScript side are derived from.
#[must_use]
pub fn name_version_from_package_key(
    key: &PackageKey,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
) -> (String, String) {
    let metadata_key = key.without_peer();
    let version = recorded_version(key, packages)
        .unwrap_or_else(|| metadata_key.suffix.version().to_string());
    (metadata_key.name.to_string(), version)
}

const PATCH_HASH_PREFIX: &str = "(patch_hash=";

/// What [`check_patched_dep_paths`] could establish about a lockfile's
/// `(patch_hash=...)` segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchedDepPathsStatus {
    /// Every segment matches the patch `patchedDependencies` gives its package.
    UpToDate,
    /// At least one segment disagrees with `patchedDependencies`.
    Stale,
    /// No segment was shown to disagree, but at least one could not be judged.
    Indeterminate,
}

/// A patched dependency's hash is recorded in a lockfile more than once:
///
/// 1. In `patchedDependencies`, which is the authoritative source of truth.
/// 2. As a `(patch_hash=...)` segment on every reference to a patched
///    package — importer entries, `snapshots:` keys, and the dependency
///    edges between snapshots. (`packages:` is keyed without the patch
///    hash, so it carries none of these.)
///
/// They disagree only when the file was hand-edited or a merge conflict was
/// resolved wrongly: nothing pnpm writes can produce it, and the
/// config-vs-lockfile gate in [`crate::check_lockfile_settings`] cannot see
/// it, because that compares `patchedDependencies` against the configured
/// patches and never reads the segments.
///
/// Judging a segment needs the package's version and the patch set, and a
/// lockfile in that state can be missing either. Neither failing is a
/// patch-hash problem, so [`PatchedDepPathsStatus::Indeterminate`] keeps them
/// apart from a hash that genuinely disagrees: a caller can re-resolve without
/// reporting a cause it has not established.
///
/// Stops at the first segment that definitely disagrees.
#[must_use]
pub fn check_patched_dep_paths(lockfile: &Lockfile) -> PatchedDepPathsStatus {
    // The map is the reference every segment is judged against, so a key it
    // cannot resolve to a patch set leaves nothing to judge them with. The
    // resolver reports the key itself, against the configured patches, where
    // the one the user can act on lives.
    let Ok(groups) = group_patched_dependencies(
        lockfile.patched_dependencies
            .iter()
            .flatten()
            .map(|(key, hash)| {
                (key.clone(), PatchInput { hash: hash.clone(), patch_file_path: None })
            }),
    ) else {
        return PatchedDepPathsStatus::Indeterminate;
    };

    let packages = lockfile.packages.as_ref();
    let mut indeterminate = false;
    for key in patched_dep_paths(lockfile) {
        match judge(&key, &groups, packages) {
            Verdict::Stale => return PatchedDepPathsStatus::Stale,
            Verdict::Indeterminate => indeterminate = true,
            Verdict::Ok => {}
        }
    }
    if indeterminate {
        PatchedDepPathsStatus::Indeterminate
    } else {
        PatchedDepPathsStatus::UpToDate
    }
}

enum Verdict {
    Ok,
    Stale,
    Indeterminate,
}

fn judge(
    key: &PackageKey,
    groups: &PatchGroupRecord,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
) -> Verdict {
    let rendered = key.to_string();
    let suffix = index_of_dep_path_suffix(&rendered);
    let Some(start) = suffix.patch_hash_index else {
        return Verdict::Ok;
    };
    let end = suffix.peers_index.unwrap_or(rendered.len());
    let Some(recorded) = rendered[start..end]
        .strip_prefix(PATCH_HASH_PREFIX)
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return Verdict::Stale;
    };
    let name = key.without_peer().name.to_string();
    // A non-registry package's version slot holds its resolution, so the
    // version a patch was matched against is only the one its entry records.
    // Without that, a key that selects on version cannot be matched either way.
    let version = match recorded_version(key, packages) {
        Some(version) => version,
        None if patch_selects_on_version(groups.get(&name)) => return Verdict::Indeterminate,
        None => String::new(),
    };
    // A conflict between two configured ranges is the resolver's to report
    // (`ERR_PNPM_PATCH_KEY_CONFLICT`), and leaves this with no patch to
    // compare the segment against.
    let Ok(patch) = get_patch_info(Some(groups), &name, &version) else {
        return Verdict::Indeterminate;
    };
    if patch.is_some_and(|patch| patch.hash == recorded) { Verdict::Ok } else { Verdict::Stale }
}

/// Whether which patch applies for `group`, if any, can depend on the
/// package's version. No entry resolves to no patch for every version, and a
/// bare-name entry to the same patch for every version. Either way the version
/// cannot change the answer.
fn patch_selects_on_version(group: Option<&PatchGroup>) -> bool {
    group.is_some_and(|group| !group.exact.is_empty() || !group.range.is_empty())
}

/// Every dependency path in the lockfile that can carry a `(patch_hash=...)`
/// segment: the `snapshots:` keys and every reference pointing at one.
fn patched_dep_paths(lockfile: &Lockfile) -> impl Iterator<Item = PackageKey> + '_ {
    let importers = lockfile.importers
        .values()
        .flat_map(|importer| {
            [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies]
                .into_iter()
                .flatten()
                .flat_map(importer_references)
        });
    let snapshots = lockfile.snapshots
        .iter()
        .flatten()
        .flat_map(|(key, snapshot)| {
            std::iter::once(key.clone())
                .chain(
                    [&snapshot.dependencies, &snapshot.optional_dependencies]
                        .into_iter()
                        .flatten()
                        .flat_map(snapshot_references),
                )
        });
    importers.chain(snapshots)
}

fn importer_references(deps: &ResolvedDependencyMap) -> impl Iterator<Item = PackageKey> + '_ {
    deps.iter()
        .filter_map(|(alias, spec)| spec.version.resolved_key(alias))
}

fn snapshot_references(
    deps: &HashMap<PkgName, SnapshotDepRef>,
) -> impl Iterator<Item = PackageKey> + '_ {
    deps.iter()
        .filter_map(|(alias, reference)| reference.resolve(alias))
}

#[cfg(test)]
mod tests;
