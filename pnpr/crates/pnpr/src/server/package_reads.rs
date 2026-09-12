use super::{
    Action, AppState, CanonicalPackageName, ConcreteKind, Ecosystem, HeaderMap, Identity,
    RegistryError, Resolved, Response, Value, addressed_registry, authorize, caller_scoped,
    ensure_osv_allowed, extract_upstream_version_manifest, extract_version_manifest,
    is_osv_vulnerable_packument_version, not_found, packument_bytes_response, packument_response,
    read_source_packument, registry_endpoint, resolve_version_or_tag, revision_source_registry,
    serve_packument_via_upstream, serve_tarball_via_upstream, tarball_response, wants_abbreviated,
};
use axum::response::IntoResponse;

// --------------------------------------------------------------------
// Handler bodies.
// --------------------------------------------------------------------

pub(super) async fn serve_packument(
    state: &AppState,
    identity: &Identity,
    headers: &HeaderMap,
    registry: Option<&str>,
    raw_name: &str,
) -> Response {
    let Some(target) = addressed_registry(state, registry, Ecosystem::Npm) else {
        return not_found();
    };
    let base = registry_endpoint(state, Ecosystem::Npm, registry);
    let response =
        serve_registry_packument(state, identity, headers, &target, raw_name, &base).await;
    caller_scoped(state, Ecosystem::Npm, registry, Some(raw_name), response)
}

pub(super) async fn serve_version_manifest(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    version_or_tag: &str,
) -> Response {
    let Some(target) = addressed_registry(state, registry, Ecosystem::Npm) else {
        return not_found();
    };
    let base = registry_endpoint(state, Ecosystem::Npm, registry);
    let response =
        serve_registry_version_manifest(state, identity, &target, raw_name, version_or_tag, &base)
            .await;
    caller_scoped(state, Ecosystem::Npm, registry, Some(raw_name), response)
}

pub(super) async fn serve_registry_version_manifest(
    state: &AppState,
    identity: &Identity,
    registry: &str,
    raw_name: &str,
    version_or_tag: &str,
    tarball_base: &str,
) -> Response {
    let name = match CanonicalPackageName::parse(raw_name, pnpr_package_name::Ecosystem::Npm) {
        Ok(name) => name,
        Err(err) => return err.into_response(),
    };
    let resolved_source = resolve_registry_source(state, registry, name.as_str());
    let bytes = match read_source_packument(state, identity, &resolved_source, &name).await {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return not_found(),
        Err(err) => return err.into_response(),
    };
    let packument: Value = match serde_json::from_slice(&bytes) {
        Ok(packument) => packument,
        Err(err) => return RegistryError::Json(err).into_response(),
    };
    if osv_hides_version(state, &packument, name.as_str(), version_or_tag) {
        return not_found();
    }
    let revision_registry = match &resolved_source {
        RegistrySource::Upstream(source) => revision_source_registry(state, registry, source),
        RegistrySource::Hosted(_) | RegistrySource::Unclaimed | RegistrySource::NotFound => None,
    };
    let manifest = match revision_registry {
        Some(source_registry) => extract_upstream_version_manifest(
            &packument,
            &name,
            version_or_tag,
            source_registry,
            tarball_base,
        ),
        None => extract_version_manifest(&packument, &name, version_or_tag, tarball_base),
    };
    let Some(manifest) = manifest else {
        return not_found();
    };
    match serde_json::to_vec(&manifest) {
        Ok(body) => packument_bytes_response(body, "application/json", None),
        Err(err) => RegistryError::Json(err).into_response(),
    }
}

pub(super) fn osv_hides_version(
    state: &AppState,
    packument: &Value,
    package_name: &str,
    version_or_tag: &str,
) -> bool {
    state.inner.osv_index.as_ref().is_some_and(|osv_index| {
        let resolved = resolve_version_or_tag(packument, version_or_tag);
        is_osv_vulnerable_packument_version(packument, package_name, resolved, osv_index)
    })
}

