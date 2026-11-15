//! Per-package authorization against the `packages:` rules of the registry
//! a request resolved to.

use std::sync::LazyLock;

use pnpr_error::RegistryError;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use pnpr_policy::{AccessList, Identity, PackageRules, RuleTable};

use super::{Action, AppState, RegistrySource};

/// The `packages:` rules of the concrete registry a request resolved to.
/// Authorization is entirely registry-scoped — there is no global,
/// name-keyed ACL — so every check consults the one registry that serves
/// the package. The fallback (safe defaults: reads open, publishes need
/// ownership, destructive writes denied) only fires for a programmatically
/// built config whose serving tables miss the graph entry.
fn source_rules(state: &AppState, source: &RegistrySource) -> PackageRules {
    static SAFE_DEFAULTS: LazyLock<PackageRules> = LazyLock::new(PackageRules::default);
    let routing = state.routing();
    match source {
        RegistrySource::Hosted(name) => routing.hosted.get(name).map(|hosted| &hosted.rules),
        RegistrySource::Upstream(name) => {
            routing.upstreams.get(name).map(|upstream| &upstream.rules)
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => None,
    }
    .unwrap_or(&SAFE_DEFAULTS)
    .clone()
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

/// [`authorize`] for a request that continues work an owner-checked request
/// started, such as a later request of an upload session. `$owners` admits
/// any signed-in caller here, so the check reads no document.
pub(in crate::server) fn authorize_continuation(
    state: &AppState,
    identity: &Identity,
    source: &RegistrySource,
    package: &str,
    action: Action,
) -> Result<(), RegistryError> {
    let rules = source_rules(state, source).snapshot();
    let list = permission_list(&rules, package, action);
    if list.allows_with_context(identity, identity.is_authenticated()) {
        return Ok(());
    }
    Err(denial(identity, package, action))
}

/// Who owns a package, as its document records it.
pub(in crate::server) enum Ownership<'owners> {
    /// No document: a publish claims the name.
    Unclaimed,
    /// The account IDs the stored document records.
    Owners(&'owners [String]),
}

/// [`authorize_package`] decided against `ownership`, read from the package's
/// document under the package lock, so the decision holds for the write that
/// lock guards.
pub(in crate::server) fn authorize_with_ownership(
    state: &AppState,
    identity: &Identity,
    source: &RegistrySource,
    (package, ownership): (&str, &Ownership<'_>),
    action: Action,
) -> Result<(), RegistryError> {
    let rules = source_rules(state, source).snapshot();
    if allows_with_ownership(state, identity, &rules, (package, ownership), action) {
        return Ok(());
    }
    Err(denial(identity, package, action))
}

/// Whether `rules` allow `identity` to perform `action` on `package` given
/// `ownership`.
pub(in crate::server) fn allows_with_ownership(
    state: &AppState,
    identity: &Identity,
    rules: &RuleTable,
    (package, ownership): (&str, &Ownership<'_>),
    action: Action,
) -> bool {
    let is_owner = owns(state, identity, ownership, action);
    permission_list(rules, package, action).allows_with_context(identity, is_owner)
}

/// [`authorize_with_ownership`] against the maintainers an npm `packument`
/// records.
pub(in crate::server) fn authorize_against_packument(
    state: &AppState,
    identity: &Identity,
    source: &RegistrySource,
    (package, packument): (&str, &serde_json::Value),
    action: Action,
) -> Result<(), RegistryError> {
    let owners = packument_owners(packument);
    let ownership = Ownership::Owners(&owners);
    authorize_with_ownership(state, identity, source, (package, &ownership), action)
}

/// Whether `identity` counts as an owner for `action`: an admin, an account
/// the document records, or any caller claiming an unclaimed name.
fn owns(state: &AppState, identity: &Identity, ownership: &Ownership<'_>, action: Action) -> bool {
    let Some(user) = identity.user_id() else {
        return false;
    };
    if is_admin(state, identity) {
        return true;
    }
    match ownership {
        Ownership::Unclaimed => matches!(action, Action::Publish),
        Ownership::Owners(owners) => owners.iter().any(|owner| owner == user),
    }
}

fn is_admin(state: &AppState, identity: &Identity) -> bool {
    identity
        .user_id()
        .is_some_and(|user| state.inner.config.identity.admins.contains(user))
}

/// The maintainer account IDs an npm `packument` records.
fn packument_owners(packument: &serde_json::Value) -> Vec<String> {
    packument
        .get("maintainers")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("userId").and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .collect()
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

/// Read the recorded owners of `package` from the primary metadata store,
/// never a replica's copy that may predate an owner's removal.
async fn read_ownership(
    state: &AppState,
    (package, ecosystem): (&str, Ecosystem),
    namespace: Option<&str>,
) -> Result<Option<Vec<String>>, RegistryError> {
    let canonical = CanonicalPackageName::from_stored(package, ecosystem);
    let storage = state.inner.storage.for_hosted(namespace.unwrap_or(""), ecosystem);
    storage.read_package_owners(&canonical).await
}

/// Whether deciding `action` for `identity` on `package` under `rules` requires
/// reading `package`'s recorded owners.
pub(in crate::server) fn needs_package_owners(
    state: &AppState,
    identity: &Identity,
    rules: &RuleTable,
    package: &str,
    action: Action,
) -> bool {
    if !identity.is_authenticated() || is_admin(state, identity) {
        return false;
    }
    let list = permission_list(rules, package, action);
    !list.allows(identity) && list.references_owners()
}

/// Outcome of [`authorize_without_owners`].
pub(in crate::server) enum AuthorizationDecision {
    /// Allowed by the rule (or admin role) without reading stored owners.
    AllowedWithoutOwners,
    /// The rule references `$owners` and needs the stored document's owners.
    NeedsOwners,
}

/// Authorize `identity` for `action` on `package` when the rule can decide
/// without reading the package's stored owners.
pub(in crate::server) fn authorize_without_owners(
    state: &AppState,
    identity: &Identity,
    source: &RegistrySource,
    package: &str,
    action: Action,
) -> Result<AuthorizationDecision, RegistryError> {
    let rules = source_rules(state, source).snapshot();
    let list = permission_list(&rules, package, action);
    if list.allows(identity) {
        return Ok(AuthorizationDecision::AllowedWithoutOwners);
    }
    if list.references_owners()
        && identity.is_authenticated()
        && list.allows_with_context(identity, true)
    {
        if is_admin(state, identity) {
            return Ok(AuthorizationDecision::AllowedWithoutOwners);
        }
        return Ok(AuthorizationDecision::NeedsOwners);
    }
    Err(denial(identity, package, action))
}

pub(in crate::server) async fn authorize_package(
    state: &AppState,
    identity: &Identity,
    source: &RegistrySource,
    package: &str,
    action: Action,
    namespace: Option<&str>,
) -> Result<(), RegistryError> {
    match authorize_without_owners(state, identity, source, package, action)? {
        AuthorizationDecision::AllowedWithoutOwners => return Ok(()),
        AuthorizationDecision::NeedsOwners => {}
    }
    if is_package_owner(state, identity, (source, package), namespace, action).await? {
        return Ok(());
    }
    Err(denial(identity, package, action))
}

async fn is_package_owner(
    state: &AppState,
    identity: &Identity,
    (source, package): (&RegistrySource, &str),
    namespace: Option<&str>,
    action: Action,
) -> Result<bool, RegistryError> {
    if is_admin(state, identity) {
        return Ok(true);
    }
    let ecosystem = match source {
        RegistrySource::Hosted(name) => state.routing().registries.ecosystem(name),
        _ => None,
    };
    let ecosystem = ecosystem.unwrap_or(Ecosystem::Npm);
    let owners = read_ownership(state, (package, ecosystem), namespace).await?;
    let ownership = owners.as_deref().map_or(Ownership::Unclaimed, Ownership::Owners);
    Ok(owns(state, identity, &ownership, action))
}
