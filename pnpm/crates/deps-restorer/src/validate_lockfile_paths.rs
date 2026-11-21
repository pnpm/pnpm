//! Reject a lockfile whose virtual-store slots would escape the store
//! root once materialized.
//!
//! Dependency-name validation lives in the lockfile verifier
//! ([`pnpm_lockfile_verification::verify_lockfile_dependency_names`]),
//! which the install runs unconditionally. This module covers the one
//! escape that name validation alone can't: the global-virtual-store
//! slot path inserts the package name and the version-derived segment as
//! raw path components (unlike the legacy flat name, which `/`-escapes),
//! so a traversal in the version escapes the store even when the name is
//! valid. The check needs the install-time [`VirtualStoreLayout`], which
//! is why it lives here rather than in the verifier crate.

use crate::VirtualStoreLayout;
use futures_util::{
    StreamExt,
    stream,
};
use pnpm_config::Config;
use pnpm_hooks::{
    HookContext,
    HookError,
    PnpmfileHooks,
};
use pnpm_lockfile::{
    Lockfile,
    PackageKey,
    SnapshotEntry,
};
use pnpm_lockfile_verification::{
    LocalResolutionPolicy,
    LocalResolutionProbeParent,
    PristineDeclaredSpecs,
    VerifyError,
    local_resolution_candidate_parents_for_policy,
    verify_lockfile_local_resolutions,
};
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::PackageManifest;
use pnpm_store_dir::StoreIndex;
use pristine_manifests::{
    load_pristine_parent_manifest,
    record_pristine_specs,
};
use serde_json::{
    Map,
    Value,
    json,
};
use std::{
    collections::{
        BTreeMap,
        BTreeSet,
        HashMap,
    },
    path::{
        Path,
        PathBuf,
    },
    sync::Arc,
};

mod pristine_manifests;

const PROBE_SENTINEL_SPEC: &str = "0.0.0";
const PROBE_ALT_SENTINEL_SPEC: &str = "0.0.0-pnpm-probe";
pub(super) const PROBE_FIELDS: [&str; 3] =
    ["dependencies", "optionalDependencies", "peerDependencies"];

type CatalogMap = BTreeMap<String, BTreeMap<String, String>>;

/// Runtime drivers consulted when enriching a [`LocalResolutionPolicy`] with
/// pristine candidate manifests and `.pnpmfile.cjs` hook rewrites.
#[derive(Default, Clone, Copy)]
pub struct LocalPolicyEnrichDrivers<'a> {
    pub config: Option<&'a Config>,
    pub http_client: Option<&'a ThrottledClient>,
    pub hook: Option<&'a dyn PnpmfileHooks>,
}

/// Build the [`LocalResolutionPolicy`] reflecting the active workspace
/// `overrides`, `packageExtensions`, and `pnpmfile` hooks.
#[must_use]
pub fn local_resolution_policy_for_config(
    config: &Config,
    has_pnpmfile_hook: bool,
) -> LocalResolutionPolicy {
    let containment_root = config.workspace_dir.as_deref().unwrap_or(&config.dir);
    let lockfile_dir = config.lockfile_dir_for(&config.dir);
    local_resolution_policy_for_workspace(config, containment_root, lockfile_dir, has_pnpmfile_hook)
}

/// Like [`local_resolution_policy_for_config`], anchored on explicit discovered
/// `containment_root` and `lockfile_dir` paths.
#[must_use]
pub fn local_resolution_policy_for_workspace(
    config: &Config,
    containment_root: &Path,
    lockfile_dir: &Path,
    has_pnpmfile_hook: bool,
) -> LocalResolutionPolicy {
    local_resolution_policy_for_workspace_with_catalogs(
        config,
        None,
        (None, containment_root, lockfile_dir),
        has_pnpmfile_hook,
    )
}

/// Like [`local_resolution_policy_for_workspace`], using `catalogs_override`
/// when present and rebasing overrides from `overrides_root_dir` when set.
#[must_use]
pub fn local_resolution_policy_for_workspace_with_catalogs(
    config: &Config,
    catalogs_override: Option<&CatalogMap>,
    (overrides_root_dir, containment_root, lockfile_dir): (Option<&Path>, &Path, &Path),
    has_pnpmfile_hook: bool,
) -> LocalResolutionPolicy {
    let lockfile_norm = pnpm_fs::lexical_normalize(lockfile_dir);
    let root_norm = pnpm_fs::lexical_normalize(containment_root);
    let overrides_norm = overrides_root_dir.map(pnpm_fs::lexical_normalize);
    let workspace_depth = compute_workspace_depth(&root_norm, &lockfile_norm);
    let allow_deep_links =
        matches!(config.link_workspace_packages, pnpm_config::LinkWorkspacePackages::Deep);
    let overrides = collect_config_override_specs(config);
    let extensions = collect_config_extension_specs(config);
    let effective_catalogs = catalogs_override.or(config.catalogs.as_ref());
    let mut policy = LocalResolutionPolicy::from_scoped_inputs_with_roots(
        overrides,
        extensions,
        has_pnpmfile_hook,
        (overrides_norm.as_deref(), Some(&lockfile_norm)),
        |catalog_name, pkg_name| lookup_catalog_entry(effective_catalogs, catalog_name, pkg_name),
    )
    .with_workspace_scope(allow_deep_links, workspace_depth)
    .with_workspace_dir(&root_norm);
    if let Some(cats) = effective_catalogs {
        policy = policy.with_catalogs(cats.clone());
    }
    policy
}

