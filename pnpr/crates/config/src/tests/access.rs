use super::{
    Config, Identity, Path, RegistryError, hosted_rules_config, hosted_rules_err, listen,
    load_with_graph, user,
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
fn registry_level_access_is_the_default_for_omitted_fields() {
    let yaml = "\
storage: ./s
registries:
  local:
    type: hosted
    access: team
    packages:
      '@team/*': {}
      '@team/open':
        access: $all
";
    let config = load_with_graph(yaml, Path::new("/x"), listen(), None).unwrap();
    let rules = &config.routing.hosted["local"].rules;
    // Omitted `access` falls back to the registry-level default...
    assert!(
        rules
            .snapshot()
            .for_package("@team/x")
            .access
            .allows(&user("team")),
    );
    assert!(
        !rules
            .snapshot()
            .for_package("@team/x")
            .access
            .allows(&user("carol")),
    );
    // ...while the more specific key overrides it.
    assert!(
        rules
            .snapshot()
            .for_package("@team/open")
            .access
            .allows(&Identity::Anonymous),
    );
}

#[test]
fn rule_usernames_grant_per_user_access() {
    // Bare names are usernames/groups, not a config error.
    let config = hosted_rules_config(
        "      '@team/*':\n        access: [alice, bob]\n        publish: alice\n",
    );
    let table = config.routing.hosted["local"].rules.snapshot();
    let team = table.for_package("@team/x");
    assert!(team.access.allows(&user("alice")));
    assert!(team.access.allows(&user("bob")));
    assert!(!team.access.allows(&user("carol")));
    assert!(!team.access.allows(&Identity::Anonymous));
    assert!(team.publish.allows(&user("alice")));
    assert!(!team.publish.allows(&user("bob")));
}

#[test]
fn teams_are_scoped_to_their_registry() {
    // `corp` cannot reference `local`'s team: an access list resolves only
    // against the owning registry's `teams:` map, so cross-registry reuse
    // is a loud config error (share a roster with a YAML anchor instead).
    let yaml = r"
registries:
  local:
    type: hosted
    teams:
      platform: { name: platform, members: [alice] }
    packages:
      '@team/*':
        access: team:platform
  corp:
    type: hosted
    namespace: corp
    packages:
      '@corp/*':
        access: team:platform
";
    let err = load_with_graph(yaml, Path::new("/x"), listen(), None).unwrap_err();
    assert!(
        matches!(
            &err,
            RegistryError::InvalidConfig { reason }
                if reason.contains(r#"registry "corp""#)
                    && reason.contains("does not declare")
                    && reason.contains("no `teams:`"),
        ),
        "unexpected error: {err}",
    );
}

#[test]
fn bare_token_matching_a_team_name_stays_a_username() {
    // A bare token is a username even when a team of the same name exists;
    // only the explicit `team:` form reaches the member set.
    let yaml = r"
registries:
  local:
    type: hosted
    teams:
      platform: { name: platform, members: [alice] }
    packages:
      '@team/*':
        access: platform
";
    let config = load_with_graph(yaml, Path::new("/x"), listen(), None).unwrap();
    let table = config.routing.hosted["local"].rules.snapshot();
    let access = table.for_package("@team/x").access;
    assert!(access.allows(&Identity::user("platform")));
    assert!(!access.allows(&Identity::user("alice")));
}

#[test]
fn undeclared_team_reference_names_the_declared_teams() {
    let yaml = r"
registries:
  local:
    type: hosted
    teams:
      platform: { name: platform, members: [alice] }
      release: { name: release, members: [carol] }
    packages:
      '@team/*':
        access: team:platfrm
";
    let err = load_with_graph(yaml, Path::new("/x"), listen(), None).unwrap_err();
    assert!(
        matches!(
            &err,
            RegistryError::InvalidConfig { reason }
                if reason.contains("team:platfrm")
                    && reason.contains(r#""platform", "release""#),
        ),
        "unexpected error: {err}",
    );
}

#[test]
fn rule_scalar_access_value_is_one_token() {
    let config = hosted_rules_config("      '@team/*':\n        access: alice\n");
    let table = config.routing.hosted["local"].rules.snapshot();
    let access = table.for_package("@team/x").access;
    assert!(access.allows(&user("alice")));
    assert!(!access.allows(&user("bob")));
}

#[test]
fn rule_space_separated_access_list_is_a_config_error() {
    // Verdaccio's space-separated form must not be silently misread as a
    // single token that admits nobody; the error points at the YAML
    // sequence spelling.
    for packages in [
        "      '@team/*':\n        access: alice bob\n",
        "      '@team/*':\n        access: [alice bob]\n",
    ] {
        let err = hosted_rules_err(packages);
        assert!(
            matches!(
                &err,
                RegistryError::InvalidConfig { reason }
                    if reason.contains(r#""alice bob""#) && reason.contains("[alice, bob]"),
            ),
            "unexpected error for {packages:?}: {err}",
        );
    }
}

#[test]
fn registry_level_access_list_is_validated_too() {
    let yaml = "\
storage: ./s
registries:
  local:
    type: hosted
    access: all
";
    let err = load_with_graph(yaml, Path::new("/x"), listen(), None).unwrap_err();
    assert!(
        matches!(
            &err,
            RegistryError::InvalidConfig { reason }
                if reason.contains(r#"registry "local""#) && reason.contains("$all"),
        ),
        "unexpected error: {err}",
    );
}

#[test]
fn team_declarations_are_validated() {
    // A team member list is one username per entry — never a
    // space-separated string.
    let split_members = "    teams:\n      platform: { name: platform, members: alice bob }\n";
    // A team name is spliced into `team:<name>` tokens, so a name the
    // grammar cannot express is rejected at declaration.
    let sigil_name = "    teams:\n      $all: { name: platform, members: [alice] }\n";
    let colon_name = "    teams:\n      'a:b': { name: platform, members: [alice] }\n";
    // A member is a plain username: a built-in group — in any spelling —
    // would silently become a user nobody is named after, and a `team:`
    // reference would be an unsupported nested team.
    let builtin_member = "    teams:\n      platform: { name: platform, members: [$all] }\n";
    let alias_member = "    teams:\n      platform: { name: platform, members: [authenticated] }\n";
    let at_alias_member = "    teams:\n      platform: { name: platform, members: ['@all'] }\n";
    let nested_team_member =
        "    teams:\n      platform: { name: platform, members: ['team:release'] }\n";
    for (teams, needle) in [
        (split_members, r#""alice bob""#),
        (sigil_name, "cannot contain `:` or start with `$`"),
        (colon_name, "cannot contain `:` or start with `$`"),
        (builtin_member, "built-in groups belong in the access lists"),
        (alias_member, "built-in groups belong in the access lists"),
        (at_alias_member, "built-in groups belong in the access lists"),
        (nested_team_member, "cannot include another team"),
    ] {
        let yaml = format!("storage: ./s\nregistries:\n  local:\n    type: hosted\n{teams}");
        let err = load_with_graph(&yaml, Path::new("/x"), listen(), None).unwrap_err();
        assert!(
            matches!(
                &err,
                RegistryError::InvalidConfig { reason } if reason.contains(needle),
            ),
            "unexpected error for {yaml:?}: {err}",
        );
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
fn unpublish_rule_rejected_on_cargo_and_pypi_but_accepted_on_oci() {
    for eco in ["cargo", "pypi"] {
        let yaml = format!(
            "storage: ./s\nregistries:\n  local:\n    type: hosted\n    ecosystem: {eco}\n    packages:\n      pkg:\n        unpublish: $authenticated\n",
        );
        let err = load_with_graph(&yaml, Path::new("/x"), listen(), None)
            .expect_err("unpublish is not supported on cargo/pypi");
        match err {
            RegistryError::InvalidConfig { reason } => {
                assert!(
                    reason.contains("does not support unpublishing"),
                    "unexpected error for {eco}: {reason}",
                );
            }
            other => panic!("expected InvalidConfig for {eco}, got {other:?}"),
        }
    }

    let oci_yaml = "storage: ./s\nregistries:\n  images:\n    type: hosted\n    ecosystem: oci\n    packages:\n      image:\n        unpublish: $authenticated\n";
    let oci_config = load_with_graph(oci_yaml, Path::new("/x"), listen(), None).unwrap();
    assert!(oci_config.routing.hosted.contains_key("images"));
}

#[test]
fn publish_defaults_to_owners_in_every_ecosystem() {
    for eco in ["npm", "cargo", "pypi", "oci"] {
        let yaml = format!(
            "storage: ./s\nregistries:\n  local:\n    type: hosted\n    ecosystem: {eco}\n    packages:\n      pkg: {{}}\n",
        );
        let config = load_with_graph(&yaml, Path::new("/x"), listen(), None).unwrap();
        let table = config.routing.hosted["local"].rules.snapshot();
        let publish = table.for_package("pkg").publish;
        assert!(publish.references_owners(), "{eco}");
        assert!(!publish.allows(&user("alice")), "{eco}");
    }
}

#[test]
fn owners_token_rejected_in_read_access_and_accepted_in_publish() {
    let bad_package_access = "\
storage: ./s
registries:
  local:
    type: hosted
    packages:
      '**':
        access: $owners
        publish: $authenticated
";
    let err = load_with_graph(bad_package_access, Path::new("/x"), listen(), None)
        .expect_err("$owners cannot be used for read access");
    assert!(err.to_string().contains("$owners"), "{err}");

    let bad_registry_access = "\
storage: ./s
registries:
  local:
    type: hosted
    access: $owners
";
    let err = load_with_graph(bad_registry_access, Path::new("/x"), listen(), None)
        .expect_err("$owners cannot be used for registry access");
    assert!(err.to_string().contains("$owners"), "{err}");

    let good_publish_unpublish = "\
storage: ./s
registries:
  local:
    type: hosted
    packages:
      '**':
        access: $all
        publish: $owners
        unpublish: $owners
";
    let config = load_with_graph(good_publish_unpublish, Path::new("/x"), listen(), None).unwrap();
    let snapshot = config.routing.hosted["local"].rules.snapshot();
    assert!(
        snapshot.rules()[0].publish
            .as_ref()
            .unwrap()
            .references_owners(),
    );
    assert!(
        snapshot.rules()[0].unpublish
            .as_ref()
            .unwrap()
            .references_owners(),
    );
}
