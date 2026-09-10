use super::{
    ABBREVIATED_CONTENT_TYPE, AppState, Arc, Body, CanonicalPackageName, HashSet, HeaderMap,
    IndexMap, Integrity, RegistryError, Response, StatusCode, Utc, Value, abbreviate_packument,
    declared_tarball_integrity, header, private_no_cache, rewrite_tarball_urls,
    rewrite_upstream_tarball_urls, streaming, tarball_basename,
};

/// The version a tarball request resolves to, plus that version's declared
/// `dist.integrity`. The version is found by matching `filename` against
/// each version's `dist.tarball` basename rather than parsing it out of
/// the filename, so a non-canonical name (see [`rewrite_tarball_urls`])
/// resolves to the right version, integrity, and OSV identity.
pub(super) struct TarballDist {
    pub(super) version: String,
    pub(super) integrity: Integrity,
}

/// The `versions[v].dist` subset the tarball serve path reads. Every tarball
/// request re-reads its package's packument to bind the filename to a
/// declared version and integrity; deserializing into this projection instead
/// of a full `serde_json::Value` skips building (and allocating) the rest of
/// the document on that hot path.
#[derive(serde::Deserialize)]
pub(super) struct PackumentDists {
    #[serde(default)]
    pub(super) versions: IndexMap<String, VersionDist>,
}

#[derive(serde::Deserialize)]
pub(super) struct VersionDist {
    #[serde(default)]
    pub(super) dist: Option<DistBlock>,
}

#[derive(serde::Deserialize)]
pub(super) struct DistBlock {
    #[serde(default)]
    pub(super) tarball: Option<String>,
    #[serde(default)]
    pub(super) integrity: Option<String>,
    /// Legacy hex sha1 — the only hash pre-2017 npm publishes carry.
    #[serde(default)]
    pub(super) shasum: Option<String>,
}

pub(super) fn expected_tarball_dist(
    packument: &[u8],
    name: &CanonicalPackageName,
    filename: &str,
) -> Result<Option<TarballDist>, RegistryError> {
    let packument: PackumentDists = serde_json::from_slice(packument)?;
    let mut matches = packument.versions.iter().filter_map(|(version, manifest)| {
        let dist = manifest.dist.as_ref()?;
        dist.tarball
            .as_deref()
            .and_then(tarball_basename)
            .is_some_and(|basename| basename == filename)
            .then_some((version, dist))
    });
    let Some((version, dist)) = matches.next() else {
        return Ok(None);
    };
    // A tarball name must identify exactly one declaring version, or the
    // integrity and OSV checks below could bind to the wrong one. Two
    // versions sharing a basename is a malformed/hostile packument, never a
    // legitimate registry, so fail closed rather than pick by iteration order.
    if matches.next().is_some() {
        return Err(tarball_integrity_error(
            name.as_str(),
            filename,
            "packument declares the same dist.tarball basename for multiple versions".to_string(),
        ));
    }
    let integrity = declared_tarball_integrity(dist, name, filename, version)?;
    Ok(Some(TarballDist { version: version.clone(), integrity }))
}

pub(super) fn tarball_stream_error(
    err: streaming::BlobStreamError,
    name: &CanonicalPackageName,
    filename: &str,
) -> RegistryError {
    tarball_stream_error_for_package(err, name.as_str(), filename)
}

pub(super) fn tarball_stream_error_for_package(
    err: streaming::BlobStreamError,
    package: &str,
    filename: &str,
) -> RegistryError {
    match err {
        streaming::BlobStreamError::Upstream { url, source } => {
            RegistryError::UpstreamBody { url, source }
        }
        streaming::BlobStreamError::Io(err) => RegistryError::Io(err),
        streaming::BlobStreamError::Integrity(err) => tarball_integrity_error(
            package,
            filename,
            format!("integrity verification failed: {err}"),
        ),
        streaming::BlobStreamError::TooLarge { limit, received } => tarball_integrity_error(
            package,
            filename,
            format!("tarball body exceeds {limit} byte limit (received {received} bytes)"),
        ),
    }
}

pub(super) fn tarball_integrity_error(
    package: &str,
    filename: &str,
    reason: String,
) -> RegistryError {
    RegistryError::TarballIntegrity {
        package: package.to_string(),
        filename: filename.to_string(),
        reason,
    }
}

/// True when the client's `Accept` header offers the
/// `application/vnd.npm.install-v1+json` abbreviated MIME. We do a
/// substring match rather than full RFC-7231 q-value parsing — the
/// npm client always sends it as the top-priority option and a
/// substring presence is a reliable signal.
pub(super) fn wants_abbreviated(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|accept| accept.contains(ABBREVIATED_CONTENT_TYPE))
}

