use super::{
    BinaryArchive, BinaryResolution, BinarySpec, DirectoryResolution, GitResolution,
    LockfileFormError, LockfileFormOptions, LockfileResolution, PlatformAssetResolution,
    PlatformAssetTarget, PlatformSelector, RegistryOptions, RegistryResolution, RegistryServerType,
    TarballResolution, TarballRevision, TarballUrlOptions, VariationsResolution,
    integrity_addressed_registry_tarball_url, is_git_hosted_tarball_url,
    is_integrity_addressed_registry_tarball_url, libc_matches, npm_tarball_url,
    registry_server_type, select_platform_variant,
};
use crate::serialize_yaml;
use pretty_assertions::assert_eq;
use ssri::Integrity;
use std::collections::BTreeMap;
use text_block_macros::text_block;

const GIT_COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

/// No declared server type — the strict default every registry but
/// registry.npmjs.org is read with.
fn undeclared_form(registry: &str, include_tarball_url: bool) -> LockfileFormOptions<'_> {
    LockfileFormOptions { registry, server_type: None, include_tarball_url }
}

fn integrity(integrity_str: &str) -> Integrity {
    integrity_str.parse().expect("parse integrity string")
}

/// Render a resolution exactly as it appears under a `packages:` entry, then
/// dedent the `resolution:` block. Exercises the real write path: the deep key
/// sort and the single-line-vs-block decision both depend on the `resolution`
/// key and its enclosing `packages` context, so a bare top-level serialization
/// would not reflect what pnpm writes.
fn render_resolution(resolution: &LockfileResolution) -> String {
    let document = serde_json::json!({
        "packages": {
            "p@1.0.0": { "resolution": serde_json::to_value(resolution).unwrap() },
        },
    });
    serialize_yaml::to_string(&document)
        .unwrap()
        .lines()
        .skip_while(|line| !line.trim_start().starts_with("resolution:"))
        .map(|line| line.strip_prefix("    ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

// -----------------------------------------------------------------------------
// `select_platform_variant` / `libc_matches` — Slice B
// -----------------------------------------------------------------------------

fn binary_resolution(url: &str) -> LockfileResolution {
    LockfileResolution::Binary(BinaryResolution {
        url: url.to_string(),
        integrity: integrity(
            "sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==",
        ),
        bin: BinarySpec::Single("bin/node".to_string()),
        archive: BinaryArchive::Tarball,
        prefix: None,
    })
}

fn target(os: &str, cpu: &str, libc: Option<&str>) -> PlatformAssetTarget {
    PlatformAssetTarget { os: os.to_string(), cpu: cpu.to_string(), libc: libc.map(str::to_string) }
}

fn variant(url: &str, targets: Vec<PlatformAssetTarget>) -> PlatformAssetResolution {
    PlatformAssetResolution { resolution: binary_resolution(url), targets }
}

fn selector(os: &str, cpu: &str, libc: Option<&str>) -> PlatformSelector {
    PlatformSelector { os: os.to_string(), cpu: cpu.to_string(), libc: libc.map(str::to_string) }
}

const SHA512: &str = "sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==";
const REVISION_SHA512: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

// --- Custom resolutions ---

fn custom_cdn_resolution() -> LockfileResolution {
    let mut extra = serde_json::Map::new();
    extra.insert(
        "url".to_string(),
        serde_json::Value::String("https://cdn.example.com/pkg.tgz".to_string()),
    );
    extra.insert("integrity".to_string(), serde_json::Value::String(SHA512.to_string()));
    LockfileResolution::Custom(super::CustomResolution {
        resolution_type: "custom:cdn".to_string().try_into().expect("custom type tag"),
        extra,
    })
}

const ARTIFACTORY_REGISTRY: &str = "https://artifactory.example/artifactory/api/npm/npm-virtual/";

fn artifactory_form(include_tarball_url: bool) -> LockfileFormOptions<'static> {
    LockfileFormOptions {
        registry: ARTIFACTORY_REGISTRY,
        server_type: Some(RegistryServerType::Artifactory),
        include_tarball_url,
    }
}

mod integrity;

mod lockfile_deserialize_tarball_resolution;

mod lockfile_deserialize_rejects_malformed_builtin;

mod behavior;

mod streaming;

mod security;
