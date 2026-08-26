use futures_util::future::join_all;
use indexmap::IndexMap;
use node_semver::{Range, Version};
use pnpm_config_parse_overrides::{PackageSelector, VersionOverride};
use pnpm_lockfile::{
    BundledDependencies, ImporterDepVersion, Lockfile, LockfileFormOptions, LockfileResolution,
    PackageKey, PackageMetadata, PkgName, PkgNameVerPeer, PkgVerPeer, Prefix, RegistryOptions,
    ResolvedDependencyMap, SnapshotDepRef, SnapshotEntry, StringOrList, pick_registry_for_package,
    registry_server_type,
};
use pnpm_resolving_deps_resolver::ManifestHook;
use pnpm_resolving_resolver_base::{ResolveOptions, ResolveResult, Resolver, WantedDependency};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};

pub(crate) struct FastOverride {
    pub name: PkgName,
    pub new_version: Option<Version>,
    pub old_version: Option<Version>,
    pub parent: Option<PackageSelector>,
}

pub(crate) struct RewritePlan {
    pub overrides: Vec<FastOverride>,
    pub peer_names: HashSet<PkgName>,
    pub replacements: HashMap<PackageKey, PackageKey>,
}

struct ResolvedOverride {
    manifest: Arc<Value>,
    resolution: LockfileResolution,
}

pub(crate) struct FastOverrideOptions<'a> {
    pub context: RewriteContext<'a>,
    pub parsed_overrides: &'a [VersionOverride],
    pub resolved_overrides: &'a IndexMap<String, String>,
}

/// What a rewrite needs regardless of which setting drove it: the
/// lockfile being rewritten, and the resolver that fetches the manifest
/// of every version the rewrite introduces.
pub(crate) struct RewriteContext<'a> {
    pub lockfile: &'a Lockfile,
    pub resolver: &'a dyn Resolver,
    pub resolve_options: &'a ResolveOptions,
    pub manifest_hook: Option<&'a ManifestHook>,
    pub registries: &'a HashMap<String, String>,
    pub registry_options_by_url: &'a BTreeMap<String, RegistryOptions>,
    pub lockfile_include_tarball_url: bool,
}

pub(crate) async fn try_fast_update_overrides(opts: FastOverrideOptions<'_>) -> Option<Lockfile> {
    let plan =
        build_rewrite_plan(opts.context.lockfile, opts.parsed_overrides, opts.resolved_overrides)?;
    let mut updated = apply_rewrite_plan(&opts.context, &plan).await?;
    updated.overrides = Some(opts.resolved_overrides.clone());
    Some(updated)
}

/// Move every package named by `plan` to its new version, rebuilding the
/// affected `packages:` and `snapshots:` entries from the new version's
/// manifest and redirecting everything that referenced the old key.
///
/// `None` whenever the move cannot be proven safe from the lockfile plus
/// the resolved manifests — a locked child that the new version's
/// manifest no longer admits, a registry result that does not match what
/// was asked for, or an entry the rewrite would have to overwrite with
/// something different.
pub(crate) async fn apply_rewrite_plan(
    context: &RewriteContext<'_>,
    plan: &RewritePlan,
) -> Option<Lockfile> {
    let resolutions = join_all(
        plan.overrides
            .iter()
            .filter(|override_entry| {
                override_entry.new_version.is_some()
                    && plan
                        .replacements
                        .iter()
                        .any(|(old, new)| old != new && old.name == override_entry.name)
            })
            .map(|override_entry| resolve_override(context, override_entry)),
    )
    .await;
    let resolved: HashMap<_, _> =
        resolutions.into_iter().collect::<Option<Vec<_>>>()?.into_iter().collect();
    rewrite_lockfile(context, plan, &resolved)
}

async fn resolve_override(
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

fn build_rewrite_plan(
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
        let removes_dependency = new_value == "-";
        if parsed.target_pkg.bare_specifier.is_some()
            || parsed.converge
            || parsed_overrides.iter().any(|candidate| {
                candidate.selector != *selector
                    && candidate.target_pkg.name == parsed.target_pkg.name
            })
        {
            return None;
        }
        let name = PkgName::parse(&parsed.target_pkg.name).ok()?;
        let new_version = if removes_dependency {
            None
        } else {
            Some(overridden_version(lockfile, &name, new_value)?)
        };
        overrides.push(FastOverride {
            name,
            new_version,
            old_version: match (removes_dependency, old_value) {
                (true, _) => None,
                (false, Some(value)) => Some(Version::parse(value).ok()?),
                (false, None) => None,
            },
            parent: parsed.parent_pkg.clone(),
        });
    }
    if overrides.is_empty() {
        return None;
    }

    build_replacement_plan(lockfile, overrides)
}

