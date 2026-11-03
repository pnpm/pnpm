//! Reading the variant archives a publish body references.

use super::{
    DECLARATION_FIELD, Declarations, RegistryError, STORED_FIELD, Value, VariantDeclaration,
    bad_request, invalid_tag,
};

/// Remove variant references, and any server-owned variant field, from a
/// publish body, and return the references.
pub(in super::super) fn take_declarations(
    incoming: &mut Value,
) -> Result<Declarations, RegistryError> {
    let mut declared = Declarations::new();
    let Some(versions) = incoming.get_mut("versions").and_then(Value::as_object_mut) else {
        return Ok(declared);
    };
    for (version, manifest) in versions.iter_mut() {
        let Some(manifest) = manifest.as_object_mut() else { continue };
        manifest.remove(STORED_FIELD);
        let Some(list) = manifest.remove(DECLARATION_FIELD) else { continue };
        declared.insert(version.clone(), version_declarations(version, list)?);
    }
    Ok(declared)
}

/// The variant archives one version references: tag to the `.tgz` integrity
/// its upload is verified against. What each archive is a variant for is
/// read from the archive itself, and must give the tag that names it.
fn version_declarations(
    version: &str,
    list: Value,
) -> Result<Vec<VariantDeclaration>, RegistryError> {
    let Value::Object(list) = list else {
        return Err(bad_request(format!(
            "{DECLARATION_FIELD} of {version} must map variant tags to integrities",
        )));
    };
    list.into_iter()
        .map(|(tag, integrity)| {
            if let Some(reason) = invalid_tag(&tag) {
                return Err(bad_request(format!("a variant of {version}: {reason}")));
            }
            let Value::String(tgz_integrity) = integrity else {
                return Err(bad_request(format!("variant {tag:?} of {version} has no integrity")));
            };
            Ok(VariantDeclaration { tag, tgz_integrity })
        })
        .collect()
}
