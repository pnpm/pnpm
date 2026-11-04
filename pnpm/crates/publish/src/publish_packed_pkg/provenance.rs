//! The provenance statement a publish signs and attaches.

use super::{OidcHttpOptions, PackedPkg, PublishPackedPkgError, clean_version, variants};
use crate::{
    capabilities::{Clock, EnvVar, OidcFetch},
    provenance_gen::{SignProvenance, generate_provenance, generate_provenance_for},
};
use pnpm_reporter::Reporter;

/// Sign the version's provenance statement and attach it: one statement
/// naming every archive the version publishes, its `any` archive and each
/// platform variant.
pub(super) async fn attach_provenance<Sys, Reporter>(
    document: &mut serde_json::Value,
    (name, version): (&str, &str),
    pkg: &PackedPkg<'_>,
    http: &OidcHttpOptions,
) -> Result<(), PublishPackedPkgError>
where
    Sys: EnvVar + Clock + OidcFetch + SignProvenance,
    Reporter: self::Reporter,
{
    // The statement names the version as the document keys it, without
    // build metadata, which is how the registry names its archives.
    let version = &clean_version(version)?;
    let attachment = if pkg.publishes_staged_archives() {
        let subjects = variants::provenance_subjects(pkg, (name, version))?;
        generate_provenance_for::<Sys, Reporter>(name, version, &subjects, http).await
    } else {
        generate_provenance::<Sys, Reporter>(name, version, pkg.tarball_data, http).await
    }
    .map_err(PublishPackedPkgError::Provenance)?;
    document["_attachments"][attachment.bundle_name.as_str()] = serde_json::json!({
        "content_type": attachment.content_type,
        "data": attachment.data,
        "length": attachment.data.len(),
    });
    Ok(())
}
