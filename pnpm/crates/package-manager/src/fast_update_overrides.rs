use futures_util::future::join_all;
use indexmap::IndexMap;
use node_semver::{Range, Version};
use pacquet_config_parse_overrides::VersionOverride;
use pacquet_lockfile::{
    BundledDependencies, ImporterDepVersion, Lockfile, LockfileResolution, PackageKey,
    PackageMetadata, PkgName, PkgNameVerPeer, PkgVerPeer, Prefix, ResolvedDependencyMap,
    SnapshotDepRef, SnapshotEntry, StringOrList, pick_registry_for_package,
};
use pacquet_resolving_deps_resolver::ManifestHook;
use pacquet_resolving_resolver_base::{ResolveOptions, ResolveResult, Resolver, WantedDependency};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

struct FastOverride {
    name: PkgName,
    new_version: Version,
    old_version: Option<Version>,
}

struct RewritePlan {
    overrides: Vec<FastOverride>,
    replacements: HashMap<PackageKey, PackageKey>,
}

struct ResolvedOverride {
    manifest: Arc<Value>,
    resolution: LockfileResolution,
}

pub(crate) struct FastOverrideOptions<'a> {
    pub lockfile: &'a Lockfile,
    pub parsed_overrides: &'a [VersionOverride],
    pub resolved_overrides: &'a IndexMap<String, String>,
    pub resolver: &'a dyn Resolver,
    pub resolve_options: &'a ResolveOptions,
    pub manifest_hook: Option<&'a ManifestHook>,
    pub registries: &'a HashMap<String, String>,
    pub lockfile_include_tarball_url: bool,
}

pub(crate) async fn try_fast_update_overrides(opts: FastOverrideOptions<'_>) -> Option<Lockfile> {
    let plan = build_rewrite_plan(opts.lockfile, opts.parsed_overrides, opts.resolved_overrides)?;
    let resolutions = join_all(
        plan.overrides
            .iter()
            .filter(|override_entry| {
                plan.replacements
                    .iter()
                    .any(|(old, new)| old != new && old.name == override_entry.name)
            })
            .map(|override_entry| resolve_override(&opts, override_entry)),
    )
    .await;
    let resolved: HashMap<_, _> =
        resolutions.into_iter().collect::<Option<Vec<_>>>()?.into_iter().collect();
    rewrite_lockfile(&opts, &plan, &resolved)
}

async fn resolve_override(
    opts: &FastOverrideOptions<'_>,
    override_entry: &FastOverride,
) -> Option<(PkgName, ResolvedOverride)> {
    let name = override_entry.name.to_string();
    let version = override_entry.new_version.to_string();
    let wanted = WantedDependency {
        alias: Some(name.clone()),
        bare_specifier: Some(version.clone()),
        ..WantedDependency::default()
    };
    let result = opts.resolver.resolve(&wanted, opts.resolve_options).await.ok()??;
    let manifest = result.manifest.as_ref().map(Arc::clone)?;
    let manifest = match opts.manifest_hook {
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
        if parsed.parent_pkg.is_some()
            || parsed.target_pkg.bare_specifier.is_some()
            || parsed.converge
            || parsed_overrides.iter().any(|candidate| {
                candidate.selector != *selector
                    && candidate.target_pkg.name == parsed.target_pkg.name
            })
        {
            return None;
        }
        let name = PkgName::parse(&parsed.target_pkg.name).ok()?;
        if overrides.iter().any(|entry: &FastOverride| entry.name == name) {
            return None;
        }
        overrides.push(FastOverride {
            name,
            new_version: Version::parse(new_value).ok()?,
            old_version: match old_value {
                Some(value) => Some(Version::parse(value).ok()?),
                None => None,
            },
        });
    }
    if overrides.is_empty() {
        return None;
    }

    let by_name: HashMap<&PkgName, &FastOverride> =
        overrides.iter().map(|entry| (&entry.name, entry)).collect();
    let mut replacements = HashMap::new();
    for (alias, key) in all_dependency_keys(lockfile) {
        let Some(override_entry) = by_name.get(alias) else { continue };
        let key = key?;
        if key.name != *alias
            || !key.suffix.peer().is_empty()
            || key.suffix.prefix() != Prefix::None
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
                    if tarball.integrity.is_some() && tarball.git_hosted != Some(true)
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
        let new_suffix: PkgVerPeer = override_entry.new_version.to_string().parse().ok()?;
        replacements.insert(key, PkgNameVerPeer::new(alias.clone(), new_suffix));
    }
    for (alias, key) in all_dependency_keys(lockfile) {
        if key.is_some_and(|key| replacements.contains_key(&key)) && !by_name.contains_key(alias) {
            return None;
        }
    }
    Some(RewritePlan { overrides, replacements })
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
            .and_then(Value::as_object)
            .is_none_or(serde_json::Map::is_empty)
        && manifest
            .get("peerDependenciesMeta")
            .and_then(Value::as_object)
            .is_none_or(serde_json::Map::is_empty)
        && manifest.get("deprecated").is_none()
        && manifest.get("bundledDependencies").is_none()
        && manifest.get("bundleDependencies").is_none()
        && manifest
            .get("engines")
            .and_then(Value::as_object)
            .is_none_or(|engines| !engines.contains_key("runtime"))
        && matches!(
            result.resolution,
            LockfileResolution::Tarball(ref tarball)
                if tarball.integrity.is_some() && tarball.git_hosted != Some(true)
        )
}

