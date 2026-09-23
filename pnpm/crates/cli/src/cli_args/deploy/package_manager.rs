use miette::Context;
use pnpm_package_manifest::PackageManifest;
use serde_json::{Map, Value};
use std::path::Path;

const PACKAGE_MANAGER: &str = "packageManager";
const DEV_ENGINES: &str = "devEngines";

/// Copy the workspace root's `packageManager` and `devEngines.packageManager`
/// into a deployed manifest that declares neither, so the deploy directory
/// pins the package manager the workspace uses.
pub(super) fn inherit_package_manager(
    manifest: &mut Value,
    workspace_root_manifest: Option<&Value>,
) {
    let Some(workspace_root_manifest) = workspace_root_manifest else { return };
    let Some(manifest) = manifest.as_object_mut() else { return };
    if manifest.contains_key(PACKAGE_MANAGER) || dev_engines_package_manager(manifest).is_some() {
        return;
    }
    if let Some(package_manager) = workspace_root_manifest.get(PACKAGE_MANAGER) {
        manifest.insert(PACKAGE_MANAGER.to_string(), package_manager.clone());
    }
    let Some(package_manager) =
        workspace_root_manifest.as_object().and_then(dev_engines_package_manager)
    else {
        return;
    };
    let dev_engines = manifest
        .entry(DEV_ENGINES)
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(dev_engines) = dev_engines.as_object_mut() {
        dev_engines.insert(PACKAGE_MANAGER.to_string(), package_manager.clone());
    }
}

/// [`inherit_package_manager`] applied to the manifest at `manifest_path`.
pub(super) fn write_inherited_package_manager(
    manifest_path: &Path,
    workspace_root_manifest: Option<&Value>,
) -> miette::Result<()> {
    let mut manifest =
        PackageManifest::from_path(manifest_path.to_path_buf()).wrap_err("read deployed manifest")?;
    inherit_package_manager(manifest.value_mut(), workspace_root_manifest);
    manifest.save().wrap_err("write deployed manifest")
}

fn dev_engines_package_manager(manifest: &Map<String, Value>) -> Option<&Value> {
    manifest.get(DEV_ENGINES)?.get(PACKAGE_MANAGER)
}
