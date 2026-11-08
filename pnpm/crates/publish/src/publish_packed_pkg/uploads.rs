//! `pnpm publish --upload-variant`: upload archives of a version to pnpr's
//! staging area from the machine that built them, for a later publish on
//! another machine to reference by integrity
//! ([platform variants](https://github.com/pnpm/rfcs/blob/1a9891a2073f7c03a89fc6756eb78881853fa218/pnpr/text/0000-platform-variants.md#uploading-variants-from-several-machines)).
//!
//! The upload publishes nothing. It always sends the `PUT`, without asking
//! whether the archive is staged already: uploading staged bytes again only
//! renews them, so a rerun of the job is safe.

use super::{
    PublishNetwork, PublishPackedPkgError, PublishPackedPkgOptions, Value, manifest_string,
    publish_authorization, registry_for_display,
};
use crate::{
    capabilities::{Clock, EnvVar, OidcFetch},
    provenance_gen::SignProvenance,
    publish_options::create_publish_options,
    staged_archives::{StagedArchive, upload_archives},
};
use pnpm_reporter::Reporter;

/// One archive to upload, and its integrity once computed.
pub struct UploadedArchive<'a> {
    pub tarball_data: &'a [u8],
    pub integrity: String,
}

impl<'a> UploadedArchive<'a> {
    #[must_use]
    pub fn new(tarball_data: &'a [u8]) -> Self {
        UploadedArchive { tarball_data, integrity: super::variants::sri_sha512(tarball_data) }
    }
}

/// Upload `archives` of the version `manifest` describes to its registry's
/// staging area. The registry must be declared with `serverType: pnpr`. A
/// dry run resolves the registry and uploads nothing.
pub async fn upload_variant_archives<Sys, Reporter>(
    manifest: &Value,
    archives: &[UploadedArchive<'_>],
    opts: &PublishPackedPkgOptions,
    network: &PublishNetwork<'_>,
) -> Result<(), PublishPackedPkgError>
where
    Sys: EnvVar + Clock + OidcFetch + SignProvenance,
    Reporter: self::Reporter,
{
    // A statement signed here would name one archive, not the version; the
    // job that publishes the version signs one statement for all of them.
    if opts.registry.provenance == Some(true) {
        return Err(PublishPackedPkgError::UploadWithProvenance);
    }
    let input = opts.registry.create_options_input();
    let resolved = create_publish_options::<Sys, Reporter>(manifest, &input, true)
        .await
        .map_err(PublishPackedPkgError::from)?;
    let registry = &resolved.registry;
    if !opts.pnpr_registries.contains(registry.as_str()) {
        return Err(PublishPackedPkgError::VariantsNeedPnpr {
            registry: registry_for_display(registry),
        });
    }
    if opts.dry_run {
        return Ok(());
    }
    let name = manifest_string(manifest, "name");
    let authorization = publish_authorization(&resolved, network, registry, &name);
    let staged: Vec<StagedArchive<'_>> = archives
        .iter()
        .map(|archive| StagedArchive {
            integrity: archive.integrity.clone(),
            bytes: archive.tarball_data,
        })
        .collect();
    upload_archives(network.client, registry, authorization.as_deref(), &staged)
        .await
        .map_err(PublishPackedPkgError::Staging)
}