// --------------------------------------------------------------------
// Registry dispatch. A `/~<name>/` request resolves the package to
// exactly one concrete origin through the validated registry graph
// ([`pnpr_registry`]) and serves it there — authoritatively. Every concrete
// registry's declared `patterns:` are enforced here, before storage or any
// upstream is consulted, on the direct address and through a router alike; a
// router selects the first source whose patterns claim the name. An unclaimed
// name is a definitive 404 (never a fall-through to another origin), and a
// selected-but-unavailable upstream surfaces an *error* rather than a 404
// (the via-upstream path returns `UpstreamUnavailable`), so a down private
// source can never be reported as "not found" and pushed onto a public origin
// one layer out.
// --------------------------------------------------------------------

/// The concrete origin a `/~<name>/` request resolved to, owned so it can be
/// held across an `await` without borrowing the config.
pub(super) enum RegistrySource {
    /// An upstream registry (public or private), served via its `/~<source>/`
    /// upstream machinery. The id is a key in [`Config::upstreams`](pnpr_config::Config::upstreams).
    Upstream(String),
    /// A hosted registry, served from the hosted store.
    Hosted(String),
    /// No declared namespace claims the package — the addressed registry's
    /// patterns don't cover it, or none of a router's sources claim it. A
    /// definitive 404 on reads; writes reject it with a reason instead, so a
    /// typo'd scope fails loudly rather than 404-ing later.
    Unclaimed,
    /// The registry id is unknown — a definitive not-found with no fall-through.
    NotFound,
}

/// The registry the path-less base (`https://<pnpr>/`) aliases, owned so it can be
/// held across an `await`. `None` disables the path-less base entirely — the
/// bare host has no registry and every request is a not-found, so clients must
/// address a `/~<name>/`. There is no legacy hosted-then-proxy path: a
/// path-less request resolves through the registry graph or it does not resolve.
pub(super) fn default_registry_target(state: &AppState, ecosystem: Ecosystem) -> Option<String> {
    state.inner.config.registries.default_for(ecosystem).map(str::to_string)
}

/// Resolve an npm request; see [`resolve_ecosystem_source`].
pub(super) fn resolve_registry_source(
    state: &AppState,
    registry: &str,
    package: &str,
) -> RegistrySource {
    resolve_ecosystem_source(state, registry, Ecosystem::Npm, package)
}

/// Resolve `package` through `registry` for one ecosystem's surface: only
/// the sources that speak that ecosystem's protocol are considered.
pub(super) fn resolve_ecosystem_source(
    state: &AppState,
    registry: &str,
    ecosystem: Ecosystem,
    package: &str,
) -> RegistrySource {
    match state.inner.config.registries.resolve(registry, ecosystem, package) {
        Resolved::Concrete { registry, kind: ConcreteKind::Upstream } => {
            RegistrySource::Upstream(registry.to_string())
        }
        Resolved::Concrete { registry, kind: ConcreteKind::Hosted } => {
            RegistrySource::Hosted(registry.to_string())
        }
        // An unclaimed name is definitive — never a fall-through to another
        // origin, and never a storage or upstream consultation.
        Resolved::Unclaimed => RegistrySource::Unclaimed,
        // The graph is the only dispatch table: server construction folds
        // every configured upstream into it (`ensure_valid_registry_graph`), so
        // a name it doesn't know is a definitive not-found — there is no
        // upstream-table side door that would skip namespace enforcement.
        Resolved::UnknownRegistry => RegistrySource::NotFound,
    }
}

