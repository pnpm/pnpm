//! Versions with platform variants in a batch
//! ([platform variants](https://github.com/pnpm/rfcs/blob/1a9891a2073f7c03a89fc6756eb78881853fa218/pnpr/text/0000-platform-variants.md#sending-a-version-with-variants)).
//!
//! Every package's archives are staged before the batch is committed, and
//! each document references its archives by integrity, as a single publish
//! of the version does. An archive another job staged is referenced once a
//! `HEAD` shows the registry still holds it for the caller.

use super::{BatchGroup, BatchPublishError, DocumentArchives};
use crate::{
    PackedPkg,
    publish_packed_pkg::{
        StagedLengths, reference_staged_archives, staged_lengths, variant_staged_archives,
    },
    staged_archives::StagedArchive,
};
use futures_util::future::try_join_all;
use pnpm_network::ThrottledClient;
use serde_json::Value;

/// How `package`'s archives travel in a batch, and the archives it stages
/// when its group `stages`. A version with variants, or with an `any`
/// archive another job staged, always stages: the registry check refused it
/// for a registry that does not.
pub(super) fn document_archives<'a>(
    package: &'a PackedPkg<'a>,
    integrity: &str,
    stages: bool,
) -> (DocumentArchives<'a>, Vec<StagedArchive<'a>>) {
    if package.publishes_staged_archives() {
        return (DocumentArchives::Variants(package), variant_staged_archives(package, integrity));
    }
    let staged = stages.then(|| StagedArchive {
        integrity: integrity.to_string(),
        bytes: package.tarball_data,
    });
    (DocumentArchives::Single(integrity.to_string()), staged.into_iter().collect())
}

/// How many archives the documents reference that other jobs staged before
/// this publish, which a commit references beside the ones it stages.
pub(super) fn references_staged_before(archives: &[DocumentArchives<'_>]) -> usize {
    archives
        .iter()
        .map(|archives| match archives {
            DocumentArchives::Variants(package) => {
                usize::from(package.any_staged.is_some())
                    + package.variants
                        .iter()
                        .filter(|variant| {
                            matches!(variant.archive, crate::VariantArchive::Staged(_))
                        })
                        .count()
            }
            DocumentArchives::Single(_) => 0,
        })
        .sum()
}

/// [`reference_variant_archives`] for every group, before any group is
/// published, so an archive missing on one registry publishes nothing to
/// another.
pub(super) async fn reference_every_group(
    groups: &mut [BatchGroup<'_>],
    authorizations: &[Option<String>],
    client: &ThrottledClient,
) -> Result<(), BatchPublishError> {
    for (group, authorization) in groups.iter_mut().zip(authorizations) {
        reference_variant_archives(group, client, authorization.as_deref()).await?;
    }
    Ok(())
}

/// Make each document of a version with variants reference its staged
/// archives, after asking the registry for the size of every archive another
/// job staged, all at once. One the registry does not hold for the caller
/// fails the batch before anything is uploaded or committed.
async fn reference_variant_archives(
    group: &mut BatchGroup<'_>,
    client: &ThrottledClient,
    authorization: Option<&str>,
) -> Result<(), BatchPublishError> {
    let packages: Vec<_> = group.archives
        .iter()
        .enumerate()
        .filter_map(|(index, archives)| match archives {
            DocumentArchives::Variants(package) => Some((index, *package)),
            DocumentArchives::Single(_) => None,
        })
        .collect();
    let registry = &group.registry;
    let lengths: Vec<StagedLengths> = try_join_all(
        packages
            .iter()
            .map(|(_, package)| staged_lengths(package, client, registry, authorization)),
    )
    .await?;
    for ((index, package), lengths) in packages.into_iter().zip(&lengths) {
        let version = package.published_manifest
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or_default();
        reference_staged_archives(&mut group.documents[index], package, (version, lengths));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
