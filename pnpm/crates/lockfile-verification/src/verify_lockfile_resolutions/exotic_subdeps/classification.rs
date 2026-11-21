use pnpm_lockfile::{
    BinaryResolution,
    LockfileResolution,
    PackageKey,
    PackageMetadata,
    PkgName,
    Prefix,
    TarballResolution,
    VariationsResolution,
    VersionPart,
    is_git_hosted_tarball_url,
};

use super::{
    super::offline_checks::is_http_tarball_url,
    ExoticSubdepsScope,
};

const DEFAULT_NODE_PREFIXES: &[&str] = &[
    "https://nodejs.org/download/",
    "https://nodejs.org/dist/",
    "https://unofficial-builds.nodejs.org/download/release/",
];

const DEFAULT_DENO_PREFIXES: &[&str] = &["https://github.com/denoland/deno/releases/download/"];

const DEFAULT_BUN_PREFIXES: &[&str] = &["https://github.com/oven-sh/bun/releases/download/"];

pub(super) struct RuntimeEdgeRef<'a> {
    pub(super) alias: &'a PkgName,
    pub(super) bare_key: &'a PackageKey,
    pub(super) metadata_version: Option<&'a str>,
    pub(super) scope: &'a ExoticSubdepsScope,
}

pub(super) fn exotic_resolved_via(
    alias: &PkgName,
    child_key: &PackageKey,
    metadata: Option<&PackageMetadata>,
    scope: &ExoticSubdepsScope,
) -> Option<&'static str> {
    let bare_key = child_key.without_peer();
    match metadata.map(|meta| &meta.resolution) {
        Some(resolution) => {
            let edge = RuntimeEdgeRef {
                alias,
                bare_key: &bare_key,
                metadata_version: metadata.and_then(|meta| meta.version.as_deref()),
                scope,
            };
            resolution_exotic_resolved_via(&edge, resolution)
        }
        None => missing_metadata_exotic_via(&bare_key),
    }
}

fn missing_metadata_exotic_via(bare_key: &PackageKey) -> Option<&'static str> {
    if bare_key.suffix.prefix() == Prefix::Runtime {
        return Some("url");
    }
    match bare_key.suffix.version() {
        VersionPart::NonSemver(_) => Some(classify_non_semver_locator(bare_key)),
        _ => None,
    }
}

pub(super) fn fallback_resolution(child_key: &PackageKey) -> LockfileResolution {
    LockfileResolution::Tarball(TarballResolution {
        tarball: child_key.suffix.version().to_string(),
        integrity: None,
        revision: None,
        git_hosted: None,
        path: None,
    })
}

fn resolution_exotic_resolved_via(
    edge: &RuntimeEdgeRef<'_>,
    resolution: &LockfileResolution,
) -> Option<&'static str> {
    match resolution {
        LockfileResolution::Git(_) => Some("git-repository"),
        LockfileResolution::Directory(_) | LockfileResolution::Custom(_) => None,
        LockfileResolution::Registry(_) => {
            (!is_registry_key(edge.bare_key)).then(|| classify_non_semver_locator(edge.bare_key))
        }
        LockfileResolution::Tarball(tarball) => tarball_exotic_resolved_via(edge.bare_key, tarball),
        LockfileResolution::Binary(binary) => binary_exotic_resolved_via(edge, binary),
        LockfileResolution::Variations(variations) => {
            variations_exotic_resolved_via(edge, variations)
        }
    }
}

fn binary_exotic_resolved_via(
    edge: &RuntimeEdgeRef<'_>,
    binary: &BinaryResolution,
) -> Option<&'static str> {
    let allowed = locked_runtime_version(edge)
        .is_some_and(|locked_version| {
            is_allowed_runtime_binary_url(
                edge.bare_key.name.bare.as_str(),
                &locked_version,
                &binary.url,
                edge.scope,
            )
        });
    (!allowed).then_some("url")
}

fn tarball_exotic_resolved_via(
    bare_key: &PackageKey,
    tarball: &TarballResolution,
) -> Option<&'static str> {
    if tarball.is_git_hosted() {
        return Some("git-repository");
    }
    let non_exotic = (is_registry_key(bare_key) && is_http_tarball_url(&tarball.tarball))
        || (is_file_key(bare_key) && tarball.tarball.starts_with("file:"));
    (!non_exotic).then_some("url")
}

fn variations_exotic_resolved_via(
    edge: &RuntimeEdgeRef<'_>,
    variations: &VariationsResolution,
) -> Option<&'static str> {
    if variations.variants.is_empty() {
        return Some("url");
    }
    variations.variants
        .iter()
        .find_map(|variant| resolution_exotic_resolved_via(edge, &variant.resolution))
}

pub(super) fn is_registry_key(bare_key: &PackageKey) -> bool {
    bare_key.suffix.prefix() == Prefix::None
        && matches!(
            bare_key.suffix.version(),
            VersionPart::Semver(_) | VersionPart::RegistryQualified { .. },
        )
}

fn is_file_key(bare_key: &PackageKey) -> bool {
    bare_key.suffix.prefix() == Prefix::None
        && matches!(bare_key.suffix.version(), VersionPart::File(_))
}

