//! Tests for the per-caller access gate the install accelerator applies
//! before serving a package's files: a digest in the store is not a
//! bearer capability, so [`deny_unauthorized_packages`] checks every
//! served package against pnpr's own `packages:` policy.

use axum::http::StatusCode;

use super::{deny_unauthorized_packages, diff::PackageIndexEntry};
use crate::policy::{AccessList, Identity, PackagePolicies, PackagePolicy};

fn served(name: &str) -> Vec<PackageIndexEntry> {
    vec![PackageIndexEntry {
        integrity: "sha512-deadbeef".to_string(),
        pkg_id: format!("{name}@1.0.0"),
        raw: Vec::new(),
    }]
}

fn anonymous() -> Identity {
    Identity::Anonymous
}

fn user() -> Identity {
    Identity::User { username: "alice".to_string() }
}

/// `registry_mock_defaults` gates `@private/*` to `$authenticated`.
fn policies() -> PackagePolicies {
    PackagePolicies::registry_mock_defaults()
}

/// `@team/*` is restricted to the single user `alice`, so an authenticated
/// caller who isn't `alice` is forbidden rather than merely unauthenticated.
fn team_owned_by_alice() -> PackagePolicies {
    let team =
        PackagePolicy::new("@team/*", AccessList::parse("alice"), AccessList::parse("alice"))
            .expect("pattern compiles");
    let rest =
        PackagePolicy::new("**", AccessList::parse("$all"), AccessList::parse("$authenticated"))
            .expect("pattern compiles");
    PackagePolicies::new(vec![team, rest])
}

#[test]
fn anonymous_caller_is_denied_a_private_package() {
    let denied = deny_unauthorized_packages(&policies(), &anonymous(), &served("@private/foo"));
    assert_eq!(denied.map(|response| response.status()), Some(StatusCode::UNAUTHORIZED));
}

#[test]
fn authenticated_caller_is_allowed_a_private_package() {
    let denied = deny_unauthorized_packages(&policies(), &user(), &served("@private/foo"));
    assert!(denied.is_none());
}

#[test]
fn anonymous_caller_is_allowed_a_public_package() {
    let denied = deny_unauthorized_packages(&policies(), &anonymous(), &served("is-positive"));
    assert!(denied.is_none());
}

#[test]
fn authenticated_caller_outside_the_allowed_set_is_forbidden() {
    let bob = Identity::User { username: "bob".to_string() };
    let denied = deny_unauthorized_packages(&team_owned_by_alice(), &bob, &served("@team/foo"));
    assert_eq!(denied.map(|response| response.status()), Some(StatusCode::FORBIDDEN));
}

#[test]
fn authenticated_caller_in_the_allowed_set_is_allowed() {
    let denied = deny_unauthorized_packages(&team_owned_by_alice(), &user(), &served("@team/foo"));
    assert!(denied.is_none());
}
