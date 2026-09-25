use crate::{
    ImporterDepVersion, Lockfile, PackageKey, PackageMetadata, PkgName, PkgVerPeer, SnapshotDepRef,
};
use pnpm_patching::{
    PatchGroup, PatchGroupRecord, PatchInput, get_patch_info, group_patched_dependencies, parse_key,
};
use std::collections::{HashMap, HashSet};

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
    /// Every depPath carries the segment `patchedDependencies` gives its
    /// package, or none when no patch applies.
    UpToDate,
    /// At least one depPath disagrees with `patchedDependencies`.
    Stale,
    /// No depPath was shown to disagree, but at least one could not be judged.
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
/// patches and never reads the segments. A depPath missing the segment its
/// patch calls for disagrees as much as one carrying the wrong hash.
///
/// Judging a depPath needs the package's version and the patch set, and a
/// lockfile in that state can be missing either. Neither failing is a
/// patch-hash problem, so [`PatchedDepPathsStatus::Indeterminate`] keeps them
/// apart from a hash that genuinely disagrees: a caller can re-resolve without
/// reporting a cause it has not established. A `patchedDependencies` key that
/// does not resolve leaves only its own package without a verdict.
///
/// Stops at the first depPath that definitely disagrees.
#[must_use]
pub fn check_patched_dep_paths(lockfile: &Lockfile) -> PatchedDepPathsStatus {
    let mut checker = Checker::new(lockfile);
    let stale = checker.visit_importers(lockfile) || checker.visit_snapshots(lockfile);
    if stale {
        PatchedDepPathsStatus::Stale
    } else if checker.indeterminate {
        PatchedDepPathsStatus::Indeterminate
    } else {
        PatchedDepPathsStatus::UpToDate
    }
}

#[derive(Clone, Copy)]
enum Verdict {
    Ok,
    Stale,
    Indeterminate,
}

struct Checker<'a> {
    groups: PatchGroupRecord,
    /// Packages with a `patchedDependencies` key that does not resolve to a
    /// patch set. The resolver reports the key itself, against the configured
    /// patches, where the one the user can act on lives.
    unusable: HashSet<String>,
    /// Every package named in `patchedDependencies`. A depPath for any other
    /// package is judged only when it carries a segment, which keeps the walk
    /// from allocating for the unpatched bulk of the graph.
    patched_names: HashSet<PkgName>,
    packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    verdicts: HashMap<PackageKey, Verdict>,
    indeterminate: bool,
}

impl<'a> Checker<'a> {
    fn new(lockfile: &'a Lockfile) -> Self {
        let mut usable = Vec::new();
        let mut unusable = HashSet::new();
        for (key, hash) in lockfile.patched_dependencies.iter().flatten() {
            let entry = (key.clone(), PatchInput { hash: hash.clone(), patch_file_path: None });
            if group_patched_dependencies([entry.clone()]).is_ok() {
                usable.push(entry);
            } else {
                unusable.insert(parse_key(key).name.unwrap_or(key).to_string());
            }
        }
        let groups = group_patched_dependencies(usable)
            .expect("every key was grouped on its own without an error");
        let patched_names = groups
            .keys()
            .chain(&unusable)
            .filter_map(|name| PkgName::parse(name.as_str()).ok())
            .collect();
        Checker {
            groups,
            unusable,
            patched_names,
            packages: lockfile.packages.as_ref(),
            verdicts: HashMap::new(),
            indeterminate: false,
        }
    }

    /// Returns `true` once a depPath definitely disagrees.
    fn visit_importers(&mut self, lockfile: &Lockfile) -> bool {
        lockfile.importers
            .values()
            .flat_map(|importer| {
                [
                    &importer.dependencies,
                    &importer.dev_dependencies,
                    &importer.optional_dependencies,
                ]
            })
            .flatten()
            .flatten()
            .any(|(alias, spec)| self.visit_importer_reference(alias, &spec.version))
    }

