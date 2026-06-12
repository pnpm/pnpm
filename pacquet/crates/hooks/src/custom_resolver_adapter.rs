use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use serde_json::Value;

use pacquet_resolving_resolver_base::{
    LatestQuery, PkgResolutionId, PreferredVersions, ResolveError, ResolveFuture,
    ResolveLatestFuture, ResolveOptions, ResolveResult, Resolver, VersionSelectorEntry,
    WantedDependency,
};

use crate::CustomResolver;

/// Adapts a [`CustomResolver`] (JSON-value-based, from a pnpmfile hook) to the
/// [`Resolver`] trait (typed structs) so it can be inserted into the resolver
/// chain. Mirrors `resolveFromCustomResolvers` in
/// <https://github.com/pnpm/pnpm/blob/1627943d2a/resolving/default-resolver/src/index.ts#L66-L98>.
pub struct CustomResolverAdapter {
    resolver: Arc<dyn CustomResolver>,
    can_resolve_cache: Mutex<HashMap<String, bool>>,
}

impl CustomResolverAdapter {
    pub fn new(resolver: Arc<dyn CustomResolver>) -> Self {
        Self { resolver, can_resolve_cache: Mutex::new(HashMap::new()) }
    }

    fn cache_key(wanted: &WantedDependency) -> String {
        format!(
            "{}@{}",
            wanted.alias.as_deref().unwrap_or(""),
            wanted.bare_specifier.as_deref().unwrap_or(""),
        )
    }

    fn wanted_to_value(wanted: &WantedDependency) -> Value {
        serde_json::json!({
            "alias": wanted.alias,
            "bareSpecifier": wanted.bare_specifier,
            "injected": wanted.injected,
            "optional": wanted.optional,
            "prevSpecifier": wanted.prev_specifier,
        })
    }

    fn preferred_versions_to_value(pv: &PreferredVersions) -> Value {
        let mut map = serde_json::Map::new();
        for (pkg_name, selectors) in pv {
            let mut smap = serde_json::Map::new();
            for (key, entry) in selectors {
                let val = match entry {
                    VersionSelectorEntry::Plain(typ) => {
                        serde_json::to_value(typ).unwrap_or(Value::Null)
                    }
                    VersionSelectorEntry::Weighted(w) => serde_json::json!({
                        "selectorType": w.selector_type,
                        "weight": w.weight,
                    }),
                };
                smap.insert(key.clone(), val);
            }
            map.insert(pkg_name.clone(), Value::Object(smap));
        }
        Value::Object(map)
    }

    fn opts_to_value(opts: &ResolveOptions) -> Value {
        serde_json::json!({
            "lockfileDir": opts.lockfile_dir.to_string_lossy(),
            "projectDir": opts.project_dir.to_string_lossy(),
            "preferredVersions": Self::preferred_versions_to_value(&opts.preferred_versions),
            "currentPkg": opts.current_pkg,
        })
    }
}

impl Resolver for CustomResolverAdapter {
    fn resolve<'a>(
        &'a self,
        wanted_dependency: &'a WantedDependency,
        opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        Box::pin(async move {
            let key = Self::cache_key(wanted_dependency);

            let cached = self.can_resolve_cache.lock().unwrap().get(&key).copied();
            let can = if let Some(cached) = cached {
                cached
            } else {
                let wanted_val = Self::wanted_to_value(wanted_dependency);
                let result = self.resolver.can_resolve(wanted_val).await.map_err(|err| {
                    Box::new(std::io::Error::other(err.to_string())) as ResolveError
                })?;
                self.can_resolve_cache.lock().unwrap().insert(key, result);
                result
            };

            if !can {
                return Ok(None);
            }

            let wanted_val = Self::wanted_to_value(wanted_dependency);
            let opts_val = Self::opts_to_value(opts);

            let result =
                self.resolver.resolve(wanted_val, opts_val).await.map_err(|err| {
                    Box::new(std::io::Error::other(err.to_string())) as ResolveError
                })?;

            let id = result.get("id").and_then(Value::as_str).ok_or_else(|| {
                let err: ResolveError = Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Custom resolver did not return an 'id' field",
                ));
                err
            })?;

            let resolution_val = result.get("resolution").ok_or_else(|| {
                let err: ResolveError = Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Custom resolver did not return a 'resolution' field",
                ));
                err
            })?;

            let resolution = serde_json::from_value(resolution_val.clone()).map_err(|err| {
                let resolve_err: ResolveError = Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Custom resolver returned invalid resolution: {err}"),
                ));
                resolve_err
            })?;

            // pnpm spreads the hook's whole result (`{ ...result,
            // resolvedVia: 'custom-resolver' }`), so a manifest the
            // resolver returns must survive — without it the installer
            // would re-fetch the tarball just to read `package.json`.
            let manifest = match result.get("manifest") {
                Some(manifest_val) => {
                    Some(Arc::new(serde_json::from_value(manifest_val.clone()).map_err(|err| {
                        let resolve_err: ResolveError = Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("Custom resolver returned invalid manifest: {err}"),
                        ));
                        resolve_err
                    })?))
                }
                None => None,
            };

            Ok(Some(ResolveResult {
                id: PkgResolutionId::from(id.to_string()),
                name_ver: None,
                latest: None,
                published_at: None,
                manifest,
                resolution,
                resolved_via: "custom-resolver".to_string(),
                normalized_bare_specifier: None,
                alias: wanted_dependency.alias.clone(),
                policy_violation: None,
            }))
        })
    }

    fn resolve_latest<'a>(
        &'a self,
        _query: &'a LatestQuery,
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(async move { Ok(None) })
    }
}

#[cfg(test)]
mod tests;
