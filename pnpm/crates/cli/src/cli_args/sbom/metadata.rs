use super::{
    HashSet, InstallabilityOptions, LockfileResolution, PackageMetadata, PkgNameVerPeer,
    WalkContext, WantedPlatformRef, extract_author, extract_homepage,
    platform_is_supported_with_inference, safe_read_package_json_from_dir,
};

/// The manifest's `repository` field as a URL an SBOM may publish, or
/// `None` when the value is not one. A `CycloneDX` `externalReferences[].url`
/// is an `iri-reference`, so a raw value like the npm `owner/repo`
/// shorthand fails schema validation in consumers such as Dependency-Track.
/// Absolute URLs are parsed and emitted in their normalized form, the
/// shorthand is expanded to the GitHub URL npm's own hosted-git-info
/// derives, and anything else (an scp-style remote, an email, a relative
/// path) is dropped.
pub(super) fn extract_repository(manifest: &serde_json::Value) -> Option<String> {
    let repo = manifest.get("repository")?;
    let raw = repo.as_str().or_else(|| repo.get("url").and_then(|u| u.as_str()))?.trim();
    repository_url(raw)
}

/// A `repository` value safe to emit as an SBOM URL: an absolute URL in its
/// normalized form, with embedded credentials stripped, or the expanded npm
/// `owner/repo` GitHub shorthand.
fn repository_url(raw: &str) -> Option<String> {
    if raw.contains("://") {
        return url_without_credentials(raw).map(|url| url.to_string());
    }
    github_shorthand_url(raw)
}

/// The npm `owner/repo` shorthand, which npm's hosted-git-info resolves to
/// a `git+https` GitHub URL (what `normalize-package-data` and `npm view`
/// derive, so an SBOM shows the URL npm itself would). A value is shorthand
/// only when it has exactly two non-empty segments and no scheme marker,
/// user, fragment, or whitespace.
fn github_shorthand_url(raw: &str) -> Option<String> {
    let segments = raw.split('/').collect::<Vec<_>>();
    if segments.len() != 2 || segments.iter().any(|segment| segment.is_empty()) {
        return None;
    }
    if raw.starts_with('.')
        || raw.contains(|ch: char| ch.is_ascii_whitespace() || matches!(ch, ':' | '@' | '#'))
    {
        return None;
    }
    let repository = if raw.ends_with(".git") { raw.to_string() } else { format!("{raw}.git") };
    Some(format!("git+https://github.com/{repository}"))
}

/// An absolute URL validated and normalized by the WHATWG parser, with any
/// `user:password` removed, or `None` when the value does not parse or the
/// password cannot be removed. Parsing is what makes the emitted form a
/// valid iri-reference: the serialized URL percent-encodes the whitespace
/// and control characters a raw passthrough would publish, and query or
/// fragment text can never be mistaken for userinfo. A bare username without
/// a password is part of the URL, not a credential, and stays. An SBOM is a
/// published artifact, so a URL whose password cannot be removed is dropped
/// rather than published with the secret.
pub(super) fn url_without_credentials(raw: &str) -> Option<url::Url> {
    let mut url = url::Url::parse(raw).ok()?;
    if url.password().is_some() {
        remove_userinfo(&mut url)?;
    }
    Some(url)
}

/// An absolute URL validated and normalized by the WHATWG parser, with all
/// userinfo removed, or `None` when the value does not parse or the userinfo
/// cannot be removed. `bugs` URLs always drop their userinfo (not just
/// password-bearing credentials) so no account name travels with a published
/// SBOM.
pub(super) fn url_without_userinfo(raw: &str) -> Option<url::Url> {
    let mut url = url::Url::parse(raw).ok()?;
    if url.username() != "" || url.password().is_some() {
        remove_userinfo(&mut url)?;
    }
    Some(url)
}

fn remove_userinfo(url: &mut url::Url) -> Option<()> {
    url.set_username("").ok()?;
    url.set_password(None).ok()?;
    Some(())
}

