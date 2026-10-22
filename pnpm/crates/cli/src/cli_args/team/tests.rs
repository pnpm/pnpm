use super::{
    TeamContext, TeamError, parse_scope_team,
    registry::{TeamInfo, UserInfo, org_team_url, registry_for_scope, team_url, team_user_url},
    render::{render_members, render_teams},
};
use pnpm_config::Config;
use pnpm_network::normalize_registry_url;
use std::collections::HashMap;

#[test]
fn parse_scope_team_returns_scope_only() {
    let st = parse_scope_team("@myorg").expect("should parse");
    assert_eq!(st.scope, "myorg");
    assert!(st.team.is_none());
}

#[test]
fn parse_scope_team_returns_scope_and_team() {
    let st = parse_scope_team("@myorg:developers").expect("should parse");
    assert_eq!(st.scope, "myorg");
    assert_eq!(st.team.as_deref(), Some("developers"));
}

#[test]
fn parse_scope_team_rejects_non_at_spec() {
    let err = parse_scope_team("myorg").unwrap_err();
    assert!(matches!(err, TeamError::InvalidScope { .. }));
}

#[test]
fn parse_scope_team_handles_nested_scopes() {
    let st = parse_scope_team("@scope:sub:team").expect("should parse");
    assert_eq!(st.scope, "scope");
    assert_eq!(st.team.as_deref(), Some("sub:team"));
}

#[test]
fn parse_scope_team_rejects_empty_team_name() {
    let err = parse_scope_team("@myorg:").unwrap_err();
    assert!(matches!(err, TeamError::InvalidScope { .. }));
}

#[test]
fn parse_scope_team_rejects_empty_scope() {
    let err = parse_scope_team("@").unwrap_err();
    assert!(matches!(err, TeamError::InvalidScope { .. }));
}

#[test]
fn parse_scope_team_rejects_empty_scope_with_team() {
    let err = parse_scope_team("@:team").unwrap_err();
    assert!(matches!(err, TeamError::InvalidScope { .. }));
}

#[test]
fn render_teams_empty_returns_no_teams_message() {
    let result = render_teams("myorg", &[], false, false).expect("should render");
    assert_eq!(result, "@myorg has no teams");
}

#[test]
fn render_teams_returns_formatted_list() {
    let teams =
        [TeamInfo { name: "developers".to_string() }, TeamInfo { name: "admins".to_string() }];
    let result = render_teams("myorg", &teams, false, false).expect("should render");
    assert_eq!(result, "@myorg has the following teams:\n  @myorg:developers\n  @myorg:admins");
}

#[test]
fn render_teams_parseable_format() {
    let teams =
        [TeamInfo { name: "developers".to_string() }, TeamInfo { name: "admins".to_string() }];
    let result = render_teams("myorg", &teams, true, false).expect("should render");
    assert_eq!(result, "developers\nadmins");
}

#[test]
fn render_teams_json_format() {
    let teams =
        [TeamInfo { name: "developers".to_string() }, TeamInfo { name: "admins".to_string() }];
    let result = render_teams("myorg", &teams, false, true).expect("should render");
    assert_eq!(result, "[\n  \"developers\",\n  \"admins\"\n]");
}

#[test]
fn render_members_empty_returns_no_members_message() {
    let result = render_members("myorg", "team1", &[], false, false).expect("should render");
    assert_eq!(result, "@myorg:team1 has no members");
}

#[test]
fn render_members_returns_formatted_list() {
    let members = [UserInfo { name: "alice".to_string() }, UserInfo { name: "bob".to_string() }];
    let result = render_members("myorg", "team1", &members, false, false).expect("should render");
    assert_eq!(result, "@myorg:team1 has the following members:\n  alice\n  bob");
}

#[test]
fn render_members_parseable_format() {
    let members = [UserInfo { name: "alice".to_string() }, UserInfo { name: "bob".to_string() }];
    let result = render_members("myorg", "team1", &members, true, false).expect("should render");
    assert_eq!(result, "alice\nbob");
}

