use crate::{
    config::Config,
    error::{RegistryError, Result},
    package_name::PackageName,
    revision::{
        HostedRevisionDist, HostedRevisionPackument, RevisionField, hosted_original_reference,
        original_integrity,
    },
    storage::{HostedRevisionRefWrite, Storage},
    streaming::{self, MAX_TARBALL_BYTES},
};
use futures_util::StreamExt as _;
use pnpm_crypto_hash::{create_hex_hash_bytes, integrity_addressed_tarball_path};
use ssri::Integrity;
use std::collections::BTreeSet;

/// Summary of one hosted-revision backfill run.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RevisionBackfillReport {
    pub stores_scanned: usize,
    pub packages_scanned: usize,
    pub versions_scanned: usize,
    pub indexed: usize,
    pub already_indexed: usize,
    pub skipped: usize,
    pub invalid: usize,
}

/// Validate and index legacy hosted package artifacts for SHA-512 digest routes.
///
/// The scan is serial so its memory, file-handle, and object-store request use
/// stays bounded independently of the size of the hosted repository. Existing
/// references make the operation idempotent. In dry-run mode every candidate
/// is still streamed and integrity-checked, but no index is changed. Stop the
/// pnpr writer while changing a filesystem store; S3 writes use conditional
/// updates and may run alongside the service.
pub async fn backfill_hosted_revision_refs(
    config: &Config,
    dry_run: bool,
) -> Result<RevisionBackfillReport> {
    let storage =
        Storage::new(&config.hosted_store, config.storage.clone(), config.cache_storage.clone());
    let mut report = RevisionBackfillReport::default();
    let mut scanned_orgs = BTreeSet::new();
    let owner = backfill_owner();

    for (registry, hosted) in &config.hosted {
        if !scanned_orgs.insert(hosted.org.clone()) {
            continue;
        }
        report.stores_scanned += 1;
        let hosted_storage = storage.for_hosted(&hosted.org);
        let packages = hosted_storage.hosted_package_names_for_backfill().await?;
        for raw_name in packages {
            report.packages_scanned += 1;
            let package = match PackageName::parse(&raw_name) {
                Ok(package) => package,
                Err(err) => {
                    report.invalid += 1;
                    tracing::warn!(%registry, package = %raw_name, error = %err, "hosted revision backfill skipped an invalid package");
                    continue;
                }
            };
            let Some(bytes) = hosted_storage.read_hosted_packument(&package).await? else {
                report.invalid += 1;
                tracing::warn!(%registry, package = %raw_name, "hosted revision backfill found no packument after listing the package");
                continue;
            };
            let packument = match serde_json::from_slice::<HostedRevisionPackument>(&bytes) {
                Ok(packument) => packument,
                Err(err) => {
                    report.invalid += 1;
                    tracing::warn!(%registry, package = %raw_name, error = %err, "hosted revision backfill skipped an invalid packument");
                    continue;
                }
            };
            for (version, manifest) in packument.versions {
                report.versions_scanned += 1;
                let Some(dist) = manifest.dist else {
                    report.skipped += 1;
                    continue;
                };
                let integrity = match eligible_original_integrity(&dist) {
                    OriginalIntegrity::Eligible(integrity) => integrity,
                    OriginalIntegrity::Unsupported => {
                        report.skipped += 1;
                        continue;
                    }
                    OriginalIntegrity::Invalid => {
                        report.invalid += 1;
                        tracing::warn!(%registry, package = %raw_name, %version, "hosted revision backfill rejected invalid revision metadata");
                        continue;
                    }
                };
                let reference = hosted_original_reference(package.as_str(), &version, &integrity)
                    .expect("eligible integrity creates a hosted original reference");
                let filename = package.tarball_name_for_version(&version);
                if package.parse_tarball_name(&filename).is_err() {
                    report.invalid += 1;
                    tracing::warn!(%registry, package = %raw_name, %version, "hosted revision backfill rejected an invalid version path");
                    continue;
                }
                match verify_hosted_tarball(&hosted_storage, &package, &filename, &integrity)
                    .await?
                {
                    TarballValidation::Valid => {}
                    TarballValidation::Missing => {
                        report.invalid += 1;
                        tracing::warn!(%registry, package = %raw_name, %version, %filename, "hosted revision backfill found no tarball for packument metadata");
                        continue;
                    }
                    TarballValidation::Invalid(reason) => {
                        report.invalid += 1;
                        tracing::warn!(%registry, package = %raw_name, %version, %filename, %reason, "hosted revision backfill rejected a tarball");
                        continue;
                    }
                }
                if hosted_storage
                    .read_hosted_revision_refs(&reference.digest)
                    .await?
                    .iter()
                    .any(|existing| existing == &reference.bytes)
                {
                    report.already_indexed += 1;
                    continue;
                }
                report.indexed += 1;
                if dry_run {
                    continue;
                }
                match hosted_storage
                    .write_hosted_revision_ref(
                        &reference.digest,
                        &reference.ref_id,
                        &owner,
                        &reference.bytes,
                    )
                    .await?
                {
                    HostedRevisionRefWrite::Committed => {
                        report.indexed -= 1;
                        report.already_indexed += 1;
                    }
                    HostedRevisionRefWrite::Claimed | HostedRevisionRefWrite::AlreadyClaimed => {
                        hosted_storage
                            .commit_hosted_revision_ref(
                                &reference.digest,
                                &reference.ref_id,
                                &owner,
                            )
                            .await?;
                    }
                }
            }
        }
    }
    Ok(report)
}