pub(super) fn extract_bugs_url(manifest: &serde_json::Value) -> Option<String> {
    let bugs = manifest.get("bugs")?;
    let raw = if let Some(s) = bugs.as_str() { s } else { bugs.get("url")?.as_str()? };
    let url = url_without_userinfo(raw)?;
    (url.scheme() == "http" || url.scheme() == "https").then(|| url.to_string())
}

fn registry_tarball_url(registry: &str, name: &str, version: &str) -> String {
    let registry = registry.trim_end_matches('/');
    let basename = name.rsplit('/').next().unwrap_or(name);
    format!("{registry}/{name}/-/{basename}-{version}.tgz")
}

pub(super) fn tarball_url_for_component(
    resolution: &LockfileResolution,
    name: &str,
    version: &str,
    registry: &str,
) -> Option<String> {
    match resolution {
        LockfileResolution::Registry(_) => Some(registry_tarball_url(registry, name, version)),
        LockfileResolution::Tarball(r) => Some(r.tarball.clone()),
        LockfileResolution::Git(r) => {
            let needs_prefix = r.repo.contains("://") && !r.repo.starts_with("git+");
            let prefix = if needs_prefix { "git+" } else { "" };
            Some(format!("{prefix}{}#{}", r.repo, r.commit))
        }
        _ => None,
    }
}

pub(super) fn encode_purl_name(name: &str) -> String {
    if let Some(rest) = name.strip_prefix('@') { format!("%40{rest}") } else { name.to_string() }
}

pub(super) fn build_purl(name: &str, version: &str) -> String {
    format!("pkg:npm/{}@{}", encode_purl_name(name), version)
}

pub(super) fn is_simple_spdx_id(license: &str) -> bool {
    !license.is_empty()
        && license
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '.' || ch == '+')
}

pub(super) fn classify_license(license: &str) -> serde_json::Value {
    let is_expression =
        license.split_whitespace().any(|word| word == "AND" || word == "OR" || word == "WITH");
    if is_expression {
        serde_json::json!({ "expression": license })
    } else if is_simple_spdx_id(license) {
        serde_json::json!({ "license": { "id": license } })
    } else {
        serde_json::json!({ "license": { "name": license } })
    }
}

/// The resolution's integrity, but only where pnpm checks the downloaded
/// bytes against it — so an SBOM never publishes a checksum as an assurance
/// pnpm did not make. A git resolution's recorded hash is not one: nothing
/// verifies a checkout against it (see
/// [`pnpm_lockfile::GitResolution::integrity`]).
pub(super) fn integrity_string(resolution: &LockfileResolution) -> Option<String> {
    match resolution {
        LockfileResolution::Registry(r) => Some(r.integrity.to_string()),
        LockfileResolution::Tarball(r) => r.integrity.as_ref().map(ToString::to_string),
        LockfileResolution::Binary(r) => Some(r.integrity.to_string()),
        _ => None,
    }
}

pub(super) fn peer_names_from_manifest(manifest: &serde_json::Value) -> HashSet<String> {
    let regular: HashSet<&str> = ["dependencies", "devDependencies", "optionalDependencies"]
        .iter()
        .flat_map(|field| {
            manifest
                .get(field)
                .and_then(|v| v.as_object())
                .into_iter()
                .flat_map(|obj| obj.keys().map(String::as_str))
        })
        .collect();

    manifest
        .get("peerDependencies")
        .and_then(|v| v.as_object())
        .into_iter()
        .flat_map(|obj| obj.keys())
        .filter(|name| !regular.contains(name.as_str()))
        .cloned()
        .collect()
}

pub(super) struct PkgMetadata {
    pub(super) license: Option<String>,
    pub(super) description: Option<String>,
    pub(super) author: Option<String>,
    pub(super) homepage: Option<String>,
    pub(super) repository: Option<String>,
    pub(super) bugs_url: Option<String>,
}

