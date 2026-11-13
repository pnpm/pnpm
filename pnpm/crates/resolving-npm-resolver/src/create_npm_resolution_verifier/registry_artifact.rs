use super::{
    Arc, DistStats, LockfileResolution, NpmResolutionVerifier, OnceCell, PkgName,
    RegistryArtifactHistory, ResolutionVerification, TARBALL_REVISION_MISMATCH_VIOLATION_CODE,
    TarballRevision, current_history_violation, current_revision_number,
    has_complete_sha512_digest, is_revision_registry_tarball_url, lockfile_revision,
    missing_artifact_violation, redact_url_credentials, select_revision, tarball_url_violation,
    to_registry_url,
};
use crate::pick_package::{ReleaseRecordError, ReleaseRecordRequest};

impl NpmResolutionVerifier {
    /// Confirm the lockfile-pinned tarball URL is the artifact the
    /// registry's own metadata lists for this exact `name@version`.
    ///
    /// Fail-closed: the entry passes only when the registry metadata
    /// affirmatively lists this version with a matching tarball URL. If the
    /// metadata can't be fetched, doesn't list the version, or omits
    /// `dist.tarball`, the entry can't be confirmed and is rejected —
    /// otherwise a tampered lockfile could smuggle a malicious URL past the
    /// check by pointing it at a `name@version` the registry can't vouch for.
    pub(super) async fn run_registry_artifact_check(
        &self,
        registry: &str,
        name: &PkgName,
        version: &str,
        resolution: &LockfileResolution,
        lockfile_tarball: Option<&str>,
    ) -> Option<ResolutionVerification> {
        let artifact = match self.published_artifact(registry, name, version).await {
            Ok(artifact) => artifact,
            Err(violation) => return Some(violation),
        };
        let Some(artifact) = artifact else {
            return missing_artifact_violation(resolution, lockfile_tarball);
        };
        // pnpr's own document carries the selected revision without a URL or
        // inline history; historical revisions and split-out entries live in
        // release records. Honor the marker only for a declared pnpr registry.
        if self.routing.is_pnpr_registry(registry) && artifact.install_v2.is_some() {
            return self.verify_pnpr_document_artifact(
                registry,
                (name, version),
                resolution,
                lockfile_tarball,
                &artifact,
            )
            .await;
        }
        verify_npm_artifact(&artifact, (name, version), resolution, lockfile_tarball, registry)
    }

    async fn verify_pnpr_document_artifact(
        &self,
        registry: &str,
        (name, version): (&PkgName, &str),
        resolution: &LockfileResolution,
        lockfile_tarball: Option<&str>,
        artifact: &RegistryArtifactHistory,
    ) -> Option<ResolutionVerification> {
        let requested = lockfile_revision(resolution);
        let current_revision = match pnpr_current_revision(artifact, requested) {
            Ok(current_revision) => current_revision,
            Err(violation) => return Some(violation),
        };
        let pkg_name = name.to_string();
        let Some(requested) = requested else {
            let published_tarball =
                pnpr_published_tarball_url(registry, &pkg_name, version, current_revision);
            return tarball_url_violation(lockfile_tarball, Some(&published_tarball));
        };
        let current_revision = current_revision.expect("checked when requested is Some");
        let integrity = resolution.checkable_integrity().expect("checked before artifact binding");
        if let Some(violation) = pnpr_revision_url_violation(
            registry,
            (&pkg_name, version, requested),
            lockfile_tarball,
            integrity,
        ) {
            return Some(violation);
        }
        self.verify_pnpr_revision_integrity(
            registry,
            (&pkg_name, version),
            (requested, current_revision),
            integrity,
            artifact,
        )
        .await
    }

    async fn verify_pnpr_revision_integrity(
        &self,
        registry: &str,
        (name, version): (&str, &str),
        (requested, current_revision): (u64, u64),
        integrity: &ssri::Integrity,
        artifact: &RegistryArtifactHistory,
    ) -> Option<ResolutionVerification> {
        if requested == current_revision
            && let Some(current_integrity) = artifact.current.integrity.as_ref()
        {
            return (current_integrity != integrity).then(|| {
                revision_integrity_mismatch(requested)
            });
        }
        match self.fetch_release_record_integrity(registry, name, version, requested).await {
            Ok(Some(record_integrity)) if &record_integrity == integrity => None,
            Ok(_) => Some(revision_integrity_mismatch(requested)),
            Err(message) => Some(ResolutionVerification::FetchFailed { message }),
        }
    }

