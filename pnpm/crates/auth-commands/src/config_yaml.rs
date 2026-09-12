//! The global `config.yaml` half of `pnpm login` / `pnpm logout`.
//!
//! A login's two outputs are a credential and a route, and both have a
//! declared home in the global config: the token goes to `_auth`, keyed by
//! registry URL then by scope, and the scope reached through it goes to
//! `registries`. Only these two settings can carry a login's result to a
//! later command, because they are the only ones the reader accepts from a
//! file no repository can write.
//!
//! Every function here is a pure text-to-value transformation. The caller
//! reads the document, hands it over, and writes the returned values back
//! through its own capability seam, so the merge rules are testable without
//! a filesystem.

use pnpm_config::validate_json_auth_registry;
use serde_json::{Map, Value, json};

/// Base name of pnpm's global config file, inside `configDir`.
pub const GLOBAL_CONFIG_YAML_FILENAME: &str = "config.yaml";

/// The `_auth` scope key standing for the registry itself, used when a login
/// claims no package scope. Matches `pnpm-config`'s `DEFAULT_REGISTRY_SCOPE`.
const DEFAULT_SCOPE: &str = "@";

/// The top-level key holding registry credentials.
const AUTH_KEY: &str = "_auth";

/// The top-level key holding registry declarations.
const REGISTRIES_KEY: &str = "registries";

/// The document does not parse as a YAML mapping, so no merge can preserve
/// what it already holds.
#[derive(Debug, derive_more::Display, derive_more::Error, miette::Diagnostic)]
#[display("Failed to parse the global config file as YAML: {source}")]
#[diagnostic(code(ERR_PNPM_AUTH_CONFIG_YAML_PARSE))]
pub struct ParseConfigYamlError {
    #[error(source)]
    source: Box<serde_saphyr::Error>,
}

