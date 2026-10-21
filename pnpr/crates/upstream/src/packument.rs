use super::{
    CanonicalPackageName, Integrity, MAX_TARBALL_REVISION, TarballRevision, Value,
    integrity_addressed_registry_tarball_url, is_integrity_addressed_registry_tarball_url,
};

/// Rewrite every `dist.tarball` in `value` to a URL served by *this*
/// registry instead of whatever URL the source put there. The new URL is
/// `{public_url}/{pkg}/-/{basename}`, where `basename` is the last
/// `/`-separated segment of the original tarball URL. This handles both
/// npm's canonical `/{pkg}/-/{basename}` shape and verdaccio's
/// `/{scope}/{name}/-/{scope}/{filename}` shape uniformly — we only look
/// at the basename, never at the path prefix.
///
/// The basename is preserved verbatim rather than reconstructed from the
/// version, so a non-canonical tarball name (e.g. esprima-fb's zero-padded
/// `esprima-fb-3001.0001.0000-dev-harmony-fb.tgz` for version
/// `3001.1.0-dev-harmony-fb`) survives into the client's lockfile and is
/// fetched back from the path the upstream actually hosts. The tarball
/// endpoint binds each request to a version's declared `dist.integrity`
/// (see `serve_tarball`), so a preserved name can't smuggle in unverified
/// bytes.
///
/// Walks both packument shape (`{ "versions": { v: { dist: ... } } }`)
/// and single-version manifest shape (`{ dist: ... }` at the top level)
/// so a single helper covers both endpoints.
pub fn rewrite_tarball_urls(value: &mut Value, pkg: &CanonicalPackageName, public_url: &str) {
    rewrite_tarball_urls_from_registry(value, pkg, None, public_url);
}

/// Rewrite tarball URLs from an upstream registry, preserving valid revision routes.
pub fn rewrite_upstream_tarball_urls(
    value: &mut Value,
    pkg: &CanonicalPackageName,
    source_registry: &str,
    public_url: &str,
) {
    rewrite_tarball_urls_from_registry(value, pkg, Some(source_registry), public_url);
}

pub(super) fn rewrite_tarball_urls_from_registry(
    value: &mut Value,
    pkg: &CanonicalPackageName,
    source_registry: Option<&str>,
    public_url: &str,
) {
    let public_url = public_url.trim_end_matches('/');
    if let Some(versions) = value.get_mut("versions").and_then(Value::as_object_mut) {
        for manifest in versions.values_mut() {
            rewrite_dist_tarball(manifest, pkg, source_registry, public_url);
        }
    }
    rewrite_dist_tarball(value, pkg, source_registry, public_url);
}

pub(super) fn rewrite_dist_tarball(
    value: &mut Value,
    pkg: &CanonicalPackageName,
    source_registry: Option<&str>,
    public_url: &str,
) {
    // Every string `dist.tarball` must be rewritten to a route on *this*
    // server, where integrity and OSV are enforced — never passed through.
    // When the upstream URL has no usable basename (e.g. it ends in `/`), fall
    // back to the version-derived canonical name (the manifest carries its own
    // `version`) so a malformed URL still points at pnpr (and 404s there)
    // rather than directing the client at an arbitrary upstream host.
    let version = value
        .get("version")
        .and_then(Value::as_str)
        .map(str::to_string);
    let fallback = version
        .as_deref()
        .map(|version| pkg.tarball_name_for_version(version));
    let Some(dist) = value.get_mut("dist").and_then(Value::as_object_mut) else {
        return;
    };
    dist.remove("pnpr");
    rewrite_dist_attestations(dist, pkg, version.as_deref(), public_url);
    if let Some(source_registry) = source_registry {
        rewrite_upstream_revision_tarball_urls(dist, source_registry, public_url);
    }
    let revision_url = source_registry.and_then(|source_registry| {
        unique_revision_url(dist, source_registry, public_url)
    });
    if let Some(revision_url) = revision_url {
        dist.insert("tarball".to_string(), Value::String(revision_url));
        // They sign or hash a source `.tgz`, not the revision's tar.
        for source_field in ["shasum", "signatures", "npm-signature"] {
            dist.remove(source_field);
        }
        return;
    }
    // Without a validated selection, the version is ordinary metadata.
    dist.remove("revision");
    dist.remove("revisions");
    let Some(tarball_value) = dist.get_mut("tarball") else { return };
    if !tarball_value.is_string() {
        return;
    }
    let filename = tarball_value
        .as_str()
        .and_then(tarball_basename)
        .map(str::to_owned)
        .or(fallback)
        .unwrap_or_default();
    *tarball_value = Value::String(format!("{public_url}/{}/-/{filename}", pkg.as_str()));
}

fn rewrite_dist_attestations(
    dist: &mut serde_json::Map<String, Value>,
    pkg: &CanonicalPackageName,
    version: Option<&str>,
    public_url: &str,
) {
    if let Some(attestations) = dist.get_mut("attestations").and_then(Value::as_object_mut)
        && let Some(version) = version
    {
        attestations.insert(
            "url".to_string(),
            Value::String(format!("{public_url}/-/npm/v1/attestations/{}@{version}", pkg.as_str())),
        );
    }
}