/// The version an override moves its target to.
///
/// A range names the highest already-locked version satisfying it,
/// because `preferredVersions` makes the resolver reuse a version the
/// graph already holds rather than the highest published.
fn overridden_version(lockfile: &Lockfile, name: &PkgName, value: &str) -> Option<Version> {
    if let Ok(version) = Version::parse(value) {
        return Some(version);
    }
    let range = Range::parse(value).ok()?;
    // `resolutionMode` only moves direct dependencies to the low end of
    // their range, and an override names a package at any depth.
    crate::fast_update_importers::locked_version_resolution_would_pick(
        lockfile.packages.as_ref(),
        name,
        &range,
        false,
    )
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
    // Two entries naming one package would each claim its key, and only
    // one of them could win.
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
        let old_snapshot = lockfile.snapshots.as_ref()?.get(&key)?;
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
        let new_suffix: PkgVerPeer =
            override_entry.new_version.as_ref()?.to_string().parse().ok()?;
        replacements.insert(key, PkgNameVerPeer::new(alias.clone(), new_suffix));
    }
    for (alias, key) in all_dependency_keys(lockfile) {
        if key.is_some_and(|key| replacements.contains_key(&key)) && !by_name.contains_key(alias) {
            return None;
        }
    }
    Some(RewritePlan { overrides, peer_names, replacements })
}

fn get_peer_names(lockfile: &Lockfile) -> HashSet<PkgName> {
    let mut result = HashSet::new();
    for metadata in lockfile.packages.as_ref().into_iter().flat_map(|map| map.values()) {
        for name in metadata.peer_dependencies.as_ref().into_iter().flat_map(|map| map.keys()) {
            if let Ok(name) = PkgName::parse(name) {
                result.insert(name);
            }
        }
        for name in metadata.peer_dependencies_meta.as_ref().into_iter().flat_map(|map| map.keys())
        {
            if let Ok(name) = PkgName::parse(name) {
                result.insert(name);
            }
        }
    }
    for snapshot in lockfile.snapshots.as_ref().into_iter().flat_map(|map| map.values()) {
        for name in snapshot.transitive_peer_dependencies.iter().flatten() {
            if let Ok(name) = PkgName::parse(name) {
                result.insert(name);
            }
        }
    }
    result
}

fn all_dependency_keys(lockfile: &Lockfile) -> Vec<(&PkgName, Option<PackageKey>)> {
    let mut result = Vec::new();
    for importer in lockfile.importers.values() {
        for dependencies in [
            importer.dependencies.as_ref(),
            importer.dev_dependencies.as_ref(),
            importer.optional_dependencies.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            for (alias, spec) in dependencies {
                result.push((alias, spec.version.resolved_key(alias)));
            }
        }
    }
    for snapshot in lockfile.snapshots.as_ref().into_iter().flat_map(|map| map.values()) {
        for dependencies in
            [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()]
                .into_iter()
                .flatten()
        {
            for (alias, dep_ref) in dependencies {
                result.push((alias, dep_ref.resolve(alias)));
            }
        }
    }
    result
}