/// The top-level fields a login or logout wants written back, each paired
/// with the value to set. A `null` value deletes the key.
pub type ConfigYamlFields = Vec<(&'static str, Value)>;

/// The fields recording that `token` was granted for `scope` at `registry`.
///
/// `registry` must already be normalized, since it becomes a map key that a
/// later login has to match to update rather than duplicate.
///
/// An unscoped login writes no route: its token covers the registry itself,
/// and declaring the registry's `scopes` would claim it as the default for
/// resolution, which logging in does not decide.
pub fn login_fields(
    document: Option<&str>,
    registry: &str,
    scope: Option<&str>,
    token: &str,
) -> Result<ConfigYamlFields, ParseConfigYamlError> {
    let root = parse_document(document)?;
    let mut fields =
        vec![(AUTH_KEY, auth_with_token(&root, registry, scope.unwrap_or(DEFAULT_SCOPE), token))];
    if let Some(scope) = scope {
        fields.push((REGISTRIES_KEY, registries_with_route(&root, registry, scope)));
    }
    Ok(fields)
}

/// The fields dropping `registry`'s own credential — the one a `pnpm logout`
/// revokes — or an empty list when it holds none.
///
/// Tokens the same registry holds for package scopes are separate grants that
/// were not revoked, so they stay: taking them away would lose access to them
/// without ending them. The route outlives the credential too — which registry
/// a scope resolves from is a preference the user may still want, and
/// `pnpm logout` has never claimed to unpick it.
pub fn logout_fields(
    document: Option<&str>,
    registry: &str,
) -> Result<ConfigYamlFields, ParseConfigYamlError> {
    let root = parse_document(document)?;
    let Some(mut auth) = object_at(&root, AUTH_KEY) else {
        return Ok(Vec::new());
    };
    let registry = &key_for_registry(&auth, registry);
    let Some(mut scopes) = auth.get(registry).and_then(as_object) else {
        return Ok(Vec::new());
    };
    if scopes.remove(DEFAULT_SCOPE).is_none() {
        return Ok(Vec::new());
    }
    if scopes.is_empty() {
        auth.remove(registry);
    } else {
        auth.insert(registry.to_owned(), Value::Object(scopes));
    }
    let value = if auth.is_empty() { Value::Null } else { Value::Object(auth) };
    Ok(vec![(AUTH_KEY, value)])
}

fn parse_document(document: Option<&str>) -> Result<Map<String, Value>, ParseConfigYamlError> {
    let Some(text) = document.filter(|text| !text.trim().is_empty()) else {
        return Ok(Map::new());
    };
    match serde_saphyr::from_str::<Value>(text) {
        Ok(Value::Object(root)) => Ok(root),
        // A document that is valid YAML but not a mapping holds no settings
        // to preserve, so the fields replace it wholesale.
        Ok(_) => Ok(Map::new()),
        Err(source) => Err(ParseConfigYamlError { source: Box::new(source) }),
    }
}

/// Record `token` for `scope` at `registry`, dropping any credential the same
/// package scope holds elsewhere.
///
/// The reader infers a route from every `_auth` entry and lets the last one
/// win, so a stale entry for the scope would keep resolving it to the registry
/// it used to reach — with that registry's token — however plainly
/// `registries` names the new one. The bare `@` is exempt: it is not a scope
/// but each registry's own credential, and logging in to one is no reason to
/// forget another.
fn auth_with_token(root: &Map<String, Value>, registry: &str, scope: &str, token: &str) -> Value {
    let mut auth = object_at(root, AUTH_KEY).unwrap_or_default();
    let registry = &key_for_registry(&auth, registry);
    if scope != DEFAULT_SCOPE {
        for (url, scopes) in &mut auth {
            if url != registry
                && let Some(scopes) = scopes.as_object_mut()
            {
                scopes.remove(scope);
            }
        }
        auth.retain(|_, scopes| scopes.as_object().is_none_or(|scopes| !scopes.is_empty()));
    }
    let mut scopes = auth.get(registry).and_then(as_object).unwrap_or_default();
    scopes.insert(scope.to_owned(), json!({ "authToken": token }));
    auth.insert(registry.to_owned(), Value::Object(scopes));
    Value::Object(auth)
}

/// Route `scope` to `registry`, in whichever of the setting's two shapes the
/// document already uses.
///
/// The shapes cannot be mixed in one map — the reader rejects a `registries`
/// that does — so a document already written as `<scope>: <url>` entries is
/// extended in that shape rather than converted.
///
/// A scope reaches one registry: the reader refuses a `registries` that
/// routes one to two, so logging the same scope in somewhere else has to take
/// it away from wherever it pointed before, or the next command to read the
/// config would fail to load it at all.
fn registries_with_route(root: &Map<String, Value>, registry: &str, scope: &str) -> Value {
    let mut registries = object_at(root, REGISTRIES_KEY).unwrap_or_default();
    if registries.values().any(Value::is_string) {
        registries.insert(scope.to_owned(), Value::String(registry.to_owned()));
        return Value::Object(registries);
    }
    for (url, entry) in &mut registries {
        if url != registry {
            unroute_scope(entry, scope);
        }
    }
    registries.retain(|_, entry| entry.as_object().is_none_or(|entry| !entry.is_empty()));

    let mut declaration = registries.get(registry).and_then(as_object).unwrap_or_default();
    let mut scopes =
        declaration.get("scopes").and_then(Value::as_array).cloned().unwrap_or_default();
    if !scopes.iter().any(|existing| existing.as_str() == Some(scope)) {
        scopes.push(Value::String(scope.to_owned()));
    }
    declaration.insert("scopes".to_owned(), Value::Array(scopes));
    registries.insert(registry.to_owned(), Value::Object(declaration));
    Value::Object(registries)
}

/// Drop `scope` from a registry declaration's `scopes`, and the now-empty
/// `scopes` list with it, leaving whatever else the declaration states.
fn unroute_scope(entry: &mut Value, scope: &str) {
    let Some(declaration) = entry.as_object_mut() else {
        return;
    };
    let Some(scopes) = declaration.get_mut("scopes").and_then(Value::as_array_mut) else {
        return;
    };
    scopes.retain(|existing| existing.as_str() != Some(scope));
    if scopes.is_empty() {
        declaration.remove("scopes");
    }
}

/// The `_auth` key naming `registry`, whatever spelling it is written in.
///
/// The reader canonicalizes every key before matching a credential to a host,
/// so spellings that differ only in scheme case, host case, a default port or
/// a trailing slash are one entry to it, and the last of them wins. They must
/// be one entry here too, or a login would leave the credential it replaced
/// beside its replacement — still the one the reader picks — and a logout
/// would leave it behind. Canonicalized the same way for the same reason, and
/// the last match taken for the same reason.
fn key_for_registry(auth: &Map<String, Value>, registry: &str) -> String {
    auth.keys()
        .rfind(|key| validate_json_auth_registry(key).as_deref() == Ok(registry))
        .cloned()
        .unwrap_or_else(|| registry.to_owned())
}

fn object_at(root: &Map<String, Value>, key: &str) -> Option<Map<String, Value>> {
    root.get(key).and_then(as_object)
}

fn as_object(value: &Value) -> Option<Map<String, Value>> {
    value.as_object().cloned()
}

#[cfg(test)]
mod tests;
