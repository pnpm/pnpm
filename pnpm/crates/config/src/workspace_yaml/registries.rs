//! The `registries` setting: one entry per registry, keyed by its URL.
//!
//! Every fact in an entry is a fact about that server — how it lays out
//! tarball URLs, and which routes reach it — so a registry is declared once
//! and each fact is stated in one place. [`RegistryLookups`] is the inverse of
//! the routes, because a scope resolves to exactly one registry while a
//! registry serves many.

use super::LoadWorkspaceYamlError;
use crate::workspace_yaml::{
    normalize_registry_url, redact_registry_url, registry_url_has_userinfo,
};
use pnpm_lockfile::{RegistryOptions, RegistryServerType};
use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, Visitor, value::MapAccessDeserializer},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

/// The bare scope standing for the registry that packages resolve from when
/// no scope matches — the one the `registry` setting names.
pub const DEFAULT_REGISTRY_SCOPE: &str = "@";

/// Credentials and TLS material stay in `.npmrc`, which is not committed.
/// `registries` lives in `pnpm-workspace.yaml`, which is, so accepting these
/// would invite secrets into version control. Rejecting is better than
/// ignoring: a silently dropped `_authToken` reads as configured.
const SECRET_REGISTRY_FIELDS: &[&str] = &[
    "_auth",
    "_authToken",
    "_password",
    "username",
    "tokenHelper",
    "ca",
    "cafile",
    "cert",
    "certfile",
    "key",
    "keyfile",
];

/// One entry of the `registries` map.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(untagged)]
pub enum RegistryEntry {
    /// The older `<scope>: <url>` shape, which is the scope lookup already.
    ScopeRoute(String),
    Declaration(RegistryDeclaration),
}

/// Everything a project declares about one registry.
///
/// Unknown fields are captured rather than refused by serde on purpose, for
/// the same reason this module validates after parsing: a parse error renders
/// the offending source line verbatim, so refusing an `_authToken` field at
/// parse time would print the very credential being refused.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryDeclaration {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_type: Option<RegistryServerType>,
    /// See [`RegistryOptions::supports_time_field`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_time_field: Option<bool>,
    /// The scopes routed here, `@`-prefixed. A bare `@` is the scope-less
    /// default registry, the one the `registry` setting names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
    /// The bare-specifier prefix this registry answers to, as in
    /// `"foo": "work:^1.0.0"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(flatten)]
    pub unknown: BTreeMap<String, serde_json::Value>,
}

impl Eq for RegistryDeclaration {}

impl<'de> Deserialize<'de> for RegistryEntry {
    fn deserialize<Deser: Deserializer<'de>>(deserializer: Deser) -> Result<Self, Deser::Error> {
        deserializer.deserialize_any(RegistryEntryVisitor)
    }
}

/// Written out rather than derived as `#[serde(untagged)]` so that the error
/// from a malformed declaration survives: an untagged enum reports only that
/// no variant matched, which would hide, say, a `scopes` that is not a list.
struct RegistryEntryVisitor;

impl<'de> Visitor<'de> for RegistryEntryVisitor {
    type Value = RegistryEntry;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a registry URL or a registry declaration")
    }

    fn visit_str<Err: serde::de::Error>(self, value: &str) -> Result<Self::Value, Err> {
        Ok(RegistryEntry::ScopeRoute(value.to_owned()))
    }

    fn visit_map<Map: MapAccess<'de>>(self, map: Map) -> Result<Self::Value, Map::Error> {
        RegistryDeclaration::deserialize(MapAccessDeserializer::new(map))
            .map(RegistryEntry::Declaration)
    }
}

/// The three lookups the rest of pnpm reads, split out of the declarations.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RegistryLookups {
    /// Scope-routed URLs, `@`-prefixed and normalized.
    pub registries_by_scope: BTreeMap<String, String>,
    /// The registry a bare `@` routed to, if any.
    pub default_registry: Option<String>,
    /// Prefix-addressed URLs, deliberately kept as written: a named
    /// registry's URL is what a lockfile's recorded tarball URLs are matched
    /// against, so normalizing it would change what an existing lockfile
    /// verifies against.
    pub registries_by_prefix: BTreeMap<String, String>,
    pub registry_options_by_url: BTreeMap<String, RegistryOptions>,
}

