use super::super::oidc_groups::{session_identity, validate_group_grants};
use pnpr_auth::{AuthState, oidc::SessionUser};
use pnpr_config::{Config, oidc::OidcTeamGrant};
use pnpr_fixtures::MockConfigPresets;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

const PROVIDER: &str = "\
auth:
  oidc:
    - name: company
      issuer: https://issuer.example
      audience: pnpr
      login:
        users:
          - subject: '1'
            userId: bob
        groups:
          teams:
            - group: eng
              registry: local
              teamId: platform
";

fn config(registries: &str) -> Config {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("config.yaml");
    std::fs::write(&path, format!("storage: ./s\n{PROVIDER}registries:\n{registries}")).unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));
    Config::from_yaml(&path, listen, None).unwrap()
}

const LOCAL: &str = "  local:
    type: hosted
    teams:
      platform: { name: platform, members: [] }
    packages:
      '@secret/*':
        access: [team:platform]
      '**': {}
";

fn session(teams: Vec<(&str, &str)>) -> SessionUser {
    let teams = teams
        .into_iter()
        .map(|(registry, team)| OidcTeamGrant {
            group: "eng".to_string(),
            registry: registry.to_string(),
            team_id: team.to_string(),
        })
        .collect();
    SessionUser { user_id: "bob".to_string(), teams }
}

#[test]
fn a_session_grant_admits_its_holder_to_team_rules() {
    let config = config(LOCAL);
    let rules = &config.routing.hosted["local"].rules;

    let member =
        session_identity(&config, session(vec![("local", "platform")]), "alice".to_string());
    assert!(rules.access_admits("@secret/x", &member));
    let other =
        session_identity(&config, session(vec![("missing", "platform")]), "alice".to_string());
    assert!(!rules.access_admits("@secret/x", &other));
    assert!(!rules.access_admits(
        "@secret/x",
        &session_identity(&config, session(Vec::new()), "alice".to_string())
    ));
}

#[test]
fn group_grants_must_name_a_hosted_registry_and_a_declared_team() {
    validate_group_grants(&config(LOCAL)).unwrap();

    let undeclared = LOCAL
        .replace(
            "platform: { name: platform, members: [] }",
            "release: { name: release, members: [] }",
        )
        .replace("team:platform", "$authenticated");
    let err = validate_group_grants(&config(&undeclared)).unwrap_err();
    assert!(err.to_string().contains("does not declare the team"), "{err}");

    let api_managed =
        undeclared.replace("type: hosted\n", "type: hosted\n    teamsManagedBy: api\n");
    validate_group_grants(&config(&api_managed)).unwrap();

    let renamed = LOCAL.replace("  local:", "  other:");
    let err = validate_group_grants(&config(&renamed)).unwrap_err();
    assert!(err.to_string().contains("is not a hosted registry"), "{err}");
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
    assert!(super::super::authentication::require_active_account(&state, &id).await.is_ok());
    users.delete_user(&id).await.unwrap();
    assert!(matches!(
        super::super::authentication::require_active_account(&state, &id).await,
        Err(pnpr_error::RegistryError::Unauthenticated { .. }),
    ));
}
