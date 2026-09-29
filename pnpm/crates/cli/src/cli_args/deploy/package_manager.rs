use miette::Context;
use pnpm_package_manifest::PackageManifest;
use serde_json::{Map, Value};
use std::path::Path;

const PACKAGE_MANAGER: &str = "packageManager";
const DEV_ENGINES: &str = "devEngines";

/// Copy the workspace root's `packageManager` field into a deployed manifest
/// that pins no package manager itself.
///
/// The root's `devEngines` is never copied. It is a development-time contract
/// that package managers enforce by default, and npm refuses to run any script
/// of a manifest whose `devEngines.packageManager` names another package
/// manager.
pub(super) fn inherit_package_manager(manifest: &mut Value, engine_pin_manifest: Option<&Value>) {
    let Some(package_manager) =
        engine_pin_manifest.and_then(Value::as_object).and_then(declared_package_manager)
    else {
        return;
    };
    let Some(manifest) = manifest.as_object_mut() else { return };
    if declared_package_manager(manifest).is_some()
        || dev_engines_package_manager(manifest).is_some()
    {
        return;
    }
    manifest.insert(PACKAGE_MANAGER.to_string(), package_manager.clone());
}

pub(super) fn write_inherited_package_manager(
    manifest_path: &Path,
    engine_pin_manifest: Option<&Value>,
) -> miette::Result<()> {
    let mut manifest =
        PackageManifest::from_path(manifest_path.to_path_buf()).wrap_err("read deployed manifest")?;
    inherit_package_manager(manifest.value_mut(), engine_pin_manifest);
    manifest.save().wrap_err("write deployed manifest")
}

fn declared_package_manager(manifest: &Map<String, Value>) -> Option<&Value> {
    manifest
        .get(PACKAGE_MANAGER)
        .filter(|value| !value.is_null())
}

fn dev_engines_package_manager(manifest: &Map<String, Value>) -> Option<&Value> {
    manifest
        .get(DEV_ENGINES)?
        .get(PACKAGE_MANAGER)
        .filter(|value| !value.is_null())
}