/// Populate `live_workspace_dirs` on `policy` from the install's live project
/// manifests and their `publishConfig.directory` entries.
#[must_use]
pub fn attach_live_project_manifests<'m>(
    policy: LocalResolutionPolicy,
    manifests: impl IntoIterator<Item = &'m (PathBuf, &'m PackageManifest)>,
) -> LocalResolutionPolicy {
    let mut dirs = BTreeSet::new();
    for (project_dir, manifest) in manifests {
        let norm_dir = pnpm_fs::lexical_normalize(project_dir);
        if let Some(rel) = policy.relative_path_from_lockfile_dir(&norm_dir) {
            dirs.insert(rel);
        }
        if let Some(pub_dir) = publish_directory_of(manifest) {
            let norm_pub = pnpm_fs::lexical_normalize(&norm_dir.join(pub_dir));
            if let Some(rel) = policy.relative_path_from_lockfile_dir(&norm_pub) {
                dirs.insert(rel);
            }
        }
    }
    if dirs.is_empty() { policy } else { policy.with_live_workspace_dirs(dirs) }
}

/// Populate `selected_importer_ids` on `policy` from the project manifests
/// actually installed in this run.
#[must_use]
pub fn attach_selected_project_manifests<'m>(
    policy: LocalResolutionPolicy,
    manifests: impl IntoIterator<Item = &'m (PathBuf, &'m PackageManifest)>,
) -> LocalResolutionPolicy {
    let mut ids = BTreeSet::new();
    for (project_dir, _) in manifests {
        let norm_dir = pnpm_fs::lexical_normalize(project_dir);
        if let Some(rel) = policy.relative_path_from_lockfile_dir(&norm_dir) {
            ids.insert(rel);
        }
    }
    policy.with_selected_importer_ids(ids)
}

/// Enrich `policy` with local edges rewritten by an active `pnpmfile` hook.
///
/// # Errors
///
/// See [`enrich_local_policy_for_install`].
pub async fn enrich_local_policy_with_pnpmfile_hook(
    policy: LocalResolutionPolicy,
    lockfile: &Lockfile,
    hook: Option<&dyn PnpmfileHooks>,
) -> Result<LocalResolutionPolicy, HookError> {
    enrich_local_policy_for_install(
        policy,
        lockfile,
        LocalPolicyEnrichDrivers { config: None, http_client: None, hook },
    )
    .await
}

/// Enrich `policy` with pristine candidate manifest specs (for child-ranged
/// overrides) and `.pnpmfile.cjs` hook rewrites when `lockfile` contains local
/// non-importer edges that would otherwise fail.
///
/// # Errors
///
/// The `readPackage` hook's error when it fails on a parent's real manifest.
/// An error on a placeholder manifest only withholds that parent's rewrites.
pub async fn enrich_local_policy_for_install(
    mut policy: LocalResolutionPolicy,
    lockfile: &Lockfile,
    drivers: LocalPolicyEnrichDrivers<'_>,
) -> Result<LocalResolutionPolicy, HookError> {
    if !matches!(
        verify_lockfile_local_resolutions(lockfile, Some(policy.clone())),
        Err(VerifyError::LocalSubdepNotAllowed { .. }),
    ) {
        return Ok(policy);
    }
    if let Some(hook) = drivers.hook
        && hook.has_after_all_resolved().await.unwrap_or(false)
    {
        policy = policy.with_after_all_resolved_hook(true);
    }
    let modes = ProbeModes {
        pristine_specs: policy.has_child_ranged_edges(),
        read_package: should_probe_read_package(lockfile, drivers.hook).await,
    };
    if !modes.pristine_specs && !modes.read_package {
        return Ok(policy);
    }
    let candidates = local_resolution_candidate_parents_for_policy(lockfile, &policy);
    if candidates.is_empty() {
        return Ok(policy);
    }
    probe_candidates_for_policy(policy, candidates, drivers, modes).await
}

