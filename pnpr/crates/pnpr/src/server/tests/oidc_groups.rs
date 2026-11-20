use super::super::team_directory::{
    reload_teams,
    session_identity,
};
use pnpr_auth::{
    AuthState,
    oidc::SessionUser,
};
use pnpr_config::{
    Config,
    oidc::OidcTeamGrant,
};
use pnpr_fixtures::MockConfigPresets;
use std::net::{
    Ipv4Addr,
    SocketAddr,
    SocketAddrV4,
};

fn session(teams: &[&str]) -> SessionUser {
    let teams = teams
        .iter()
        .map(|team| OidcTeamGrant { group: "eng".to_string(), team_id: (*team).to_string() })
        .collect();
    SessionUser { user_id: "bob".to_string(), teams }
}

#[tokio::test]
async fn a_session_grant_joins_its_holder_to_teams_that_exist() {
    let directory = tempfile::tempdir().unwrap();
    let config = Config::static_serve(
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
        directory.path().to_path_buf(),
    );
    let (state, _router) =
        super::super::routing::state_and_router(config, AuthState::in_memory(), None).unwrap();
    let storage = &state.inner.storage;
    storage.create_team("platform-id", "platform").await.unwrap();
    storage.create_team("release-id", "release").await.unwrap();
    assert!(storage.add_team_member("release-id", "bob").await.unwrap());
    assert!(reload_teams(&state).await);

    let teams = |granted: &[&str]| {
        session_identity(&state, session(granted), "bob".to_string()).teams().to_vec()
    };
    assert_eq!(teams(&["platform-id"]), ["release-id", "platform-id"]);
    assert_eq!(teams(&["missing-id", "release-id"]), ["release-id"]);
    assert_eq!(teams(&[]), ["release-id"]);
}

#[tokio::test]
async fn retired_identities_without_passwords_are_refused() {
    let directory = tempfile::tempdir().unwrap();
    let config = Config::static_serve(
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
        directory.path().to_path_buf(),
    );
    let auth = AuthState::in_memory();
    let users = std::sync::Arc::clone(&auth.users);
    let id = users.provision_user("oidc-member").await.unwrap();
    let (state, _router) = super::super::routing::state_and_router(config, auth, None).unwrap();
    assert!(super::super::authentication::active_account_name(&state, &id).await.is_ok());
    users.delete_user(&id).await.unwrap();
    assert!(matches!(
        super::super::authentication::active_account_name(&state, &id).await,
        Err(pnpr_error::RegistryError::Unauthenticated { .. }),
    ));
}
