//! The global `config.yaml` half of `pnpm login` / `pnpm logout`.
//!
//! A login's token goes to `auth.ini`, but the route `--scope` asks for goes
//! to `registries`, the setting that declares which registry a scope resolves
//! from. `_auth` can hold a token for the same registry and outranks
//! `auth.ini`, so a login drops the entry its new token replaces, and a logout
//! drops the one it revokes.
//!
//! Every function here is a pure text-to-value transformation. The caller
//! reads the document, hands it over, and writes the returned values back
//! through its own capability seam, so the merge rules are testable without
//! a filesystem.

use pnpm_config::validate_json_auth_registry;
use serde_json::{Map, Value};

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

/// The fields recording a login for `scope` at `registry`: the `_auth`
/// credentials the new token replaces are dropped, and a scoped login routes
/// the scope to `registry`.
///
/// `registry` must already be normalized, since it is matched against the
/// keys of both settings.
///
/// An unscoped login writes no route: declaring the registry's `scopes` would
/// claim it as the default for resolution, which logging in does not decide.
pub fn login_fields(
    document: Option<&str>,
    registry: &str,
    scope: Option<&str>,
) -> Result<ConfigYamlFields, ParseConfigYamlError> {
    let root = parse_document(document)?;
    let mut fields: ConfigYamlFields =
        auth_without(&root, registry, scope.unwrap_or(DEFAULT_SCOPE))
            .map(|auth| (AUTH_KEY, auth))
            .into_iter()
            .collect();
    if let Some(scope) = scope {
        fields.push((REGISTRIES_KEY, registries_with_route(&root, registry, scope)));
    }
    Ok(fields)
}

/// The fields dropping `registry`'s own credential, the one a `pnpm logout`
/// revokes, or an empty list when it holds none.
///
/// Tokens the same registry holds for package scopes are separate grants that
/// were not revoked, so they stay: taking them away would lose access to them
/// without ending them. The route outlives the credential too. Which registry
/// a scope resolves from is a preference the user may still want, and
/// `pnpm logout` has never claimed to unpick it.
pub fn logout_fields(
    document: Option<&str>,
    registry: &str,
) -> Result<ConfigYamlFields, ParseConfigYamlError> {
    let root = parse_document(document)?;
    Ok(auth_without(&root, registry, DEFAULT_SCOPE)
        .map(|auth| (AUTH_KEY, auth))
        .into_iter()
        .collect())
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

/// `_auth` without the credentials `scope` holds, or `None` when it holds
/// none. Returns `Value::Null` when nothing is left, which deletes the key.
///
/// The bare `@` is each registry's own credential, so only `registry`'s is
/// dropped. A package scope's token is dropped at every registry, since the
/// reader infers a route from each `_auth` entry and a login moves the scope.
///
/// The reader canonicalizes every key before matching a credential to a
/// host, so spellings that differ only in scheme case, host case, a default
/// port or a trailing slash are one registry here too.
fn auth_without(root: &Map<String, Value>, registry: &str, scope: &str) -> Option<Value> {
    let mut auth = object_at(root, AUTH_KEY)?;
    let mut removed = false;
    for (url, scopes) in &mut auth {
        let applies =
            scope != DEFAULT_SCOPE || validate_json_auth_registry(url).as_deref() == Ok(registry);
        if applies && let Some(scopes) = scopes.as_object_mut() {
            removed |= scopes.remove(scope).is_some();
        }
    }
    if !removed {
        return None;
    }
    auth.retain(|_, scopes| {
        scopes
            .as_object()
            .is_none_or(|scopes| !scopes.is_empty())
    });
    Some(if auth.is_empty() { Value::Null } else { Value::Object(auth) })
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
    registries.retain(|_, entry| {
        entry
            .as_object()
            .is_none_or(|entry| !entry.is_empty())
    });

    let mut declaration = registries
        .get(registry)
        .and_then(as_object)
        .unwrap_or_default();
    let mut scopes = declaration
        .get("scopes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if !scopes
        .iter()
        .any(|existing| existing.as_str() == Some(scope))
    {
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

fn object_at(root: &Map<String, Value>, key: &str) -> Option<Map<String, Value>> {
    root.get(key).and_then(as_object)
}

fn as_object(value: &Value) -> Option<Map<String, Value>> {
    value.as_object().cloned()
}

#[cfg(test)]
mod tests;