    async fn fetch_release_record_integrity(
        &self,
        registry: &str,
        name: &str,
        version: &str,
        revision: u64,
    ) -> Result<Option<ssri::Integrity>, String> {
        let url = format!("{}/{version}+r{revision}", to_registry_url(registry, name));
        let cell = {
            let mut cache = self.lookup_context.release_records.lock().await;
            Arc::clone(
                cache
                    .entry(url.clone())
                    .or_insert_with(|| Arc::new(OnceCell::new())),
            )
        };
        cell.get_or_init(|| {
            self.load_release_record_integrity(registry, &url, (name, version, revision))
        })
        .await
        .clone()
    }

    async fn load_release_record_integrity(
        &self,
        registry: &str,
        url: &str,
        (name, version, revision): (&str, &str, u64),
    ) -> Result<Option<ssri::Integrity>, String> {
        let fetch = ReleaseRecordRequest {
            cache_dir: self.metadata.cache_dir.as_deref(),
            forbids_registry: self.metadata.offline,
            http: crate::MetadataHttpClient {
                http_client: &self.metadata.http_client,
                auth_headers: &self.metadata.auth_headers,
                retry_opts: self.metadata.retry_opts,
            },
            registry,
            url,
            name,
            version,
            revision,
        };
        match fetch.run().await {
            Ok(record) => {
                self.record_release_dist_stats(name, version, &record);
                Ok(record.dist.integrity.clone())
            }
            Err(ReleaseRecordError::NotFound(_)) => Ok(None),
            Err(ReleaseRecordError::Other(error)) => Err(redact_url_credentials(&format!(
                "GET {}: {error}",
                redact_url_credentials(url),
            ))),
        }
    }

    fn record_release_dist_stats(
        &self,
        name: &str,
        version: &str,
        record: &pnpm_registry::PackageVersion,
    ) {
        let Some(sink) = self.artifacts.observed_stats.as_ref() else {
            return;
        };
        let stats = DistStats {
            unpacked_size: record.dist.unpacked_size,
            file_count: record.dist.file_count,
            requires_build: record.dist.pnpr.as_ref().and_then(|pnpr| pnpr.requires_build),
        };
        if stats.unpacked_size.is_some()
            || stats.file_count.is_some()
            || stats.requires_build.is_some()
        {
            sink.insert((name.to_string(), version.to_string()), stats);
        }
    }

    /// The registry's own record for this version, recording the dist stats
    /// on the way when a sink is installed.
    ///
    /// A fetch failure propagates the registry's own error (already
    /// credential-redacted) so the install aborts with it rather than
    /// mislabeling a transport failure as a tampering-style URL mismatch.
    /// Still fail-closed: the entry never reaches the filesystem because the
    /// install never proceeds.
    pub(super) async fn published_artifact(
        &self,
        registry: &str,
        name: &PkgName,
        version: &str,
    ) -> Result<Option<RegistryArtifactHistory>, ResolutionVerification> {
        let meta = match self.fetch_abbreviated_meta(registry, name).await {
            Ok(meta) => meta,
            Err(message) => return Err(ResolutionVerification::FetchFailed { message }),
        };
        if let Some(sink) = self.artifacts.observed_stats.as_ref()
            && let Some(stats) = meta.version_dist_stats
                .as_ref()
                .and_then(|stats| stats.get(version))
        {
            sink.insert((name.to_string(), version.to_string()), *stats);
        }
        Ok(meta.version_artifacts.and_then(|artifacts| artifacts.get(version).cloned()))
    }
}

