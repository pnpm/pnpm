use super::{AccessList, AccessToken, Identity, PackageRule, PackageRules};
use crate::registry::PackagePattern;

fn list(token: &str) -> AccessList {
    AccessList::from_tokens([token])
}

fn rule(pattern: &str, access: Option<&str>) -> PackageRule {
    PackageRule {
        pattern: PackagePattern::parse(pattern).expect("test pattern parses"),
        access: access.map(list),
        publish: access.map(list),
        unpublish: access.map(list),
    }
}

/// The registry-mock shape from `Config::proxy`, reduced to what these
/// selection tests exercise.
fn registry_mock_rules() -> PackageRules {
    PackageRules::new(
        vec![
            rule("@pnpm.e2e/*", None),
            rule("@pnpm.e2e/needs-auth", Some("$authenticated")),
            rule("@private/*", Some("$authenticated")),
        ],
        None,
    )
    .with_default_unpublish(list("$authenticated"))
}

fn user(name: &str) -> Identity {
    Identity::user(name)
}

#[test]
fn token_parsing_maps_builtins_and_usernames() {
    assert_eq!(AccessToken::from("$all"), AccessToken::All);
    assert_eq!(AccessToken::from("$authenticated"), AccessToken::Authenticated);
    assert_eq!(AccessToken::from("$anonymous"), AccessToken::Anonymous);
    assert_eq!(AccessToken::from("admin"), AccessToken::User("admin".to_string()));
    // Only the `$`-sigiled spellings are built-ins. The alias spellings
    // verdaccio accepted are plain usernames here — YAML loading rejects
    // them before they reach this constructor, and a programmatic caller
    // just gets a username that matches nobody (deny-safe).
    assert_eq!(AccessToken::from("@all"), AccessToken::User("@all".to_string()));
    assert_eq!(AccessToken::from("all"), AccessToken::User("all".to_string()));
    assert_eq!(AccessToken::from("authenticated"), AccessToken::User("authenticated".to_string()));
}

#[test]
fn all_admits_everyone() {
    let list = list("$all");
    assert!(list.allows(&Identity::Anonymous));
    assert!(list.allows(&user("alice")));
}

#[test]
fn authenticated_admits_only_logged_in() {
    let list = list("$authenticated");
    assert!(!list.allows(&Identity::Anonymous));
    assert!(list.allows(&user("alice")));
}

#[test]
fn anonymous_admits_only_logged_out() {
    let list = list("$anonymous");
    assert!(list.allows(&Identity::Anonymous));
    assert!(!list.allows(&user("alice")));
}

#[test]
fn usernames_grant_per_user_access() {
    let list = AccessList::from_tokens(["alice", "bob"]);
    assert!(list.allows(&user("alice")));
    assert!(list.allows(&user("bob")));
    assert!(!list.allows(&user("carol")));
    assert!(!list.allows(&Identity::Anonymous));
}

#[test]
fn team_tokens_admit_members_only() {
    let list = AccessList::new(vec![AccessToken::Team {
        name: "platform".to_string(),
        members: ["alice".to_string()].into(),
    }]);
    assert!(list.allows(&user("alice")));
    assert!(!list.allows(&user("bob")));
    // The team's *name* is not a username: a user who happens to be called
    // like the team gains nothing.
    assert!(!list.allows(&user("platform")));
    assert!(!list.allows(&Identity::Anonymous));
}

#[test]
fn bare_tokens_are_usernames_not_team_names() {
    // `platform` as a bare token admits only the user of that name — team
    // membership is reachable exclusively through a `team:` token.
    let list = list("platform");
    assert!(list.allows(&user("platform")));
    assert!(!list.allows(&user("alice")));
    assert!(!list.allows(&Identity::Anonymous));
}

#[test]
fn mixed_token_list_is_a_union() {
    // `[$authenticated, admin]` — any logged-in user OR (redundantly)
    // the `admin` name; satisfied by any authenticated caller.
    let list = AccessList::from_tokens(["$authenticated", "admin"]);
    assert!(list.allows(&user("carol")));
    assert!(!list.allows(&Identity::Anonymous));
}

#[test]
fn empty_list_admits_no_one() {
    let list = AccessList::default();
    assert!(list.is_empty());
    assert!(!list.allows(&Identity::Anonymous));
    assert!(!list.allows(&user("alice")));
}

