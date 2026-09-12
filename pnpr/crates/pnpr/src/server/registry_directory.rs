use axum::{
    Json,
    extract::State,
    response::{IntoResponse as _, Response},
};
use pnpr_config::Config;
use pnpr_registry::{Ecosystem, PackagePattern, Registries, Registry};
use serde_json::{Value, json};

use super::{AppState, AuthedCaller, private_no_cache};
use pnpr_policy::{Identity, PackageRules};

pub(super) async fn serve(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
) -> Response {
    let config = &state.inner.config;
    let registries = &config.registries;
    let visible = |key: &str| registry_is_visible(config, &identity, key);
    let mut entries = Vec::new();
    let mut defaults = serde_json::Map::new();
    for ecosystem in Ecosystem::all() {
        for key in registries.names() {
            let Some(entry) = directory_entry(config, key, ecosystem, &visible) else {
                continue;
            };
            entries.push(entry);
            if registries.default_for(ecosystem) == Some(key) {
                defaults.insert(ecosystem.to_string(), json!(Registries::local_name(key)));
            }
        }
    }
    let ecosystems: serde_json::Map<String, Value> = Ecosystem::all()
        .map(|ecosystem| ecosystem_capability(registries, &entries, ecosystem))
        .collect();
    private_no_cache(
        Json(json!({
            "registries": entries, "defaultRegistries": defaults, "ecosystems": ecosystems,
        }))
        .into_response(),
    )
}

/// What one ecosystem's clients can expect of this server.
fn ecosystem_capability(
    registries: &Registries,
    entries: &[Value],
    ecosystem: Ecosystem,
) -> (String, Value) {
    let name = ecosystem.to_string();
    let available = entries.iter().any(|entry| entry["ecosystem"] == name);
    let prefixed = ecosystem != Ecosystem::Oci && !registries.is_only_ecosystem(ecosystem);
    let mut capability = json!({ "available": available, "prefixed": prefixed });
    if ecosystem == Ecosystem::Oci {
        capability["namedPrefixed"] = json!(!registries.is_only_ecosystem(ecosystem));
    }
    (name, capability)
}

/// Whether a caller can see a registry at all: it must be one whose default
/// access admits them.
fn registry_is_visible(config: &Config, identity: &Identity, key: &str) -> bool {
    match config.registries.get(key) {
        Some(Registry::Hosted { .. }) => config
            .hosted
            .get(key)
            .is_some_and(|hosted| hosted.rules.default_access().allows(identity)),
        Some(Registry::Upstream { .. }) => config.upstreams.get(key).is_some_and(|upstream| {
            upstream.access.as_ref().is_none_or(|access| access.allows(identity))
                && upstream.rules.default_access().allows(identity)
        }),
        _ => false,
    }
}

/// One registry's directory entry, or `None` when this ecosystem does not
/// reach it or the caller cannot see any of its sources.
fn directory_entry(
    config: &Config,
    key: &str,
    ecosystem: Ecosystem,
    visible: &impl Fn(&str) -> bool,
) -> Option<Value> {
    let registries = &config.registries;
    if registries.ecosystem(key).is_some_and(|declared| declared != ecosystem) {
        return None;
    }
    let sources = registries.sources(key, ecosystem);
    if !sources.iter().any(|source| visible(source)) {
        return None;
    }
    let registry = registries.get(key)?;
    let (kind, patterns, route_sources) = match registry {
        Registry::Hosted { patterns } => {
            ("hosted", disclosed_patterns(&config.hosted[key].rules, patterns), None)
        }
        Registry::Upstream { patterns } => {
            ("upstream", disclosed_patterns(&config.upstreams[key].rules, patterns), None)
        }
        Registry::Router { .. } => ("router", None, disclosed_sources(&sources, visible)),
    };
    Some(json!({
        "name": Registries::local_name(key), "kind": kind,
        "ecosystem": ecosystem.to_string(),
        "patterns": patterns, "sources": route_sources,
    }))
}

/// A router's sources, disclosed only when the caller can see every one.
fn disclosed_sources(sources: &[&str], visible: &impl Fn(&str) -> bool) -> Option<Vec<String>> {
    sources
        .iter()
        .all(|source| visible(source))
        .then(|| sources.iter().map(|source| Registries::local_name(source).to_string()).collect())
}

/// A registry's claimed patterns, disclosed only when its rules do not refine
/// access per package — otherwise the pattern list would say what a caller may
/// not read.
fn disclosed_patterns(rules: &PackageRules, patterns: &[PackagePattern]) -> Option<Vec<String>> {
    (!rules.refines_access()).then(|| {
        if patterns.is_empty() {
            vec!["**".to_string()]
        } else {
            patterns.iter().map(ToString::to_string).collect()
        }
    })
}
