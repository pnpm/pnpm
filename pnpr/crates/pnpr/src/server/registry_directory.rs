use axum::{
    Json,
    extract::State,
    response::{IntoResponse as _, Response},
};
use pnpr_registry::{Ecosystem, Registry};
use serde_json::{Value, json};

use super::{AppState, AuthedCaller, private_no_cache};

pub(super) async fn serve(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
) -> Response {
    let config = &state.inner.config;
    let registries = &config.registries;
    let visible = |name: &str| match registries.get(name) {
        Some(Registry::Hosted { .. }) => config
            .hosted
            .get(name)
            .is_some_and(|hosted| hosted.rules.default_access().allows(&identity)),
        Some(Registry::Upstream { .. }) => config.upstreams.get(name).is_some_and(|upstream| {
            upstream.access.as_ref().is_none_or(|access| access.allows(&identity))
                && upstream.rules.default_access().allows(&identity)
        }),
        _ => false,
    };
    let mut entries = Vec::new();
    for name in registries.names() {
        let Some(registry) = registries.get(name) else { continue };
        let sources = match registry {
            Registry::Router { sources } => sources.iter().map(String::as_str).collect(),
            _ => vec![name],
        };
        let ecosystems: Vec<_> = Ecosystem::all()
            .filter(|ecosystem| {
                sources.iter().any(|source| {
                    visible(source) && registries.ecosystem(source) == Some(*ecosystem)
                })
            })
            .map(|ecosystem| ecosystem.to_string())
            .collect();
        if ecosystems.is_empty() {
            continue;
        }
        let (kind, patterns, route_sources) = match registry {
            Registry::Hosted { patterns } | Registry::Upstream { patterns } => {
                let (kind, rules) = match registry {
                    Registry::Hosted { .. } => ("hosted", &config.hosted[name].rules),
                    _ => ("upstream", &config.upstreams[name].rules),
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
            Registry::Router { sources } => {
                ("router", None, sources.iter().all(|source| visible(source)).then_some(sources))
            }
        };
        entries.push(json!({
            "name": name, "kind": kind, "ecosystems": ecosystems,
            "patterns": patterns, "sources": route_sources,
        }));
    }
    let default_registry = registries
        .default_registry()
        .filter(|name| entries.iter().any(|entry| entry["name"] == *name));
    let ecosystems: serde_json::Map<String, Value> = Ecosystem::all()
        .map(|ecosystem| {
            let name = ecosystem.to_string();
            let available = entries.iter().any(|entry| {
                entry["ecosystems"]
                    .as_array()
                    .is_some_and(|ecosystems| ecosystems.iter().any(|value| value == &name))
            });
            let prefixed = ecosystem != Ecosystem::Oci && !registries.is_only_ecosystem(ecosystem);
            (name, json!({ "available": available, "prefixed": prefixed }))
        })
        .collect();
    private_no_cache(
        Json(json!({
            "registries": entries, "defaultRegistry": default_registry, "ecosystems": ecosystems,
        }))
        .into_response(),
    )
}
