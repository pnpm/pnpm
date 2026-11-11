//! Two accounts admitted to first-publish the same name before either
//! commits: the one that stages second meets the owner the first recorded.

use super::fence::{images, manifest, push_as, put_blob};
use crate::server::{AppState, publishing::commit_publishes, routing::state_and_router};
use pnpr_auth::AuthState;
use pnpr_error::RegistryError;
use pnpr_fixtures::MockConfigPresets;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use pnpr_policy::PackageRules;
use tempfile::TempDir;

/// The static fixture registry with its rules reset to the defaults, so
/// `publish` is `$owners`.
fn state(temp: &TempDir) -> AppState {
    let mut config =
        crate::Config::static_serve("127.0.0.1:0".parse().unwrap(), temp.path().join("store"));
    config.routing.hosted.get_mut("local").unwrap().rules = PackageRules::new(Vec::new(), None);
    state_and_router(config, AuthState::in_memory(), None).unwrap().0
}

#[tokio::test]
async fn a_competing_first_push_is_refused_once_another_account_claims_the_repository() {
    let temp = TempDir::new().unwrap();
    let state = state(&temp);
    let repository = CanonicalPackageName::parse("acme/app", Ecosystem::Oci).unwrap();
    let layer = put_blob(&images(&state), &repository, b"layer").await;
    let alice = push_as(&repository, manifest(&layer), "alice");
    let bob = push_as(&repository, manifest(&layer), "bob");

    let staged = alice
        .stage(&state)
        .await
        .map_err(|_| "alice stages")
        .unwrap();
    commit_publishes(&state, vec![staged]).await.unwrap();

    let refused = bob
        .stage(&state)
        .await
        .err()
        .expect("bob does not own the repository");
    let refused = RegistryError::from(refused);
    assert!(matches!(refused, RegistryError::Forbidden { .. }), "{refused:?}");
}