enum OriginalIntegrity {
    Eligible(Integrity),
    Unsupported,
    Invalid,
}

fn eligible_original_integrity(dist: &HostedRevisionDist) -> OriginalIntegrity {
    let integrity = match &dist.revision {
        RevisionField::Missing => {
            let Some(raw) = dist.integrity.as_deref() else {
                return OriginalIntegrity::Unsupported;
            };
            match raw.parse() {
                Ok(integrity) => integrity,
                Err(_) => return OriginalIntegrity::Invalid,
            }
        }
        RevisionField::Present(_) => match original_integrity(dist) {
            Some(integrity) => integrity,
            None => return OriginalIntegrity::Invalid,
        },
    };
    if integrity_addressed_tarball_path(&integrity).is_none() {
        return OriginalIntegrity::Unsupported;
    }
    OriginalIntegrity::Eligible(integrity)
}

enum TarballValidation {
    Valid,
    Missing,
    Invalid(String),
}

async fn verify_hosted_tarball(
    storage: &Storage,
    package: &PackageName,
    filename: &str,
    integrity: &Integrity,
) -> Result<TarballValidation> {
    let Some((body, content_length)) = storage.open_hosted_tarball(package, filename).await? else {
        return Ok(TarballValidation::Missing);
    };
    if content_length.is_some_and(|length| length > MAX_TARBALL_BYTES) {
        return Ok(TarballValidation::Invalid(format!(
            "tarball exceeds the {MAX_TARBALL_BYTES} byte limit",
        )));
    }
    let mut checker = streaming::integrity_checker(integrity)
        .map_err(|err| RegistryError::Internal { reason: err.to_string() })?;
    let mut received = 0_u64;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|err| RegistryError::Io(std::io::Error::other(err)))?;
        received = received.saturating_add(chunk.len() as u64);
        if received > MAX_TARBALL_BYTES {
            return Ok(TarballValidation::Invalid(format!(
                "tarball exceeds the {MAX_TARBALL_BYTES} byte limit",
            )));
        }
        checker.input(&chunk);
    }
    match checker.result() {
        Ok(_) => Ok(TarballValidation::Valid),
        Err(err) => Ok(TarballValidation::Invalid(err.to_string())),
    }
}

fn backfill_owner() -> String {
    let mut random = [0_u8; 32];
    getrandom::fill(&mut random).expect("OS CSPRNG must be available");
    create_hex_hash_bytes(&random)
}

#[cfg(test)]
mod tests;
