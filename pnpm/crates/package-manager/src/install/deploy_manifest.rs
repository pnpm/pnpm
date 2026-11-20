use std::sync::Arc;

pub fn apply_deploy_manifest_hook(manifest: &mut serde_json::Value) {
    let names =
        deploy_dependency_names(manifest, "workspace:").map(str::to_owned).collect::<Vec<_>>();
    inject_deploy_dependencies_meta(manifest, names);
    copy_deploy_linked_dependencies(manifest);
}
pub(crate) fn apply_deploy_manifest_hook_to_arc(
    mut manifest: Arc<serde_json::Value>,
) -> Arc<serde_json::Value> {
    if deploy_dependency_names(&manifest, "workspace:").next().is_none()
        && deploy_dependency_names(&manifest, "link:").next().is_none()
    {
        return manifest;
    }
    apply_deploy_manifest_hook(Arc::make_mut(&mut manifest));
    manifest
}
fn deploy_dependency_names<'a>(
    manifest: &'a serde_json::Value,
    protocol: &'static str,
) -> impl Iterator<Item = &'a str> {
    ["optionalDependencies", "dependencies", "devDependencies"]
        .into_iter()
        .filter_map(move |field| manifest.get(field)?.as_object())
        .flat_map(|dependencies| dependencies.iter())
        .filter_map(move |(name, specifier)| {
            specifier
                .as_str()
                .is_some_and(|specifier| specifier.starts_with(protocol))
                .then_some(name.as_str())
        })
}

fn copy_deploy_linked_dependencies(manifest: &mut serde_json::Value) {
    for field in ["optionalDependencies", "dependencies", "devDependencies"] {
        let Some(dependencies) = manifest.get_mut(field).and_then(serde_json::Value::as_object_mut)
        else {
            continue;
        };
        for specifier in dependencies.values_mut() {
            let Some(directory) = specifier
                .as_str()
                .and_then(|value| value.strip_prefix("link:"))
            else {
                continue;
            };
            *specifier = serde_json::Value::String(format!("file:{directory}"));
        }
    }
}

pub(super) fn inject_deploy_dependencies_meta(
    manifest: &mut serde_json::Value,
    names: Vec<String>,
) {
    if names.is_empty() {
        return;
    }
    let Some(object) = manifest.as_object_mut() else { return };
    let dependencies_meta = object
        .entry("dependenciesMeta")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    let Some(meta_object) = dependencies_meta.as_object_mut() else { return };
    for name in names {
        let dependency_meta = meta_object.entry(name).or_insert(serde_json::Value::Null);
        match dependency_meta {
            serde_json::Value::Object(object) => {
                object.insert("injected".to_owned(), serde_json::Value::Bool(true));
            }
            value => *value = serde_json::json!({ "injected": true }),
        }
    }
}
