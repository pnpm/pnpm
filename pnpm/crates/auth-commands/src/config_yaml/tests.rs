use super::{ConfigYamlFields, Value, login_fields, logout_fields};
use pretty_assertions::assert_eq;
use serde_json::json;

/// The value `login_fields` assigns to `key`, or `None` when the login does
/// not touch that field.
fn field(fields: &ConfigYamlFields, key: &str) -> Option<Value> {
    fields
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, value)| value.clone())
}

fn login(document: Option<&str>, scope: Option<&str>) -> ConfigYamlFields {
    login_fields(document, "https://registry.example/", scope).expect("the document parses")
}

#[test]
fn a_scoped_login_records_its_route() {
    let fields = login(None, Some("@acme"));

    assert_eq!(field(&fields, "_auth"), None);
    assert_eq!(
        field(&fields, "registries"),
        Some(json!({ "https://registry.example/": { "scopes": ["@acme"] } })),
    );
}

/// Declaring the registry's `scopes` would claim it as the default for
/// resolution.
#[test]
fn an_unscoped_login_records_no_route() {
    assert_eq!(login(None, None), vec![]);
}

/// `_auth` outranks `auth.ini`, so a credential the new token replaces must
/// not stay behind to shadow it. Other grants stay.
#[test]
fn a_scoped_login_drops_the_scope_s_old_tokens_and_keeps_the_rest() {
    let document = "\
_auth:
  https://other.example/:
    '@': { authToken: other-token }
    '@acme': { authToken: moved-token }
  https://registry.example/:
    '@': { authToken: own-token }
    '@acme': { authToken: stale-token }
    '@kept': { authToken: kept-token }
";

    assert_eq!(
        field(&login(Some(document), Some("@acme")), "_auth"),
        Some(json!({
            "https://other.example/": { "@": { "authToken": "other-token" } },
            "https://registry.example/": {
                "@": { "authToken": "own-token" },
                "@kept": { "authToken": "kept-token" },
            },
        })),
    );
}

/// The bare `@` is each registry's own credential rather than a scope, so an
/// unscoped login to one registry must not forget another's.
#[test]
fn an_unscoped_login_drops_only_its_registry_s_own_token() {
    let document = "\
_auth:
  https://other.example/:
    '@': { authToken: other-token }
  https://registry.example/:
    '@': { authToken: stale-token }
    '@acme': { authToken: scoped-token }
";

    assert_eq!(
        field(&login(Some(document), None), "_auth"),
        Some(json!({
            "https://other.example/": { "@": { "authToken": "other-token" } },
            "https://registry.example/": { "@acme": { "authToken": "scoped-token" } },
        })),
    );
}

/// A registry left holding nothing goes with its last credential, rather than
/// staying as an empty entry the reader would still read a route from.
#[test]
fn a_login_that_empties_the_setting_deletes_it() {
    let document = "\
_auth:
  https://zzz-old.example/:
    '@acme': { authToken: old-token }
";

    assert_eq!(field(&login(Some(document), Some("@acme")), "_auth"), Some(Value::Null));
}

/// The reader matches a credential to a host by the normalized URL, so every
/// spelling of the registry names the credential the login replaces.
#[test]
fn a_login_drops_every_spelling_of_its_registry() {
    let document = "\
_auth:
  https://registry.example:
    '@': { authToken: shadowed-token }
  HTTPS://Registry.Example:443/:
    '@': { authToken: winning-token }
  https://other.example/:
    '@': { authToken: other-token }
";

    assert_eq!(
        field(&login(Some(document), None), "_auth"),
        Some(json!({ "https://other.example/": { "@": { "authToken": "other-token" } } })),
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
    let error = login_fields(Some("_auth: [unclosed\n"), "https://registry.example/", None);

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

/// A logout must find a differently spelled entry, or it would revoke a token on the registry and
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
