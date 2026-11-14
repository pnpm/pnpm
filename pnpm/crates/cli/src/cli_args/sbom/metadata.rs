use super::{
    HashMap, HashSet, HostedGit, HostedOpts, InstallabilityOptions, LockfileResolution,
    PackageMetadata, PkgNameVerPeer, RegistryContext, RegistryServerType, TarballUrlOptions,
    WalkContext, WantedPlatformRef, extract_author, extract_homepage, npm_tarball_url,
    platform_is_supported_with_inference, registry_server_type, revision_registry_tarball_url,
    safe_read_package_json_from_dir,
};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

/// The manifest's `repository` field as a URL an SBOM may publish.
/// `CycloneDX` requires an `iri-reference` here, so a raw value like the npm
/// `owner/repo` shorthand fails validation in consumers such as
/// Dependency-Track.
pub(super) fn extract_repository(manifest: &serde_json::Value) -> Option<String> {
    let repo = manifest.get("repository")?;
    let raw = repo
        .as_str()
        .or_else(|| repo.get("url").and_then(|u| u.as_str()))?
        .trim();
    repository_url(raw)
}

fn repository_url(raw: &str) -> Option<String> {
    if let Some(url) = absolute_url(raw) {
        return Some(url);
    }
    let hosted = HostedGit::from_url(raw)?;
    if !hosted.names_repository() {
        return None;
    }
    let expanded = hosted.https(HostedOpts::default())?;
    url_without_credentials(&expanded).map(|url| url.to_string())
}

/// The value as an absolute URL. One with no host is not: `github:owner/repo`
/// belongs to the shorthand parser, and `mailto:` and `file:` values name no
/// repository a consumer can reach.
fn absolute_url(raw: &str) -> Option<String> {
    let url = url_without_credentials(raw)?;
    url.host_str()
        .is_some_and(|host| !host.is_empty())
        .then(|| url.to_string())
}

/// An absolute URL in the WHATWG parser's normalized form, without the
/// userinfo an SBOM must not publish. Parsing is most of what makes the result
/// a valid iri-reference: it percent-encodes whitespace and control
/// characters, and query text can never be mistaken for userinfo.
pub(super) fn url_without_credentials(raw: &str) -> Option<url::Url> {
    let mut url = url::Url::parse(raw).ok()?;
    if !percent_escapes_are_complete(url.as_str()) {
        return None;
    }
    // `ssh` and `git+ssh` address their host as `git@github.com`, so a
    // username with no password is part of the address there. Under any other
    // scheme it can be the secret itself: GitHub and GitLab take a token in
    // place of the whole `user:password`.
    let ssh_login = url.password().is_none() && matches!(url.scheme(), "ssh" | "git+ssh");
    if !ssh_login && (!url.username().is_empty() || url.password().is_some()) {
        url.set_username("").ok()?;
        url.set_password(None).ok()?;
    }
    Some(url)
}

/// The parser keeps a `%` that begins no `%XX` escape as the manifest wrote
/// it, and an iri-reference admits no such thing.
fn percent_escapes_are_complete(url: &str) -> bool {
    url.split('%')
        .skip(1)
        .all(|rest| {
            let mut escape = rest.bytes().take(2);
            escape.len() == 2 && escape.all(|byte| byte.is_ascii_hexdigit())
        })
}

/// The manifest's `description` as an SBOM may publish it. A blank one
/// describes nothing, so it is dropped rather than serialized as an empty
/// string.
pub(super) fn extract_description(manifest: &serde_json::Value) -> Option<String> {
    manifest
        .get("description")
        .and_then(|v| v.as_str())
        .filter(|description| !description.is_empty())
        .map(ToString::to_string)
}

pub(super) fn extract_bugs_url(manifest: &serde_json::Value) -> Option<String> {
    let bugs = manifest.get("bugs")?;
    let raw = if let Some(s) = bugs.as_str() { s } else { bugs.get("url")?.as_str()? };
    let url = url_without_credentials(raw)?;
    (url.scheme() == "http" || url.scheme() == "https").then(|| url.to_string())
}

/// Characters left unescaped by JavaScript's `encodeURIComponent`, matching
/// `@pnpm/deps.compliance.sbom`'s PURL qualifier encoding.
const ENCODE_URI_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

/// Strip credentials, query parameters, and fragments from an `http` or
/// `https` registry URL before publishing it in an SBOM, keeping origin and
/// path so registries that differ only by path remain distinct. `None` when
/// `raw` is not a valid `http(s)` URL with a host, so a malformed string can
/// never publish embedded credentials.
pub(super) fn sanitize_registry_url(raw: &str) -> Option<String> {
    let mut parsed = url::Url::parse(raw).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    if parsed.host_str().is_none_or(str::is_empty) {
        return None;
    }
    if !parsed.username().is_empty() {
        parsed.set_username("").ok()?;
    }
    if parsed.password().is_some() {
        parsed.set_password(None).ok()?;
    }
    parsed.set_query(None);
    parsed.set_fragment(None);
    percent_escapes_are_complete(parsed.as_str()).then(|| parsed.to_string())
}

