use std::sync::Arc;

use node_semver::Version;
use pnpm_config::TrustPolicy;
use pnpm_lockfile::{LockfileResolution, PkgName, PkgNameVer};
use pnpm_resolving_resolver_base::{
    CurrentPkg, ResolveError, ResolveOptions, ResolveResult, UpdateBehavior, WantedDependency,
    WorkspacePackages,
};
use pnpm_store_dir::{SharedReadonlyStoreIndex, store_index_key};

use super::{
    NPM_REGISTRY_RESOLVED_VIA, release_policy::detect_min_release_age_violation,
    workspace_pick::prefer_workspace_pick,
};
use crate::pick_package_from_meta::{
    RegistryPackageSpec, RegistryPackageSpecType, semver_range::semver_satisfies_loose,
};

pub(crate) async fn fast_path_pick(
    store_index: Option<&SharedReadonlyStoreIndex>,
    wanted_dependency: &WantedDependency,
    opts: &ResolveOptions,
    spec: &RegistryPackageSpec,
    workspace_packages_active: Option<&Arc<WorkspacePackages>>,
) -> Result<Option<ResolveResult>, ResolveError> {
    if let Some(result) =
        peek_manifest_from_store(store_index, wanted_dependency, opts, spec).await?
    {
        return Ok(Some(result));
    }
    Ok(prefer_workspace_pick(workspace_packages_active, spec, wanted_dependency, opts))
}

pub(crate) async fn peek_manifest_from_store(
    store_index: Option<&SharedReadonlyStoreIndex>,
    wanted_dependency: &WantedDependency,
    opts: &ResolveOptions,
    spec: &RegistryPackageSpec,
) -> Result<Option<ResolveResult>, ResolveError> {
    let Some(store_index) = store_index else {
        return Ok(None);
    };
    let Some(current_pkg) = is_eligible_for_store_peek(opts, spec) else {
        return Ok(None);
    };
    let Some(manifest) = fetch_cached_manifest(store_index, current_pkg).await else {
        return Ok(None);
    };
    build_peek_result(wanted_dependency, opts, spec, current_pkg, manifest)
}

fn is_eligible_for_store_peek<'a>(
    opts: &'a ResolveOptions,
    spec: &RegistryPackageSpec,
) -> Option<&'a CurrentPkg> {
    let current_pkg = opts.refresh.current_pkg.as_ref()?;
    if opts.refresh.update != UpdateBehavior::Off {
        return None;
    }
    if opts.refresh.update_checksums {
        return None;
    }
    if spec.revision.is_some() {
        return None;
    }
    if opts.policy.published_by.is_some() && current_pkg.published_at.is_none() {
        return None;
    }
    if opts.policy.trust_policy == Some(TrustPolicy::NoDowngrade) {
        return None;
    }
    if opts.policy.package_version_guard.is_some() {
        return None;
    }
    if let LockfileResolution::Tarball(tarball) = &current_pkg.resolution
        && tarball.is_git_hosted()
    {
        return None;
    }
    current_pkg.resolution.checkable_integrity()?;
    Some(current_pkg)
}

async fn fetch_cached_manifest(
    store_index: &SharedReadonlyStoreIndex,
    current_pkg: &CurrentPkg,
) -> Option<serde_json::Value> {
    let integrity = current_pkg.resolution.checkable_integrity()?;
    let key = store_index_key(&integrity.to_string(), current_pkg.id.as_str());
    let index = Arc::clone(store_index);
    let entry = tokio::task::spawn_blocking(move || {
        let guard = index.lock().ok()?;
        guard.get(&key).ok().flatten()
    })
    .await
    .ok()
    .flatten()?;
    entry.manifest
}

fn build_peek_result(
    wanted_dependency: &WantedDependency,
    opts: &ResolveOptions,
    spec: &RegistryPackageSpec,
    current_pkg: &CurrentPkg,
    manifest: serde_json::Value,
) -> Result<Option<ResolveResult>, ResolveError> {
    let (Some(name), Some(version)) = (
        manifest.get("name").and_then(serde_json::Value::as_str),
        manifest.get("version").and_then(serde_json::Value::as_str),
    ) else {
        return Ok(None);
    };
    if name != spec.name || !matches_current_pkg(current_pkg, name, version) {
        return Ok(None);
    }
    if !version_satisfies_spec(version, spec) {
        return Ok(None);
    }
    let (Ok(pkg_name), Ok(semver_ver)) = (PkgName::parse(name), Version::parse(version)) else {
        return Ok(None);
    };
    let policy_violation = detect_min_release_age_violation(
        &pkg_name,
        version,
        current_pkg.published_at.as_deref(),
        &current_pkg.resolution,
        opts.policy.published_by,
        opts.policy.published_by_exclude.as_ref(),
    );
    let name_ver = PkgNameVer::new(pkg_name, semver_ver);
    Ok(Some(ResolveResult {
        id: current_pkg.id.clone(),
        policy_violation,
        resolution: current_pkg.resolution.clone(),
        resolved_via: NPM_REGISTRY_RESOLVED_VIA.to_string(),
        normalized_bare_specifier: spec.normalized_bare_specifier.clone(),
        alias: wanted_dependency.alias.clone(),
        package: pnpm_resolving_resolver_base::ResolvedPackageInfo {
            name_ver: Some(name_ver),
            latest: None,
            published_at: current_pkg.published_at.clone(),
            manifest: Some(Arc::new(manifest)),
            non_deprecated_alternative: None,
        },
    }))
}

fn matches_current_pkg(current_pkg: &CurrentPkg, name: &str, version: &str) -> bool {
    format!("{name}@{version}") == current_pkg.id.as_str()
        || (current_pkg.name.as_deref() == Some(name)
            && current_pkg.version.as_deref() == Some(version))
}

fn version_satisfies_spec(version: &str, spec: &RegistryPackageSpec) -> bool {
    match spec.spec_type {
        RegistryPackageSpecType::Range => {
            spec.fetch_spec == "*" || semver_satisfies_loose(version, &spec.fetch_spec)
        }
        RegistryPackageSpecType::Version => version == spec.fetch_spec,
        RegistryPackageSpecType::Tag => true,
    }
}
