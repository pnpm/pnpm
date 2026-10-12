use super::super::oidc_groups::{session_identity, validate_group_grants};
use pnpr_auth::oidc::SessionUser;
use pnpr_config::{Config, oidc::OidcTeamGrant};
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
            username: bob
        groups:
          teams:
            - group: eng
              registry: local
              team: platform
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
      platform: []
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
            team: team.to_string(),
        })
        .collect();
    SessionUser { username: "bob".to_string(), teams }
}

#[test]
fn a_session_grant_admits_its_holder_to_team_rules() {
    let config = config(LOCAL);
    let rules = &config.routing.hosted["local"].rules;

    let member = session_identity(&config, session(vec![("local", "platform")]));
    assert!(rules.access_admits("@secret/x", &member));
    let other = session_identity(&config, session(vec![("missing", "platform")]));
    assert!(!rules.access_admits("@secret/x", &other));
    assert!(!rules.access_admits("@secret/x", &session_identity(&config, session(Vec::new()))));
}

#[test]
fn group_grants_must_name_a_hosted_registry_and_a_declared_team() {
    validate_group_grants(&config(LOCAL)).unwrap();

    let undeclared =
        LOCAL.replace("platform: []", "release: []").replace("team:platform", "$authenticated");
    let err = validate_group_grants(&config(&undeclared)).unwrap_err();
    assert!(err.to_string().contains("does not declare the team"), "{err}");

    let api_managed =
        undeclared.replace("type: hosted\n", "type: hosted\n    teamsManagedBy: api\n");
    validate_group_grants(&config(&api_managed)).unwrap();

    let renamed = LOCAL.replace("  local:", "  other:");
    let err = validate_group_grants(&config(&renamed)).unwrap_err();
    assert!(err.to_string().contains("is not a hosted registry"), "{err}");
}
