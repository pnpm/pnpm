use super::{normalize_tarball_name, strip_build_metadata};
use crate::PackError;
use pnpm_package_name::is_valid_old_npm_package_name;
use serde_json::Value;

/// The name the tarball is packed under, once the manifest's name *and*
/// version are known to be publishable.
///
/// Both are interpolated into the default tarball filename
/// (`<name>-<version>.tgz`) and the manifest is attacker-controlled, so a
/// path separator in the version would let it smuggle path components into the
/// join and write the tarball outside `dest_dir`. A real semver version never
/// contains one. The version itself is read back off the publish manifest by
/// [`published_identity`], which a `publishConfig` rename can change.
pub(super) fn packed_identity(manifest: &Value) -> Result<&str, PackError> {
    let name = manifest
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or(PackError::PackageNameNotFound)?;
    if !is_valid_old_npm_package_name(name) {
        return Err(PackError::InvalidPackageName {
            name: name.to_string(),
        });
    }
    let version = manifest
        .get("version")
        .and_then(Value::as_str)
        .filter(|version| !version.is_empty())
        .ok_or(PackError::PackageVersionNotFound)?;
    if version.contains('/') || version.contains('\\') {
        return Err(PackError::InvalidPackageVersion {
            version: version.to_string(),
        });
    }
    Ok(name)
}

/// Pack the project at `opts.dir` into a tarball and return the result.
///
/// `R` threads the reporter through the lifecycle-script emits; `Sys`
/// is the filesystem seam for the tarball write phase
/// ([`crate::Host`] in production).
/// The tarball name and version the publish manifest settles on.
///
/// Semver build metadata (the `+<build>` segment) is stripped so the tarball
/// name, the packed manifest and any registry metadata all agree on the
/// version. See [pnpm/pnpm#11518](https://github.com/pnpm/pnpm/issues/11518).
///
/// The name is read back off the publish manifest so a `publishConfig.name`
/// rename reaches the filename too. That rename never went through
/// [`packed_identity`], so it is validated here: it lands in the tarball
/// filename, where a separator would smuggle path components into the join and
/// write outside `dest_dir`.
pub(super) fn published_identity(
    publish_manifest: &mut Value,
    name: &str,
) -> Result<(String, String), PackError> {
    let published_version = strip_build_metadata(
        publish_manifest
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or(""),
    )
    .to_string();
    if let Some(object) = publish_manifest.as_object_mut() {
        object.insert(
            "version".to_string(),
            Value::String(published_version.clone()),
        );
    }
    let published_name = publish_manifest
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .unwrap_or(name);
    if !is_valid_old_npm_package_name(published_name) {
        return Err(PackError::InvalidPackageName {
            name: published_name.to_string(),
        });
    }
    Ok((normalize_tarball_name(published_name), published_version))
}