fn locked_runtime_version(edge: &RuntimeEdgeRef<'_>) -> Option<String> {
    if edge.bare_key.suffix.prefix() != Prefix::Runtime
        || edge.alias != &edge.bare_key.name
        || edge.bare_key.name.scope.is_some()
        || !matches!(edge.bare_key.name.bare.as_str(), "node" | "deno" | "bun")
    {
        return None;
    }
    let VersionPart::Semver(version) = edge.bare_key.suffix.version() else {
        return None;
    };
    let locked = version.to_string();
    if edge.metadata_version.is_some_and(|recorded| recorded != locked) {
        return None;
    }
    Some(locked)
}

fn is_allowed_runtime_binary_url(
    runtime_name: &str,
    locked_version: &str,
    url: &str,
    scope: &ExoticSubdepsScope,
) -> bool {
    if !is_http_tarball_url(url) || url.contains('\\') || has_unsafe_url_path_segments(url) {
        return false;
    }
    let Ok(parsed) = url::Url::parse(url) else { return false };
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return false;
    }
    let target = RuntimeArtifactTarget { runtime_name, locked_version, parsed: &parsed };
    match runtime_name {
        "node" => matches_any_prefix(&target, DEFAULT_NODE_PREFIXES, Some(&scope.node_mirrors)),
        "deno" => matches_any_prefix(&target, DEFAULT_DENO_PREFIXES, None),
        "bun" => matches_any_prefix(&target, DEFAULT_BUN_PREFIXES, Some(&scope.bun_mirrors)),
        _ => false,
    }
}

struct RuntimeArtifactTarget<'a> {
    runtime_name: &'a str,
    locked_version: &'a str,
    parsed: &'a url::Url,
}

fn matches_any_prefix(
    target: &RuntimeArtifactTarget<'_>,
    defaults: &[&str],
    mirrors: Option<&std::collections::BTreeSet<String>>,
) -> bool {
    defaults.iter().any(|prefix| url_matches_prefix(target, prefix))
        || mirrors.is_some_and(|custom| {
            custom.iter().any(|prefix| url_matches_prefix(target, prefix))
        })
}

fn url_matches_prefix(target: &RuntimeArtifactTarget<'_>, raw_prefix: &str) -> bool {
    let Ok(prefix) = url::Url::parse(raw_prefix) else { return false };
    if !prefix.path().ends_with('/')
        || target.parsed.scheme() != prefix.scheme()
        || target.parsed.host_str() != prefix.host_str()
        || target.parsed.port_or_known_default() != prefix.port_or_known_default()
    {
        return false;
    }
    let Some(rel_path) = target.parsed.path().strip_prefix(prefix.path()) else {
        return false;
    };
    runtime_artifact_subpath_matches(target.runtime_name, target.locked_version, rel_path)
}

fn runtime_artifact_subpath_matches(
    runtime_name: &str,
    locked_version: &str,
    rel_path: &str,
) -> bool {
    let segments: Vec<&str> = rel_path.split('/').collect();
    if segments.iter().any(|seg| seg.is_empty()) {
        return false;
    }
    match runtime_name {
        "node" => match segments.as_slice() {
            [release_dir, file_name] | [_, release_dir, file_name] => {
                *release_dir == format!("v{locked_version}")
                    && file_name.starts_with(&format!("node-v{locked_version}-"))
                    && (file_name.ends_with(".tar.gz") || file_name.ends_with(".zip"))
            }
            _ => false,
        },
        "deno" => matches!(
            segments.as_slice(),
            [release_dir, file_name]
                if *release_dir == format!("v{locked_version}")
                    && file_name.starts_with("deno-")
                    && file_name.ends_with(".zip"),
        ),
        "bun" => matches!(
            segments.as_slice(),
            [release_dir, file_name]
                if *release_dir == format!("bun-v{locked_version}")
                    && file_name.starts_with("bun-")
                    && file_name.ends_with(".zip"),
        ),
        _ => false,
    }
}

fn has_unsafe_url_path_segments(url: &str) -> bool {
    let Some((_, after_scheme)) = url.split_once("://") else { return true };
    let path_and_tail = after_scheme
        .find('/')
        .map_or("", |slash| &after_scheme[slash..]);
    let path_end = path_and_tail
        .find(['?', '#'])
        .unwrap_or(path_and_tail.len());
    let raw_path = &path_and_tail[..path_end];
    has_encoded_separator_or_dot(raw_path)
        || raw_path
            .split('/')
            .any(|seg| seg == "." || seg == "..")
}

fn has_encoded_separator_or_dot(raw_path: &str) -> bool {
    let bytes = raw_path.as_bytes();
    bytes
        .windows(3)
        .any(|win| {
            win[0] == b'%'
                && (win[1..].eq_ignore_ascii_case(b"2e")
                    || win[1..].eq_ignore_ascii_case(b"2f")
                    || win[1..].eq_ignore_ascii_case(b"5c"))
        })
}

pub(super) fn normalize_mirror_prefix(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || !is_http_tarball_url(trimmed) {
        return None;
    }
    if trimmed.ends_with('/') { Some(trimmed.to_string()) } else { Some(format!("{trimmed}/")) }
}

fn classify_non_semver_locator(bare_key: &PackageKey) -> &'static str {
    let raw = bare_key.suffix.version().to_string();
    if raw.starts_with("git+")
        || raw.starts_with("github:")
        || raw.starts_with("gitlab:")
        || raw.starts_with("bitbucket:")
        || is_git_hosted_tarball_url(&raw)
    {
        "git-repository"
    } else {
        "url"
    }
}
