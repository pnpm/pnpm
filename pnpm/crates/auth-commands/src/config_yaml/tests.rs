use super::{ConfigYamlFields, Value, json, login_fields, logout_fields};
use pretty_assertions::assert_eq;

/// The value `login_fields` assigns to `key`, or `None` when the login does
/// not touch that field.
fn field(fields: &ConfigYamlFields, key: &str) -> Option<Value> {
    fields.iter().find(|(name, _)| *name == key).map(|(_, value)| value.clone())
}

fn login(document: Option<&str>, scope: Option<&str>) -> ConfigYamlFields {
    login_fields(document, "https://registry.example/", scope, "granted-token")
        .expect("the document parses")
}

#[test]
fn a_scoped_login_records_the_token_and_its_route() {
    let fields = login(None, Some("@acme"));

    assert_eq!(
        field(&fields, "_auth"),
        Some(json!({
            "https://registry.example/": { "@acme": { "authToken": "granted-token" } },
        })),
    );
    assert_eq!(
        field(&fields, "registries"),
        Some(json!({ "https://registry.example/": { "scopes": ["@acme"] } })),
    );
}

/// An unscoped login's token covers the registry itself, and declaring the
/// registry's `scopes` would claim it as the default for resolution.
#[test]
fn an_unscoped_login_records_no_route() {
    let fields = login(None, None);

    assert_eq!(
        field(&fields, "_auth"),
        Some(json!({
            "https://registry.example/": { "@": { "authToken": "granted-token" } },
        })),
    );
    assert_eq!(field(&fields, "registries"), None);
}

#[test]
fn a_login_keeps_the_credentials_of_other_registries_and_scopes() {
    let document = "\
_auth:
  https://other.example/:
    '@': { authToken: other-token }
  https://registry.example/:
    '@kept': { authToken: kept-token }
";

    assert_eq!(
        field(&login(Some(document), Some("@acme")), "_auth"),
        Some(json!({
            "https://other.example/": { "@": { "authToken": "other-token" } },
            "https://registry.example/": {
                "@kept": { "authToken": "kept-token" },
                "@acme": { "authToken": "granted-token" },
            },
        })),
    );
}

#[test]
fn logging_in_again_replaces_that_scope_s_token() {
    let document = "\
_auth:
  https://registry.example/:
    '@acme': { authToken: stale-token }
";

    assert_eq!(
        field(&login(Some(document), Some("@acme")), "_auth"),
        Some(json!({
            "https://registry.example/": { "@acme": { "authToken": "granted-token" } },
        })),
    );
}

#[test]
fn a_route_joins_the_registry_s_existing_declaration() {
    let document = "\
registries:
  https://registry.example/:
    serverType: pnpr
    scopes: ['@already']
";

    assert_eq!(
        field(&login(Some(document), Some("@acme")), "registries"),
        Some(json!({
            "https://registry.example/": {
                "serverType": "pnpr",
                "scopes": ["@already", "@acme"],
            },
        })),
    );
}

#[test]
fn a_route_that_is_already_declared_is_not_repeated() {
    let document = "\
registries:
  https://registry.example/:
    scopes: ['@acme']
";

    assert_eq!(
        field(&login(Some(document), Some("@acme")), "registries"),
        Some(json!({ "https://registry.example/": { "scopes": ["@acme"] } })),
    );
}

/// The reader refuses a `registries` map that mixes declarations with
/// `<scope>: <url>` entries, so a document written in the older shape is
/// extended in it.
#[test]
fn a_route_follows_the_scope_keyed_shape_the_document_already_uses() {
    let document = "\
registries:
  '@existing': https://other.example/
";

    assert_eq!(
        field(&login(Some(document), Some("@acme")), "registries"),
        Some(json!({
            "@existing": "https://other.example/",
            "@acme": "https://registry.example/",
        })),
    );
}

/// A `pnpm logout` revokes the registry's own token and nothing else, so the
/// grants it did not end must survive it.
#[test]
fn a_logout_drops_the_registry_s_own_token_and_keeps_its_scoped_ones() {
    let document = "\
_auth:
  https://registry.example/:
    '@': { authToken: default-token }
    '@acme': { authToken: scoped-token }
  https://other.example/:
    '@': { authToken: other-token }
";

    assert_eq!(
        logout_fields(Some(document), "https://registry.example/").expect("the document parses"),
        vec![(
            "_auth",
            json!({
                "https://registry.example/": { "@acme": { "authToken": "scoped-token" } },
                "https://other.example/": { "@": { "authToken": "other-token" } },
            }),
        )],
    );
}

/// Logging out of a registry whose only credential is a scoped one revokes
/// nothing of its own, so there is nothing to take away.
#[test]
fn a_logout_of_a_registry_holding_only_scoped_tokens_writes_nothing() {
    let document = "\
_auth:
  https://registry.example/:
    '@acme': { authToken: scoped-token }
";

    assert!(
        logout_fields(Some(document), "https://registry.example/")
            .expect("the document parses")
            .is_empty(),
    );
}

#[test]
fn a_logout_of_the_last_registry_deletes_the_setting() {
    let document = "\
_auth:
  https://registry.example/:
    '@': { authToken: only-token }
";

    assert_eq!(
        logout_fields(Some(document), "https://registry.example/").expect("the document parses"),
        vec![("_auth", Value::Null)],
    );
}

#[test]
fn a_logout_of_a_registry_with_no_credential_writes_nothing() {
    let document = "\
_auth:
  https://other.example/:
    '@': { authToken: other-token }
";

    assert!(
        logout_fields(Some(document), "https://registry.example/")
            .expect("the document parses")
            .is_empty(),
    );
    assert!(logout_fields(None, "https://registry.example/").expect("no document").is_empty());
}

