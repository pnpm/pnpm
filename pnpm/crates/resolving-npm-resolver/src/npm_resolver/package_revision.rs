use super::{
    Cow, Integrity, InvalidRevisionSpecifierError, InvalidTarballRevisionMetadataError,
    MalformedRevisionHistoryError, NoMatchingRevisionError, PackageVersion, RegistryPackageSpec,
    RegistryRevisionSelector, ResolveError, TarballRevision, has_complete_sha512_digest,
    is_on_revision_tarball_route, is_revision_registry_tarball_url,
};

/// The tarball's revision, when the registry serves one: it must pair
/// with a `<registry>/<name>@<version>+r<revision>` URL and complete sha512
/// integrity. A revision URL without a revision is an ordinary tarball URL:
/// npm's documents served by pnpr give the selected revision that way.
pub(super) fn tarball_revision(
    picked: &PackageVersion,
    tarball: &str,
    integrity: Option<&Integrity>,
    registry: &str,
) -> Result<Option<TarballRevision>, ResolveError> {
    let revision = parse_dist_revision(picked)?;
    let revision_addressed = revision
        .zip(integrity)
        .is_some_and(|(revision, integrity)| {
            has_complete_sha512_digest(integrity)
                && is_revision_registry_tarball_url(
                    tarball,
                    &picked.name,
                    &picked.version.to_string(),
                    revision,
                    registry,
                )
        });
    match (revision, revision_addressed) {
        (Some(_), false) => Err(Box::new(InvalidTarballRevisionMetadataError::new(
            tarball,
            "the URL does not match its revision, complete sha512 integrity, and registry",
        ))),
        _ => Ok(revision),
    }
}

fn parse_dist_revision(picked: &PackageVersion) -> Result<Option<TarballRevision>, ResolveError> {
    picked.dist.revision
        .as_ref()
        .map(|revision| {
            revision
                .as_u64()
                .ok_or_else(|| "the revision is not a nonnegative safe integer".to_string())
                .and_then(|revision| {
                    TarballRevision::try_from(revision).map_err(|error| error.to_string())
                })
        })
        .transpose()
        .map_err(|reason| {
            Box::new(InvalidTarballRevisionMetadataError::new(&picked.dist.tarball, reason))
                as ResolveError
        })
}

pub(super) fn select_package_revision<'a>(
    picked: &'a PackageVersion,
    spec: &RegistryPackageSpec,
    registry: &str,
) -> Result<Cow<'a, PackageVersion>, ResolveError> {
    // pnpr's own document marks the revision without a history or a URL: the
    // URL is rebuilt from it, and other revisions are in release records.
    let from_pnpr_document =
        picked.dist.revisions.is_none() && picked.dist.tarball.is_empty();
    if !from_pnpr_document {
        validate_current_package_revision(picked, registry)?;
    }
    let Some(selector) = spec.revision.as_ref() else {
        return Ok(Cow::Borrowed(picked));
    };
    let requested = match selector {
        RegistryRevisionSelector::Valid(revision) => *revision,
        RegistryRevisionSelector::Invalid(specifier) => {
            return Err(Box::new(InvalidRevisionSpecifierError { specifier: specifier.clone() }));
        }
    };
    let no_matching_revision = || {
        Box::new(NoMatchingRevisionError {
            name: picked.name.clone(),
            version: picked.version.to_string(),
            revision: requested,
        }) as ResolveError
    };
    // A version outside the revision protocol answers only for revision 0,
    // which leaves its ordinary resolution unchanged. A version from pnpr's
    // own document has no history: a requested revision was already read
    // from its release record.
    if picked.dist.revisions.is_none() {
        if answers_for_revision(picked, requested, registry) {
            return Ok(Cow::Borrowed(picked));
        }
        return Err(no_matching_revision());
    }
    let Some(record) = package_revision_record(picked, requested, registry)? else {
        return Err(no_matching_revision());
    };
    apply_revision_record(picked, requested, &record)
}

