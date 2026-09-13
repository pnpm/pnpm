use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use serde_json::Value;
use std::collections::HashMap;

pub(crate) fn manifest_publish_config(
    manifest: &PackageManifest,
) -> (Option<String>, Option<bool>) {
    let publish_config = manifest.value().get("publishConfig");
    let publish_directory = publish_config
        .and_then(|publish_config| publish_config.get("directory"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let link_directory = publish_directory
        .as_ref()
        .and_then(|_| {
            publish_config
                .and_then(|publish_config| publish_config.get("linkDirectory"))
                .and_then(Value::as_bool)
                .filter(|link_directory| !link_directory)
        });
    (publish_directory, link_directory)
}

/// Map each direct-dep alias to the manifest group it appears in.
/// `optionalDependencies` wins over `dependencies` wins over
/// `devDependencies` when an alias is duplicated across groups
/// (first-write-wins over the dependency fields).
pub(in super::super) fn manifest_alias_to_group(
    manifest: &PackageManifest,
) -> HashMap<String, DependencyGroup> {
    let mut out: HashMap<String, DependencyGroup> = HashMap::new();
    for group in [
        DependencyGroup::Optional,
        DependencyGroup::Prod,
        DependencyGroup::Dev,
    ] {
        for (alias, _) in manifest.dependencies([group]) {
            out
                .entry(alias.to_string())
                .or_insert(group);
        }
    }
    out
}

/// Look up the user-written specifier for `alias` in the manifest's
/// `optionalDependencies` / `dependencies` / `devDependencies` maps —
/// plus `peerDependencies` when `auto_install_peers` materializes those
/// into the importer's dependencies. Returns `None` for an alias the
/// manifest doesn't declare in any of those groups, including a peer the
/// hoist installed while `autoInstallPeers` is off: such entries stay out
/// of the importer's `specifiers` map and are only reachable through the
/// snapshots graph.
pub(in super::super) fn read_manifest_specifier(
    manifest: &PackageManifest,
    alias: &str,
    auto_install_peers: bool,
) -> Option<String> {
    let materialized_peers = auto_install_peers.then_some(DependencyGroup::Peer);
    for group in [
        DependencyGroup::Optional,
        DependencyGroup::Prod,
        DependencyGroup::Dev,
    ]
    .into_iter()
    .chain(materialized_peers)
    {
        let group_key: &str = group.into();
        if let Some(map) = manifest
            .value()
            .get(group_key)
            .and_then(Value::as_object)
            && let Some(spec) = map.get(alias).and_then(Value::as_str)
        {
            return Some(spec.to_string());
        }
    }
    None
}

pub(super) fn manifest_dependencies_meta(manifest: &PackageManifest) -> Option<serde_json::Value> {
    manifest
        .value()
        .get("dependenciesMeta")
        .filter(|value| {
            value
                .as_object()
                .is_some_and(|meta| !meta.is_empty())
        })
        .cloned()
}
