use super::{HashMap, PkgName, Value};

pub(super) fn effective_dependencies(manifest: &Value) -> Option<HashMap<PkgName, String>> {
    let optional = manifest_dependency_map(manifest, "optionalDependencies")?;
    Some(
        manifest_dependency_map(manifest, "dependencies")?
            .into_iter()
            .filter(|(name, _)| !optional.contains_key(name))
            .collect(),
    )
}

pub(super) fn manifest_dependency_map(
    manifest: &Value,
    key: &str,
) -> Option<HashMap<PkgName, String>> {
    let Some(value) = manifest.get(key) else {
        return Some(HashMap::new());
    };
    let map = value.as_object()?;
    map
        .iter()
        .map(|(name, spec)| Some((PkgName::parse(name).ok()?, spec.as_str()?.to_string())))
        .collect()
}
