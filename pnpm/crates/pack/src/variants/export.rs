//! The specifiers in an entry's `manifest` go through the same `workspace:`
//! and `catalog:` replacement as the base manifest's.

use pnpm_exportable_manifest::{
    CreateExportableManifestError, CreateExportableManifestOptions, create_exportable_manifest,
};
use serde_json::{Map, Value};
use std::path::Path;

/// The dependency fields an entry's `manifest` may replace. Peer
/// dependencies are shared by every archive, so no entry sets them.
const DEPENDENCY_FIELDS: [&str; 3] = ["dependencies", "devDependencies", "optionalDependencies"];

/// Replace the `workspace:` and `catalog:` specifiers in the dependency
/// fields of every entry's `manifest` of `publish_manifest`, the exportable
/// manifest of the project at `dir`, as `opts` replaces the base manifest's.
pub(crate) fn export_entry_dependencies(
    dir: &Path,
    publish_manifest: &mut Value,
    opts: &CreateExportableManifestOptions<'_>,
) -> Result<(), CreateExportableManifestError> {
    let identity: Map<String, Value> = ["name", "version"]
        .into_iter()
        .filter_map(|field| Some((field.to_string(), publish_manifest.get(field)?.clone())))
        .collect();
    let Some(Value::Array(entries)) = publish_manifest.pointer_mut("/publishConfig/variants")
    else {
        return Ok(());
    };
    for entry_manifest in entries
        .iter_mut()
        .filter_map(|entry| entry.get_mut("manifest"))
        .filter_map(Value::as_object_mut)
    {
        export_dependencies(dir, (&identity, entry_manifest), opts)?;
    }
    Ok(())
}

/// Replace the specifiers of each dependency field `manifest` sets, as in a
/// manifest of the package's `identity`, its name and version.
fn export_dependencies(
    dir: &Path,
    (identity, manifest): (&Map<String, Value>, &mut Map<String, Value>),
    opts: &CreateExportableManifestOptions<'_>,
) -> Result<(), CreateExportableManifestError> {
    for field in DEPENDENCY_FIELDS {
        let Some(dependencies) = manifest.get(field).filter(|value| value.is_object()) else {
            continue;
        };
        let mut alone = identity.clone();
        alone.insert(field.into(), dependencies.clone());
        let fields_only = CreateExportableManifestOptions { embed_readme: false, ..*opts };
        let exported = create_exportable_manifest(dir, &Value::Object(alone), &fields_only)?;
        if let Some(exported) = exported.get(field) {
            manifest.insert(field.into(), exported.clone());
        }
    }
    Ok(())
}