fn verify_npm_artifact(
    artifact: &RegistryArtifactHistory,
    (name, version): (&PkgName, &str),
    resolution: &LockfileResolution,
    lockfile_tarball: Option<&str>,
    registry: &str,
) -> Option<ResolutionVerification> {
    let requested = lockfile_revision(resolution);
    let revision_aware = requested.is_some()
        || artifact.current.revision.is_some()
        || !artifact.revisions.is_empty();
    if !revision_aware {
        return tarball_url_violation(lockfile_tarball, artifact.current.tarball.as_deref());
    }
    let current_revision = match current_revision_number(artifact) {
        Ok(current_revision) => current_revision,
        Err(violation) => return Some(violation),
    };
    let pkg_name = name.to_string();
    if let Some(violation) =
        current_history_violation(artifact, &pkg_name, version, current_revision, registry)
    {
        return Some(violation);
    }
    // An entry outside the revision protocol keeps its ordinary binding,
    // even when the registry later makes the version revision-aware.
    let Some(requested) = requested else {
        return tarball_url_violation(lockfile_tarball, artifact.current.tarball.as_deref());
    };
    let integrity = resolution.checkable_integrity().expect("checked before artifact binding");
    let selected = match select_revision(artifact, requested, current_revision, integrity) {
        Ok(selected) => selected,
        Err(violation) => return Some(violation),
    };
    unaddressed_revision_violation(
        selected.tarball.as_deref(),
        (&pkg_name, version, requested),
        integrity,
        registry,
    )
    .or_else(|| tarball_url_violation(lockfile_tarball, selected.tarball.as_deref()))
}

fn pnpr_current_revision(
    artifact: &RegistryArtifactHistory,
    requested: Option<u64>,
) -> Result<Option<u64>, ResolutionVerification> {
    if artifact.current.revision.is_none() && requested.is_none() {
        return Ok(None);
    }
    current_revision_number(artifact).map(Some)
}

fn pnpr_revision_url_violation(
    registry: &str,
    (name, version, requested): (&str, &str, u64),
    lockfile_tarball: Option<&str>,
    integrity: &ssri::Integrity,
) -> Option<ResolutionVerification> {
    let expected_tarball = TarballRevision::try_from(requested)
        .ok()
        .and_then(|revision| {
            pnpm_lockfile::revision_registry_tarball_url(name, version, revision, registry)
        });
    unaddressed_revision_violation(
        expected_tarball.as_deref(),
        (name, version, requested),
        integrity,
        registry,
    )
    .or_else(|| tarball_url_violation(lockfile_tarball, expected_tarball.as_deref()))
}

/// A revision is only trustworthy when its URL matches its revision and its
/// integrity is a complete sha512 digest.
fn unaddressed_revision_violation(
    tarball: Option<&str>,
    (name, version, requested): (&str, &str, u64),
    integrity: &ssri::Integrity,
    registry: &str,
) -> Option<ResolutionVerification> {
    let revision_addressed = TarballRevision::try_from(requested)
        .is_ok_and(|revision| {
            has_complete_sha512_digest(integrity)
                && tarball.is_some_and(|tarball| {
                    is_revision_registry_tarball_url(tarball, name, version, revision, registry)
                })
        });
    (!revision_addressed).then(|| ResolutionVerification::Err {
        code: TARBALL_REVISION_MISMATCH_VIOLATION_CODE,
        reason: format!(
            "has revision {requested} that is not addressed by its revision URL and complete sha512 integrity",
        ),
    })
}

fn pnpr_published_tarball_url(
    registry: &str,
    name: &str,
    version: &str,
    current_revision: Option<u64>,
) -> String {
    current_revision
        .and_then(|revision| TarballRevision::try_from(revision).ok())
        .and_then(|revision| {
            pnpm_lockfile::revision_registry_tarball_url(name, version, revision, registry)
        })
        .unwrap_or_else(|| {
            pnpm_lockfile::npm_tarball_url(
                name,
                version,
                pnpm_lockfile::TarballUrlOptions {
                    registry,
                    server_type: Some(pnpm_lockfile::RegistryServerType::Pnpr),
                },
            )
        })
}

fn revision_integrity_mismatch(requested: u64) -> ResolutionVerification {
    ResolutionVerification::Err {
        code: TARBALL_REVISION_MISMATCH_VIOLATION_CODE,
        reason: format!(
            "has revision {requested} with an integrity that does not match the registry's current or historical metadata",
        ),
    }
}