/// What [`probe_candidates_for_policy`] collects from each candidate parent.
#[derive(Clone, Copy)]
struct ProbeModes {
    /// Record the parent's declared child specs, for child-ranged overrides.
    pristine_specs: bool,
    /// Replay the `readPackage` hook to observe its rewrites.
    read_package: bool,
}

/// Candidate parent manifests read or fetched at once while enriching the
/// policy, so a lockfile with many candidate parents does not wait on one
/// registry round trip after another.
const PRISTINE_MANIFEST_CONCURRENCY: usize = 16;

async fn probe_candidates_for_policy(
    policy: LocalResolutionPolicy,
    candidates: Vec<LocalResolutionProbeParent>,
    drivers: LocalPolicyEnrichDrivers<'_>,
    modes: ProbeModes,
) -> Result<LocalResolutionPolicy, HookError> {
    let store_index = match drivers.config {
        Some(cfg) => StoreIndex::open_shared(&cfg.store_dir, cfg.frozen_store).await,
        None => None,
    };
    let store_index = store_index.as_ref();
    let probed: Vec<(LocalResolutionProbeParent, Option<Value>)> = stream::iter(candidates)
        .map(|parent| async move {
            let pristine = load_pristine_parent_manifest(
                &parent,
                drivers.config,
                store_index,
                drivers.http_client,
            )
            .await;
            (parent, pristine)
        })
        .buffered(PRISTINE_MANIFEST_CONCURRENCY)
        .collect()
        .await;
    let mut pristine_specs = PristineDeclaredSpecs::new();
    let mut rewrites = Vec::new();
    for (parent, pristine) in &probed {
        if modes.pristine_specs
            && let Some(manifest) = pristine
        {
            record_pristine_specs(parent, manifest, &mut pristine_specs);
        }
        if modes.read_package
            && let Some(hook) = drivers.hook
        {
            collect_probed_parent_rewrites(hook, parent, pristine.as_ref(), &mut rewrites).await?;
        }
    }
    Ok(policy
        .with_pristine_declared_specs(pristine_specs)
        .with_probed_source_hook_rewrites(rewrites))
}

async fn should_probe_read_package(lockfile: &Lockfile, hook: Option<&dyn PnpmfileHooks>) -> bool {
    let Some(hook) = hook else {
        return false;
    };
    let lockfile_has_hook = lockfile.pnpmfile_checksum.is_some()
        || lockfile.untracked_pnpmfile_read_package_hook().unwrap_or(false);
    lockfile_has_hook && hook.has_read_package().await.unwrap_or(false)
}

async fn collect_probed_parent_rewrites(
    hook: &dyn PnpmfileHooks,
    parent: &LocalResolutionProbeParent,
    pristine: Option<&Value>,
    out: &mut Vec<(PackageKey, String, Option<String>, String, String)>,
) -> Result<(), HookError> {
    // Fresh resolution rewrites in-archive `file:` dependencies before the
    // hook sees the manifest, so the replay and its rewrite baseline do too.
    let prepared = pristine
        .cloned()
        .map(|mut manifest| {
            pnpm_lockfile::link_file_deps_inside_manifest(&mut manifest);
            manifest
        });
    let pristine = prepared.as_ref();
    let probe =
        pristine.cloned().unwrap_or_else(|| build_probe_manifest(parent, PROBE_SENTINEL_SPEC));
    let hooked =
        match hook.read_package(probe, HookContext { log: Arc::new(|_| {}), dir: None }).await {
            Ok(hooked) => hooked,
            // The hook may not handle a placeholder manifest. Only an error
            // on the real manifest is one the install would also hit.
            Err(_) if pristine.is_none() => return Ok(()),
            Err(error) => return Err(error),
        };
    let alt_hooked = if pristine.is_none() {
        hook.read_package(
            build_probe_manifest(parent, PROBE_ALT_SENTINEL_SPEC),
            HookContext { log: Arc::new(|_| {}), dir: None },
        )
        .await
        .ok()
    } else {
        None
    };
    record_verified_hook_rewrites(parent, pristine, &hooked, alt_hooked.as_deref(), out);
    Ok(())
}

fn record_verified_hook_rewrites(
    parent: &LocalResolutionProbeParent,
    pristine: Option<&Value>,
    hooked: &Value,
    alt_hooked: Option<&Value>,
    out: &mut Vec<(PackageKey, String, Option<String>, String, String)>,
) {
    for field in PROBE_FIELDS {
        let Some(entries) = hooked.get(field).and_then(Value::as_object) else {
            continue;
        };
        for (alias, val) in entries {
            let Some(post_spec) = val.as_str() else {
                continue;
            };
            if is_hook_rewritten_spec(pristine, alt_hooked, field, alias, post_spec) {
                out.push((
                    parent.pkg_key.clone(),
                    parent.name.clone(),
                    parent.version.clone(),
                    alias.clone(),
                    post_spec.to_string(),
                ));
            }
        }
    }
}

