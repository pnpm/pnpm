//! Reuse gate: decide whether the prior lockfile already satisfies a
//! wanted dependency, so the tree walker can reuse its recorded
//! resolution + subtree instead of re-resolving from the registry.
//! See `pnpm/plans/LOCKFILE_RESOLUTION_REUSE.md`.

use node_semver::{Range, Version};
use pnpm_lockfile::{
    BundledDependencies, Lockfile, LockfileResolution, PkgName, PkgNameVer, PkgNameVerPeer,
    ProjectSnapshot, RegistryContext, ResolvedDependencySpec, SnapshotEntry, StringOrList,
    TarballResolution, TarballUrlOptions, integrity_addressed_registry_tarball_url,
    npm_tarball_url, pick_registry_for_package, registry_server_type,
};
use pnpm_resolving_parse_wanted_dependency::git_specifiers_are_equivalent;
use pnpm_resolving_resolver_base::{CurrentPkg, PkgResolutionId, ResolveResult};
use serde_json::{Map, Value};

/// The `currentPkg` payload for re-resolving `key`'s edge: the prior
/// lockfile entry shaped into what the resolver expects.
pub(crate) fn current_pkg_from_lockfile(
    lockfile: &Lockfile,
    key: &PkgNameVerPeer,
    registry_context: &RegistryContext,
) -> Option<CurrentPkg> {
    let metadata_key = key.without_peer();
    let metadata = lockfile.packages.as_ref()?.get(&metadata_key)?;
    let name = metadata_key.name.to_string();
    let registry_qualified = metadata_key.suffix.registry_qualified();
    let version = metadata
        .version
        .clone()
        .or_else(|| metadata_key.suffix.version_semver().map(ToString::to_string))
        .or_else(|| registry_qualified.map(|(_, version)| version.to_string()));
    let resolution = match &metadata.resolution {
        LockfileResolution::Registry(registry_resolution) => {
            // A registry-qualified key reconstructs its tarball from its
            // named registry; everything else routes by scope. An
            // unknown alias (or an unthreaded registry map) withholds
            // `currentPkg` so the dep re-resolves normally.
            let (registry, tarball_version) = match registry_qualified {
                Some((registry_name, version)) => (
                    registry_context.registries_by_prefix.get(registry_name)?.clone(),
                    version.to_string(),
                ),
                None => (
                    pick_registry_for_package(&registry_context.registries, &name, None),
                    metadata_key.suffix.version().to_string(),
                ),
            };
            if registry.is_empty() {
                return None;
            }
            let tarball = match registry_resolution.revision {
                Some(_) => integrity_addressed_registry_tarball_url(
                    &registry_resolution.integrity,
                    &registry,
                )?,
                None => npm_tarball_url(
                    &name,
                    &tarball_version,
                    TarballUrlOptions {
                        registry: &registry,
                        server_type: registry_server_type(
                            &registry_context.registry_options_by_url,
                            &registry,
                        ),
                    },
                ),
            };
            LockfileResolution::Tarball(TarballResolution {
                tarball,
                integrity: Some(registry_resolution.integrity.clone()),
                revision: registry_resolution.revision,
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
    let satisfied = if let Some((registry_name, version)) = key.suffix.registry_qualified() {
        let range = reduce_named_registry_spec(registry_name, &key.name, bare_specifier)?
            .parse::<Range>()
            .ok()?;
        satisfies_with_prereleases(&range, version)
    } else {
        let range = bare_specifier.parse::<Range>().ok()?;
        satisfies_with_prereleases(&range, key.suffix.version_semver()?)
    };
    satisfied.then_some(key)
}

/// Reduce a named-registry specifier (`<registryName>:[<name>@]<range>`) to
/// the range to semver-check against a registry-qualified lockfile key.
///
/// `None` whenever the specifier cannot describe `key_name` from
/// `registry_name`, so the caller falls through to a fresh resolve:
///
/// * a different registry — a qualified entry must never satisfy a spec
///   aimed at another registry;
/// * a different package — the aliased shape `<registry>:<name>@<range>`
///   names its package explicitly, and an entry recorded for some other
///   name is a different dependency even though the manifest alias is
///   unchanged.
fn reduce_named_registry_spec<'a>(
    registry_name: &str,
    key_name: &PkgName,
    bare_specifier: &'a str,
) -> Option<&'a str> {
    let body = bare_specifier.strip_prefix(registry_name)?.strip_prefix(':')?;
    // `@scope/name@range` splits at the last `@`; a bare `range` has none
    // (or only the leading one of a scope, which never delimits a version).
    let Some(index) = body.rfind('@').filter(|index| *index > 0) else {
        return Some(body);
    };
    let (spec_name, range) = (&body[..index], &body[index + 1..]);
    (spec_name == key_name.to_string()).then_some(range)
}

/// The snapshot key (`snapshots:` / `packages:` map key) the prior
/// lockfile resolved `alias` to in importer `importer_id`, when the
/// recorded resolution still satisfies the manifest's `bare_specifier`.
///
/// Registry entries use semver satisfaction. Git entries require the same
/// selector (allowing equivalent hosted-repository spellings) so moving refs
/// keep their locked commit during unrelated dependency changes. Other
/// resolution shapes fall through to a normal resolve.
pub(crate) fn reusable_importer_dep(
    lockfile: &Lockfile,
    importer_id: &str,
    alias: &str,
    bare_specifier: &str,
) -> Option<PkgNameVerPeer> {
    let name: PkgName = alias.parse().ok()?;
    let spec = importer_dep(lockfile.importers.get(importer_id)?, &name)?;
    let key = spec.version.resolved_key(&name)?;
    let metadata = lockfile.packages.as_ref()?.get(&key.without_peer())?;
    let is_git = matches!(metadata.resolution, LockfileResolution::Git(_))
        || matches!(
            metadata.resolution,
            LockfileResolution::Tarball(ref tarball) if tarball.git_hosted == Some(true),
        );
    if is_git
        && (spec.specifier == bare_specifier
            || git_specifiers_are_equivalent(&spec.specifier, bare_specifier))
    {
        return Some(key);
    }
    let ver_peer = spec.version.ver_peer()?;
    let satisfied = if let Some((registry_name, version)) = ver_peer.registry_qualified() {
        let range = reduce_named_registry_spec(registry_name, &key.name, bare_specifier)?
            .parse::<Range>()
            .ok()?;
        satisfies_with_prereleases(&range, version)
    } else {
        let range = bare_specifier.parse::<Range>().ok()?;
        satisfies_with_prereleases(&range, ver_peer.version_semver()?)
    };
    if !satisfied {
        return None;
    }
    Some(key)
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

/// Whether `version` satisfies `range`, keeping a prerelease eligible
/// for a range that carries none of its own by retrying with the
/// prerelease tag stripped — the same rule peer binding applies
/// elsewhere in this crate, and looser than the range semantics the
/// required-peer picker needs.
fn satisfies_with_prereleases(range: &Range, version: &Version) -> bool {
    if range.satisfies(version) {
        return true;
    }
    if !version.is_prerelease() {
        return false;
    }
    let release = Version {
        major: version.major,
        minor: version.minor,
        patch: version.patch,
        pre_release: Vec::new(),
        build: Vec::new(),
    };
    range.satisfies(&release)
}

/// Synthesize the [`ResolveResult`] a fresh resolve of `key` would have
/// produced, reading the recorded resolution + manifest metadata out of
/// the prior lockfile instead of hitting the registry or git remote.
///
/// Registry tarballs, git resolutions, and git-hosted tarballs contain all
/// required resolution and manifest metadata in `lockfile.packages`.
/// Directory, binary, variations, custom, and arbitrary remote-tarball
/// resolutions fall through because the lockfile does not capture enough
/// resolver state to reproduce them safely.
///
/// The synthesized result reproduces the node shape a fresh resolve
/// yields:
///
/// * Registry `id` / `name_ver` use the peer-stripped `name@version`; git
///   results retain their URL-shaped lockfile key and have no `name_ver`.
/// * `resolution` is cloned from [`pnpm_lockfile::PackageMetadata`]
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
    let metadata = lockfile.packages.as_ref()?.get(&metadata_key)?;
    let registry_version = metadata_key.suffix.version_semver().cloned();
    let git_resolution = match &metadata.resolution {
        LockfileResolution::Registry(_) => false,
        LockfileResolution::Tarball(tarball)
            if tarball.integrity.is_some() && tarball.git_hosted != Some(true) =>
        {
            false
        }
        LockfileResolution::Tarball(tarball) if tarball.git_hosted == Some(true) => true,
        LockfileResolution::Git(_) => true,
        // Custom resolutions fall through with the rest: reuse would
        // bypass the pnpmfile custom resolver that owns them.
        LockfileResolution::Tarball(_)
        | LockfileResolution::Directory(_)
        | LockfileResolution::Binary(_)
        | LockfileResolution::Variations(_)
        | LockfileResolution::Custom(_) => return None,
    };
    let (id, name_ver, resolved_via) = if git_resolution {
        (metadata_key.to_string(), None, "git-repository")
    } else if let Some((_, version)) = metadata_key.suffix.registry_qualified() {
        // A reused registry-qualified entry keeps its qualified id so the
        // rebuilt graph re-emits the same lockfile key.
        let name_ver = PkgNameVer::new(metadata_key.name.clone(), version.clone());
        (metadata_key.to_string(), Some(name_ver), "named-registry")
    } else {
        let name_ver = PkgNameVer::new(metadata_key.name.clone(), registry_version?);
        (name_ver.to_string(), Some(name_ver), "npm-registry")
    };
    let manifest_version =
        metadata.version.clone().or_else(|| name_ver.as_ref().map(|nv| nv.suffix.to_string()));
    let manifest = synthesize_manifest(&metadata_key.name, manifest_version.as_deref(), metadata);
    Some(ResolveResult {
        id: PkgResolutionId::from(id),
        name_ver,
        latest: None,
        published_at: None,
        manifest: Some(std::sync::Arc::new(manifest)),
        resolution: metadata.resolution.clone(),
        resolved_via: resolved_via.to_string(),
        normalized_bare_specifier: None,
        alias: Some(alias.to_string()),
        policy_violation: None,
    })
}

/// Reconstruct the minimal manifest fragment downstream consumers read
/// off a reused [`ResolveResult`]. Carries the peer / platform /
/// packaging metadata the lockfile records, in the shape it recorded it,
/// so a rewrite re-emits the entry unchanged; omits `dependencies`
/// because a reused node's children come from the snapshot graph, not
/// the manifest.
fn synthesize_manifest(
    name: &PkgName,
    version: Option<&str>,
    metadata: &pnpm_lockfile::PackageMetadata,
) -> Value {
    let mut manifest = Map::new();
    manifest.insert("name".to_string(), Value::String(name.to_string()));
    if let Some(version) = version {
        manifest.insert("version".to_string(), Value::String(version.to_string()));
    }

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
        let value = match libc {
            StringOrList::String(single) => Value::String(single.clone()),
            StringOrList::List(names) => string_array(names),
        };
        manifest.insert("libc".to_string(), value);
    }
    if let Some(deprecated) = metadata.deprecated.as_ref() {
        manifest.insert("deprecated".to_string(), Value::String(deprecated.clone()));
    }
    if let Some(bundled) = metadata.bundled_dependencies.as_ref() {
        let value = match bundled {
            BundledDependencies::Boolean(bundles_all) => Value::Bool(*bundles_all),
            BundledDependencies::Names(names) => string_array(names),
        };
        manifest.insert("bundledDependencies".to_string(), value);
    }
    // `has_bin: Some(true)` round-trips as a truthy `bin` so the
    // bundled-manifest bin linker sees a non-empty bin set; the exact
    // bin paths live in the store-index bundled manifest the install
    // pass reads, not here.
    if metadata.has_bin == Some(true) {
        manifest.insert("bin".to_string(), Value::String(name.to_string()));
    }

    Value::Object(manifest)
}

fn string_array(items: &[String]) -> Value {
    Value::Array(items.iter().map(|item| Value::String(item.clone())).collect())
}

#[cfg(test)]
mod tests;