pub(super) fn read_pkg_metadata_from_store(
    key: &PkgNameVerPeer,
    pkg_name: &str,
    ctx: &WalkContext<'_>,
) -> PkgMetadata {
    let empty = PkgMetadata {
        license: None,
        description: None,
        author: None,
        homepage: None,
        repository: None,
        bugs_url: None,
    };
    let store_name = key.to_virtual_store_name(ctx.virtual_store_dir_max_length);
    for virtual_store_dir in ctx.virtual_store_dirs {
        let pkg_dir = virtual_store_dir.join(&store_name).join("node_modules").join(pkg_name);
        if let Ok(Some(manifest)) = safe_read_package_json_from_dir(&pkg_dir) {
            return PkgMetadata {
                license: manifest.get("license").and_then(|v| v.as_str()).map(ToString::to_string),
                description: manifest
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(ToString::to_string),
                author: extract_author(&manifest),
                homepage: extract_homepage(&manifest),
                repository: extract_repository(&manifest),
                bugs_url: extract_bugs_url(&manifest),
            };
        }
    }
    empty
}

/// Whether `package` is an optional dependency that pnpm would not install on
/// this host, matching the `pnpm licenses` filter. Such a package is recorded
/// in the lockfile but never fetched, so no metadata can be read for it from
/// the virtual store.
pub(super) fn platform_incompatible_optional(
    name: &str,
    snapshot_optional: bool,
    package: Option<&PackageMetadata>,
    installability: &InstallabilityOptions<'_>,
) -> bool {
    if !snapshot_optional {
        return false;
    }
    let Some(package) = package else {
        return false;
    };
    !platform_is_supported_with_inference(
        name,
        WantedPlatformRef {
            os: package.os.as_deref(),
            cpu: package.cpu.as_deref(),
            libc: package.libc.as_deref(),
        },
        installability,
    )
}

pub(super) fn generate_uuid_v4() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let state = RandomState::new();
    let mut hasher = state.build_hasher();
    hasher.write_u64(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64,
    );
    let half_a = hasher.finish();
    let mut hasher2 = state.build_hasher();
    hasher2.write_u64(!half_a);
    let half_b = hasher2.finish();
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&half_a.to_le_bytes());
    bytes[8..].copy_from_slice(&half_b.to_le_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    use std::fmt::Write;
    let mut uuid = String::with_capacity(36);
    for (i, byte) in bytes.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            uuid.push('-');
        }
        let _ = write!(uuid, "{byte:02x}");
    }
    uuid
}

pub(super) fn normalize_link_path(base_importer_id: &str, link_target: &str) -> Option<String> {
    let mut parts: Vec<&str> = if base_importer_id == "." {
        Vec::new()
    } else {
        base_importer_id.split('/').filter(|segment| !segment.is_empty()).collect()
    };
    for segment in link_target.split('/') {
        match segment {
            "" | "." => continue,
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() { Some(".".to_string()) } else { Some(parts.join("/")) }
}

pub(super) fn sanitize_package_name(name: &str) -> String {
    name.strip_prefix('@').unwrap_or(name).replace('/', "-")
}

pub(super) fn sanitize_path_segment(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|ch| {
            if matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
                || ch.is_ascii_control()
            {
                '-'
            } else {
                ch
            }
        })
        .collect();
    if sanitized == "." || sanitized == ".." || sanitized.trim().is_empty() {
        "-".to_string()
    } else {
        sanitized
    }
}

pub(super) fn base64_to_hex(input: &str) -> Option<String> {
    use base64::Engine;
    use std::fmt::Write;
    let bytes = base64::engine::general_purpose::STANDARD.decode(input).ok()?;
    let mut hex = String::with_capacity(bytes.len() * 2);
    for b in &bytes {
        let _ = write!(hex, "{b:02x}");
    }
    Some(hex)
}