fn is_hook_rewritten_spec(
    pristine: Option<&Value>,
    alt_hooked: Option<&Value>,
    field: &str,
    alias: &str,
    post_spec: &str,
) -> bool {
    if let Some(orig) = pristine {
        return lookup_field_spec(orig, field, alias) != Some(post_spec);
    }
    post_spec != PROBE_SENTINEL_SPEC
        && post_spec != PROBE_ALT_SENTINEL_SPEC
        && alt_hooked
            .and_then(|alt| lookup_field_spec(alt, field, alias))
            .is_some_and(|alt_spec| alt_spec == post_spec)
}

fn lookup_field_spec<'a>(manifest: &'a Value, field: &str, alias: &str) -> Option<&'a str> {
    manifest
        .get(field)
        .and_then(Value::as_object)
        .and_then(|map| map.get(alias))
        .and_then(Value::as_str)
}

fn build_probe_manifest(parent: &LocalResolutionProbeParent, sentinel: &str) -> Value {
    let to_map = |aliases: &[String]| -> Value {
        aliases
            .iter()
            .map(|alias| (alias.clone(), Value::String(sentinel.to_string())))
            .collect::<Map<String, Value>>()
            .into()
    };
    json!({
        "name": parent.name,
        "version": parent.version.as_deref().unwrap_or(sentinel),
        "dependencies": to_map(&parent.dep_aliases),
        "optionalDependencies": to_map(&parent.optional_aliases),
        "peerDependencies": to_map(&parent.peer_aliases),
        "devDependencies": {},
    })
}

fn publish_directory_of(manifest: &PackageManifest) -> Option<&str> {
    manifest
        .value()
        .get("publishConfig")
        .and_then(|cfg| cfg.get("directory"))
        .and_then(Value::as_str)
}

fn compute_workspace_depth(containment_root: &Path, lockfile_dir: &Path) -> Option<usize> {
    lockfile_dir
        .strip_prefix(containment_root)
        .ok()
        .map(Path::components)
        .map(Iterator::count)
}

fn collect_config_override_specs(config: &Config) -> Vec<(&str, &str)> {
    config.overrides
        .iter()
        .flat_map(|map| map.iter())
        .map(|(sel, spec)| (sel.as_str(), spec.as_str()))
        .collect()
}

fn collect_config_extension_specs(config: &Config) -> Vec<(&str, &str, &str)> {
    config.package_extensions
        .iter()
        .flat_map(|map| map.iter())
        .flat_map(|(parent_sel, ext)| {
            [
                ext.dependencies.as_ref(),
                ext.optional_dependencies.as_ref(),
                ext.peer_dependencies.as_ref(),
            ]
            .into_iter()
            .flatten()
            .flat_map(|deps| deps.iter())
            .map(move |(alias, spec)| (parent_sel.as_str(), alias.as_str(), spec.as_str()))
        })
        .collect()
}

fn lookup_catalog_entry<'c>(
    catalogs: Option<&'c CatalogMap>,
    catalog_name: &str,
    pkg_name: &str,
) -> Option<&'c str> {
    catalogs?
        .get(catalog_name)?
        .get(pkg_name)
        .map(String::as_str)
}

/// Reject the install when any snapshot's computed virtual-store slot
/// resolves outside the store root or its package directory. The whole
/// `snapshots` map is scanned (not just the survivors of the warm-install
/// skip filter) so a poisoned snapshot that would be skipped as unchanged
/// is still rejected before any directory is created.
///
/// Surfaces [`VerifyError::InvalidDependencyAlias`]
/// (`ERR_PNPM_INVALID_DEPENDENCY_NAME`), listing every offending snapshot key.
pub fn validate_virtual_store_slot_containment(
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    layout: &VirtualStoreLayout,
) -> Result<(), VerifyError> {
    let Some(snapshots) = snapshots else {
        return Ok(());
    };
    let mut escaped: BTreeSet<String> = BTreeSet::new();
    for key in snapshots.keys() {
        if !layout.is_slot_contained(key) {
            escaped.insert(key.to_string());
        }
    }
    if escaped.is_empty() {
        return Ok(());
    }
    let escaped: Vec<String> = escaped.into_iter().collect();
    Err(VerifyError::invalid_dependency_aliases(&escaped))
}

#[cfg(test)]
mod tests;