pub(super) fn encode_repository_url(sanitized_url: &str) -> String {
    utf8_percent_encode(sanitized_url, ENCODE_URI_COMPONENT).to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SanitizedNamedRegistry {
    pub(super) sanitized_url: String,
    pub(super) encoded_repository_url: String,
    pub(super) server_type: Option<RegistryServerType>,
}

/// Registry routing context with named, scoped, and default registry URLs
/// sanitized once per SBOM walk.
pub(super) struct SanitizedRegistries {
    context: RegistryContext,
    by_prefix: HashMap<String, Option<SanitizedNamedRegistry>>,
    routed: HashMap<String, SanitizedNamedRegistry>,
}

fn sanitize_registry_entry(
    raw_url: &str,
    context: &RegistryContext,
) -> Option<SanitizedNamedRegistry> {
    let sanitized_url = sanitize_registry_url(raw_url)?;
    let encoded_repository_url = encode_repository_url(&sanitized_url);
    let server_type = registry_server_type(&context.registry_options_by_url, raw_url)
        .or_else(|| registry_server_type(&context.registry_options_by_url, &sanitized_url));
    Some(SanitizedNamedRegistry { sanitized_url, encoded_repository_url, server_type })
}

impl SanitizedRegistries {
    pub(super) fn new(context: RegistryContext) -> Self {
        let by_prefix = context.registries_by_prefix
            .iter()
            .map(|(prefix, raw_url)| (prefix.clone(), sanitize_registry_entry(raw_url, &context)))
            .collect();
        let routed = context.registries
            .values()
            .map(String::as_str)
            .chain(std::iter::once("https://registry.npmjs.org/"))
            .filter_map(|raw_url| {
                sanitize_registry_entry(raw_url, &context).map(|entry| (raw_url.to_string(), entry))
            })
            .collect();
        Self { context, by_prefix, routed }
    }

    pub(super) fn resolve_named(
        &self,
        key: &PkgNameVerPeer,
    ) -> miette::Result<Option<&SanitizedNamedRegistry>> {
        let Some((registry_name, _)) = key.suffix.registry_qualified() else {
            return Ok(None);
        };
        match self.by_prefix.get(registry_name) {
            None => Err(miette::miette!(
                code = "ERR_PNPM_MISSING_NAMED_REGISTRY",
                help = format!(
                    r#"Add a registries entry with "prefix: {registry_name}" to pnpm-workspace.yaml."#,
                ),
                r#"Cannot describe package "{key}": its registry prefix '{registry_name}:' is not declared by the registries setting."#,
            )),
            Some(None) => Err(miette::miette!(
                code = "ERR_PNPM_INVALID_NAMED_REGISTRY_URL",
                help = "Provide a URL that starts with http:// or https://, e.g. https://npm.pkg.example.com/",
                "The named registry alias '{registry_name}' is not mapped to a valid http(s) URL.",
            )),
            Some(Some(entry)) => Ok(Some(entry)),
        }
    }

    pub(super) fn tarball_url_for_component(
        &self,
        resolution: &LockfileResolution,
        name: &str,
        version: &str,
        named_registry: Option<&SanitizedNamedRegistry>,
    ) -> Option<String> {
        match resolution {
            LockfileResolution::Registry(reg) => {
                self.registry_tarball_url(reg, name, version, named_registry)
            }
            LockfileResolution::Tarball(tarball) => Some(tarball.tarball.clone()),
            LockfileResolution::Git(git) => {
                let needs_prefix = git.repo.contains("://") && !git.repo.starts_with("git+");
                let prefix = if needs_prefix { "git+" } else { "" };
                Some(format!("{prefix}{}#{}", git.repo, git.commit))
            }
            _ => None,
        }
    }

    fn registry_tarball_url(
        &self,
        reg: &pnpm_lockfile::RegistryResolution,
        name: &str,
        version: &str,
        named_registry: Option<&SanitizedNamedRegistry>,
    ) -> Option<String> {
        let entry = named_registry.or_else(|| self.routed.get(self.context.routed_registry(name)))?;
        if let Some(revision) = reg.revision {
            return revision_registry_tarball_url(name, version, revision, &entry.sanitized_url);
        }
        Some(npm_tarball_url(
            name,
            version,
            TarballUrlOptions { registry: &entry.sanitized_url, server_type: entry.server_type },
        ))
    }
}

pub(super) fn encode_purl_name(name: &str) -> String {
    if let Some(rest) = name.strip_prefix('@') { format!("%40{rest}") } else { name.to_string() }
}

pub(super) fn build_purl(name: &str, version: &str) -> String {
    build_purl_with_encoded_registry(name, version, None)
}

pub(super) fn build_purl_with_encoded_registry(
    name: &str,
    version: &str,
    encoded_repository_url: Option<&str>,
) -> String {
    let base = format!("pkg:npm/{}@{}", encode_purl_name(name), version);
    match encoded_repository_url {
        Some(encoded) => format!("{base}?repository_url={encoded}"),
        None => base,
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
        let pkg_dir = virtual_store_dir
            .join(&store_name)
            .join("node_modules")
            .join(pkg_name);
        if let Ok(Some(manifest)) = safe_read_package_json_from_dir(&pkg_dir) {
            return PkgMetadata {
                license: manifest
                    .get("license")
                    .and_then(|v| v.as_str())
                    .map(ToString::to_string),
                description: extract_description(&manifest),
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
        base_importer_id
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect()
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
    name.strip_prefix('@')
        .unwrap_or(name)
        .replace('/', "-")
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
