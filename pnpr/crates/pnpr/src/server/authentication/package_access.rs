//! Per-package authorization against the `packages:` rules of the registry
//! a request resolved to.

use std::sync::LazyLock;

use pnpr_error::RegistryError;
use pnpr_policy::{AccessList, Identity, PackageRules, RuleTable};

use super::{Action, AppState, RegistrySource};

/// The `packages:` rules of the concrete registry a request resolved to.
/// Authorization is entirely registry-scoped — there is no global,
/// name-keyed ACL — so every check consults the one registry that serves
/// the package. The fallback (safe defaults: reads open, publishes need
/// ownership, destructive writes denied) only fires for a programmatically
/// built config whose serving tables miss the graph entry.
fn source_rules<'a>(state: &'a AppState, source: &RegistrySource) -> &'a PackageRules {
    static SAFE_DEFAULTS: LazyLock<PackageRules> = LazyLock::new(PackageRules::default);
    match source {
        RegistrySource::Hosted(name) => {
            state.inner.config.routing.hosted.get(name).map(|hosted| &hosted.rules)
        }
        RegistrySource::Upstream(name) => {
            state.inner.config.routing.upstreams.get(name).map(|upstream| &upstream.rules)
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => None,
    }
    .unwrap_or(&SAFE_DEFAULTS)
}

/// Check an already-resolved `identity` against the resolved source
/// registry's per-package rule (the most specific `packages:` entry, its
/// omitted fields falling back to the registry defaults). Returns `Ok(())`
/// when the call is allowed; otherwise the appropriate `Unauthenticated` /
/// `Forbidden` error. The identity is resolved once by [`authenticate`](super::authenticate), so
/// every handler — including the search endpoint that filters many packages —
/// authorizes synchronously against it.
pub(in crate::server) fn authorize(
    state: &AppState,
    identity: &Identity,
    source: &RegistrySource,
    package: &str,
    action: Action,
) -> Result<(), RegistryError> {
    let rules = source_rules(state, source).snapshot();
    if permission_list(&rules, package, action).allows(identity) {
        return Ok(());
    }
    Err(denial(identity, package, action))
}

/// [`authorize_package`] decided against `packument`, the package's document
/// as read under the package lock, so the decision holds for the write that
/// lock guards.
pub(in crate::server) fn authorize_against_packument(
    state: &AppState,
    identity: &Identity,
    source: &RegistrySource,
    (package, packument): (&str, &serde_json::Value),
    action: Action,
) -> Result<(), RegistryError> {
    let rules = source_rules(state, source).snapshot();
    let is_owner = identity
        .user_id()
        .is_some_and(|user| {
            state.inner.config.identity.admins.contains(user)
                || maintainers_include(packument, user)
        });
    if permission_list(&rules, package, action).allows_with_context(identity, is_owner) {
        return Ok(());
    }
    Err(denial(identity, package, action))
}

fn permission_list<'rules>(
    rules: &'rules RuleTable,
    package: &str,
    action: Action,
) -> &'rules AccessList {
    let effective = rules.for_package(package);
    match action {
        Action::Access => effective.access,
        Action::Publish => effective.publish,
        Action::Unpublish => effective.unpublish,
    }
}

/// An anonymous caller gets a chance to authenticate (401); an authenticated
/// caller isn't in the allowed set (403).
fn denial(identity: &Identity, package: &str, action: Action) -> RegistryError {
    match identity {
        Identity::User { username, .. } => RegistryError::Forbidden {
            user: username.clone(),
            action: action.label(),
            resource: format!("package {package:?}"),
        },
        _ => RegistryError::Unauthenticated { resource: format!("package {package:?}") },
    }
}

fn maintainers_include(packument: &serde_json::Value, user_id: &str) -> bool {
    packument
        .get("maintainers")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|maintainers| {
            maintainers
                .iter()
                .any(|entry| {
                    entry.get("userId").and_then(serde_json::Value::as_str) == Some(user_id)
                })
        })
}

async fn is_package_owner(
    state: &AppState,
    identity: &Identity,
    package: &str,
    org: Option<&str>,
    action: Action,
) -> Result<bool, RegistryError> {
    let Some(username) = identity.user_id() else {
        return Ok(false);
    };
    if state.inner.config.identity.admins.contains(username) {
        return Ok(true);
    }
    let Ok(canonical) =
        pnpr_package_name::CanonicalPackageName::parse(package, pnpr_package_name::Ecosystem::Npm)
    else {
        return Ok(false);
    };
    let storage = crate::server::hosted_storage(state, org);
    // An authorization check reads the current document, never a replica's
    // copy that may predate a maintainer's removal.
    if let Some(stored) = storage.read_hosted_document_for_update(&canonical).await? {
        return Ok(serde_json::from_slice::<serde_json::Value>(&stored.bytes)
            .is_ok_and(|packument| maintainers_include(&packument, username)));
    }
    if let Some(tombstone) = storage.read_tombstone(&canonical).await? {
        return Ok(tombstone.maintainers
            .iter()
            .any(|maintainer| maintainer == username));
    }
    // Brand new package: initial publish claims the package; cannot unpublish nonexistent package
    Ok(matches!(action, Action::Publish))
}

pub(in crate::server) async fn authorize_package(
    state: &AppState,
    identity: &Identity,
    source: &RegistrySource,
    package: &str,
    action: Action,
    org: Option<&str>,
) -> Result<(), RegistryError> {
    let rules = source_rules(state, source).snapshot();
    let list = permission_list(&rules, package, action);
    if list.allows(identity) {
        return Ok(());
    }
    if list.references_owners() && identity.is_authenticated() {
        let is_owner = is_package_owner(state, identity, package, org, action).await?;
        if list.allows_with_context(identity, is_owner) {
            return Ok(());
        }
    }
    Err(denial(identity, package, action))
}