/// The integrity-addressed tarball route on this server for the packument's
/// selected revision, when the history lists revision 0 once and exactly one
/// revision matches the selection.
pub(super) fn unique_revision_url(
    dist: &serde_json::Map<String, Value>,
    source_registry: &str,
    public_url: &str,
) -> Option<String> {
    let revision = dist.get("revision")?.as_u64()?;
    TarballRevision::try_from(revision).ok()?;
    let integrity: Integrity = dist
        .get("integrity")?
        .as_str()?
        .parse()
        .ok()?;
    let tarball = dist.get("tarball")?.as_str()?;
    if !is_integrity_addressed_registry_tarball_url(tarball, &integrity, source_registry) {
        return None;
    }
    let revision_url = integrity_addressed_registry_tarball_url(&integrity, public_url)?;
    let history = dist.get("revisions")?.as_array()?;
    let originals = history
        .iter()
        .filter(|entry| entry.get("revision").and_then(Value::as_u64) == Some(0))
        .count();
    if originals != 1 {
        return None;
    }
    let matches = history
        .iter()
        .filter(|entry| {
            entry.get("revision").and_then(Value::as_u64) == Some(revision)
                && entry
                    .get("integrity")
                    .and_then(Value::as_str)
                    .and_then(|integrity| integrity.parse::<Integrity>().ok())
                    .is_some_and(|candidate| candidate == integrity)
                && entry.get("tarball").and_then(Value::as_str) == Some(revision_url.as_str())
        })
        .count();
    (matches == 1).then_some(revision_url)
}

pub(super) fn rewrite_upstream_revision_tarball_urls(
    dist: &mut serde_json::Map<String, Value>,
    source_registry: &str,
    public_url: &str,
) {
    let Some(revisions) = dist.get_mut("revisions").and_then(Value::as_array_mut) else {
        return;
    };
    revisions.retain_mut(|revision| {
        revision
            .as_object_mut()
            .is_some_and(|revision| rewrite_revision_tarball(revision, source_registry, public_url))
    });
}

/// Point one revision's tarball at pnpr, dropping the revision when anything
/// about it fails to check out against the source registry.
pub(super) fn rewrite_revision_tarball(
    revision: &mut serde_json::Map<String, Value>,
    source_registry: &str,
    public_url: &str,
) -> bool {
    let Some(number) = revision.get("revision").and_then(Value::as_u64) else {
        return false;
    };
    if number > MAX_TARBALL_REVISION || !revision.get("manifest").is_some_and(Value::is_object) {
        return false;
    }
    let Some(integrity) = revision
        .get("integrity")
        .and_then(Value::as_str)
        .and_then(|integrity| integrity.parse::<Integrity>().ok())
    else {
        return false;
    };
    let addressed = revision
        .get("tarball")
        .and_then(Value::as_str)
        .is_some_and(|tarball| {
            is_integrity_addressed_registry_tarball_url(tarball, &integrity, source_registry)
        });
    if !addressed {
        return false;
    }
    let Some(tarball) = integrity_addressed_registry_tarball_url(&integrity, public_url) else {
        return false;
    };
    revision.insert("tarball".to_string(), Value::String(tarball));
    true
}

/// The tarball filename a `dist.tarball` URL points at: the final path
/// segment, with any `?query`/`#fragment` stripped. This basename is the
/// trust key shared by the rewritten public URL ([`rewrite_tarball_urls`])
/// and the serve-time version match (`expected_tarball_dist`), so both
/// must derive it identically — including for query-bearing URLs (signed
/// CDN links), where the query is not part of the route path a client
/// later requests. Returns `None` for a URL whose path ends in `/`.
#[must_use]
pub fn tarball_basename(url: &str) -> Option<&str> {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url);
    path.rsplit('/')
        .next()
        .filter(|segment| !segment.is_empty())
}

/// Look up the version manifest for `version_or_tag` inside a parsed
/// packument: if the string matches a dist-tag it resolves through
/// `dist-tags[tag]` first, otherwise it's taken as a literal version.
/// Returns the version's manifest *with* the `dist.tarball` rewritten
/// to point at this server.
#[must_use]
pub fn extract_version_manifest(
    packument: &Value,
    pkg: &CanonicalPackageName,
    version_or_tag: &str,
    public_url: &str,
) -> Option<Value> {
    extract_version_manifest_from_registry(packument, pkg, version_or_tag, None, public_url)
}

/// Extract a version manifest while preserving valid upstream revision routes.
#[must_use]
pub fn extract_upstream_version_manifest(
    packument: &Value,
    pkg: &CanonicalPackageName,
    version_or_tag: &str,
    source_registry: &str,
    public_url: &str,
) -> Option<Value> {
    extract_version_manifest_from_registry(
        packument,
        pkg,
        version_or_tag,
        Some(source_registry),
        public_url,
    )
}

pub(super) fn extract_version_manifest_from_registry(
    packument: &Value,
    pkg: &CanonicalPackageName,
    version_or_tag: &str,
    source_registry: Option<&str>,
    public_url: &str,
) -> Option<Value> {
    let resolved = packument
        .get("dist-tags")
        .and_then(|tags| tags.get(version_or_tag))
        .and_then(Value::as_str)
        .unwrap_or(version_or_tag);
    let mut manifest = packument
        .get("versions")?
        .get(resolved)?
        .clone();
    rewrite_tarball_urls_from_registry(&mut manifest, pkg, source_registry, public_url);
    Some(manifest)
}