fn is_safe_registry_result(
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

fn rewrite_lockfile(
    context: &RewriteContext<'_>,
    plan: &RewritePlan,
    resolved: &HashMap<PkgName, ResolvedOverride>,
) -> Option<Lockfile> {
    let mut updated = context.lockfile.clone();
    for importer in updated.importers.values_mut() {
        rewrite_importer_dependencies(&mut importer.dependencies, plan);
        rewrite_importer_dependencies(&mut importer.dev_dependencies, plan);
        rewrite_importer_dependencies(&mut importer.optional_dependencies, plan);
    }
    let original_snapshots = context.lockfile.snapshots.as_ref()?;
    let mut snapshots = original_snapshots.clone();
    let mut packages = context.lockfile.packages.clone()?;
    for (key, snapshot) in &mut snapshots {
        rewrite_snapshot_dependencies(&mut snapshot.dependencies, plan, Some(key));
        rewrite_snapshot_dependencies(&mut snapshot.optional_dependencies, plan, Some(key));
    }
    for (old_key, new_key) in &plan.replacements {
        if old_key == new_key {
            continue;
        }
        let replacement = resolved.get(&old_key.name)?;
        let old_snapshot = original_snapshots.get(old_key)?;
        let dependencies = validate_dependencies(
            effective_dependencies(&replacement.manifest)?,
            old_snapshot.dependencies.as_ref(),
            original_snapshots,
            context.lockfile.packages.as_ref()?,
            plan,
            new_key,
        )?;
        let optional_dependencies = validate_dependencies(
            manifest_dependency_map(&replacement.manifest, "optionalDependencies")?,
            old_snapshot.optional_dependencies.as_ref(),
            original_snapshots,
            context.lockfile.packages.as_ref()?,
            plan,
            new_key,
        )?;
        let snapshot =
            SnapshotEntry { dependencies, optional_dependencies, ..old_snapshot.clone() };
        if let Some(existing) = snapshots.get(new_key)
            && existing != &snapshot
        {
            return None;
        }
        snapshots.insert(new_key.clone(), snapshot);
        let metadata_key = new_key.without_peer();
        let registry =
            pick_registry_for_package(context.registries, &old_key.name.to_string(), None);
        let metadata = package_metadata(
            &replacement.manifest,
            replacement
                .resolution
                .to_lockfile_form(
                    &old_key.name.to_string(),
                    &new_key.suffix.version().to_string(),
                    LockfileFormOptions {
                        registry: &registry,
                        server_type: registry_server_type(
                            context.registry_options_by_url,
                            &registry,
                        ),
                        include_tarball_url: context.lockfile_include_tarball_url,
                    },
                )
                .ok()?,
        );
        if let Some(existing) = packages.get(&metadata_key)
            && existing != &metadata
        {
            return None;
        }
        packages.insert(metadata_key, metadata);
    }
    updated.snapshots = Some(snapshots);
    updated.packages = Some(packages);
    crate::fast_update_lockfile::prune_unreachable_packages(&mut updated);
    Some(updated)
}

fn rewrite_importer_dependencies(
    dependencies: &mut Option<ResolvedDependencyMap>,
    plan: &RewritePlan,
) {
    let Some(map) = dependencies else { return };
    map.retain(|alias, _| !should_remove_dependency(alias, None, &plan.overrides));
    for (alias, spec) in map.iter_mut() {
        let Some(old_key) = spec.version.resolved_key(alias) else { continue };
        let Some(new_key) = plan.replacements.get(&old_key) else { continue };
        // An importer is not a package, so a `parent>child` selector never
        // names it — the same exit removals take here.
        if !should_replace_dependency(alias, None, &plan.overrides) {
            continue;
        }
        spec.version = ImporterDepVersion::Regular(new_key.suffix.clone());
    }
    if map.is_empty() {
        *dependencies = None;
    }
}

fn rewrite_snapshot_dependencies(
    dependencies: &mut Option<HashMap<PkgName, SnapshotDepRef>>,
    plan: &RewritePlan,
    parent_key: Option<&PackageKey>,
) {
    let Some(map) = dependencies else { return };
    rewrite_snapshot_dependency_map(map, plan, parent_key);
    if map.is_empty() {
        *dependencies = None;
    }
}

fn rewrite_snapshot_dependency_map(
    dependencies: &mut HashMap<PkgName, SnapshotDepRef>,
    plan: &RewritePlan,
    parent_key: Option<&PackageKey>,
) {
    dependencies.retain(|alias, _| !should_remove_dependency(alias, parent_key, &plan.overrides));
    for (alias, dep_ref) in dependencies {
        let Some(old_key) = dep_ref.resolve(alias) else { continue };
        let Some(new_key) = plan.replacements.get(&old_key) else { continue };
        if !should_replace_dependency(alias, parent_key, &plan.overrides) {
            continue;
        }
        *dep_ref = SnapshotDepRef::Plain(new_key.suffix.clone());
    }
}

fn should_remove_dependency(
    alias: &PkgName,
    parent_key: Option<&PackageKey>,
    overrides: &[FastOverride],
) -> bool {
    overrides.iter().any(|override_entry| {
        override_entry.new_version.is_none()
            && override_entry.name == *alias
            && override_applies_to(override_entry, parent_key)
    })
}

/// Whether an edge on `alias` owned by `parent_key` is one a replacing
/// override moves. A `parent>child` selector names only the edges out of
/// that parent, so other dependents keep the version they have and the
/// shared prune decides whether it survives.
fn should_replace_dependency(
    alias: &PkgName,
    parent_key: Option<&PackageKey>,
    overrides: &[FastOverride],
) -> bool {
    overrides.iter().any(|override_entry| {
        override_entry.new_version.is_some()
            && override_entry.name == *alias
            && override_applies_to(override_entry, parent_key)
    })
}

/// Whether `override_entry`'s parent selector names `parent_key`. A
/// selector without a parent names every edge; one with a parent names
/// only edges out of a package it matches, which an importer never is.
fn override_applies_to(override_entry: &FastOverride, parent_key: Option<&PackageKey>) -> bool {
    let Some(parent) = override_entry.parent.as_ref() else { return true };
    let Some(parent_key) = parent_key else { return false };
    if parent_key.name.to_string() != parent.name {
        return false;
    }
    match parent.bare_specifier.as_deref() {
        None => true,
        Some(range) => parent_key
            .suffix
            .version_semver()
            .is_some_and(|version| Range::parse(range).is_ok_and(|range| range.satisfies(version))),
    }
}

fn validate_dependencies(
    manifest_dependencies: HashMap<PkgName, String>,
    locked_dependencies: Option<&HashMap<PkgName, SnapshotDepRef>>,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    packages: &HashMap<PackageKey, PackageMetadata>,
    plan: &RewritePlan,
    parent_key: &PackageKey,
) -> Option<Option<HashMap<PkgName, SnapshotDepRef>>> {
    let locked_dependencies = locked_dependencies.cloned().unwrap_or_default();
    for name in locked_dependencies.keys() {
        if !manifest_dependencies.contains_key(name) && plan.peer_names.contains(name) {
            return None;
        }
    }
    let mut rewritten = HashMap::new();
    for (name, range) in manifest_dependencies {
        if should_remove_dependency(&name, Some(parent_key), &plan.overrides) {
            continue;
        }
        let range = Range::parse(&range).ok()?;
        let dep_ref = match locked_dependencies.get(&name) {
            Some(dep_ref) => {
                let mut dep_ref = dep_ref.clone();
                if let Some(old_key) = dep_ref.resolve(&name)
                    && let Some(new_key) = plan.replacements.get(&old_key)
                {
                    dep_ref = SnapshotDepRef::Plain(new_key.suffix.clone());
                }
                dep_ref
            }
            None => find_reusable_dependency(&name, &range, snapshots, packages, plan)?,
        };
        let key = dep_ref.resolve(&name)?;
        if !range.satisfies(key.suffix.version_semver()?) {
            return None;
        }
        rewritten.insert(name, dep_ref);
    }
    Some((!rewritten.is_empty()).then_some(rewritten))
}

fn find_reusable_dependency(
    name: &PkgName,
    range: &Range,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    packages: &HashMap<PackageKey, PackageMetadata>,
    plan: &RewritePlan,
) -> Option<SnapshotDepRef> {
    if plan.peer_names.contains(name) || plan.overrides.iter().any(|entry| entry.name == *name) {
        return None;
    }
    let mut candidates = snapshots.iter().filter(|(key, snapshot)| {
        if key.name != *name
            || !key.suffix.peer().is_empty()
            || key.suffix.prefix() != Prefix::None
            || !key.suffix.version_semver().is_some_and(|version| range.satisfies(version))
            || snapshot.optional
            || snapshot.patched == Some(true)
            || snapshot.id.is_some()
            || snapshot.transitive_peer_dependencies.is_some()
        {
            return false;
        }
        packages.get(&key.without_peer()).is_some_and(|metadata| {
            metadata.peer_dependencies.is_none()
                && metadata.peer_dependencies_meta.is_none()
                && (matches!(metadata.resolution, LockfileResolution::Registry(_))
                    || matches!(
                        metadata.resolution,
                        LockfileResolution::Tarball(ref tarball)
                            if tarball.integrity.is_some() && tarball.git_hosted != Some(true),
                    ))
        })
    });
    let (key, _) = candidates.next()?;
    candidates.next().is_none().then(|| SnapshotDepRef::Plain(key.suffix.clone()))
}

fn effective_dependencies(manifest: &Value) -> Option<HashMap<PkgName, String>> {
    let optional = manifest_dependency_map(manifest, "optionalDependencies")?;
    Some(
        manifest_dependency_map(manifest, "dependencies")?
            .into_iter()
            .filter(|(name, _)| !optional.contains_key(name))
            .collect(),
    )
}

fn manifest_dependency_map(manifest: &Value, key: &str) -> Option<HashMap<PkgName, String>> {
    let Some(value) = manifest.get(key) else {
        return Some(HashMap::new());
    };
    let map = value.as_object()?;
    map.iter()
        .map(|(name, spec)| Some((PkgName::parse(name).ok()?, spec.as_str()?.to_string())))
        .collect()
}

fn package_metadata(manifest: &Value, resolution: LockfileResolution) -> PackageMetadata {
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

fn string_map(manifest: &Value, key: &str) -> Option<HashMap<String, String>> {
    let map = manifest.get(key)?.as_object()?;
    Some(
        map.iter()
            .filter_map(|(name, value)| Some((name.clone(), value.as_str()?.to_string())))
            .collect(),
    )
}

fn string_list(manifest: &Value, key: &str) -> Option<Vec<String>> {
    let values = manifest.get(key)?.as_array()?;
    let values: Vec<String> =
        values.iter().filter_map(Value::as_str).map(ToString::to_string).collect();
    (!values.is_empty()).then_some(values)
}

#[cfg(test)]
mod tests;
