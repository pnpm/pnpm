use crate::cli_args::package_manager::{exact_version, wanted_package_manager};
use miette::Context;
use pnpm_package_manifest::PackageManifest;
use serde_json::{Map, Value};
use std::path::Path;

const PACKAGE_MANAGER: &str = "packageManager";
const DEV_ENGINES: &str = "devEngines";

/// Copy the workspace root's package manager pin into a deployed manifest that
/// declares none, so the deploy directory pins the package manager the
/// workspace uses.
///
/// Only the `packageManager` field is inherited. The root's `devEngines` is a
/// development-time contract that package managers enforce by default — npm
/// refuses to run any script of a manifest whose `devEngines.packageManager`
/// names another package manager — so copying it into a deploy directory makes
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
/// The pin pnpm itself resolves is the one the deploy directory should pin,
/// and `devEngines.packageManager` outranks `packageManager`, so an exact
/// entry there is turned into the equivalent `packageManager` spec. A root
/// that names no such entry, or names only a range that could never be
/// installed, keeps whatever its `packageManager` field says.
fn inherited_package_manager(engine_pin_manifest: &Value) -> Option<Value> {
    if let Some(package_manager) = dev_engines_package_manager_spec(engine_pin_manifest) {
        return Some(Value::String(package_manager));
    }
    declared_package_manager(engine_pin_manifest.as_object()?).cloned()
}

/// The `<name>@<version>` spec the root's `devEngines.packageManager` pins to
/// a single version, or `None` when the root declares no such entry or pins a
/// range or a dist-tag.
///
/// Corepack installs the version named exactly, so a range names nothing it
/// can honor.
fn dev_engines_package_manager_spec(engine_pin_manifest: &Value) -> Option<String> {
    engine_pin_manifest.get(DEV_ENGINES)?.get(PACKAGE_MANAGER)?;
    let wanted = wanted_package_manager(engine_pin_manifest)?;
    let version = exact_version(wanted.version.as_deref()?)?;
    Some(format!("{}@{version}", wanted.name))
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