#[test]
fn defaults_match_registry_mock_config() {
    let policies = registry_mock_rules();

    let needs_auth = policies.for_package("@pnpm.e2e/needs-auth");
    assert!(!needs_auth.access.allows(&Identity::Anonymous));
    assert!(needs_auth.access.allows(&user("alice")));

    let private = policies.for_package("@private/foo");
    assert!(!private.access.allows(&Identity::Anonymous));

    let public = policies.for_package("@pnpm.e2e/no-deps");
    assert!(public.access.allows(&Identity::Anonymous));
    assert!(!public.publish.allows(&Identity::Anonymous));
    assert!(public.publish.allows(&user("alice")));
    assert!(!public.unpublish.allows(&Identity::Anonymous));
    assert!(public.unpublish.allows(&user("alice")));
}

#[test]
fn most_specific_rule_wins_regardless_of_key_order() {
    // The exact key beats the scope key whichever is declared first: the
    // specificity chain (exact > @scope/* > @*/* > **) makes selection
    // order-free, so a YAML round-trip that reorders mapping keys cannot
    // change which access rule applies.
    let scope_first = PackageRules::new(
        vec![rule("@acme/*", Some("$all")), rule("@acme/secret", Some("$authenticated"))],
        None,
    );
    let exact_first = PackageRules::new(
        vec![rule("@acme/secret", Some("$authenticated")), rule("@acme/*", Some("$all"))],
        None,
    );
    for rules in [&scope_first, &exact_first] {
        assert!(!rules.for_package("@acme/secret").access.allows(&Identity::Anonymous));
        assert!(rules.for_package("@acme/other").access.allows(&Identity::Anonymous));
    }
}

#[test]
fn specificity_chain_orders_all_four_tiers() {
    let rules = PackageRules::new(
        vec![
            rule("**", Some("everyone")),
            rule("@*/*", Some("scoped")),
            rule("@acme/*", Some("acme")),
            rule("@acme/exact", Some("exact")),
        ],
        None,
    );
    assert!(rules.for_package("@acme/exact").access.allows(&user("exact")));
    assert!(!rules.for_package("@acme/exact").access.allows(&user("acme")));
    assert!(rules.for_package("@acme/other").access.allows(&user("acme")));
    assert!(rules.for_package("@beta/pkg").access.allows(&user("scoped")));
    assert!(rules.for_package("unscoped").access.allows(&user("everyone")));
}

#[test]
fn omitted_rule_fields_fall_back_to_registry_default_not_broader_keys() {
    // The exact key wins and omits `access`; the fallback is the
    // registry-level default, never the broader scope key's field.
    let rules = PackageRules::new(
        vec![
            PackageRule {
                pattern: PackagePattern::parse("@acme/*").expect("parses"),
                access: Some(list("$authenticated")),
                publish: None,
                unpublish: None,
            },
            PackageRule {
                pattern: PackagePattern::parse("@acme/open").expect("parses"),
                access: None,
                publish: None,
                unpublish: None,
            },
        ],
        None, // registry default access: $all
    );
    // `@acme/open`: exact key wins, omits access -> registry default ($all).
    assert!(rules.for_package("@acme/open").access.allows(&Identity::Anonymous));
    // Other scope names: scope key wins with its own access.
    assert!(!rules.for_package("@acme/foo").access.allows(&Identity::Anonymous));
}

#[test]
fn unclaimed_name_still_answers_with_defaults() {
    // `for_package` on a name outside the key set answers with the
    // registry defaults; namespace enforcement (404 before this lookup)
    // is the routing graph's job, not the rules'.
    let rules = PackageRules::new(vec![rule("@acme/*", Some("$authenticated"))], None);
    assert!(rules.for_package("unclaimed").access.allows(&Identity::Anonymous));
}

#[test]
fn falls_back_to_safe_defaults_when_no_rules_match() {
    let policies = PackageRules::default();
    let effective = policies.for_package("anything");
    assert!(effective.access.allows(&Identity::Anonymous));
    assert!(!effective.publish.allows(&Identity::Anonymous));
    assert!(effective.publish.allows(&user("alice")));
    assert!(!effective.unpublish.allows(&Identity::Anonymous));
    assert!(!effective.unpublish.allows(&user("alice")));
}
