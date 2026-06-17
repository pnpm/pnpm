use pacquet_lockfile::{PackageMetadata, PeerDependencyMeta};
use pacquet_resolving_resolver_base::ResolveResult;
use serde_json::Value;
use std::collections::HashMap;

pub(crate) fn package_metadata(
    name: &str,
    version: &str,
    result: &ResolveResult,
    registry: &str,
    lockfile_include_tarball_url: bool,
) -> PackageMetadata {
    let manifest = result.manifest.as_deref();
    PackageMetadata {
        resolution: result.resolution.to_lockfile_form(
            name,
            version,
            registry,
            lockfile_include_tarball_url,
        ),
        version: None,
        engines: read_engines(manifest),
        cpu: read_string_list(manifest, "cpu"),
        os: read_string_list(manifest, "os"),
        libc: read_string_list(manifest, "libc"),
        deprecated: manifest
            .and_then(|m| m.get("deprecated"))
            .and_then(Value::as_str)
            .map(ToString::to_string),
        has_bin: manifest_has_bin(manifest),
        prepare: None,
        bundled_dependencies: read_string_list(manifest, "bundledDependencies")
            .or_else(|| read_string_list(manifest, "bundleDependencies")),
        peer_dependencies: read_string_map(manifest, "peerDependencies"),
        peer_dependencies_meta: read_peer_dependencies_meta(manifest),
    }
}

pub(crate) fn read_dependency_map(manifest: Option<&Value>, key: &str) -> HashMap<String, String> {
    read_string_map(manifest, key).unwrap_or_default()
}

fn read_engines(manifest: Option<&Value>) -> Option<HashMap<String, String>> {
    let entries: Vec<(String, String)> = match manifest?.get("engines")? {
        Value::Object(map) => map
            .iter()
            .filter_map(|(name, value)| {
                let range = value.as_str()?;
                (range != "*").then(|| (name.clone(), range.to_string()))
            })
            .collect(),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .filter_map(|(index, value)| {
                let range = value.as_str()?;
                (range != "*").then(|| (index.to_string(), range.to_string()))
            })
            .collect(),
        _ => return None,
    };
    (!entries.is_empty()).then(|| entries.into_iter().collect())
}

fn read_string_map(manifest: Option<&Value>, key: &str) -> Option<HashMap<String, String>> {
    let out: HashMap<String, String> = manifest?
        .get(key)?
        .as_object()?
        .iter()
        .filter_map(|(name, value)| Some((name.clone(), value.as_str()?.to_string())))
        .collect();
    (!out.is_empty()).then_some(out)
}

fn read_string_list(manifest: Option<&Value>, key: &str) -> Option<Vec<String>> {
    match manifest?.get(key)? {
        Value::String(value) if !value.is_empty() => Some(vec![value.clone()]),
        Value::Array(items) => {
            let out: Vec<String> =
                items.iter().filter_map(Value::as_str).map(ToString::to_string).collect();
            (!out.is_empty()).then_some(out)
        }
        _ => None,
    }
}

fn manifest_has_bin(manifest: Option<&Value>) -> Option<bool> {
    let present = match manifest?.get("bin")? {
        Value::String(value) => !value.is_empty(),
        Value::Object(map) => !map.is_empty(),
        _ => false,
    };
    present.then_some(true)
}

fn read_peer_dependencies_meta(
    manifest: Option<&Value>,
) -> Option<HashMap<String, PeerDependencyMeta>> {
    let out: HashMap<String, PeerDependencyMeta> = manifest?
        .get("peerDependenciesMeta")?
        .as_object()?
        .iter()
        .filter_map(|(name, value)| {
            value
                .as_object()?
                .get("optional")?
                .as_bool()
                .filter(|optional| *optional)
                .map(|_| (name.clone(), PeerDependencyMeta { optional: true }))
        })
        .collect();
    (!out.is_empty()).then_some(out)
}
