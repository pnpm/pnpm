use super::{RewriteContext, RewritePlan};
use indexmap::IndexMap;
use node_semver::{Range, Version};
use pnpm_config_parse_overrides::{PackageSelector, VersionOverride};
use pnpm_lockfile::{
    BundledDependencies, Lockfile, LockfileResolution, PackageKey, PackageMetadata, PkgName,
    PkgNameVerPeer, PkgVerPeer, Prefix, StringOrList,
};
use pnpm_resolving_resolver_base::{ResolveResult, WantedDependency};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

pub(crate) struct FastOverride {
    pub name: PkgName,
    pub new_version: Option<Version>,
    pub old_version: Option<Version>,
    pub parent: Option<PackageSelector>,
}
pub(super) struct ResolvedOverride {
    pub(super) manifest: Arc<Value>,
    pub(super) resolution: LockfileResolution,
}
pub(super) async fn resolve_override(
    context: &RewriteContext<'_>,
    override_entry: &FastOverride,
) -> Option<(PkgName, ResolvedOverride)> {
    let name = override_entry.name.to_string();
    let version = override_entry.new_version.as_ref()?.to_string();
    let wanted = WantedDependency {
        alias: Some(name.clone()),
        bare_specifier: Some(version.clone()),
        ..WantedDependency::default()
    };
    let result = context.resolver.resolve(&wanted, context.resolve_options).await.ok()??;
    let manifest = result.manifest.as_ref().map(Arc::clone)?;
    let manifest = match context.manifest_hook {
        Some(hook) => hook(manifest),
        None => manifest,
    };
    if !is_safe_registry_result(&result, &manifest, &name, &version) {
        return None;
    }
    Some((
        override_entry.name.clone(),
        ResolvedOverride { manifest, resolution: result.resolution },
    ))
}
pub(super) fn build_rewrite_plan(
    lockfile: &Lockfile,
    parsed_overrides: &[VersionOverride],
    resolved_overrides: &IndexMap<String, String>,
) -> Option<RewritePlan> {
    let old_overrides = lockfile.overrides.as_ref();
    if old_overrides
        .is_some_and(|old| old.keys().any(|selector| !resolved_overrides.contains_key(selector)))
    {
        return None;
    }
    let parsed_by_selector: HashMap<&str, &VersionOverride> =
        parsed_overrides.iter().map(|entry| (entry.selector.as_str(), entry)).collect();
    let mut overrides = Vec::new();
    for (selector, new_value) in resolved_overrides {
        let old_value = old_overrides.and_then(|old| old.get(selector));
        if old_value == Some(new_value) {
            continue;
        }
        let parsed = parsed_by_selector.get(selector.as_str())?;
        overrides.push(fast_override(lockfile, parsed_overrides, parsed, new_value, old_value)?);
    }
    if overrides.is_empty() {
        return None;
    }

    build_replacement_plan(lockfile, overrides)
}
/// One override entry as the rewrite plan records it. `None` for an override
/// shape a rewrite cannot express: one carrying its own specifier, a
/// converging one, or one that shares its target package with another entry.
pub(super) fn fast_override(
    lockfile: &Lockfile,
    parsed_overrides: &[VersionOverride],
    parsed: &VersionOverride,
    new_value: &str,
    old_value: Option<&String>,
) -> Option<FastOverride> {
    if parsed.target_pkg.bare_specifier.is_some()
        || parsed.converge
        || parsed_overrides.iter().any(|candidate| {
            candidate.selector != parsed.selector
                && candidate.target_pkg.name == parsed.target_pkg.name
        })
    {
        return None;
    }
    let removes_dependency = new_value == "-";
    let name = PkgName::parse(&parsed.target_pkg.name).ok()?;
    let new_version = if removes_dependency {
        None
    } else {
        Some(overridden_version(lockfile, &name, new_value)?)
    };
    let old_version = match (removes_dependency, old_value) {
        (true, _) => None,
        (false, Some(value)) => Some(Version::parse(value).ok()?),
        (false, None) => None,
    };
    Some(FastOverride { name, new_version, old_version, parent: parsed.parent_pkg.clone() })
}
/// The version an override moves its target to.
///
/// A range names the highest already-locked version satisfying it,
/// because `preferredVersions` makes the resolver reuse a version the
/// graph already holds rather than the highest published.
pub(super) fn overridden_version(
    lockfile: &Lockfile,
    name: &PkgName,
    value: &str,
) -> Option<Version> {
    if let Ok(version) = Version::parse(value) {
        return Some(version);
    }
    let range = Range::parse(value).ok()?;
    // `resolutionMode` only moves direct dependencies to the low end of
    // their range, and an override names a package at any depth.
    let pick = crate::fast_update_importers::locked_version_resolution_would_pick(
        lockfile.snapshots.as_ref(),
        name,
        &range,
        false,
    )?;
    // The plan records the target as a bare version, and a peer variant is
    // a key `build_replacement_plan` refuses to rewrite anyway.
    (!pick.peer_suffixed).then_some(pick.version)
}
/// Work out which locked packages each entry of `overrides` moves, and
/// refuse the ones a rewrite cannot express: a package carrying a peer
/// suffix or a registry qualifier, one that is patched, optional, or has
/// its own peer dependencies, and any key that something outside
/// `overrides` also reaches.
pub(crate) fn build_replacement_plan(
    lockfile: &Lockfile,
    overrides: Vec<FastOverride>,
) -> Option<RewritePlan> {
    // Two entries naming one package would each claim its key, and only one
    // of them could win. Both callers reject that earlier for their own
    // reasons; this keeps the plan itself from expressing it.
    if overrides
        .iter()
        .enumerate()
        .any(|(index, entry)| overrides[..index].iter().any(|other| other.name == entry.name))
    {
        return None;
    }
    let peer_names = get_peer_names(lockfile);
    if overrides
        .iter()
        .filter(|entry| entry.new_version.is_none())
        .any(|entry| peer_names.contains(&entry.name))
    {
        return None;
    }

    let by_name: HashMap<&PkgName, &FastOverride> = overrides
        .iter()
        .filter(|entry| entry.new_version.is_some())
        .map(|entry| (&entry.name, entry))
        .collect();
    let mut replacements = HashMap::new();
    for (alias, key) in all_dependency_keys(lockfile) {
        let Some(override_entry) = by_name.get(alias) else { continue };
        let key = key?;
        let replacement = override_replacement(lockfile, alias, &key, override_entry)?;
        replacements.insert(key, replacement);
    }
    for (alias, key) in all_dependency_keys(lockfile) {
        if key.is_some_and(|key| replacements.contains_key(&key)) && !by_name.contains_key(alias) {
            return None;
        }
    }
    Some(RewritePlan { overrides, peer_names, replacements })
}
/// The key one locked package is rewritten to, or `None` when the rewrite
/// cannot express the move.
pub(super) fn override_replacement(
    lockfile: &Lockfile,
    alias: &PkgName,
    key: &PackageKey,
    override_entry: &FastOverride,
) -> Option<PkgNameVerPeer> {
    if key.name != *alias
        || !key.suffix.peer().is_empty()
        || key.suffix.prefix() != Prefix::None
        // The fast path rebuilds the dep path as `<alias>@<version>`,
        // which would drop the registry qualifier of a named-registry
        // package.
        || key.suffix.registry_qualified().is_some()
        || override_entry
            .old_version
            .as_ref()
            .is_some_and(|old| key.suffix.version_semver() != Some(old))
    {
        return None;
    }
    let old_snapshot = lockfile.snapshots.as_ref()?.get(key)?;
    let old_metadata = lockfile.packages.as_ref()?.get(&key.without_peer())?;
    let safe_resolution = matches!(old_metadata.resolution, LockfileResolution::Registry(_))
        || matches!(
            old_metadata.resolution,
            LockfileResolution::Tarball(ref tarball)
                if tarball.integrity.is_some() && tarball.git_hosted != Some(true),
        );
    if old_snapshot.optional
        || old_snapshot.patched == Some(true)
        || old_snapshot.id.is_some()
        || old_metadata.peer_dependencies.is_some()
        || old_metadata.peer_dependencies_meta.is_some()
        || !safe_resolution
    {
        return None;
    }
    let new_suffix: PkgVerPeer = override_entry.new_version.as_ref()?.to_string().parse().ok()?;
    Some(PkgNameVerPeer::new(alias.clone(), new_suffix))
}
pub(super) fn get_peer_names(lockfile: &Lockfile) -> HashSet<PkgName> {
    let mut result = HashSet::new();
    for metadata in lockfile.packages.as_ref().into_iter().flat_map(|map| map.values()) {
        insert_parsed_names(
            &mut result,
            metadata.peer_dependencies.as_ref().into_iter().flat_map(|map| map.keys()),
        );
        insert_parsed_names(
            &mut result,
            metadata.peer_dependencies_meta.as_ref().into_iter().flat_map(|map| map.keys()),
        );
    }
    for snapshot in lockfile.snapshots.as_ref().into_iter().flat_map(|map| map.values()) {
        insert_parsed_names(&mut result, snapshot.transitive_peer_dependencies.iter().flatten());
    }
    result
}
/// Names that do not parse as a package name cannot be a peer of anything the
/// rewrite touches, so they are dropped rather than failing the plan.
pub(super) fn insert_parsed_names<'a>(
    result: &mut HashSet<PkgName>,
    names: impl Iterator<Item = &'a String>,
) {
    for name in names {
        if let Ok(name) = PkgName::parse(name) {
            result.insert(name);
        }
    }
}
pub(super) fn all_dependency_keys(lockfile: &Lockfile) -> Vec<(&PkgName, Option<PackageKey>)> {
    let importer_keys = lockfile.importers.values().flat_map(|importer| {
        [
            importer.dependencies.as_ref(),
            importer.dev_dependencies.as_ref(),
            importer.optional_dependencies.as_ref(),
        ]
        .into_iter()
        .flatten()
        .flatten()
        .map(|(alias, spec)| (alias, spec.version.resolved_key(alias)))
    });
    let snapshot_keys =
        lockfile.snapshots.as_ref().into_iter().flat_map(|map| map.values()).flat_map(|snapshot| {
            [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()]
                .into_iter()
                .flatten()
                .flatten()
                .map(|(alias, dep_ref)| (alias, dep_ref.resolve(alias)))
        });
    importer_keys.chain(snapshot_keys).collect()
}
pub(super) fn is_safe_registry_result(
    result: &ResolveResult,
    manifest: &Value,
    name: &str,
    version: &str,
) -> bool {
    result.resolved_via == "npm-registry"
        && result.policy_violation.is_none()
        && result.name_ver.as_ref().is_some_and(|name_ver| {
            name_ver.name.to_string() == name && name_ver.suffix.to_string() == version
        })
        && manifest.get("name").and_then(Value::as_str) == Some(name)
        && manifest.get("version").and_then(Value::as_str) == Some(version)
        && manifest
            .get("peerDependencies")
            .is_none_or(|value| value.as_object().is_some_and(serde_json::Map::is_empty))
        && manifest
            .get("peerDependenciesMeta")
            .is_none_or(|value| value.as_object().is_some_and(serde_json::Map::is_empty))
        && manifest.get("deprecated").is_none()
        && manifest.get("bundledDependencies").is_none()
        && manifest.get("bundleDependencies").is_none()
        && manifest.get("engines").is_none_or(|value| {
            value.as_object().is_some_and(|engines| !engines.contains_key("runtime"))
        })
        && matches!(
            result.resolution,
            LockfileResolution::Tarball(ref tarball)
                if tarball.integrity.is_some() && tarball.git_hosted != Some(true),
        )
}
pub(super) fn package_metadata(
    manifest: &Value,
    resolution: LockfileResolution,
) -> PackageMetadata {
    PackageMetadata {
        resolution,
        version: None,
        engines: string_map(manifest, "engines")
            .map(|map| map.into_iter().filter(|(_, range)| range != "*").collect())
            .filter(|map: &HashMap<_, _>| !map.is_empty()),
        cpu: string_list(manifest, "cpu"),
        os: string_list(manifest, "os"),
        libc: manifest.get("libc").and_then(|value| match value {
            Value::String(value) => Some(StringOrList::String(value.clone())),
            Value::Array(_) => string_list(manifest, "libc").map(StringOrList::List),
            _ => None,
        }),
        deprecated: None,
        has_bin: crate::dependencies_graph_to_lockfile::manifest_has_bin(Some(manifest)),
        prepare: None,
        bundled_dependencies: BundledDependencies::from_manifest(Some(manifest)),
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}
pub(super) fn string_map(manifest: &Value, key: &str) -> Option<HashMap<String, String>> {
    let map = manifest.get(key)?.as_object()?;
    Some(
        map.iter()
            .filter_map(|(name, value)| Some((name.clone(), value.as_str()?.to_string())))
            .collect(),
    )
}
pub(super) fn string_list(manifest: &Value, key: &str) -> Option<Vec<String>> {
    let values = manifest.get(key)?.as_array()?;
    let values: Vec<String> =
        values.iter().filter_map(Value::as_str).map(ToString::to_string).collect();
    (!values.is_empty()).then_some(values)
}