/// The route outlives the credential: which registry a scope resolves from
/// is a preference the user may still want.
#[test]
fn a_logout_leaves_the_route_in_place() {
    let document = "\
_auth:
  https://registry.example/:
    '@acme': { authToken: scoped-token }
registries:
  https://registry.example/:
    scopes: ['@acme']
";

    let fields =
        logout_fields(Some(document), "https://registry.example/").expect("the document parses");

    assert_eq!(field(&fields, "registries"), None);
}

#[test]
fn a_document_that_is_not_yaml_is_an_error() {
    let error = login_fields(Some("_auth: [unclosed\n"), "https://registry.example/", None, "t");

    assert!(error.is_err(), "a malformed document must not be silently replaced");
}

/// The reader refuses a `registries` that routes one scope to two registries,
/// so a login that moves a scope has to take it off the registry it left.
#[test]
fn a_route_moves_off_the_registry_it_used_to_point_at() {
    let document = "\
registries:
  https://old.example/:
    serverType: pnpr
    scopes: ['@acme', '@kept']
  https://bare.example/:
    scopes: ['@acme']
";

    assert_eq!(
        field(&login(Some(document), Some("@acme")), "registries"),
        Some(json!({
            "https://old.example/": { "serverType": "pnpr", "scopes": ["@kept"] },
            "https://registry.example/": { "scopes": ["@acme"] },
        })),
    );
}

/// The reader infers a route from every `_auth` entry and lets the last win,
/// so a scope left recorded at its old registry keeps resolving there — with
/// that registry's token — however plainly `registries` names the new one.
#[test]
fn a_login_takes_its_scope_off_the_registry_that_used_to_hold_it() {
    let document = "\
_auth:
  https://zzz-old.example/:
    '@acme': { authToken: old-token }
    '@kept': { authToken: kept-token }
";

    assert_eq!(
        field(&login(Some(document), Some("@acme")), "_auth"),
        Some(json!({
            "https://zzz-old.example/": { "@kept": { "authToken": "kept-token" } },
            "https://registry.example/": { "@acme": { "authToken": "granted-token" } },
        })),
    );
}

/// A registry left holding nothing goes with its last credential, rather than
/// staying as an empty entry the reader would still read a route from.
#[test]
fn a_registry_emptied_by_a_login_is_dropped() {
    let document = "\
_auth:
  https://zzz-old.example/:
    '@acme': { authToken: old-token }
";

    assert_eq!(
        field(&login(Some(document), Some("@acme")), "_auth"),
        Some(json!({
            "https://registry.example/": { "@acme": { "authToken": "granted-token" } },
        })),
    );
}

/// The bare `@` is each registry's own credential rather than a scope, so an
/// unscoped login to one registry must not forget another's.
#[test]
fn an_unscoped_login_keeps_other_registries_own_tokens() {
    let document = "\
_auth:
  https://other.example/:
    '@': { authToken: other-token }
";

    assert_eq!(
        field(&login(Some(document), None), "_auth"),
        Some(json!({
            "https://other.example/": { "@": { "authToken": "other-token" } },
            "https://registry.example/": { "@": { "authToken": "granted-token" } },
        })),
    );
}

/// The reader matches a credential to a host by the normalized URL, so two
/// spellings of one registry are one entry to it and the later wins. A login
/// must therefore replace the entry it found rather than write a second one
/// beside it.
#[test]
fn a_login_replaces_a_differently_spelled_entry_for_its_registry() {
    let document = "\
_auth:
  https://registry.example:
    '@acme': { authToken: stale-token }
";

    assert_eq!(
        field(&login(Some(document), Some("@acme")), "_auth"),
        Some(json!({
            "https://registry.example": { "@acme": { "authToken": "granted-token" } },
        })),
    );
}

/// And a logout must find it, or it would revoke a token on the registry and
/// leave a usable copy of it in the file.
#[test]
fn a_logout_finds_a_differently_spelled_entry_for_its_registry() {
    let document = "\
_auth:
  https://registry.example:
    '@': { authToken: only-token }
";

    assert_eq!(
        logout_fields(Some(document), "https://registry.example/").expect("the document parses"),
        vec![("_auth", Value::Null)],
    );
}

/// Scheme case, host case and a default port make no difference to the reader
/// either, so a login must replace such an entry rather than write beside it.
#[test]
fn a_login_replaces_an_entry_spelled_with_a_different_case_or_port() {
    let document = "\
_auth:
  HTTPS://Registry.Example:443/:
    '@acme': { authToken: stale-token }
";

    assert_eq!(
        field(&login(Some(document), Some("@acme")), "_auth"),
        Some(json!({
            "HTTPS://Registry.Example:443/": { "@acme": { "authToken": "granted-token" } },
        })),
    );
}

/// When several spellings of one registry are present the reader applies them
/// in order and the last wins, so that is the one a login must replace.
#[test]
fn a_login_replaces_the_entry_the_reader_would_have_picked() {
    let document = "\
_auth:
  https://registry.example:
    '@acme': { authToken: shadowed-token }
  HTTPS://Registry.Example/:
    '@acme': { authToken: winning-token }
";

    // The shadowed spelling goes with it: to the reader it named the same
    // registry, so leaving it would leave a second credential for one host.
    assert_eq!(
        field(&login(Some(document), Some("@acme")), "_auth"),
        Some(json!({
            "HTTPS://Registry.Example/": { "@acme": { "authToken": "granted-token" } },
        })),
    );
}
