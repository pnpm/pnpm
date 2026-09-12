pub(crate) use planning::{FastOverride, build_replacement_plan};

mod planning;

use planning::{ResolvedOverride, build_rewrite_plan, package_metadata, resolve_override};

use futures_util::future::join_all;
use indexmap::IndexMap;
use node_semver::Range;
use pnpm_config_parse_overrides::VersionOverride;
use pnpm_lockfile::{
    ImporterDepVersion, Lockfile, LockfileFormOptions, LockfileResolution, PackageKey,
    PackageMetadata, PkgName, Prefix, RegistryOptions, ResolvedDependencyMap, SnapshotDepRef,
    SnapshotEntry, pick_registry_for_package, registry_server_type,
};
use pnpm_resolving_deps_resolver::ManifestHook;
use pnpm_resolving_resolver_base::{ResolveOptions, Resolver};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

pub(crate) struct RewritePlan {
    pub overrides: Vec<FastOverride>,
    pub peer_names: HashSet<PkgName>,
    pub replacements: HashMap<PackageKey, PackageKey>,
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
        apply_replacement(
            context,
            plan,
            resolved,
            &mut ReplacementTarget {
                old_key,
                new_key,
                original_snapshots,
                snapshots: &mut snapshots,
                packages: &mut packages,
            },
        )?;
    }
    updated.snapshots = Some(snapshots);
    updated.packages = Some(packages);
    crate::fast_update_lockfile::prune_unreachable_packages(&mut updated);
    Some(updated)
}

/// The one replacement [`apply_replacement`] rewrites, and the maps it writes
/// the result into.
struct ReplacementTarget<'a> {
    old_key: &'a PackageKey,
    new_key: &'a PackageKey,
    original_snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
    snapshots: &'a mut HashMap<PackageKey, SnapshotEntry>,
    packages: &'a mut HashMap<PackageKey, PackageMetadata>,
}

/// Write one overridden package's snapshot and metadata under its new key.
/// `None` when the replacement disagrees with an entry already recorded
/// there, or when the new dependencies cannot be validated.
fn apply_replacement(
    context: &RewriteContext<'_>,
    plan: &RewritePlan,
    resolved: &HashMap<PkgName, ResolvedOverride>,
    target: &mut ReplacementTarget<'_>,
) -> Option<()> {
    let replacement = resolved.get(&target.old_key.name)?;
    let old_snapshot = target.original_snapshots.get(target.old_key)?;
    let dependencies = validate_dependencies(
        effective_dependencies(&replacement.manifest)?,
        old_snapshot.dependencies.as_ref(),
        target.original_snapshots,
        context.lockfile.packages.as_ref()?,
        plan,
        target.new_key,
    )?;
    let optional_dependencies = validate_dependencies(
        manifest_dependency_map(&replacement.manifest, "optionalDependencies")?,
        old_snapshot.optional_dependencies.as_ref(),
        target.original_snapshots,
        context.lockfile.packages.as_ref()?,
        plan,
        target.new_key,
    )?;
    let snapshot = SnapshotEntry { dependencies, optional_dependencies, ..old_snapshot.clone() };
    if let Some(existing) = target.snapshots.get(target.new_key)
        && existing != &snapshot
    {
        return None;
    }
    target.snapshots.insert(target.new_key.clone(), snapshot);
    apply_replacement_metadata(context, replacement, target)
}

fn apply_replacement_metadata(
    context: &RewriteContext<'_>,
    replacement: &ResolvedOverride,
    target: &mut ReplacementTarget<'_>,
) -> Option<()> {
    let metadata_key = target.new_key.without_peer();
    let registry =
        pick_registry_for_package(context.registries, &target.old_key.name.to_string(), None);
    let metadata = package_metadata(
        &replacement.manifest,
        replacement
            .resolution
            .to_lockfile_form(
                &target.old_key.name.to_string(),
                &target.new_key.suffix.version().to_string(),
                LockfileFormOptions {
                    registry: &registry,
                    server_type: registry_server_type(context.registry_options_by_url, &registry),
                    include_tarball_url: context.lockfile_include_tarball_url,
                },
            )
            .ok()?,
    );
    if let Some(existing) = target.packages.get(&metadata_key)
        && existing != &metadata
    {
        return None;
    }
    target.packages.insert(metadata_key, metadata);
    Some(())
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
        let dep_ref =
            rewritten_dep_ref(&name, &range, &locked_dependencies, snapshots, packages, plan)?;
        let key = dep_ref.resolve(&name)?;
        if !range.satisfies(key.suffix.version_semver()?) {
            return None;
        }
        rewritten.insert(name, dep_ref);
    }
    Some((!rewritten.is_empty()).then_some(rewritten))
}

/// One dependency's ref after the rewrite: the locked one moved onto its
/// replacement key, or — for a dependency the old snapshot did not record — a
/// version already in the lockfile that the range accepts.
fn rewritten_dep_ref(
    name: &PkgName,
    range: &Range,
    locked_dependencies: &HashMap<PkgName, SnapshotDepRef>,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    packages: &HashMap<PackageKey, PackageMetadata>,
    plan: &RewritePlan,
) -> Option<SnapshotDepRef> {
    let Some(dep_ref) = locked_dependencies.get(name) else {
        return find_reusable_dependency(name, range, snapshots, packages, plan);
    };
    let mut dep_ref = dep_ref.clone();
    if let Some(old_key) = dep_ref.resolve(name)
        && let Some(new_key) = plan.replacements.get(&old_key)
    {
        dep_ref = SnapshotDepRef::Plain(new_key.suffix.clone());
    }
    Some(dep_ref)
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

#[cfg(test)]
mod tests;
