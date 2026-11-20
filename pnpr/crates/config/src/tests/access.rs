use super::{
    Config,
    Identity,
    Path,
    RegistryError,
    listen,
    load_with_graph,
    user,
};

/// A hosted `namespace` becomes a storage path segment, so a traversal-y value —
/// including a Windows drive-relative prefix, which `PathBuf::join` treats as
/// a new path rather than a child — must be rejected at load rather than
/// reach the filesystem.
#[test]
fn from_yaml_str_rejects_hosted_namespace_path_traversal() {
    for namespace in ["../../etc", "C:acme", "a/b"] {
        let yaml = format!(
            "storage: ./s\nregistries:\n  evil:\n    type: hosted\n    namespace: {namespace}\n",
        );
        let err = load_with_graph(&yaml, Path::new("/x"), listen(), None)
            .expect_err("a traversal-y hosted namespace must be rejected");
        assert!(err.to_string().contains("path-safe"), "unexpected error for {namespace:?}: {err}");
    }
}

#[test]
fn auth_account_and_token_files_are_not_config_keys() {
    for yaml in
        ["auth:\n  users:\n    file: ./users.json\n", "auth:\n  tokens:\n    file: ./tokens.db\n"]
    {
        let error = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None)
            .expect_err("accounts and tokens live in the database");
        assert!(matches!(error, RegistryError::InvalidConfig { .. }), "{error:?}");
    }
}

#[test]
fn auth_block_absent_disables_registration_by_default() {
    let yaml = "storage: ./s\n";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    // Registration is opt-in: an omitted cap denies new sign-ups.
    assert_eq!(config.identity.auth.users.max_users, super::super::MaxUsers::Disabled);
}

#[test]
fn auth_max_users_absent_disables_registration() {
    let yaml = "\
storage: ./s
auth:
  users:
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(config.identity.auth.users.max_users, super::super::MaxUsers::Disabled);
}

#[test]
fn auth_max_users_negative_one_means_disabled() {
    let yaml = "\
storage: ./s
auth:
  users:
    maxUsers: -1
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(config.identity.auth.users.max_users, super::super::MaxUsers::Disabled);
}

#[test]
fn auth_max_users_positive_is_a_hard_cap() {
    let yaml = "\
storage: ./s
auth:
  users:
    maxUsers: 5
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(config.identity.auth.users.max_users, super::super::MaxUsers::Limited(5));
}

#[test]
fn auth_token_lifetimes_are_intervals() {
    use std::time::Duration;
    let config_with = |key: &str, lifetime: &str| {
        let yaml = format!("storage: ./s\nauth:\n  tokens:\n    {key}: {lifetime}\n");
        Config::from_yaml_str(&yaml, Path::new("/x"), listen(), None)
    };
    let tokens = config_with("maxLifetime", "90d").unwrap().identity.auth.tokens;
    assert_eq!(tokens.max_lifetime, Some(Duration::from_hours(90 * 24)));
    let tokens = config_with("maxLifetime", "1m").unwrap().identity.auth.tokens;
    assert_eq!(tokens.max_lifetime, Some(Duration::from_mins(1)));
    let tokens = config_with("defaultLifetime", "1w").unwrap().identity.auth.tokens;
    assert_eq!(tokens.default_lifetime, Duration::from_hours(7 * 24));
    for key in ["defaultLifetime", "maxLifetime"] {
        for invalid in ["0", "0d", "0.0004s", "59s", "soon"] {
            let error = config_with(key, invalid)
                .map(|_| ())
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains(&format!("auth.tokens.{key}")),
                "{invalid}: {error}",
            );
        }
    }
    let config = Config::from_yaml_str("storage: ./s\n", Path::new("/x"), listen(), None).unwrap();
    assert_eq!(config.identity.auth.tokens.default_lifetime, Duration::from_hours(90 * 24));
    assert_eq!(config.identity.auth.tokens.max_lifetime, None);
}

