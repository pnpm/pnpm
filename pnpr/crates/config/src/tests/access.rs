use super::{
    Config, Identity, Path, RegistryError, hosted_rules_config, hosted_rules_err, listen, user,
};

/// A hosted `org` becomes a storage path segment, so a traversal-y value —
/// including a Windows drive-relative prefix, which `PathBuf::join` treats as
/// a new path rather than a child — must be rejected at load rather than
/// reach the filesystem.
#[test]
fn from_yaml_str_rejects_hosted_org_path_traversal() {
    for org in ["../../etc", "C:acme", "a/b"] {
        let yaml =
            format!("storage: ./s\nregistries:\n  evil:\n    type: hosted\n    org: {org}\n");
        let err = Config::from_yaml_str(&yaml, Path::new("/x"), listen(), None)
            .expect_err("a traversal-y hosted org must be rejected");
        assert!(err.to_string().contains("path-safe"), "unexpected error for {org:?}: {err}");
    }
}

#[test]
fn auth_block_resolves_htpasswd_relative_to_config_dir() {
    let yaml = "\
storage: ./s
auth:
  htpasswd:
    file: ./htpasswd
upstreams: {}
";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.auth.htpasswd.file.as_deref(), Some(Path::new("/etc/pnpr/./htpasswd")));
    // Tokens default to the htpasswd sibling.
    assert_eq!(config.auth.tokens.file.as_deref(), Some(Path::new("/etc/pnpr/tokens.db")));
}

#[test]
fn auth_block_absent_disables_registration_by_default() {
    let yaml = "storage: ./s\n";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert!(config.auth.htpasswd.file.is_none());
    assert!(config.auth.tokens.file.is_none());
    // Registration is opt-in: an omitted cap denies new sign-ups.
    assert_eq!(config.auth.htpasswd.max_users, super::super::MaxUsers::Disabled);
}

#[test]
fn auth_max_users_absent_disables_registration() {
    let yaml = "\
storage: ./s
auth:
  htpasswd:
    file: ./htpasswd
upstreams: {}
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(config.auth.htpasswd.max_users, super::super::MaxUsers::Disabled);
}

#[test]
fn auth_tokens_file_explicit_override_wins_over_sibling_default() {
    let yaml = "\
storage: ./s
auth:
  htpasswd:
    file: ./htpasswd
  tokens:
    file: /var/lib/pnpr/tokens.sqlite
upstreams: {}
";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.auth.tokens.file.as_deref(), Some(Path::new("/var/lib/pnpr/tokens.sqlite")));
}

#[test]
fn auth_max_users_negative_one_means_disabled() {
    let yaml = "\
storage: ./s
auth:
  htpasswd:
    file: ./htpasswd
    max_users: -1
upstreams: {}
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(config.auth.htpasswd.max_users, super::super::MaxUsers::Disabled);
}

#[test]
fn auth_max_users_positive_is_a_hard_cap() {
    let yaml = "\
storage: ./s
auth:
  htpasswd:
    file: ./htpasswd
    max_users: 5
upstreams: {}
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(config.auth.htpasswd.max_users, super::super::MaxUsers::Limited(5));
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
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    let rules = &config.hosted["local"].rules;
    // Omitted `access` falls back to the registry-level default...
    assert!(rules.for_package("@team/x").access.allows(&user("team")));
    assert!(!rules.for_package("@team/x").access.allows(&user("carol")));
    // ...while the more specific key overrides it.
    assert!(rules.for_package("@team/open").access.allows(&Identity::Anonymous));
}

#[test]
fn rule_usernames_grant_per_user_access() {
    // Bare names are usernames/groups, not a config error.
    let config = hosted_rules_config(
        "      '@team/*':\n        access: [alice, bob]\n        publish: alice\n",
    );
    let team = config.hosted["local"].rules.for_package("@team/x");
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
      platform: [alice]
    packages:
      '@team/*':
        access: team:platform
  corp:
    type: hosted
    org: corp
    packages:
      '@corp/*':
        access: team:platform
";
    let err = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap_err();
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
      platform: [alice]
    packages:
      '@team/*':
        access: platform
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    let access = config.hosted["local"].rules.for_package("@team/x").access;
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
      platform: [alice]
      release: [carol]
    packages:
      '@team/*':
        access: team:platfrm
";
    let err = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap_err();
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
    let access = config.hosted["local"].rules.for_package("@team/x").access;
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
    let err = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap_err();
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
    let split_members = "    teams:\n      platform: alice bob\n";
    // A team name is spliced into `team:<name>` tokens, so a name the
    // grammar cannot express is rejected at declaration.
    let sigil_name = "    teams:\n      $all: [alice]\n";
    let colon_name = "    teams:\n      'a:b': [alice]\n";
    // A member is a plain username: a built-in group — in any spelling —
    // would silently become a user nobody is named after, and a `team:`
    // reference would be an unsupported nested team.
    let builtin_member = "    teams:\n      platform: [$all]\n";
    let alias_member = "    teams:\n      platform: [authenticated]\n";
    let at_alias_member = "    teams:\n      platform: ['@all']\n";
    let nested_team_member = "    teams:\n      platform: ['team:release']\n";
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
        let err = Config::from_yaml_str(&yaml, Path::new("/x"), listen(), None).unwrap_err();
        assert!(
            matches!(
                &err,
                RegistryError::InvalidConfig { reason } if reason.contains(needle),
            ),
            "unexpected error for {yaml:?}: {err}",
        );
    }
}
