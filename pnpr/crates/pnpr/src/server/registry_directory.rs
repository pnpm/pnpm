use axum::{
    Json,
    extract::State,
    response::{IntoResponse as _, Response},
};
use pnpr_registry::{Ecosystem, Registries, Registry};
use serde_json::{Value, json};

use super::{AppState, AuthedCaller, private_no_cache};

pub(super) async fn serve(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
) -> Response {
    let config = &state.inner.config;
    let registries = &config.registries;
    let visible = |key: &str| match registries.get(key) {
        Some(Registry::Hosted { .. }) => config
            .hosted
            .get(key)
            .is_some_and(|hosted| hosted.rules.default_access().allows(&identity)),
        Some(Registry::Upstream { .. }) => config.upstreams.get(key).is_some_and(|upstream| {
            upstream.access.as_ref().is_none_or(|access| access.allows(&identity))
                && upstream.rules.default_access().allows(&identity)
        }),
        _ => false,
    };
    let mut entries = Vec::new();
    let mut defaults = serde_json::Map::new();
    for ecosystem in Ecosystem::all() {
        for key in registries.names() {
            if registries.ecosystem(key).is_some_and(|declared| declared != ecosystem) {
                continue;
            }
            let sources = registries.sources(key, ecosystem);
            if !sources.iter().any(|source| visible(source)) {
                continue;
            }
            let Some(registry) = registries.get(key) else { continue };
            let (kind, patterns, route_sources) = match registry {
                Registry::Hosted { patterns } | Registry::Upstream { patterns } => {
                    let (kind, rules) = match registry {
                        Registry::Hosted { .. } => ("hosted", &config.hosted[key].rules),
                        _ => ("upstream", &config.upstreams[key].rules),
                    };
                    let patterns = (!rules.refines_access()).then(|| {
                        if patterns.is_empty() {
                            vec!["**".to_string()]
                        } else {
                            patterns.iter().map(ToString::to_string).collect()
                        }
                    });
                    (kind, patterns, None)
                }
                Registry::Router { .. } => {
                    let disclosed = sources.iter().all(|source| visible(source)).then(|| {
                        sources
                            .iter()
                            .map(|source| Registries::local_name(source))
                            .collect::<Vec<_>>()
                    });
                    ("router", None, disclosed)
                }
            };
            let name = Registries::local_name(key);
            entries.push(json!({
                "name": name, "kind": kind, "ecosystem": ecosystem.to_string(),
                "patterns": patterns, "sources": route_sources,
            }));
            if registries.default_for(ecosystem) == Some(key) {
                defaults.insert(ecosystem.to_string(), json!(name));
            }
        }
    }
    let ecosystems: serde_json::Map<String, Value> = Ecosystem::all()
        .map(|ecosystem| {
            let name = ecosystem.to_string();
            let available = entries.iter().any(|entry| entry["ecosystem"] == name);
            let prefixed = ecosystem != Ecosystem::Oci && !registries.is_only_ecosystem(ecosystem);
            let mut capability = json!({ "available": available, "prefixed": prefixed });
            if ecosystem == Ecosystem::Oci {
                capability["namedPrefixed"] = json!(!registries.is_only_ecosystem(ecosystem));
            }
            (name, capability)
        })
        .collect();
    private_no_cache(
        Json(json!({
            "registries": entries, "defaultRegistries": defaults, "ecosystems": ecosystems,
        }))
        .into_response(),
    )
}
