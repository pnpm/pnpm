use super::{
    HashSet, InstallabilityOptions, LockfileResolution, PackageMetadata, PkgNameVerPeer,
    WalkContext, WantedPlatformRef, extract_author, extract_homepage,
    platform_is_supported_with_inference, safe_read_package_json_from_dir,
};

pub(super) fn extract_repository(manifest: &serde_json::Value) -> Option<String> {
    let repo = manifest.get("repository")?;
    if let Some(s) = repo.as_str() {
        return Some(s.to_string());
    }
    repo.get("url").and_then(|u| u.as_str()).map(ToString::to_string)
}

pub(super) fn strip_url_credentials(url: &str) -> String {
    if let Some(after_scheme) = url.find("://") {
        let scheme = &url[..after_scheme + 3];
        let rest = &url[after_scheme + 3..];
        if let Some(at_pos) = rest.find('@') {
            let after_host_start = &rest[at_pos + 1..];
            return format!("{scheme}{after_host_start}");
        }
    }
    url.to_string()
}

pub(super) fn extract_bugs_url(manifest: &serde_json::Value) -> Option<String> {
    let bugs = manifest.get("bugs")?;
    let url = if let Some(s) = bugs.as_str() {
        s.to_string()
    } else {
        bugs.get("url")?.as_str()?.to_string()
    };
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return None;
    }
    Some(strip_url_credentials(&url))
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