/// Whether the concrete origin `package` resolves to through `registry` serves
/// caller-gated content: a hosted registry whose access list denies anonymous
/// callers, or an upstream registry that declares `access:`. Responses from such
/// an origin vary by `Authorization` and must never land in a shared HTTP
/// cache, whichever URL surface (path-less or `/~<name>/`) served them.
pub(super) fn resolves_to_private_source(
    state: &AppState,
    registry: &str,
    ecosystem: Ecosystem,
    package: &str,
) -> bool {
    match resolve_ecosystem_source(state, registry, ecosystem, package) {
        RegistrySource::Hosted(source) => {
            state.inner.config.hosted.get(&source).is_some_and(|hosted| {
                !hosted.rules.for_package(package).access.allows(&Identity::Anonymous)
            })
        }
        // A private upstream (registry-level `access:`) is caller-gated for
        // *every* name — unlike a hosted registry, its registry-level gate is
        // enforced independently at serving (`authorized_upstream` runs
        // before per-package rules on every upstream read), so a per-package
        // `access: $all` entry cannot open a name on it and `access.is_some()`
        // alone already means the response varies by caller. A public
        // upstream can still gate individual names through a per-package
        // `access` rule.
        RegistrySource::Upstream(source) => {
            state.inner.config.upstreams.get(&source).is_some_and(|upstream| {
                upstream.access.is_some()
                    || !upstream.rules.for_package(package).access.allows(&Identity::Anonymous)
            })
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => false,
    }
}

/// Serve a packument addressed to `/~<name>/<pkg>` through the registry graph.
pub(super) async fn serve_registry_packument(
    state: &AppState,
    identity: &Identity,
    headers: &HeaderMap,
    registry: &str,
    raw_name: &str,
    tarball_base: &str,
) -> Response {
    let name = match CanonicalPackageName::parse(raw_name, pnpr_package_name::Ecosystem::Npm) {
        Ok(n) => n,
        Err(err) => return err.into_response(),
    };
    // `tarball_base` is the URL the *client* addressed (the path-less host or a
    // `/~<name>/`), not the resolved source's `/~<source>/`. The served
    // packument's `dist.tarball` URLs must stay canonical for that base so a
    // client's lockfile drops them — persisting the resolved source path would
    // bake the registry name in and break lockfile portability.
    let resolved_source = resolve_registry_source(state, registry, name.as_str());
    match &resolved_source {
        RegistrySource::Upstream(source) => {
            // The upstream registry's per-package rules gate every served
            // read, so an access-gated name can't be read even through a
            // public upstream. Checked before serving so the decision
            // precedes any existence-revealing signal like an OSV 403.
            if let Err(err) =
                authorize(state, identity, &resolved_source, name.as_str(), Action::Access)
            {
                return err.into_response();
            }
            let revision_registry = revision_source_registry(state, registry, source);
            serve_packument_via_upstream(
                state,
                identity,
                headers,
                source,
                &name,
                tarball_base,
                revision_registry,
            )
            .await
        }
        // A hosted denial answers per its gate tier (see `hosted_gate`): a
        // registry-default denial is a not-found mask, an explicit
        // `packages:` entry denies loudly so clients can prompt for auth.
        RegistrySource::Hosted(source) => {
            serve_hosted_packument(state, identity, headers, source, &name, tarball_base).await
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => not_found(),
    }
}

/// Serve a tarball addressed to `/~<name>/<pkg>/-/<file>` through the registry
/// graph. Routing is deterministic by package name, so the tarball resolves to
/// the same concrete source the packument did.
pub(super) async fn serve_registry_tarball(
    state: &AppState,
    identity: &Identity,
    registry: &str,
    raw_name: &str,
    filename: &str,
) -> Response {
    let name = match CanonicalPackageName::parse(raw_name, pnpr_package_name::Ecosystem::Npm) {
        Ok(n) => n,
        Err(err) => return err.into_response(),
    };
    let resolved_source = resolve_registry_source(state, registry, name.as_str());
    match &resolved_source {
        RegistrySource::Upstream(source) => {
            // Per-package rules before serving — see `serve_registry_packument`.
            if let Err(err) =
                authorize(state, identity, &resolved_source, name.as_str(), Action::Access)
            {
                return err.into_response();
            }
            serve_tarball_via_upstream(state, identity, source, name.as_str(), filename).await
        }
        // A hosted denial is a not-found mask, inside `serve_hosted_tarball`
        // — see `serve_registry_packument`.
        RegistrySource::Hosted(source) => {
            serve_hosted_tarball(state, identity, source, &name, filename).await
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => not_found(),
    }
}

/// How a hosted registry answers a read of `package` for `identity`:
/// admitted with the storage namespace to read from, or denied one of two
/// ways. The two denial shapes preserve the two authorization tiers the
/// merged `packages:` map folds together: an **explicit** entry's `access`
/// is declared, discoverable config — deny loudly (401/403, so a client can
/// prompt for credentials, the registry-mock `needs-auth` contract) — while
/// the registry-level **default** masks as not-found, so a blanket-private
/// registry never reveals which names exist.
pub(super) enum HostedGate {
    Allowed(String),
    /// The registry default denies the caller: indistinguishable from an
    /// absent package.
    MaskNotFound,
    /// An explicit `packages:` entry denies the caller: 401 for an
    /// anonymous caller (authenticate and retry), 403 for an authenticated
    /// one outside the allowed set.
    Denied(RegistryError),
}

/// Evaluate the hosted read gate: the effective per-package `access` (most
/// specific `packages:` entry, falling back to the registry-level default)
/// gates reads and the write routing alike — a caller who may not read a
/// hosted package may not publish, tag, or unpublish it either.
pub(super) fn hosted_gate(
    state: &AppState,
    identity: &Identity,
    source: &str,
    package: &str,
) -> HostedGate {
    let Some(hosted) = state.inner.config.hosted.get(source) else {
        return HostedGate::MaskNotFound;
    };
    let effective = hosted.rules.for_package(package);
    if effective.access.allows(identity) {
        return HostedGate::Allowed(hosted.org.clone());
    }
    // Loud denial only inside a registry the caller may see: the explicit
    // entry gates this name, but the registry-level default admits the
    // caller to the registry itself. When the default denies them too, the
    // mask below wins — an explicit rule on a blanket-private registry must
    // not become an existence probe.
    if effective.access_is_explicit && hosted.rules.default_access().allows(identity) {
        return HostedGate::Denied(match identity {
            Identity::Anonymous => {
                RegistryError::Unauthenticated { resource: format!("package {package:?}") }
            }
            Identity::User { username, .. } => RegistryError::Forbidden {
                user: username.clone(),
                action: "access",
                resource: format!("package {package:?}"),
            },
        });
    }
    HostedGate::MaskNotFound
}

/// [`hosted_gate`] flattened to a `Result` for the readers: the org to read
/// from, or the response to answer with.
pub(super) fn hosted_read_namespace(
    state: &AppState,
    identity: &Identity,
    source: &str,
    package: &str,
) -> Result<String, RegistryError> {
    match hosted_gate(state, identity, source, package) {
        HostedGate::Allowed(org) => Ok(org),
        HostedGate::MaskNotFound => Err(RegistryError::NotFound),
        HostedGate::Denied(err) => Err(err),
    }
}

pub(super) async fn serve_hosted_packument(
    state: &AppState,
    identity: &Identity,
    headers: &HeaderMap,
    source: &str,
    name: &CanonicalPackageName,
    tarball_base: &str,
) -> Response {
    let org = match hosted_read_namespace(state, identity, source, name.as_str()) {
        Ok(org) => org,
        Err(err) => return err.into_response(),
    };
    // A hosted org has no upstream fallback: a package it does not host is a
    // definitive not-found. Reads come from the org's own storage namespace.
    match state.inner.storage.for_hosted(&org).read_hosted_document(name).await {
        Ok(Some(bytes)) => match packument_response(
            name,
            &bytes,
            tarball_base,
            None,
            state.inner.osv_index.as_ref(),
            wants_abbreviated(headers),
        ) {
            Ok(response) => response,
            Err(err) => err.into_response(),
        },
        Ok(None) => not_found(),
        Err(err) => err.into_response(),
    }
}

pub(super) async fn serve_hosted_tarball(
    state: &AppState,
    identity: &Identity,
    source: &str,
    name: &CanonicalPackageName,
    filename: &str,
) -> Response {
    let org = match hosted_read_namespace(state, identity, source, name.as_str()) {
        Ok(org) => org,
        Err(err) => return err.into_response(),
    };
    let (filename, name_version) = match name.parse_tarball_name(filename) {
        Ok(parsed) => parsed,
        Err(err) => return err.into_response(),
    };
    if let Err(err) = ensure_osv_allowed(state, name, &name_version) {
        return err.into_response();
    }
    match state.inner.storage.for_hosted(&org).open_hosted_blob(name, &filename).await {
        Ok(Some((body, len))) => tarball_response(body, len),
        Ok(None) => not_found(),
        Err(err) => {
            tracing::warn!(?err, package = %name.as_str(), %filename, "hosted tarball open failed");
            err.into_response()
        }
    }
}

pub(super) async fn serve_tarball(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    filename: &str,
) -> Response {
    let Some(target) = addressed_registry(state, registry, Ecosystem::Npm) else {
        return not_found();
    };
    let response = serve_registry_tarball(state, identity, &target, raw_name, filename).await;
    caller_scoped(state, Ecosystem::Npm, registry, Some(raw_name), response)
}