/// Parse the on-disk packument, rewrite `dist.tarball` URLs, and
/// build the response. When `abbreviated` is true, strip down to
/// the npm spec's install-v1 field set (mirrors verdaccio's
/// `convertAbbreviatedManifest`) and tag the response with the
/// `application/vnd.npm.install-v1+json` content type. Parse
/// failures surface as 502 via `RegistryError::Json`.
pub(super) fn packument_response(
    name: &CanonicalPackageName,
    bytes: &[u8],
    tarball_base: &str,
    revision_registry: Option<&str>,
    osv_index: Option<&Arc<pnpr_osv::OsvIndex>>,
    abbreviated: bool,
) -> Result<Response, RegistryError> {
    let mut doc: Value = serde_json::from_slice(bytes)?;
    filter_osv_vulnerable_versions(&mut doc, name, osv_index);
    match revision_registry {
        Some(source_registry) => {
            rewrite_upstream_tarball_urls(&mut doc, name, source_registry, tarball_base);
        }
        None => rewrite_tarball_urls(&mut doc, name, tarball_base),
    }
    let last_modified = packument_last_modified(&doc);
    let (body, content_type) = if abbreviated {
        let trimmed = abbreviate_packument(&doc, Utc::now());
        (serde_json::to_vec(&trimmed)?, ABBREVIATED_CONTENT_TYPE)
    } else {
        (serde_json::to_vec(&doc)?, "application/json")
    };
    Ok(packument_bytes_response(body, content_type, last_modified))
}

pub(super) fn filter_osv_vulnerable_versions(
    packument: &mut Value,
    name: &CanonicalPackageName,
    osv_index: Option<&Arc<pnpr_osv::OsvIndex>>,
) {
    let Some(osv_index) = osv_index else { return };
    let package_name = name.as_str();
    let mut blocked_keys = HashSet::new();
    let mut retained_version_keys = HashSet::new();
    let has_time = packument.get("time").and_then(Value::as_object).is_some();
    if let Some(versions) = packument.get_mut("versions").and_then(Value::as_object_mut) {
        versions.retain(|key, manifest| {
            if version_is_vulnerable(osv_index, package_name, key, manifest) {
                blocked_keys.insert(key.clone());
                return false;
            }
            if has_time {
                retained_version_keys.insert(key.clone());
            }
            true
        });
    }
    drop_blocked_dist_tags(packument, osv_index, package_name, &blocked_keys);
    drop_blocked_time_entries(
        packument,
        osv_index,
        package_name,
        &BlockedVersions { blocked: &blocked_keys, retained: &retained_version_keys },
    );
}

/// Drop every tag pointing at a version an advisory covers.
pub(super) fn drop_blocked_dist_tags(
    packument: &mut Value,
    osv_index: &pnpr_osv::OsvIndex,
    package_name: &str,
    blocked: &HashSet<String>,
) {
    let Some(tags) = packument.get_mut("dist-tags").and_then(Value::as_object_mut) else {
        return;
    };
    tags.retain(|_, version| {
        version.as_str().is_none_or(|version| {
            !blocked.contains(version) && !osv_index.is_vulnerable(package_name, version)
        })
    });
}

/// The versions a filter pass blocked, and the ones it kept.
pub(super) struct BlockedVersions<'a> {
    pub(super) blocked: &'a HashSet<String>,
    pub(super) retained: &'a HashSet<String>,
}

/// Drop the `time` entries of blocked versions, keeping the two document-level
/// stamps and every version the filter kept.
pub(super) fn drop_blocked_time_entries(
    packument: &mut Value,
    osv_index: &pnpr_osv::OsvIndex,
    package_name: &str,
    versions: &BlockedVersions<'_>,
) {
    let Some(time) = packument.get_mut("time").and_then(Value::as_object_mut) else {
        return;
    };
    time.retain(|key, _| {
        !versions.blocked.contains(key)
            && (matches!(key.as_str(), "created" | "modified")
                || versions.retained.contains(key)
                || !osv_index.is_vulnerable(package_name, key))
    });
}

/// Whether an advisory covers a packument entry, under either the version its
/// key names or the one its manifest declares.
pub(super) fn version_is_vulnerable(
    osv_index: &pnpr_osv::OsvIndex,
    package_name: &str,
    key: &str,
    manifest: &Value,
) -> bool {
    if osv_index.is_vulnerable(package_name, key) {
        return true;
    }
    manifest
        .get("version")
        .and_then(Value::as_str)
        .is_some_and(|version| version != key && osv_index.is_vulnerable(package_name, version))
}