#[test]
fn render_members_json_format() {
    let members = [UserInfo { name: "alice".to_string() }, UserInfo { name: "bob".to_string() }];
    let result = render_members("myorg", "team1", &members, false, true).expect("should render");
    assert_eq!(result, "[\n  \"alice\",\n  \"bob\"\n]");
}

#[test]
fn org_team_url_constructs_correctly() {
    let url = org_team_url("https://registry.example.com/", "myorg");
    assert_eq!(url, "https://registry.example.com/-/org/myorg/team");
}

#[test]
fn org_team_url_handles_registry_without_trailing_slash() {
    let url = org_team_url("https://registry.example.com", "myorg");
    assert_eq!(url, "https://registry.example.com/-/org/myorg/team");
}

#[test]
fn org_team_url_encodes_scopes() {
    let url = org_team_url("https://registry.example.com/", "@myorg");
    assert_eq!(url, "https://registry.example.com/-/org/%40myorg/team");
}

#[test]
fn team_user_url_constructs_correctly() {
    let url = team_user_url("https://registry.example.com/", "myorg", "developers");
    assert_eq!(url, "https://registry.example.com/-/team/myorg/developers/user");
}

#[test]
fn team_url_constructs_correctly() {
    let url = team_url("https://registry.example.com/", "myorg", "developers");
    assert_eq!(url, "https://registry.example.com/-/team/myorg/developers");
}

#[test]
fn team_endpoints_preserve_path_prefix() {
    assert_eq!(
        org_team_url("https://registry.example.com/npm/", "@myorg"),
        "https://registry.example.com/npm/-/org/%40myorg/team",
    );
    assert_eq!(
        team_url("https://registry.example.com/npm/", "myorg", "developers"),
        "https://registry.example.com/npm/-/team/myorg/developers",
    );
    assert_eq!(
        team_user_url("https://registry.example.com/npm/", "myorg", "developers"),
        "https://registry.example.com/npm/-/team/myorg/developers/user",
    );
}

#[test]
fn normalize_registry_url_adds_trailing_slash() {
    assert_eq!(
        normalize_registry_url("https://registry.example.com"),
        "https://registry.example.com/",
    );
}

#[test]
fn normalize_registry_url_preserves_trailing_slash() {
    assert_eq!(
        normalize_registry_url("https://registry.example.com/"),
        "https://registry.example.com/",
    );
}

#[test]
fn team_error_display_and_code() {
    let err = TeamError::Unauthorized {
        action: "create team".to_string(),
        body: "unauthorized".to_string(),
    };
    let report: miette::Report = err.into();
    let formatted = format!("{report:?}");
    assert!(formatted.contains("ERR_PNPM_UNAUTHORIZED"));
}

#[test]
fn registry_for_scope_uses_scoped_registry_by_default() {
    let config = Config::default();
    let registries = HashMap::from([
        ("default".to_string(), "https://registry.example/npm/".to_string()),
        ("@myorg".to_string(), "https://scoped.example/npm/".to_string()),
    ]);
    let context = TeamContext {
        config: &config,
        http_client: pnpm_network::ThrottledClient::default(),
        retry_opts: pnpm_network::RetryOpts::default(),
        registries,
        registry_override: None,
        otp: None,
        parseable: false,
        json: false,
    };
    let registry = registry_for_scope(&context, "myorg");
    assert_eq!(registry, "https://scoped.example/npm/");
}

#[test]
fn registry_for_scope_honors_registry_override() {
    let config = Config::default();
    let registries = HashMap::from([
        ("default".to_string(), "https://registry.example/npm/".to_string()),
        ("@myorg".to_string(), "https://scoped.example/npm/".to_string()),
    ]);
    let context = TeamContext {
        config: &config,
        http_client: pnpm_network::ThrottledClient::default(),
        retry_opts: pnpm_network::RetryOpts::default(),
        registries,
        registry_override: Some("https://override.example/npm/"),
        otp: None,
        parseable: false,
        json: false,
    };
    let registry = registry_for_scope(&context, "myorg");
    assert_eq!(registry, "https://override.example/npm/");
}
