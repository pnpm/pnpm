use super::{
    JsonValue, LockfileResolution, RegistryArtifact, RegistryArtifactHistory,
    ResolutionVerification, TARBALL_REVISION_MISMATCH_VIOLATION_CODE,
    TARBALL_URL_MISMATCH_VIOLATION_CODE, TarballRevision,
    is_integrity_addressed_registry_tarball_url,
};

pub(super) fn same_tarball_url(left: &str, right: &str) -> bool {
    canonical_tarball_url(left) == canonical_tarball_url(right)
}

/// A lockfile entry the registry no longer publishes cannot be verified; an
/// entry that pins neither a URL nor a revision has nothing to bind.
pub(super) fn missing_artifact_violation(
    resolution: &LockfileResolution,
    lockfile_tarball: Option<&str>,
) -> Option<ResolutionVerification> {
    if lockfile_tarball.is_none() && lockfile_revision(resolution).is_none() {
        return None;
    }
    Some(ResolutionVerification::Err {
        code: if lockfile_tarball.is_some() {
            TARBALL_URL_MISMATCH_VIOLATION_CODE
        } else {
            TARBALL_REVISION_MISMATCH_VIOLATION_CODE
        },
        reason: "could not be verified against the registry's published metadata".to_string(),
    })
}

/// A recorded tarball URL must be the one the registry publishes.
pub(super) fn tarball_url_violation(
    lockfile_tarball: Option<&str>,
    registry_tarball: Option<&str>,
) -> Option<ResolutionVerification> {
    match (lockfile_tarball, registry_tarball) {
        (None, _) => None,
        (Some(lockfile), Some(registry)) if same_tarball_url(lockfile, registry) => None,
        (Some(lockfile), Some(registry)) => Some(ResolutionVerification::Err {
            code: TARBALL_URL_MISMATCH_VIOLATION_CODE,
            reason: format!(
                "has a tarball URL ({lockfile}) that does not match the registry's published metadata ({registry})",
            ),
        }),
        (Some(_), None) => Some(ResolutionVerification::Err {
            code: TARBALL_URL_MISMATCH_VIOLATION_CODE,
            reason: "could not be verified against the registry's published metadata".to_string(),
        }),
    }
}

/// The revision the registry currently serves; an absent one is revision 0.
pub(super) fn current_revision_number(
    artifact: &RegistryArtifactHistory,
) -> Result<u64, ResolutionVerification> {
    let Some(raw_revision) = artifact.current.revision.as_ref() else { return Ok(0) };
    let revision = raw_revision
        .as_u64()
        .and_then(|revision| TarballRevision::try_from(revision).ok())
        .map(TarballRevision::get);
    revision.ok_or_else(|| ResolutionVerification::Err {
        code: TARBALL_REVISION_MISMATCH_VIOLATION_CODE,
        reason: format!("registry metadata has an invalid current revision ({raw_revision})"),
    })
}

/// The current revision must appear exactly once in the published history,
/// with the same integrity and tarball the current record carries, and be
/// addressed by that integrity.
pub(super) fn current_history_violation(
    artifact: &RegistryArtifactHistory,
    current_revision: u64,
    registry: &str,
) -> Option<ResolutionVerification> {
    if current_revision == 0 {
        return None;
    }
    let current_history: Vec<_> = artifact
        .revisions
        .iter()
        .filter(|candidate| {
            candidate.revision.as_ref().and_then(JsonValue::as_u64) == Some(current_revision)
        })
        .collect();
    let consistent = current_history.len() == 1
        && current_history[0].integrity == artifact.current.integrity
        && matches!(
            (artifact.current.tarball.as_deref(), artifact.current.integrity.as_ref()),
            (Some(tarball), Some(integrity))
                if is_integrity_addressed_registry_tarball_url(tarball, integrity, registry),
        )
        && matches!(
            (current_history[0].tarball.as_deref(), artifact.current.tarball.as_deref()),
            (Some(history), Some(current)) if same_tarball_url(history, current),
        );
    if consistent {
        return None;
    }
    Some(ResolutionVerification::Err {
        code: TARBALL_REVISION_MISMATCH_VIOLATION_CODE,
        reason: format!(
            "registry metadata revision {current_revision} does not have exactly one matching history entry",
        ),
    })
}

/// The registry record the requested revision binds to: the current one when
/// it is the requested revision, otherwise its single history entry. Its
/// integrity must be the one the lockfile pins, and a current record must
/// not disagree with its own history entry.
pub(super) fn select_revision<'a>(
    artifact: &'a RegistryArtifactHistory,
    requested: u64,
    current_revision: u64,
    integrity: &ssri::Integrity,
) -> Result<&'a RegistryArtifact, ResolutionVerification> {
    let historical: Vec<_> = artifact
        .revisions
        .iter()
        .filter(|candidate| {
            candidate.revision.as_ref().and_then(JsonValue::as_u64) == Some(requested)
        })
        .collect();
    if historical.len() > 1 {
        return Err(ResolutionVerification::Err {
            code: TARBALL_REVISION_MISMATCH_VIOLATION_CODE,
            reason: format!(
                "revision {requested} is advertised more than once in the registry's history",
            ),
        });
    }
    let historical = historical.first().copied();
    let current_matches = current_revision == requested;
    let selected = if current_matches { Some(&artifact.current) } else { historical };
    selected
        .filter(|selected| {
            selected.integrity.as_ref() == Some(integrity)
                && (!current_matches
                    || historical.is_none_or(|historical| {
                        historical.integrity.as_ref() == Some(integrity)
                    }))
        })
        .ok_or_else(|| ResolutionVerification::Err {
            code: TARBALL_REVISION_MISMATCH_VIOLATION_CODE,
            reason: format!(
                "has revision {requested} with an integrity that does not match the registry's current or historical metadata",
            ),
        })
}

pub(super) fn lockfile_revision(resolution: &LockfileResolution) -> Option<u64> {
    match resolution {
        LockfileResolution::Registry(registry) => registry.revision.map(TarballRevision::get),
        LockfileResolution::Tarball(tarball) => tarball.revision.map(TarballRevision::get),
        _ => None,
    }
}

/// Canonicalize a tarball URL: parse-and-reserialize to drop default
/// ports (`:443`/`:80`), decode the `%2f` scoped-name separator, then
/// ignore the scheme — so a benign http/https, default-port, or
/// encoding difference between the lockfile URL and the registry
/// metadata isn't read as tampering.
pub(super) fn canonical_tarball_url(url: &str) -> String {
    let normalized = reqwest::Url::parse(url)
        .map_or_else(|_error| url.to_string(), |parsed| parsed.to_string())
        // `%2f` may survive re-serialization in either case; normalize both.
        .replace("%2F", "/")
        .replace("%2f", "/");
    match normalized.split_once("://") {
        Some((_scheme, rest)) => rest.to_string(),
        None => normalized,
    }
}