pub(super) fn filter_osv_vulnerable_dist_tags(
    tags: &mut Value,
    packument: &Value,
    name: &CanonicalPackageName,
    osv_index: Option<&Arc<pnpr_osv::OsvIndex>>,
) {
    let Some(osv_index) = osv_index else { return };
    let Some(tags) = tags.as_object_mut() else {
        return;
    };
    let package_name = name.as_str();
    tags.retain(|_, version| {
        version.as_str().is_none_or(|version| {
            !is_osv_vulnerable_packument_version(packument, package_name, version, osv_index)
        })
    });
}

pub(super) fn is_osv_vulnerable_packument_version(
    packument: &Value,
    package_name: &str,
    version: &str,
    osv_index: &pnpr_osv::OsvIndex,
) -> bool {
    if osv_index.is_vulnerable(package_name, version) {
        return true;
    }
    let manifest_version = packument
        .get("versions")
        .and_then(|versions| versions.get(version))
        .and_then(|manifest| manifest.get("version"))
        .and_then(Value::as_str);
    manifest_version.is_some_and(|manifest_version| {
        manifest_version != version && osv_index.is_vulnerable(package_name, manifest_version)
    })
}

pub(super) fn resolve_version_or_tag<'a>(packument: &'a Value, version_or_tag: &'a str) -> &'a str {
    packument
        .get("dist-tags")
        .and_then(|tags| tags.get(version_or_tag))
        .and_then(Value::as_str)
        .unwrap_or(version_or_tag)
}

pub(super) fn ensure_osv_allowed(
    state: &AppState,
    name: &CanonicalPackageName,
    version: &str,
) -> Result<(), RegistryError> {
    let Some(osv_index) = state.inner.osv_index.as_ref() else {
        return Ok(());
    };
    let ids = osv_index.vulnerability_ids(name.as_str(), version);
    if ids.is_empty() {
        return Ok(());
    }
    Err(RegistryError::OsvVulnerability {
        package: name.as_str().to_string(),
        version: version.to_string(),
        advisories: pnpr_osv::format_advisory_ids(&ids),
    })
}

pub(super) fn packument_bytes_response(
    bytes: Vec<u8>,
    content_type: &'static str,
    last_modified: Option<String>,
) -> Response {
    let mut builder =
        Response::builder().status(StatusCode::OK).header(header::CONTENT_TYPE, content_type);
    if let Some(last_modified) = last_modified {
        builder = builder.header(header::LAST_MODIFIED, last_modified);
    }
    builder.body(Body::from(bytes)).expect("static-shape response always builds")
}

/// `Last-Modified` value for a served packument: the document's
/// `time.modified` in HTTP-date form. Lets a client's release-age check
/// (pnpm's `minimumReleaseAge`) learn the package-level last-publish
/// upper bound from response headers alone — a `HEAD` costs ~no bytes
/// where the abbreviated body runs to hundreds of KB. Fractional
/// seconds round *up* to the next whole second: the header must stay
/// an upper bound on the publish time, and truncating would understate
/// it by up to 999ms — exactly the window a release-age check guards.
/// `None` when the document carries no parsable `time.modified`; the
/// header is simply omitted then.
pub(super) fn packument_last_modified(doc: &Value) -> Option<String> {
    let modified = doc.get("time")?.get("modified")?.as_str()?;
    let parsed = chrono::DateTime::parse_from_rfc3339(modified).ok()?;
    let mut whole_seconds = parsed.with_timezone(&Utc);
    if whole_seconds.timestamp_subsec_nanos() > 0 {
        whole_seconds += chrono::Duration::seconds(1);
    }
    Some(whole_seconds.format("%a, %d %b %Y %H:%M:%S GMT").to_string())
}

pub(super) fn tarball_response(body: Body, content_length: Option<u64>) -> Response {
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream");
    if let Some(len) = content_length {
        builder = builder.header(header::CONTENT_LENGTH, len);
    }
    builder.body(body).expect("static-shape response always builds")
}

pub(super) fn revision_tarball_response(
    body: Body,
    content_length: Option<u64>,
    digest: &str,
    integrity: &Integrity,
) -> Response {
    let mut response = tarball_response(body, content_length);
    let headers = response.headers_mut();
    headers.insert(
        header::CACHE_CONTROL,
        "public, max-age=31536000, immutable".parse().expect("static cache control is valid"),
    );
    headers.insert(
        header::ETAG,
        format!(r#""{digest}""#).parse().expect("canonical base64url digest is a valid ETag"),
    );
    if let [hash] = integrity.hashes.as_slice() {
        headers.insert(
            "content-digest",
            format!("sha-512=:{}:", hash.digest)
                .parse()
                .expect("canonical base64 digest is a valid header value"),
        );
    }
    private_no_cache(response)
}