/// The picked version with the revision record's manifest fields and dist
/// entries substituted in.
pub(super) fn apply_revision_record<'a>(
    picked: &PackageVersion,
    requested: u64,
    record: &ValidatedPackageRevision<'_>,
) -> Result<Cow<'a, PackageVersion>, ResolveError> {
    let mut selected =
        serde_json::to_value(picked).map_err(|error| Box::new(error) as ResolveError)?;
    let selected_object = selected.as_object_mut().expect("PackageVersion serializes as an object");
    for field in REVISION_MANIFEST_FIELDS {
        selected_object.remove(field);
    }
    for field in REVISION_MANIFEST_FIELDS {
        if let Some(value) = record.manifest.get(field) {
            selected_object.insert(field.to_string(), value.clone());
        }
    }
    let dist = selected_object
        .get_mut("dist")
        .and_then(serde_json::Value::as_object_mut)
        .expect("PackageVersion.dist serializes as an object");
    dist.insert(
        "integrity".to_string(),
        serde_json::Value::String(record.integrity_text.to_string()),
    );
    dist.insert("tarball".to_string(), serde_json::Value::String(record.tarball.to_string()));
    dist.remove("shasum");
    dist.insert("revision".to_string(), serde_json::Value::Number(requested.into()));
    serde_json::from_value(selected)
        .map(Cow::Owned)
        .map_err(|error| malformed_revision_history(picked, error.to_string()))
}

/// Whether a version without a revision history is `requested`: its marked
/// revision, else the revision its tarball URL names, else the original. npm's
/// documents served by pnpr give the selected revision only by its URL.
fn answers_for_revision(picked: &PackageVersion, requested: u64, registry: &str) -> bool {
    if let Some(current) = picked.dist.revision.as_ref() {
        return current.as_u64() == Some(requested);
    }
    if !is_on_revision_tarball_route(&picked.dist.tarball, registry) {
        return requested == 0;
    }
    TarballRevision::try_from(requested)
        .is_ok_and(|requested| {
            is_revision_registry_tarball_url(
                &picked.dist.tarball,
                &picked.name,
                &picked.version.to_string(),
                requested,
                registry,
            )
        })
}

pub(crate) fn validate_revision_selector(spec: &RegistryPackageSpec) -> Result<(), ResolveError> {
    let Some(RegistryRevisionSelector::Invalid(specifier)) = spec.revision.as_ref() else {
        return Ok(());
    };
    Err(Box::new(InvalidRevisionSpecifierError { specifier: specifier.clone() }))
}

pub(super) struct ValidatedPackageRevision<'a> {
    pub(super) integrity: Integrity,
    pub(super) integrity_text: &'a str,
    pub(super) tarball: &'a str,
    pub(super) manifest: &'a serde_json::Map<String, serde_json::Value>,
}

pub(super) fn package_revision_record<'a>(
    picked: &'a PackageVersion,
    requested: u64,
    registry: &str,
) -> Result<Option<ValidatedPackageRevision<'a>>, ResolveError> {
    let Some(revisions) = picked.dist.revisions.as_ref() else { return Ok(None) };
    let Some(revisions) = revisions.as_array() else {
        return Err(malformed_revision_history(picked, "the revisions field is not an array"));
    };
    let matches: Vec<&serde_json::Value> = revisions
        .iter()
        .filter(|entry| {
            entry.get("revision").and_then(serde_json::Value::as_u64) == Some(requested)
        })
        .collect();
    if matches.is_empty() {
        return Ok(None);
    }
    if matches.len() != 1 {
        return Err(malformed_revision_history(
            picked,
            format!("revision {requested} is advertised more than once"),
        ));
    }
    let Ok(revision) = TarballRevision::try_from(requested) else {
        return Err(malformed_revision_history(
            picked,
            "a revision is not a canonical safe integer",
        ));
    };
    validate_package_revision_record(picked, revision, registry, matches[0]).map(Some)
}

