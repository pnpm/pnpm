use super::{AccessList, AccessToken, Identity, PackagePolicies};

fn user(name: &str) -> Identity {
    Identity::User { username: name.to_string() }
}

#[test]
fn token_parsing_maps_builtins_and_names() {
    assert_eq!(AccessToken::from("$all"), AccessToken::All);
    assert_eq!(AccessToken::from("@all"), AccessToken::All);
    assert_eq!(AccessToken::from("all"), AccessToken::All);
    assert_eq!(AccessToken::from("$authenticated"), AccessToken::Authenticated);
    assert_eq!(AccessToken::from("$anonymous"), AccessToken::Anonymous);
    assert_eq!(AccessToken::from("@anonymous"), AccessToken::Anonymous);
    // Anything else is a username / group name (no longer an error).
    assert_eq!(AccessToken::from("admin"), AccessToken::Named("admin".to_string()));
}

#[test]
fn all_admits_everyone() {
    let list = AccessList::parse("$all");
    assert!(list.allows(&Identity::Anonymous));
    assert!(list.allows(&user("alice")));
}

#[test]
fn authenticated_admits_only_logged_in() {
    let list = AccessList::parse("$authenticated");
    assert!(!list.allows(&Identity::Anonymous));
    assert!(list.allows(&user("alice")));
}

#[test]
fn anonymous_admits_only_logged_out() {
    let list = AccessList::parse("$anonymous");
    assert!(list.allows(&Identity::Anonymous));
    assert!(!list.allows(&user("alice")));
}

#[test]
fn usernames_grant_per_user_access() {
    // verdaccio's per-user access: list the usernames directly.
    let list = AccessList::parse("alice bob");
    assert!(list.allows(&user("alice")));
    assert!(list.allows(&user("bob")));
    assert!(!list.allows(&user("carol")));
    assert!(!list.allows(&Identity::Anonymous));
}

#[test]
fn mixed_token_list_is_a_union() {
    // `$authenticated admin` — any logged-in user OR (redundantly)
    // the `admin` name; satisfied by any authenticated caller.
    let list = AccessList::parse("$authenticated admin");
    assert!(list.allows(&user("carol")));
    assert!(!list.allows(&Identity::Anonymous));
}

#[test]
fn empty_list_admits_no_one() {
    let list = AccessList::parse("");
    assert!(list.is_empty());
    assert!(!list.allows(&Identity::Anonymous));
    assert!(!list.allows(&user("alice")));
}

#[test]
fn defaults_match_registry_mock_config() {
    let policies = PackagePolicies::registry_mock_defaults();

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
fn first_matching_rule_wins() {
    let policies = PackagePolicies::registry_mock_defaults();
    // `@private/foo` matches `@private/*` first, not the `**`
    // catch-all: anonymous reads are denied.
    assert!(!policies.for_package("@private/foo").access.allows(&Identity::Anonymous));
}

#[test]
fn falls_back_to_safe_defaults_when_no_rules_match() {
    let policies = PackagePolicies::new(vec![]);
    let effective = policies.for_package("anything");
    assert!(effective.access.allows(&Identity::Anonymous));
    assert!(!effective.publish.allows(&Identity::Anonymous));
    assert!(effective.publish.allows(&user("alice")));
    assert!(!effective.unpublish.allows(&Identity::Anonymous));
    assert!(!effective.unpublish.allows(&user("alice")));
}