    fn visit_importer_reference(&mut self, alias: &PkgName, version: &ImporterDepVersion) -> bool {
        match version {
            ImporterDepVersion::Regular(ver_peer) => self.visit(alias, ver_peer),
            ImporterDepVersion::Alias(key) => self.visit(&key.name, &key.suffix),
            ImporterDepVersion::File(_) => version
                .resolved_key(alias)
                .is_some_and(|key| self.visit(&key.name, &key.suffix)),
            ImporterDepVersion::Link(_) => false,
        }
    }

    /// Returns `true` once a depPath definitely disagrees.
    fn visit_snapshots(&mut self, lockfile: &Lockfile) -> bool {
        lockfile.snapshots
            .iter()
            .flatten()
            .any(|(key, snapshot)| {
                self.visit(&key.name, &key.suffix)
                    || [&snapshot.dependencies, &snapshot.optional_dependencies]
                        .into_iter()
                        .flatten()
                        .flatten()
                        .any(|(alias, reference)| self.visit_snapshot_reference(alias, reference))
            })
    }

    fn visit_snapshot_reference(&mut self, alias: &PkgName, reference: &SnapshotDepRef) -> bool {
        match reference {
            SnapshotDepRef::Plain(ver_peer) => self.visit(alias, ver_peer),
            SnapshotDepRef::Alias(key) => self.visit(&key.name, &key.suffix),
            SnapshotDepRef::Link(_) => false,
        }
    }

    /// Returns `true` when the depPath definitely disagrees.
    fn visit(&mut self, name: &PkgName, ver_peer: &PkgVerPeer) -> bool {
        if !ver_peer.peer().starts_with(PATCH_HASH_PREFIX)
            && !self.patched_names.contains(name)
        {
            return false;
        }
        let key = PackageKey::new(name.clone(), ver_peer.clone());
        let verdict = match self.verdicts.get(&key) {
            Some(verdict) => *verdict,
            None => {
                let verdict = self.judge(&key);
                self.verdicts.insert(key, verdict);
                verdict
            }
        };
        match verdict {
            Verdict::Ok => false,
            Verdict::Stale => true,
            Verdict::Indeterminate => {
                self.indeterminate = true;
                false
            }
        }
    }

    fn judge(&self, key: &PackageKey) -> Verdict {
        let recorded = match key.suffix.peer().strip_prefix(PATCH_HASH_PREFIX) {
            Some(rest) => match rest.split_once(')') {
                Some((hash, _)) => Some(hash),
                None => return Verdict::Indeterminate,
            },
            None => None,
        };
        let name = key.name.to_string();
        if self.unusable.contains(&name) {
            return Verdict::Indeterminate;
        }
        // A non-registry package's version slot holds its resolution, so the
        // version a patch was matched against is only the one its entry records.
        // Without that, a key that selects on version cannot be matched either way.
        let version = match recorded_version(key, self.packages) {
            Some(version) => version,
            None if patch_selects_on_version(self.groups.get(&name)) => {
                return Verdict::Indeterminate;
            }
            None => String::new(),
        };
        // A conflict between two configured ranges is the resolver's to report
        // (`ERR_PNPM_PATCH_KEY_CONFLICT`), and leaves this with no patch to
        // compare the segment against.
        let Ok(patch) = get_patch_info(Some(&self.groups), &name, &version) else {
            return Verdict::Indeterminate;
        };
        if patch.map(|patch| patch.hash.as_str()) == recorded {
            Verdict::Ok
        } else {
            Verdict::Stale
        }
    }
}

/// Whether which patch applies for `group`, if any, can depend on the
/// package's version. No entry resolves to no patch for every version, and a
/// bare-name entry to the same patch for every version. Either way the version
/// cannot change the answer.
fn patch_selects_on_version(group: Option<&PatchGroup>) -> bool {
    group.is_some_and(|group| !group.exact.is_empty() || !group.range.is_empty())
}

#[cfg(test)]
mod tests;
