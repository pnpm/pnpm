use super::{AddedRoot, DepKind, HashMap, PackageDiff, RemovedRoot, Value};

pub(super) fn diff_key(kind: DepKind) -> &'static str {
    match kind {
        DepKind::Prod => "prod",
        DepKind::Optional => "optional",
        DepKind::Peer => "peer",
        DepKind::Dev => "dev",
        DepKind::NodeModulesOnly => "nodeModulesOnly",
    }
}

pub(super) fn added_diff(added: &AddedRoot) -> (DepKind, PackageDiff) {
    (
        DepKind::from_dependency_type(added.dependency_type),
        PackageDiff {
            added: true,
            from: added.linked_from.clone(),
            name: added.name.clone(),
            real_name: Some(added.real_name.clone()),
            version: added.version.clone().or_else(|| added.id.clone()),
            latest: added.latest.clone(),
        },
    )
}

pub(super) fn removed_diff(removed: &RemovedRoot) -> (DepKind, PackageDiff) {
    (
        DepKind::from_dependency_type(removed.dependency_type),
        PackageDiff {
            added: false,
            from: None,
            name: removed.name.clone(),
            real_name: None,
            version: removed.version.clone(),
            latest: None,
        },
    )
}

pub(super) fn remove_optional_from_prod(manifest: &Value) -> Value {
    let mut manifest = manifest.clone();
    let optional: Vec<String> = manifest
        .get("optionalDependencies")
        .and_then(Value::as_object)
        .map(|obj| obj.keys().cloned().collect())
        .unwrap_or_default();
    if let Some(deps) = manifest.get_mut("dependencies").and_then(Value::as_object_mut) {
        for name in optional {
            deps.remove(&name);
        }
    }
    manifest
}

/// Record every dependency of `deps` that `other` does not declare, as an
/// addition or a removal.
pub(super) fn record_missing(
    bucket: &mut HashMap<String, PackageDiff>,
    deps: &HashMap<String, String>,
    other: &HashMap<String, String>,
    added: bool,
) {
    let sign = if added { '+' } else { '-' };
    for (name, version) in deps {
        if other.contains_key(name) {
            continue;
        }
        bucket.entry(format!("{sign}{name}")).or_insert_with(|| PackageDiff {
            added,
            from: None,
            name: name.clone(),
            real_name: None,
            version: Some(version.clone()),
            latest: None,
        });
    }
}

pub(super) fn manifest_dep_versions(manifest: &Value, prop: &str) -> HashMap<String, String> {
    manifest
        .get(prop)
        .and_then(Value::as_object)
        .map(|obj| {
            obj.iter()
                .map(|(name, value)| (name.clone(), value.as_str().unwrap_or_default().to_string()))
                .collect()
        })
        .unwrap_or_default()
}
