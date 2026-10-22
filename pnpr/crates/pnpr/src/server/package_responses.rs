use super::{
    ABBREVIATED_CONTENT_TYPE, Arc, Body, CanonicalPackageName, DistView, HeaderMap, IndexMap,
    Integrity, RegistryError, Response, StatusCode, Utc, Value, abbreviate_packument,
    declared_tarball_integrity, describe_tars, filter_osv_vulnerable_versions, header,
    rewrite_tarball_urls, rewrite_upstream_tarball_urls, streaming, tarball_basename,
};

/// The version a tarball request resolves to, plus that version's declared
/// `dist.integrity`. The version is found by matching `filename` against
/// each version's `dist.tarball` basename rather than parsing it out of
/// the filename, so a non-canonical name (see [`rewrite_tarball_urls`])
/// resolves to the right version, integrity, and OSV identity.
pub(super) struct TarballDist {
    pub(super) version: String,
    pub(super) integrity: Integrity,
    pub(super) unpacked_size: Option<u64>,
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
    #[serde(default, rename = "unpackedSize")]
    pub(super) unpacked_size: Option<u64>,
}

pub(super) fn expected_tarball_dist(
    packument: &[u8],
    name: &CanonicalPackageName,
    filename: &str,
) -> Result<Option<TarballDist>, RegistryError> {
    let packument: PackumentDists = serde_json::from_slice(packument)?;
    let mut matches = packument.versions
        .iter()
        .filter_map(|(version, manifest)| {
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
    Ok(Some(TarballDist { version: version.clone(), integrity, unpacked_size: dist.unpacked_size }))
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

/// Parse the on-disk packument, rewrite `dist.tarball` URLs, and build the
/// response. Honors conditional requests (`If-None-Match`, `If-Modified-Since`)
/// with 304 Not Modified. When abbreviated MIME is requested, strip down to the
/// npm spec's install-v1 field set and tag the response with the abbreviated
/// content type.
pub(super) fn packument_response(
    name: &CanonicalPackageName,
    bytes: &[u8],
    tarball_base: &str,
    view: DistView<'_>,
    osv_index: Option<&Arc<pnpr_osv::OsvIndex>>,
    headers: &HeaderMap,
    max_age: u64,
) -> Result<Response, RegistryError> {
    let mut doc: Value = serde_json::from_slice(bytes)?;
    filter_osv_vulnerable_versions(&mut doc, name, osv_index);
    match view.revision_registry {
        Some(source_registry) => {
            rewrite_upstream_tarball_urls(&mut doc, name, source_registry, tarball_base);
        }
        None => rewrite_tarball_urls(&mut doc, name, tarball_base),
    }
    if let Some(table) = view.tar_integrities {
        describe_tars(&mut doc, table, view.tar_registry);
    }
    let abbreviated = wants_abbreviated(headers);
    let (body, content_type) = if abbreviated {
        let trimmed = abbreviate_packument(&doc, Utc::now());
        (serde_json::to_vec(&trimmed)?, ABBREVIATED_CONTENT_TYPE)
    } else {
        (serde_json::to_vec(&doc)?, "application/json")
    };
    let etag = format!(r#""{}""#, super::ecosystem::sha256_hex(&body));
    // Per RFC 9110 Section 8.8.2, Last-Modified is omitted when the representation
    // is dynamically generated or subject to content negotiation (such as OSV
    // filtering or abbreviation) and can vary independently of modification time.
    let last_modified =
        if osv_index.is_some() || abbreviated { None } else { packument_last_modified(&doc) };
    if is_not_modified(headers, &etag, last_modified.as_deref()) {
        return Ok(packument_not_modified_response(&etag, last_modified, max_age));
    }
    Ok(packument_ok_response(body, content_type, &etag, last_modified, max_age))
}

fn is_not_modified(headers: &HeaderMap, etag: &str, last_modified: Option<&str>) -> bool {
    let mut has_inm = false;
    for inm in headers.get_all(header::IF_NONE_MATCH) {
        has_inm = true;
        if inm.to_str().is_ok_and(|header_val| if_none_match_matches(header_val, etag)) {
            return true;
        }
    }
    if has_inm {
        return false;
    }
    if let Some(ims) = headers
        .get(header::IF_MODIFIED_SINCE)
        .and_then(|val| val.to_str().ok())
        && let Some(lm) = last_modified
    {
        return if_modified_since_matches(ims, lm);
    }
    false
}

fn if_none_match_matches(header_val: &str, etag: &str) -> bool {
    let matches_candidate = |candidate: &str| {
        let candidate = candidate.trim();
        candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == etag
    };
    let mut in_quotes = false;
    let mut start = 0;
    for (idx, char_code) in header_val.char_indices() {
        match char_code {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                if matches_candidate(&header_val[start..idx]) {
                    return true;
                }
                start = idx + 1;
            }
            _ => {}
        }
    }
    matches_candidate(&header_val[start..])
}

fn if_modified_since_matches(if_modified_since: &str, last_modified: &str) -> bool {
    let Ok(ims_time) = chrono::DateTime::parse_from_rfc2822(if_modified_since) else {
        return false;
    };
    let Ok(lm_time) = chrono::DateTime::parse_from_rfc2822(last_modified) else {
        return false;
    };
    lm_time <= ims_time
}

fn packument_not_modified_response(
    etag: &str,
    last_modified: Option<String>,
    max_age: u64,
) -> Response {
    let mut builder = Response::builder()
        .status(StatusCode::NOT_MODIFIED)
        .header(header::ETAG, etag)
        .header(header::VARY, "Accept")
        .header(header::CACHE_CONTROL, format!("public, max-age={max_age}"));
    if let Some(last_modified) = last_modified {
        builder = builder.header(header::LAST_MODIFIED, last_modified);
    }
    builder
        .body(Body::empty())
        .expect("static-shape response always builds")
}

fn packument_ok_response(
    body: Vec<u8>,
    content_type: &'static str,
    etag: &str,
    last_modified: Option<String>,
    max_age: u64,
) -> Response {
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::ETAG, etag)
        .header(header::VARY, "Accept")
        .header(header::CACHE_CONTROL, format!("public, max-age={max_age}"));
    if let Some(last_modified) = last_modified {
        builder = builder.header(header::LAST_MODIFIED, last_modified);
    }
    builder
        .body(Body::from(body))
        .expect("static-shape response always builds")
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
    builder
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
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
/// header is omitted then.
pub(super) fn packument_last_modified(doc: &Value) -> Option<String> {
    let modified = doc
        .get("time")?
        .get("modified")?
        .as_str()?;
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