/// The scopes `entries` routes, `@`-prefixed, with the bare `@` among them
/// when one names the default registry.
///
/// Read without consuming the map, unlike [`into_lookups`], so that a later
/// layer can tell a route a config file declared from one inferred from a
/// credential. Both spellings of the default reach it: the `scopes` list of a
/// declaration, and the older `default:` key that [`into_lookups`] reads as
/// the same thing.
#[must_use]
pub fn routed_scopes(entries: &BTreeMap<String, RegistryEntry>) -> BTreeSet<String> {
    entries
        .iter()
        .flat_map(|(key, entry)| match entry {
            RegistryEntry::ScopeRoute(_) if key == "default" => {
                vec![DEFAULT_REGISTRY_SCOPE.to_owned()]
            }
            RegistryEntry::ScopeRoute(_) => vec![key.clone()],
            RegistryEntry::Declaration(declaration) => {
                declaration.scopes.clone().unwrap_or_default()
            }
        })
        .collect()
}

/// Reject a `registries` map pnpm would otherwise read as something other
/// than what it says.
pub fn validate(entries: &BTreeMap<String, RegistryEntry>) -> Result<(), LoadWorkspaceYamlError> {
    let scope_routes: Vec<&String> = entries
        .iter()
        .filter(|(_, entry)| matches!(entry, RegistryEntry::ScopeRoute(_)))
        .map(|(scope, _)| scope)
        .collect();
    if scope_routes.len() == entries.len() {
        for scope in scope_routes {
            // A scope routes to a registry, so a URL in that position routes
            // nothing and would sit there inert. It is the declaration shape,
            // half-written.
            if looks_like_registry_url(scope) {
                return Err(LoadWorkspaceYamlError::StringValuedRegistryDeclaration {
                    registry: redact_registry_url(scope),
                });
            }
        }
        return Ok(());
    }
    if !scope_routes.is_empty() {
        return Err(LoadWorkspaceYamlError::MixedRegistriesShapes {
            scopes: quote_and_join(scope_routes.into_iter().map(String::as_str)),
        });
    }

    validate_declarations(entries.iter().filter_map(|(registry, entry)| match entry {
        RegistryEntry::Declaration(declaration) => Some((registry, declaration)),
        RegistryEntry::ScopeRoute(_) => None,
    }))
}

/// The per-declaration half of [`validate`], over declarations alone.
///
/// A pnpr request carries this shape and no other: its `registries` map is
/// always keyed by URL, which is what lets the server's boundary checks read
/// a key as a fetch target.
pub fn validate_declarations<'a>(
    entries: impl IntoIterator<Item = (&'a String, &'a RegistryDeclaration)>,
) -> Result<(), LoadWorkspaceYamlError> {
    let mut routed_scopes: BTreeMap<&str, String> = BTreeMap::new();
    let mut declared_prefixes: BTreeSet<&str> = BTreeSet::new();
    for (registry, declaration) in entries {
        let redacted = redact_registry_url(registry);
        if let Some(field) = declaration
            .unknown
            .keys()
            .find(|field| SECRET_REGISTRY_FIELDS.contains(&field.as_str()))
        {
            return Err(LoadWorkspaceYamlError::SecretInRegistryDeclaration {
                registry: redacted,
                field: field.clone(),
            });
        }
        if let Some(field) = declaration.unknown.keys().next() {
            return Err(LoadWorkspaceYamlError::UnknownRegistryDeclarationField {
                registry: redacted,
                field: field.clone(),
            });
        }
        // The map lives in the committed pnpm-workspace.yaml, and it already
        // refuses credential fields for that reason; a credential in the key
        // is the same secret in the same file.
        if registry_url_has_userinfo(registry) {
            return Err(LoadWorkspaceYamlError::CredentialsInRegistryKey { registry: redacted });
        }
        let normalized = normalize_registry_url(registry);
        for scope in declaration.scopes.iter().flatten() {
            if !scope.starts_with(DEFAULT_REGISTRY_SCOPE) {
                return Err(LoadWorkspaceYamlError::RegistryScopeWithoutAtSign {
                    registry: redacted,
                    scope: scope.clone(),
                });
            }
            if let Some(other) = routed_scopes.get(scope.as_str())
                && other != &normalized
            {
                return Err(LoadWorkspaceYamlError::ScopeRoutedTwice {
                    scope: scope.clone(),
                    registries: quote_and_join([other.as_str(), normalized.as_str()]),
                });
            }
            routed_scopes.insert(scope, normalized.clone());
        }
        if let Some(prefix) = declaration.prefix.as_deref()
            && !declared_prefixes.insert(prefix)
        {
            return Err(LoadWorkspaceYamlError::PrefixDeclaredTwice { prefix: prefix.to_owned() });
        }
    }
    Ok(())
}