#[test]
fn team_and_member_names_are_validated() {
    for name in ["$all", "a:b", "two words", ""] {
        assert!(crate::validate_team_name(name).is_err(), "accepted the team name {name:?}");
    }
    crate::validate_team_name("platform").unwrap();
    // A member is one account: a built-in group in any spelling would be an
    // account nobody has, and a `team:` reference a nested team.
    for (member, needle) in [
        ("alice bob", r#""alice bob""#),
        ("$all", "built-in groups belong in the access lists"),
        ("authenticated", "built-in groups belong in the access lists"),
        ("@all", "built-in groups belong in the access lists"),
        ("team:release", "cannot include another team"),
    ] {
        let reason = crate::validate_member_name(member).unwrap_err();
        assert!(reason.contains(needle), "unexpected error for {member:?}: {reason}");
    }
}

#[test]
fn auth_admins_are_usernames() {
    let yaml = "storage: ./s\nauth:\n  admins: [alice]\n";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert!(config.identity.is_admin(&user("alice")));
    assert!(!config.identity.is_admin(&user("bob")));
    assert!(!config.identity.is_admin(&Identity::Anonymous));

    let yaml = "storage: ./s\nauth:\n  admins: [$authenticated]\n";
    let err = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None)
        .expect_err("a built-in group is not an admin");
    assert!(err.to_string().contains("auth.admins"), "{err}");
}

#[test]
fn is_reserved_name_identifies_admins_oidc_and_workload_users() {
    let yaml = "\
storage: ./s
auth:
  admins: [admin_alice]
  oidc:
    - name: provider1
      issuer: https://issuer.example.com
      audience: pnpr
      login:
        users:
          - subject: sub-123
            userId: oidc_bob
      workloads:
        - identity:
            subject: repo:pnpm/pnpm
            userId: workload_carol
          registry: local
          packages: ['@pnpm/*']
registries:
  local:
    type: hosted
";
    let config = load_with_graph(yaml, Path::new("/x"), listen(), None).unwrap();
    assert!(config.identity.is_admin(&pnpr_policy::Identity::account(
        "admin_alice",
        "renamed-admin"
    )));
    assert!(!config.identity.is_admin(&pnpr_policy::Identity::account(
        "different-id",
        "admin_alice"
    )));
}

#[test]
fn removed_managed_by_keys_are_unknown_fields() {
    for key in ["teamsManagedBy", "rulesManagedBy"] {
        let yaml =
            format!("storage: ./s\nregistries:\n  local:\n    type: hosted\n    {key}: api\n");
        let err = load_with_graph(&yaml, Path::new("/x"), listen(), None)
            .expect_err("the overlay switches were removed");
        match err {
            RegistryError::InvalidConfig { reason } => {
                assert!(reason.contains("unknown field"), "unexpected error for {key}: {reason}");
                assert!(reason.contains(key), "unexpected error for {key}: {reason}");
            }
            other => panic!("expected InvalidConfig for {key}, got {other:?}"),
        }
    }
}

#[test]
fn access_tokens_follow_one_grammar() {
    for token in ["$all", "$authenticated", "$anonymous", "$owners", "alice", "team:platform"] {
        crate::validate_access_token(token).unwrap();
    }
    for (token, needle) in [
        ("$everyone", "unknown built-in access token"),
        ("all", r#"did you mean "$all""#),
        ("@authenticated", r#"did you mean "$authenticated""#),
        ("group:platform", r#"did you mean "team:platform""#),
        ("org:corp", "the only typed token is `team:<id>`"),
        ("team:", "names no team"),
        ("team:a:b", "a team name cannot contain `:`"),
        ("alice bob", "contains whitespace"),
        ("", "an empty string is not a token"),
    ] {
        let reason = crate::validate_access_token(token).unwrap_err();
        assert!(reason.contains(needle), "unexpected error for {token:?}: {reason}");
    }
}

#[test]
fn a_registry_declares_no_access_lists() {
    for registry in [
        "type: hosted\n    access: $all",
        "type: hosted\n    packages:\n      '@acme/*': { publish: $authenticated }",
        "type: upstream\n    url: https://registry.example/\n    access: $authenticated",
    ] {
        let yaml = format!("registries:\n  example:\n    {registry}\n");
        let error = load_with_graph(&yaml, Path::new("/x"), listen(), None).unwrap_err();
        assert!(error.to_string().contains("unknown field"), "{registry:?}: {error}");
    }
}
