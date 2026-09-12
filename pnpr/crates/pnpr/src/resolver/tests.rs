mod wire_protocol;

mod request_validation;

mod authorization;

mod behavior;

mod configuration_cache;

use axum::http::StatusCode;
use pnpm_config::{Config as PacquetConfig, RegistryDeclaration, ResolutionMode};
use pnpm_lockfile::Lockfile;
use pnpm_resolving_resolver_base::{
    PackageVersionGuard, PackageVersionGuardDecision, PackageVersionGuardFuture,
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use super::{
    cache::{MAX_RESOLUTION_CACHE_CANDIDATES_PER_KEY, cached_resolution},
    protocol::{ResolveRequest, ResolveRequestProject},
    reject_inline_url_auth, reject_invalid_patch_hashes, reject_off_allowlist_fetches,
    resolution_cache_key, store_resolution,
};
use pnpr_config::{Config as RegistryConfig, PublicRoute, UpstreamConfig};
use pnpr_policy::{AccessList, Identity, PackageRule, PackageRules};
use pnpr_route::{Footprint, PrivateAccessDescriptor, RouteContext};

fn config_for_registry(registry: &str) -> PacquetConfig {
    let mut config = PacquetConfig::new();
    config.registry = registry.to_string();
    config
}

fn config() -> PacquetConfig {
    config_for_registry("https://registry.example.test/")
}

fn deps(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries.iter().map(|(name, spec)| ((*name).to_string(), (*spec).to_string())).collect()
}

fn registry_config() -> RegistryConfig {
    RegistryConfig::proxy(
        "127.0.0.1:7677".parse::<SocketAddr>().unwrap(),
        PathBuf::from("/tmp/pnpr-resolver-cache-test"),
    )
}

fn public_registry_config(registry: &str) -> RegistryConfig {
    let mut config = registry_config();
    config
        .route_policy
        .public
        .push(PublicRoute { registry: Some(registry.to_string()), package: None });
    config
}

fn tarball_router(config: &RegistryConfig, identity: Identity) -> super::TarballRouter {
    tarball_router_with_registries(config, identity, HashMap::new())
}

fn tarball_router_with_registries(
    config: &RegistryConfig,
    identity: Identity,
    registries: HashMap<String, String>,
) -> super::TarballRouter {
    super::TarballRouter::new(
        Arc::new(RouteContext::from_config(config)),
        identity,
        config.public_url.clone(),
        registries,
    )
}

fn user(name: &str) -> Identity {
    Identity::user(name)
}

/// The standard token test upstreams carry; the credential digest it produces is
/// what [`private_alias_footprint`] records, so a footprint and an upstream built
/// with it share a credential epoch.
const ALIAS_TOKEN: &str = "Bearer alias-secret";

fn upstream_with_access(registry: &str, access: &str) -> UpstreamConfig {
    upstream_with_token(registry, access, ALIAS_TOKEN)
}

fn upstream_with_token(registry: &str, access: &str, token: &'static str) -> UpstreamConfig {
    let mut headers = reqwest::header::HeaderMap::new();
    headers
        .insert(reqwest::header::AUTHORIZATION, reqwest::header::HeaderValue::from_static(token));
    let mut upstream = UpstreamConfig::with_defaults(registry.to_string(), headers);
    upstream.access = Some(AccessList::from_tokens([access]));
    upstream
}

fn private_alias_footprint(alias: &str) -> Footprint {
    let mut footprint = Footprint::default();
    footprint.add(PrivateAccessDescriptor::Alias {
        alias: alias.to_string(),
        credential_digest: pnpr_route::credential_digest(ALIAS_TOKEN),
        package: None,
    });
    footprint
}

fn private_hosted_footprint(registry: &str, package: &str) -> Footprint {
    let mut footprint = Footprint::default();
    // Registry-qualified, matching `hosted_policy_id` in the route module.
    footprint.add(PrivateAccessDescriptor::Hosted { policy_id: format!("{registry}\0{package}") });
    footprint
}

/// Replace the `local` hosted registry's rules with a single-pattern map
/// whose `access` is `access`, so a test can rotate who may read a hosted
/// package.
fn set_local_hosted_rules(config: &mut RegistryConfig, pattern: &str, access: &str) {
    use pnpr_registry::{Ecosystem, PackagePattern};
    let rules = PackageRules::new(
        vec![PackageRule {
            pattern: PackagePattern::parse(pattern, Ecosystem::Npm).expect("test pattern parses"),
            access: Some(AccessList::from_tokens([access])),
            publish: Some(AccessList::from_tokens(["$authenticated"])),
            unpublish: None,
        }],
        None,
    );
    config.hosted.get_mut("local").expect("proxy config has a local hosted registry").rules = rules;
}

fn lockfile(version: &str) -> Lockfile {
    package_lockfile("acme", version)
}

fn package_lockfile(name: &str, version: &str) -> Lockfile {
    let mut packages = serde_json::Map::new();
    packages.insert(
        format!("{name}@{version}"),
        serde_json::json!({
            "resolution": { "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==" }
        }),
    );
    let mut dependencies = serde_json::Map::new();
    dependencies
        .insert(name.to_string(), serde_json::json!({ "specifier": "^1.0.0", "version": version }));
    serde_json::from_value(serde_json::json!({
        "lockfileVersion": "9.0",
        "importers": {
            ".": { "dependencies": dependencies }
        },
        "packages": packages
    }))
    .expect("lockfile parses")
}

fn lockfile_tarball_url(lockfile: &Lockfile, key: &str) -> String {
    let value = serde_json::to_value(lockfile).expect("lockfile serializes");
    value["packages"][key]["resolution"]["tarball"].as_str().expect("tarball URL").to_string()
}

#[derive(Debug)]
struct AllowAllVersions;

impl PackageVersionGuard for AllowAllVersions {
    fn check<'a>(&'a self, _name: &'a str, _version: &'a str) -> PackageVersionGuardFuture<'a> {
        Box::pin(async { Ok(PackageVersionGuardDecision::Allow) })
    }
}

fn lockfile_with_tarball(tarball: &str) -> Lockfile {
    serde_json::from_value(serde_json::json!({
        "lockfileVersion": "9.0",
        "importers": { ".": { "dependencies": { "acme": { "specifier": "^1.0.0", "version": "1.0.0" } } } },
        "packages": {
            "acme@1.0.0": {
                "resolution": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "tarball": tarball,
                },
            },
        },
    }))
    .expect("lockfile parses")
}