/// A revision-aware version advertises both its selected revision and a
/// history that holds revision 0 and the selected record; a version
/// outside the protocol advertises neither.
pub(super) fn validate_current_package_revision(
    picked: &PackageVersion,
    registry: &str,
) -> Result<(), ResolveError> {
    let Some(raw_revision) = picked.dist.revision.as_ref() else {
        return match picked.dist.revisions {
            Some(_) => Err(malformed_revision_history(
                picked,
                "the revision history has no selected revision",
            )),
            None => Ok(()),
        };
    };
    let revision = raw_revision
        .as_u64()
        .and_then(|revision| TarballRevision::try_from(revision).ok())
        .map(TarballRevision::get)
        .ok_or_else(|| {
            malformed_revision_history(
                picked,
                format!("current revision {raw_revision} is not a nonnegative safe integer"),
            )
        })?;
    if revision != 0 && package_revision_record(picked, 0, registry)?.is_none() {
        return Err(malformed_revision_history(picked, "the revision history has no revision 0"));
    }
    let record = package_revision_record(picked, revision, registry)?
        .ok_or_else(|| {
            malformed_revision_history(
                picked,
                format!("current revision {revision} has no history entry"),
            )
        })?;
    if picked.dist.integrity.as_ref() != Some(&record.integrity)
        || !same_registry_artifact_url(&picked.dist.tarball, record.tarball)
    {
        return Err(malformed_revision_history(
            picked,
            format!("revision {revision} does not match the current artifact"),
        ));
    }
    Ok(())
}

pub(super) fn same_registry_artifact_url(left: &str, right: &str) -> bool {
    match (reqwest::Url::parse(left), reqwest::Url::parse(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

pub(super) fn malformed_revision_history(
    picked: &PackageVersion,
    reason: impl Into<String>,
) -> ResolveError {
    Box::new(MalformedRevisionHistoryError {
        name: picked.name.clone(),
        version: picked.version.to_string(),
        reason: reason.into(),
    })
}

pub(super) const REVISION_MANIFEST_FIELDS: [&str; 12] = [
    "dependencies",
    "optionalDependencies",
    "peerDependencies",
    "peerDependenciesMeta",
    "bundledDependencies",
    "bundleDependencies",
    "bin",
    "engines",
    "os",
    "cpu",
    "libc",
    "hasInstallScript",
];

fn required_revision_str<'a>(
    picked: &PackageVersion,
    record: &'a serde_json::Value,
    field: &str,
    reason: impl FnOnce() -> String,
) -> Result<&'a str, ResolveError> {
    record
        .get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| malformed_revision_history(picked, reason()))
}

pub(super) fn validate_package_revision_record<'a>(
    picked: &PackageVersion,
    revision: TarballRevision,
    registry: &str,
    record: &'a serde_json::Value,
) -> Result<ValidatedPackageRevision<'a>, ResolveError> {
    let integrity_text = required_revision_str(picked, record, "integrity", || {
        format!("revision {revision} has no integrity")
    })?;
    let integrity = integrity_text
        .parse::<Integrity>()
        .map_err(|_| {
            malformed_revision_history(picked, format!("revision {revision} has invalid integrity"))
        })?;
    let tarball = required_revision_str(picked, record, "tarball", || {
        format!("revision {revision} has no tarball URL")
    })?;
    if !has_complete_sha512_digest(&integrity)
        || !is_revision_registry_tarball_url(
            tarball,
            &picked.name,
            &picked.version.to_string(),
            revision,
            registry,
        )
    {
        return Err(malformed_revision_history(
            picked,
            format!(
                "revision {revision} is not addressed by its revision URL and complete sha512 integrity",
            ),
        ));
    }
    let manifest = record
        .get("manifest")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| {
            malformed_revision_history(
                picked,
                format!("revision {revision} has an invalid manifest"),
            )
        })?;
    Ok(ValidatedPackageRevision { integrity, integrity_text, tarball, manifest })
}
