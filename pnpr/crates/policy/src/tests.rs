use super::{AccessList, AccessToken, Identity, PackageRule, RuleTable};
use pnpr_registry::{Ecosystem, PackagePattern};

fn list(token: &str) -> AccessList {
    AccessList::from_tokens([token])
}

fn rule(pattern: &str, access: Option<&str>) -> PackageRule {
    PackageRule {
        pattern: PackagePattern::parse(pattern, Ecosystem::Npm).expect("test pattern parses"),
        access: access.map(list),
        publish: access.map(list),
        unpublish: access.map(list),
    }
}

/// The registry-mock shape from `Config::proxy`, reduced to what these
/// selection tests exercise.
fn registry_mock_rules() -> RuleTable {
    RuleTable::new(
        vec![
            rule("@pnpm.e2e/*", None),
            rule("@pnpm.e2e/needs-auth", Some("$authenticated")),
            rule("@private/*", Some("$authenticated")),
        ],
        None,
    )
    .with_default_publish(list("$authenticated"))
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
    assert_eq!(AccessToken::from("$owners"), AccessToken::Owners);
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
fn owners_admits_only_owners() {
    let list = list("$owners");
    assert!(list.references_owners());
    assert!(!list.allows(&Identity::Anonymous));
    assert!(!list.allows(&user("alice")));
    assert!(!list.allows_with_context(&Identity::Anonymous, true));
    assert!(list.allows_with_context(&user("alice"), true));
    assert!(!list.allows_with_context(&user("alice"), false));
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
    let list = AccessList::new(vec![AccessToken::Team("platform-id".to_string())]);
    assert!(list.allows(&Identity::member("alice", vec!["platform-id".to_string()])));
    assert!(!list.allows(&Identity::member("bob", vec!["other-id".to_string()])));
    assert!(!list.allows(&user("bob")));
    // A team's ID is not an account ID.
    assert!(!list.allows(&user("platform-id")));
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
    let scope_first = RuleTable::new(
        vec![rule("@acme/*", Some("$all")), rule("@acme/secret", Some("$authenticated"))],
        None,
    );
    let exact_first = RuleTable::new(
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
    let rules = RuleTable::new(
        vec![
            rule("**", Some("everyone")),
            rule("@*/*", Some("scoped")),
            rule("@acme/*", Some("acme")),
            rule("@acme/exact", Some("exact")),
        ],
        None,
    );
    assert!(
        rules
            .for_package("@acme/exact")
            .access
            .allows(&user("exact")),
    );
    assert!(
        !rules
            .for_package("@acme/exact")
            .access
            .allows(&user("acme")),
    );
    assert!(
        rules
            .for_package("@acme/other")
            .access
            .allows(&user("acme")),
    );
    assert!(
        rules
            .for_package("@beta/pkg")
            .access
            .allows(&user("scoped")),
    );
    assert!(
        rules
            .for_package("unscoped")
            .access
            .allows(&user("everyone")),
    );
}

#[test]
fn omitted_rule_fields_fall_back_to_registry_default_not_broader_keys() {
    // The exact key wins and omits `access`; the fallback is the
    // registry-level default, never the broader scope key's field.
    let rules = RuleTable::new(
        vec![
            PackageRule {
                pattern: PackagePattern::parse("@acme/*", Ecosystem::Npm).expect("parses"),
                access: Some(list("$authenticated")),
                publish: None,
                unpublish: None,
            },
            PackageRule {
                pattern: PackagePattern::parse("@acme/open", Ecosystem::Npm).expect("parses"),
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
    let rules = RuleTable::new(vec![rule("@acme/*", Some("$authenticated"))], None);
    assert!(rules.for_package("unclaimed").access.allows(&Identity::Anonymous));
}

#[test]
fn all_access_admit_requires_the_default_and_every_refinement() {
    let public = RuleTable::default();
    assert!(public.all_access_admit(&Identity::Anonymous));

    let private_default = RuleTable::new(Vec::new(), Some(list("$authenticated")));
    assert!(!private_default.all_access_admit(&Identity::Anonymous));
    assert!(private_default.all_access_admit(&user("alice")));

    let private_refinement = RuleTable::new(vec![rule("@private/*", Some("$authenticated"))], None);
    assert!(!private_refinement.all_access_admit(&Identity::Anonymous));
    assert!(private_refinement.all_access_admit(&user("alice")));
}

#[test]
fn falls_back_to_safe_defaults_when_no_rules_match() {
    let policies = RuleTable::default();
    let effective = policies.for_package("anything");
    assert!(effective.access.allows(&Identity::Anonymous));
    assert!(!effective.publish.allows(&Identity::Anonymous));
    assert!(!effective.publish.allows(&user("alice")));
    assert!(effective.publish.allows_with_context(&user("alice"), true));
    assert!(!effective.unpublish.allows(&Identity::Anonymous));
    assert!(!effective.unpublish.allows(&user("alice")));
}

#[test]
fn admits_every_identity_requires_universal_access() {
    let all = AccessList::from_tokens(["$all"]);
    assert!(all.admits_every_identity());

    let both = AccessList::from_tokens(["$anonymous", "$authenticated"]);
    assert!(both.admits_every_identity());

    let anon_only = AccessList::from_tokens(["$anonymous"]);
    assert!(!anon_only.admits_every_identity());

    let auth_only = AccessList::from_tokens(["$authenticated"]);
    assert!(!auth_only.admits_every_identity());

    let user_only = AccessList::from_tokens(["alice"]);
    assert!(!user_only.admits_every_identity());

    let empty = AccessList::from_tokens([] as [&str; 0]);
    assert!(!empty.admits_every_identity());
}

#[test]
fn a_grant_whose_excludes_leave_no_package_admits_nobody_anywhere() {
    use crate::{Action, Grant, GrantSpec};
    use pnpr_registry::Ecosystem;

    let grant = |include: &[&str], exclude: &[&str]| {
        let spec = GrantSpec {
            name: "cachers".to_string(),
            include: include
                .iter()
                .map(ToString::to_string)
                .collect(),
            exclude: exclude
                .iter()
                .map(ToString::to_string)
                .collect(),
            subjects: vec![("alice".to_string(), Action::Cache)],
        };
        Grant::compile(&spec, Ecosystem::Npm).unwrap()
    };
    let admits = |grant: Grant| {
        RuleTable::of_grants(Vec::new(), vec![grant]).any_admits(Action::Cache, &user("alice"))
    };
    assert!(admits(grant(&[], &["@acme/*"])));
    assert!(admits(grant(&["@acme/*"], &["@acme/app"])));
    assert!(!admits(grant(&[], &["**"])));
    assert!(!admits(grant(&["@acme/app", "@acme/lib"], &["@acme/*"])));
}

#[test]
fn a_grant_for_one_name_is_found_among_grants_for_other_names() {
    use crate::{Action, Grant, GrantSpec};
    use pnpr_registry::Ecosystem;

    let grant = |include: &[&str], exclude: &[&str], subject: &str| {
        let spec = GrantSpec {
            name: format!("{subject}:{include:?}"),
            include: include
                .iter()
                .map(ToString::to_string)
                .collect(),
            exclude: exclude
                .iter()
                .map(ToString::to_string)
                .collect(),
            subjects: vec![(subject.to_string(), Action::Read)],
        };
        Grant::compile(&spec, Ecosystem::Npm).unwrap()
    };
    let table = RuleTable::of_grants(
        Vec::new(),
        vec![
            grant(&["@acme/app"], &[], "alice"),
            grant(&["@acme/lib", "@acme/app"], &[], "bob"),
            grant(&["@acme/*"], &["@acme/app"], "carol"),
            grant(&["@acme/tool", "@other/*"], &[], "dave"),
        ],
    );
    let reads =
        |name: &str, package: &str| table.permits(package, Action::Read, &user(name), false);
    assert!(reads("alice", "@acme/app") && !reads("alice", "@acme/lib"));
    assert!(reads("bob", "@acme/app") && reads("bob", "@acme/lib") && !reads("bob", "@acme/x"));
    assert!(reads("carol", "@acme/lib") && !reads("carol", "@acme/app"));
    assert!(reads("dave", "@acme/tool") && reads("dave", "@other/x") && !reads("dave", "@acme/x"));
}