fn rewrite_lockfile(
    opts: &FastOverrideOptions<'_>,
    plan: &RewritePlan,
    resolved: &HashMap<PkgName, ResolvedOverride>,
) -> Option<Lockfile> {
    let mut updated = opts.lockfile.clone();
    for importer in updated.importers.values_mut() {
        rewrite_importer_dependencies(importer.dependencies.as_mut(), plan);
        rewrite_importer_dependencies(importer.dev_dependencies.as_mut(), plan);
        rewrite_importer_dependencies(importer.optional_dependencies.as_mut(), plan);
    }
    let original_snapshots = opts.lockfile.snapshots.as_ref()?;
    let mut snapshots = original_snapshots.clone();
    let mut packages = opts.lockfile.packages.clone()?;
    for snapshot in snapshots.values_mut() {
        rewrite_snapshot_dependencies(snapshot.dependencies.as_mut(), plan);
        rewrite_snapshot_dependencies(snapshot.optional_dependencies.as_mut(), plan);
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
            plan,
        )?;
        let optional_dependencies = validate_dependencies(
            manifest_dependency_map(&replacement.manifest, "optionalDependencies")?,
            old_snapshot.optional_dependencies.as_ref(),
            plan,
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
        let metadata = package_metadata(
            &replacement.manifest,
            replacement.resolution.to_lockfile_form(
                &old_key.name.to_string(),
                &new_key.suffix.version().to_string(),
                &pick_registry_for_package(opts.registries, &old_key.name.to_string(), None),
                opts.lockfile_include_tarball_url,
            ),
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
    updated.overrides = Some(opts.resolved_overrides.clone());
    Some(updated)
}

fn rewrite_importer_dependencies(
    dependencies: Option<&mut ResolvedDependencyMap>,
    plan: &RewritePlan,
) {
    let Some(dependencies) = dependencies else { return };
    for (alias, spec) in dependencies {
        let Some(old_key) = spec.version.resolved_key(alias) else { continue };
        let Some(new_key) = plan.replacements.get(&old_key) else { continue };
        spec.version = ImporterDepVersion::Regular(new_key.suffix.clone());
    }
}

fn rewrite_snapshot_dependencies(
    dependencies: Option<&mut HashMap<PkgName, SnapshotDepRef>>,
    plan: &RewritePlan,
) {
    let Some(dependencies) = dependencies else { return };
    for (alias, dep_ref) in dependencies {
        let Some(old_key) = dep_ref.resolve(alias) else { continue };
        let Some(new_key) = plan.replacements.get(&old_key) else { continue };
        *dep_ref = SnapshotDepRef::Plain(new_key.suffix.clone());
    }
}

fn validate_dependencies(
    manifest_dependencies: HashMap<PkgName, String>,
    locked_dependencies: Option<&HashMap<PkgName, SnapshotDepRef>>,
    plan: &RewritePlan,
) -> Option<Option<HashMap<PkgName, SnapshotDepRef>>> {
    let locked_dependencies = locked_dependencies.cloned().unwrap_or_default();
    if manifest_dependencies.len() != locked_dependencies.len()
        || manifest_dependencies.keys().any(|name| !locked_dependencies.contains_key(name))
    {
        return None;
    }
    let mut rewritten = locked_dependencies;
    rewrite_snapshot_dependencies(Some(&mut rewritten), plan);
    for (name, range) in manifest_dependencies {
        let range = Range::parse(&range).ok()?;
        let key = rewritten.get(&name)?.resolve(&name)?;
        if !range.satisfies(key.suffix.version_semver()?) {
            return None;
        }
    }
    Some((!rewritten.is_empty()).then_some(rewritten))
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
        has_bin: manifest_has_bin(manifest).then_some(true),
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

fn manifest_has_bin(manifest: &Value) -> bool {
    manifest.get("bin").is_some_and(|bin| match bin {
        Value::String(path) => !path.is_empty(),
        Value::Object(entries) => !entries.is_empty(),
        _ => false,
    })
}

#[cfg(test)]
mod tests;
