//! Reuse gate: decide whether the prior lockfile already satisfies a
//! wanted dependency, so the tree walker can reuse its recorded
//! resolution + subtree instead of re-resolving from the registry.
//! Mirrors pnpm's `satisfiesWanted` / `getInfoFromLockfile` gate in
//! [`resolveDependencies.ts`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1086-L1248).
//! See `pacquet/plans/LOCKFILE_RESOLUTION_REUSE.md`.

use std::collections::HashMap;

use node_semver::Range;
use pacquet_lockfile::{
    Lockfile, LockfileResolution, PkgName, PkgNameVer, PkgNameVerPeer, ProjectSnapshot,
    ResolvedDependencySpec, SnapshotEntry, TarballResolution, npm_tarball_url,
    pick_registry_for_package,
};
use pacquet_resolving_resolver_base::{CurrentPkg, PkgResolutionId, ResolveResult};
use serde_json::{Map, Value};

use crate::hoist_peers::satisfies_including_prerelease;

/// The `currentPkg` payload for re-resolving `key`'s edge: the prior
/// lockfile entry in the shape pnpm's `getInfoFromLockfile` +
/// [`pkgSnapshotToResolution`](https://github.com/pnpm/pnpm/blob/1627943d2a/lockfile/utils/src/pkgSnapshotToResolution.ts)
/// hand the resolver.
pub(crate) fn current_pkg_from_lockfile(
    lockfile: &Lockfile,
    key: &PkgNameVerPeer,
    registries: &HashMap<String, String>,
) -> Option<CurrentPkg> {
    let metadata_key = key.without_peer();
    let metadata = lockfile.packages.as_ref()?.get(&metadata_key)?;
    let name = metadata_key.name.to_string();
    let version = metadata
        .version
        .clone()
        .or_else(|| metadata_key.suffix.version_semver().map(ToString::to_string));
    let resolution = match &metadata.resolution {
        LockfileResolution::Registry(registry_resolution) => {
            let registry = pick_registry_for_package(registries, &name, None);
            if registry.is_empty() {
                // No registry map was threaded in (e.g. the
                // single-importer entry point) — a `Registry` entry
                // can't be materialized into its tarball URL, and a
                // URL-less payload would diverge from pnpm's shape.
                return None;
            }
            let tarball_version = metadata_key.suffix.version().to_string();
            LockfileResolution::Tarball(TarballResolution {
                tarball: npm_tarball_url(&name, &tarball_version, &registry),
                integrity: Some(registry_resolution.integrity.clone()),
                git_hosted: None,
                path: None,
            })
        }
        recorded => recorded.clone(),
    };
    Some(CurrentPkg {
        id: PkgResolutionId::from(metadata_key.to_string()),
        name: Some(name),
        version,
        resolution,
        published_at: None,
    })
}

/// The prior snapshot key recorded for child edge `alias` under
/// `snapshot`'s dependency maps, when the recorded version still
/// satisfies `bare_specifier`.
pub(crate) fn prior_child_key(
    snapshot: &SnapshotEntry,
    alias: &str,
    bare_specifier: &str,
) -> Option<PkgNameVerPeer> {
    let name: PkgName = alias.parse().ok()?;
    let dep_ref = snapshot
        .dependencies
        .as_ref()
        .and_then(|deps| deps.get(&name))
        .or_else(|| snapshot.optional_dependencies.as_ref().and_then(|deps| deps.get(&name)))?;
    let key = dep_ref.resolve(&name)?;
    let range = bare_specifier.parse::<Range>().ok()?;
    let satisfied = satisfies_including_prerelease(&range, key.suffix.version_semver()?);
    satisfied.then_some(key)
}

/// The snapshot key (`snapshots:` / `packages:` map key) the prior
/// lockfile resolved `alias` to in importer `importer_id`, when the
/// recorded version still satisfies the manifest's `bare_specifier`
/// (semver-satisfies, matching pnpm's `satisfiesWanted`).
///
/// Reuse is limited to semver (registry/tarball) deps; richer shapes
/// (`link:`/`file:`/`workspace:`/`catalog:`) fall through to a normal
/// resolve.
pub(crate) fn reusable_importer_dep(
    importers: &HashMap<String, ProjectSnapshot>,
    importer_id: &str,
    alias: &str,
    bare_specifier: &str,
) -> Option<PkgNameVerPeer> {
    let name: PkgName = alias.parse().ok()?;
    let spec = importer_dep(importers.get(importer_id)?, &name)?;
    let version = spec.version.ver_peer()?.version_semver()?;
    let range = bare_specifier.parse::<Range>().ok()?;
    if !satisfies_including_prerelease(&range, version) {
        return None;
    }
    spec.version.resolved_key(&name)
}

/// The recorded resolution for `name` across the importer's prod /
/// optional / dev dependency maps.
fn importer_dep<'a>(
    importer: &'a ProjectSnapshot,
    name: &PkgName,
) -> Option<&'a ResolvedDependencySpec> {
    importer
        .dependencies
        .as_ref()
        .and_then(|deps| deps.get(name))
        .or_else(|| importer.optional_dependencies.as_ref().and_then(|deps| deps.get(name)))
        .or_else(|| importer.dev_dependencies.as_ref().and_then(|deps| deps.get(name)))
}

