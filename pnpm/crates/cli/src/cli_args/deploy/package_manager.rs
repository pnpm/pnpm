use crate::cli_args::package_manager::exact_version;
use miette::Context;
use pnpm_package_manifest::{
    PackageManifest,
    package_manager_spec::{
        dev_engines_package_managers, engine_name_version, split_spec, version_without_build,
    },
};
use serde_json::{Map, Value};
use std::path::Path;

const PACKAGE_MANAGER: &str = "packageManager";
const DEV_ENGINES: &str = "devEngines";

/// Copy the workspace root's package manager pin into a deployed manifest that
/// declares none, so the deploy directory pins the package manager the
/// workspace uses.
///
/// Only the `packageManager` field is inherited. The root's `devEngines` is a
/// development-time contract that package managers enforce by default. npm
/// refuses to run any script of a manifest whose `devEngines.packageManager`
/// names another package manager, so copying it into a deploy directory makes
/// that directory unusable outside the workspace. The pin it names is kept,
/// as the `packageManager` field corepack reads.
pub(super) fn inherit_package_manager(manifest: &mut Value, engine_pin_manifest: Option<&Value>) {
    let Some(engine_pin_manifest) = engine_pin_manifest.filter(|value| value.is_object()) else {
        return;
    };
    let Some(manifest) = manifest.as_object_mut() else { return };
    if declared_package_manager(manifest).is_some()
        || dev_engines_package_manager(manifest).is_some()
    {
        return;
    }
    if let Some(package_manager) = inherited_package_manager(engine_pin_manifest) {
        manifest.insert(PACKAGE_MANAGER.to_string(), package_manager);
    }
}

/// The `packageManager` spec a deployed manifest should declare, for a
/// workspace root that declares one.
///
/// `devEngines.packageManager` outranks `packageManager` when pnpm resolves
/// its own pin, so an exact pnpm version there becomes `pnpm@<version>`. A
/// root `packageManager` naming that same version is kept as written instead,
/// so the corepack integrity hash it may carry survives. A root with no exact
/// pnpm entry keeps whatever its `packageManager` field says.
fn inherited_package_manager(engine_pin_manifest: &Value) -> Option<Value> {
    let declared = declared_package_manager(engine_pin_manifest.as_object()?);
    let Some(version) = exact_pnpm_dev_engines_version(engine_pin_manifest) else {
        return declared.cloned();
    };
    if let Some(declared) = declared.and_then(Value::as_str)
        && let ("pnpm", Some(reference)) = split_spec(declared)
        && version_without_build(reference) == version
    {
        return Some(Value::String(declared.to_string()));
    }
    Some(Value::String(format!("pnpm@{version}")))
}

/// The version the root's pnpm `devEngines.packageManager` entry pins
/// exactly, with any integrity hash it carries, or `None` when the root has
/// no pnpm entry or it names a range or a dist-tag.
///
/// Corepack installs the version named exactly, so a range names nothing it
/// can honor. An entry for another package manager is not the pin of a pnpm
/// workspace.
fn exact_pnpm_dev_engines_version(engine_pin_manifest: &Value) -> Option<&str> {
    let (_, version) = dev_engines_package_managers(engine_pin_manifest)
        .filter_map(engine_name_version)
        .find(|(name, _)| *name == "pnpm")?;
    let version = version?;
    exact_version(version)?;
    Some(version)
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
