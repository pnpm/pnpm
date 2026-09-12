use super::{
    AppState, CanonicalPackageName, HostedGate, HostedOriginalRef, Identity, Integrity,
    RegistryError, RegistrySource, Response, StatusCode, Storage, ensure_osv_allowed, hosted_gate,
    hosted_original_is_current, not_found, open_hosted_revision_tarball, private_no_cache,
    resolve_registry_source,
};
use axum::response::IntoResponse;

/// Try the references the anonymous caller could not read, only after every
/// public one has been tried.
pub(super) async fn serve_private_revision_refs(
    scan: RevisionScan,
    digest: &str,
    integrity: &Integrity,
) -> Response {
    for (storage, package, version) in scan.private_refs {
        let response =
            open_hosted_revision_tarball(&storage, &package, &version, digest, integrity).await;
        if response.status() != StatusCode::NOT_FOUND {
            return response;
        }
    }
    match scan.policy_error {
        Some(err) => private_no_cache(err.into_response()),
        None => private_no_cache(not_found()),
    }
}

/// What the scan of a revision digest's references has found so far.
#[derive(Default)]
pub(super) struct RevisionScan {
    /// References the anonymous caller cannot read, tried only after every
    /// public one, so a public hit answers without disclosing a private
    /// registry's contents through timing.
    pub(super) private_refs: Vec<(Storage, CanonicalPackageName, String)>,
    /// A refusal to hold back until no reference can serve the digest.
    pub(super) policy_error: Option<RegistryError>,
}

/// One hosted source of a revision digest's references.
pub(super) struct RevisionSource<'a> {
    pub(super) registry: &'a str,
    pub(super) source: &'a str,
    pub(super) storage: &'a Storage,
    pub(super) digest: &'a str,
    pub(super) integrity: &'a Integrity,
}

/// Try every reference one source holds.
pub(super) async fn serve_revision_refs(
    state: &AppState,
    identity: &Identity,
    source: RevisionSource<'_>,
    refs: Vec<HostedOriginalRef>,
    scan: &mut RevisionScan,
) -> Option<Response> {
    for original in refs {
        let reference = RevisionRef {
            registry: source.registry,
            source: source.source,
            storage: source.storage,
            original,
            digest: source.digest,
            integrity: source.integrity,
        };
        if let Some(response) = serve_revision_ref(state, identity, reference, scan).await {
            return Some(response);
        }
    }
    None
}

/// One reference of a revision digest, in the hosted source that holds it.
pub(super) struct RevisionRef<'a> {
    /// The registry the request addressed, which decides where a name routes.
    pub(super) registry: &'a str,
    pub(super) source: &'a str,
    pub(super) storage: &'a Storage,
    pub(super) original: HostedOriginalRef,
    pub(super) digest: &'a str,
    pub(super) integrity: &'a Integrity,
}

/// Try one reference. `Some` is the response to send; `None` means the scan
/// continues.
pub(super) async fn serve_revision_ref(
    state: &AppState,
    identity: &Identity,
    reference: RevisionRef<'_>,
    scan: &mut RevisionScan,
) -> Option<Response> {
    let RevisionRef { registry, source, storage, original, digest, integrity } = reference;
    let package =
        match CanonicalPackageName::parse(&original.package, pnpr_package_name::Ecosystem::Npm) {
            Ok(package) => package,
            Err(err) => return Some(private_no_cache(err.into_response())),
        };
    let filename = package.tarball_name_for_version(&original.version);
    if let Err(err) = package.canonicalize_tarball_name(&filename) {
        return Some(private_no_cache(err.into_response()));
    }
    if !readable_here(state, identity, Routed { registry, source }, &package) {
        return None;
    }
    match hosted_original_is_current(storage, &package, &original.version, digest).await {
        Ok(true) => {}
        Ok(false) => return None,
        Err(err) => return Some(private_no_cache(err.into_response())),
    }
    if let Err(err) = ensure_osv_allowed(state, &package, &original.version) {
        scan.policy_error.get_or_insert(err);
        return None;
    }
    if !readable_here(state, &Identity::Anonymous, Routed { registry, source }, &package) {
        scan.private_refs.push((storage.clone(), package, original.version));
        return None;
    }
    let response =
        open_hosted_revision_tarball(storage, &package, &original.version, digest, integrity).await;
    (response.status() != StatusCode::NOT_FOUND).then_some(response)
}

/// The registry a request addressed and the hosted source being considered.
#[derive(Clone, Copy)]
pub(super) struct Routed<'a> {
    pub(super) registry: &'a str,
    pub(super) source: &'a str,
}

/// Whether the addressed registry routes this package to `source` and the
/// caller may read it there.
pub(super) fn readable_here(
    state: &AppState,
    identity: &Identity,
    routed: Routed<'_>,
    package: &CanonicalPackageName,
) -> bool {
    matches!(
        resolve_registry_source(state, routed.registry, package.as_str()),
        RegistrySource::Hosted(resolved) if resolved == routed.source,
    ) && matches!(
        hosted_gate(state, identity, routed.source, package.as_str()),
        HostedGate::Allowed(_),
    )
}

pub(super) async fn hosted_revision_refs(
    storage: &Storage,
    digest: &str,
) -> Result<Vec<HostedOriginalRef>, RegistryError> {
    storage
        .read_hosted_revision_refs(digest)
        .await?
        .into_iter()
        .map(|bytes| serde_json::from_slice(&bytes).map_err(RegistryError::Json))
        .collect()
}
