use std::fs;

use node_semver::Version;
use pnpm_config::Config;
use pnpm_lockfile::{
    LockfileResolution,
    VersionPart,
};
use pnpm_lockfile_verification::{
    LocalDepField,
    LocalResolutionProbeParent,
    PristineDeclaredSpecs,
};
use pnpm_network::ThrottledClient;
use pnpm_resolving_npm_resolver::{
    FetchFullMetadataCachedOptions,
    MetadataHttpClient,
    fetch_full_metadata_cached,
    pick_registry_for_package,
};
use pnpm_store_dir::{
    PackageFilesIndex,
    SharedReadonlyStoreIndex,
};
use serde_json::Value;

use crate::store_index_key_for_resolution;

pub(super) const PROBE_FIELD_PAIRS: [(LocalDepField, &str); 3] = [
    (LocalDepField::Dependencies, "dependencies"),
    (LocalDepField::OptionalDependencies, "optionalDependencies"),
    (LocalDepField::PeerDependencies, "peerDependencies"),
];

pub(super) async fn load_pristine_parent_manifest(
    parent: &LocalResolutionProbeParent,
    config: Option<&Config>,
    store_index: Option<&SharedReadonlyStoreIndex>,
    http_client: Option<&ThrottledClient>,
) -> Option<Value> {
    let config = config?;
    if let Some(manifest) = read_manifest_from_cas(parent, config, store_index) {
        return Some(manifest);
    }
    if !is_registry_origin_parent(parent) {
        return None;
    }
    fetch_manifest_from_registry(parent, config, http_client).await
}

fn is_registry_origin_parent(parent: &LocalResolutionProbeParent) -> bool {
    matches!(
        parent.pkg_key.suffix.version(),
        VersionPart::Semver(_) | VersionPart::RegistryQualified { .. },
    ) && matches!(parent.resolution, Some(LockfileResolution::Registry(_)))
}

pub(super) fn record_pristine_specs(
    parent: &LocalResolutionProbeParent,
    manifest: &Value,
    out: &mut PristineDeclaredSpecs,
) {
    let base_key = parent.pkg_key.without_peer();
    for (field, field_name) in PROBE_FIELD_PAIRS {
        let Some(entries) = manifest.get(field_name).and_then(Value::as_object) else {
            continue;
        };
        for (alias, val) in entries {
            if let Some(spec) = val.as_str() {
                out.insert((base_key.clone(), field, alias.clone()), spec.to_string());
            }
        }
    }
}

fn read_manifest_from_cas(
    parent: &LocalResolutionProbeParent,
    config: &Config,
    store_index: Option<&SharedReadonlyStoreIndex>,
) -> Option<Value> {
    let index_arc = store_index?;
    let resolution = parent.resolution.as_ref()?;
    let pkg_id = parent.pkg_key.pkg_id();
    let row = lookup_cas_index_row(index_arc, resolution, &pkg_id, !config.ignore_scripts)?;
    let candidate = if let Some(info) = row.files.get("package.json")
        && let Some(cas_path) = config.store_dir.cas_file_path_by_mode(&info.digest, info.mode)
        && let Ok(bytes) = fs::read(cas_path)
        && let Ok(parsed) = serde_json::from_slice::<Value>(&bytes)
    {
        Some(parsed)
    } else {
        row.manifest
    }?;
    manifest_matches_parent(parent, &candidate).then_some(candidate)
}

fn manifest_matches_parent(parent: &LocalResolutionProbeParent, manifest: &Value) -> bool {
    if manifest.get("name").and_then(Value::as_str) != Some(parent.name.as_str()) {
        return false;
    }
    if is_registry_origin_parent(parent)
        && let Some(ver) = parent.version.as_deref()
        && Version::parse(ver).is_ok()
        && manifest.get("version").and_then(Value::as_str) != Some(ver)
    {
        return false;
    }
    true
}

fn lookup_cas_index_row(
    index_arc: &SharedReadonlyStoreIndex,
    resolution: &LockfileResolution,
    pkg_id: &str,
    built_primary: bool,
) -> Option<PackageFilesIndex> {
    let guard = index_arc.lock().ok()?;
    for built in [built_primary, !built_primary] {
        let Some(key) = store_index_key_for_resolution(resolution, pkg_id, built) else {
            continue;
        };
        if let Ok(Some(row)) = guard.get(&key) {
            return Some(row);
        }
    }
    None
}

async fn fetch_manifest_from_registry(
    parent: &LocalResolutionProbeParent,
    config: &Config,
    http_client: Option<&ThrottledClient>,
) -> Option<Value> {
    let http_client = http_client?;
    let version = parent.version
        .as_deref()
        .filter(|ver| Version::parse(ver).is_ok())?;
    let registries: std::collections::HashMap<String, String> = config
        .resolved_registries()
        .into_iter()
        .collect();
    let registry_name = parent.pkg_key.suffix.registry_qualified().map(|(name, _)| name);
    let registry = pick_registry_for_package(&registries, &parent.name, registry_name);
    let opts = FetchFullMetadataCachedOptions {
        registry: &registry,
        cache_dir: Some(config.cache_dir.as_path()),
        full_metadata: false,
        filter_metadata: false,
        pnpr_format: false,
        offline: config.offline,
        priority: pnpm_network::UNPRIORITIZED,
        http: MetadataHttpClient {
            http_client,
            auth_headers: &config.auth_headers,
            retry_opts: config.retry_opts(),
        },
    };
    let packument = fetch_full_metadata_cached(&parent.name, &opts).await.ok()?;
    let pkg_version = packument.versions.get(version)?;
    serde_json::to_value(pkg_version.as_ref()).ok()
}