/// Synthesize the [`ResolveResult`] a fresh resolve of `key` would have
/// produced, reading the recorded resolution + manifest metadata out of
/// the prior lockfile instead of hitting the registry.
///
/// Conservative by design: returns `None` (so the caller resolves
/// fresh) unless the package is a plain-semver registry package with an
/// entry in `lockfile.packages`. pacquet's npm resolver records every
/// registry pick as a [`LockfileResolution::Tarball`] carrying the
/// registry tarball URL + integrity (it never emits the bare
/// `Registry` shape — see
/// [`npm_resolver`](https://github.com/pnpm/pnpm/blob/097983fbca/resolving/npm-resolver/src/index.ts)),
/// so both `Tarball` and `Registry` are accepted here. The
/// `version_semver()` gate keeps reuse to registry packages: a remote
/// (non-registry) tarball or git dep carries a URL-shaped, non-semver
/// version slot and falls through to a fresh resolve. Git-hosted
/// tarballs (which need preparation on extraction) are rejected
/// outright. Directory / git / binary / variations resolutions also
/// fall through — reusing them would need resolver state the lockfile
/// doesn't fully capture, and a wrong reuse produces a wrong tree.
///
/// The synthesized result reproduces the node shape a fresh resolve
/// yields:
///
/// * `id` / `name_ver` are the peer-stripped `name@version`, the
///   `pkgIdWithPatchHash` the dedup map keys on (the peer suffix is
///   re-derived by the peer pass).
/// * `resolution` is cloned from [`pacquet_lockfile::PackageMetadata`]
///   so the recorded integrity carries forward.
/// * `manifest` is reconstructed from the metadata's
///   `peerDependencies` / `peerDependenciesMeta` / `engines` / `cpu` /
///   `os` / `libc` / `hasBin` so `extract_peer_dependencies`
///   and the leaf classifier behave identically to a fresh resolve.
///   `dependencies` are deliberately omitted — the children come from
///   the snapshot graph, not this manifest.
pub(crate) fn synthesize_reused_result(
    lockfile: &Lockfile,
    key: &PkgNameVerPeer,
    alias: &str,
) -> Option<ResolveResult> {
    let metadata_key = key.without_peer();
    let version = metadata_key.suffix.version_semver()?.clone();
    let metadata = lockfile.packages.as_ref()?.get(&metadata_key)?;
    match &metadata.resolution {
        LockfileResolution::Registry(_) => {}
        LockfileResolution::Tarball(tarball)
            if tarball.integrity.is_some() && tarball.git_hosted != Some(true) => {}
        LockfileResolution::Tarball(_)
        | LockfileResolution::Directory(_)
        | LockfileResolution::Git(_)
        | LockfileResolution::Binary(_)
        | LockfileResolution::Variations(_) => return None,
    }
    let name_ver = PkgNameVer::new(metadata_key.name.clone(), version);
    let manifest = synthesize_manifest(&name_ver, metadata);
    Some(ResolveResult {
        id: PkgResolutionId::from(name_ver.to_string()),
        name_ver: Some(name_ver),
        latest: None,
        published_at: None,
        manifest: Some(std::sync::Arc::new(manifest)),
        resolution: metadata.resolution.clone(),
        resolved_via: "npm-registry".to_string(),
        normalized_bare_specifier: None,
        alias: Some(alias.to_string()),
        policy_violation: None,
    })
}

/// Reconstruct the minimal manifest fragment downstream consumers read
/// off a reused [`ResolveResult`]. Carries the peer / platform metadata
/// the lockfile records; omits `dependencies` because a reused node's
/// children come from the snapshot graph, not the manifest.
fn synthesize_manifest(
    name_ver: &PkgNameVer,
    metadata: &pacquet_lockfile::PackageMetadata,
) -> Value {
    let mut manifest = Map::new();
    manifest.insert("name".to_string(), Value::String(name_ver.name.to_string()));
    manifest.insert("version".to_string(), Value::String(name_ver.suffix.to_string()));

    if let Some(peers) = metadata.peer_dependencies.as_ref() {
        let map: Map<String, Value> = peers
            .iter()
            .map(|(name, range)| (name.clone(), Value::String(range.clone())))
            .collect();
        manifest.insert("peerDependencies".to_string(), Value::Object(map));
    }
    if let Some(meta) = metadata.peer_dependencies_meta.as_ref() {
        let map: Map<String, Value> = meta
            .iter()
            .map(|(name, entry)| {
                let mut obj = Map::new();
                obj.insert("optional".to_string(), Value::Bool(entry.optional));
                (name.clone(), Value::Object(obj))
            })
            .collect();
        manifest.insert("peerDependenciesMeta".to_string(), Value::Object(map));
    }
    if let Some(engines) = metadata.engines.as_ref() {
        let map: Map<String, Value> = engines
            .iter()
            .map(|(name, range)| (name.clone(), Value::String(range.clone())))
            .collect();
        manifest.insert("engines".to_string(), Value::Object(map));
    }
    if let Some(cpu) = metadata.cpu.as_ref() {
        manifest.insert("cpu".to_string(), string_array(cpu));
    }
    if let Some(os) = metadata.os.as_ref() {
        manifest.insert("os".to_string(), string_array(os));
    }
    if let Some(libc) = metadata.libc.as_ref() {
        manifest.insert("libc".to_string(), string_array(libc));
    }
    if let Some(deprecated) = metadata.deprecated.as_ref() {
        manifest.insert("deprecated".to_string(), Value::String(deprecated.clone()));
    }
    // `has_bin: Some(true)` round-trips as a truthy `bin` so the
    // bundled-manifest bin linker sees a non-empty bin set; the exact
    // bin paths live in the store-index bundled manifest the install
    // pass reads, not here.
    if metadata.has_bin == Some(true) {
        manifest.insert("bin".to_string(), Value::String(name_ver.name.to_string()));
    }

    Value::Object(manifest)
}

fn string_array(items: &[String]) -> Value {
    Value::Array(items.iter().map(|item| Value::String(item.clone())).collect())
}

#[cfg(test)]
mod tests;
