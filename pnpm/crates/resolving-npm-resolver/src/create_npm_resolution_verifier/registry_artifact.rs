use super::{
    LockfileResolution, NpmResolutionVerifier, PkgName, RegistryArtifactHistory,
    ResolutionVerification, TARBALL_REVISION_MISMATCH_VIOLATION_CODE, current_history_violation,
    current_revision_number, is_integrity_addressed_registry_tarball_url, lockfile_revision,
    missing_artifact_violation, select_revision, tarball_url_violation,
};

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
        let revision_aware = lockfile_revision(resolution).is_some()
            || artifact.current.revision.is_some()
            || !artifact.revisions.is_empty();
        if !revision_aware {
            return tarball_url_violation(lockfile_tarball, artifact.current.tarball.as_deref());
        }

        let current_revision = match current_revision_number(&artifact) {
            Ok(current_revision) => current_revision,
            Err(violation) => return Some(violation),
        };
        if let Some(violation) = current_history_violation(&artifact, current_revision, registry) {
            return Some(violation);
        }

        let requested = lockfile_revision(resolution).unwrap_or(0);
        let integrity = resolution.checkable_integrity().expect("checked before artifact binding");
        let selected = match select_revision(&artifact, requested, current_revision, integrity) {
            Ok(selected) => selected,
            Err(violation) => return Some(violation),
        };
        // A historical revision, or a current record the lockfile does not
        // name, is only trustworthy when its URL is derived from its own
        // integrity.
        let integrity_addressed = selected.tarball.as_deref().is_some_and(|tarball| {
            is_integrity_addressed_registry_tarball_url(tarball, integrity, registry)
        });
        if (requested > 0 || current_revision != requested) && !integrity_addressed {
            return Some(ResolutionVerification::Err {
                code: TARBALL_REVISION_MISMATCH_VIOLATION_CODE,
                reason: format!(
                    "has revision {requested} that is not addressed by its complete sha512 integrity",
                ),
            });
        }
        tarball_url_violation(lockfile_tarball, selected.tarball.as_deref())
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
        if let Some(sink) = self.observed_dist_stats.as_ref()
            && let Some(stats) =
                meta.version_dist_stats.as_ref().and_then(|stats| stats.get(version))
        {
            sink.insert((name.to_string(), version.to_string()), *stats);
        }
        Ok(meta.version_artifacts.and_then(|artifacts| artifacts.get(version).cloned()))
    }
}