/// Split validated declarations into the lookups. Infallible: every rejection
/// happens in [`validate`], at load time, where the offending file is known.
#[must_use]
pub fn into_lookups(entries: BTreeMap<String, RegistryEntry>) -> RegistryLookups {
    let mut lookups = RegistryLookups::default();
    let mut declarations = BTreeMap::new();
    for (registry, entry) in entries {
        match entry {
            RegistryEntry::ScopeRoute(url) => {
                let url = normalize_registry_url(&url);
                if registry == "default" {
                    lookups.default_registry = Some(url);
                } else {
                    lookups.registries_by_scope.insert(registry, url);
                }
            }
            RegistryEntry::Declaration(declaration) => {
                declarations.insert(registry, declaration);
            }
        }
    }
    extend_lookups_with_declarations(&mut lookups, declarations);
    lookups
}

/// The declarations-only half of [`into_lookups`], for a pnpr request, whose
/// `registries` map is always keyed by URL.
#[must_use]
pub fn declarations_into_lookups(
    entries: BTreeMap<String, RegistryDeclaration>,
) -> RegistryLookups {
    let mut lookups = RegistryLookups::default();
    extend_lookups_with_declarations(&mut lookups, entries);
    lookups
}

fn extend_lookups_with_declarations(
    lookups: &mut RegistryLookups,
    entries: BTreeMap<String, RegistryDeclaration>,
) {
    for (registry, declaration) in entries {
        let normalized = normalize_registry_url(&registry);
        if declaration.server_type.is_some() || declaration.supports_time_field.is_some() {
            lookups.registry_options_by_url.insert(
                normalized.clone(),
                RegistryOptions {
                    server_type: declaration.server_type,
                    supports_time_field: declaration.supports_time_field,
                },
            );
        }
        for scope in declaration.scopes.into_iter().flatten() {
            if scope == DEFAULT_REGISTRY_SCOPE {
                lookups.default_registry = Some(normalized.clone());
            } else {
                lookups.registries_by_scope.insert(scope, normalized.clone());
            }
        }
        if let Some(prefix) = declaration.prefix {
            lookups.registries_by_prefix.insert(prefix, registry);
        }
    }
}

/// Rebuild the declarations from the lookups they were split into, for a
/// client that has to describe its registries to a pnpr server.
///
/// The inverse of [`into_lookups`], minus the default registry: that one
/// travels as the request's own `registry` field, so a bare
/// [`DEFAULT_REGISTRY_SCOPE`] is not re-emitted here.
///
/// Entries are keyed by the URL each lookup holds rather than by a normalized
/// one, so a registry a prefix addresses without a trailing slash stays the
/// URL the client resolves against.
#[must_use]
pub fn to_declarations(lookups: &RegistryLookups) -> BTreeMap<String, RegistryDeclaration> {
    let mut declarations: BTreeMap<String, RegistryDeclaration> = BTreeMap::new();
    for (scope, registry) in &lookups.registries_by_scope {
        if scope == "default" {
            continue;
        }
        declarations
            .entry(registry.clone())
            .or_default()
            .scopes
            .get_or_insert_with(Vec::new)
            .push(scope.clone());
    }
    for (prefix, registry) in &lookups.registries_by_prefix {
        declarations.entry(registry.clone()).or_default().prefix = Some(prefix.clone());
    }
    for (registry, options) in &lookups.registry_options_by_url {
        let declaration = declarations.entry(registry.clone()).or_default();
        declaration.server_type = options.server_type;
        declaration.supports_time_field = options.supports_time_field;
    }
    declarations
}

/// [`to_declarations`] plus the default registry declared as the bare `@`
/// scope — the resolved view `pnpm config get registries` prints, where
/// nothing travels separately.
#[must_use]
pub fn to_resolved_declarations(
    lookups: &RegistryLookups,
) -> BTreeMap<String, RegistryDeclaration> {
    let mut declarations = to_declarations(lookups);
    if let Some(default_registry) = &lookups.default_registry {
        declarations
            .entry(default_registry.clone())
            .or_default()
            .scopes
            .get_or_insert_with(Vec::new)
            .insert(0, DEFAULT_REGISTRY_SCOPE.to_string());
    }
    declarations
}

/// Drop the entries whose request destination carries an unexpanded `${VAR}`.
/// The destination is the value of a scope route and the key of a
/// declaration, so which half is gated follows the entry's shape.
pub fn retain_without_env_placeholders(
    entries: &mut BTreeMap<String, RegistryEntry>,
    has_env_placeholder: impl Fn(&str) -> bool,
) {
    entries.retain(|registry, entry| match entry {
        RegistryEntry::ScopeRoute(url) => !has_env_placeholder(url),
        RegistryEntry::Declaration(_) => !has_env_placeholder(registry),
    });
}

fn looks_like_registry_url(key: &str) -> bool {
    key.contains("://") || key.starts_with("//")
}

fn quote_and_join<'a>(values: impl IntoIterator<Item = &'a str>) -> String {
    values.into_iter().map(|value| format!("{value:?}")).collect::<Vec<_>>().join(", ")
}
