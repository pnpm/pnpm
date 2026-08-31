use std::{collections::HashMap, sync::Arc};

use axum::{
    body::Body,
    http::{StatusCode, header},
    response::Response,
};
use pnpm_config::Config as PacquetConfig;
use pnpm_lockfile::{
    Lockfile, LockfileResolution, TarballResolution, TarballRevision, is_git_hosted_tarball_url,
    pick_registry_for_package,
};
use pnpm_package_manager::{ResolvedPackageHint, tarball_url_and_integrity};
use pnpm_resolving_npm_resolver::ObservedDistStats;
use pnpm_resolving_resolver_base::PackageVersionGuard;

use pnpr_osv::{OsvIndex, format_advisory_ids};
use pnpr_package_name::PackageName;
use pnpr_policy::Identity;
use pnpr_route::{RouteClass, RouteContext, sanitize_registry_tarball_url, strip_url_credentials};
use pnpr_upstream::tarball_basename;

#[derive(Clone)]
pub(super) struct TarballRouter {
    context: Arc<RouteContext>,
    identity: Identity,
    public_url: String,
    /// Per-scope registry map (`scope -> registry URL`, plus the default) used
    /// to classify a registry-resolved package by its *registry* route rather
    /// than its `dist.tarball` host. See [`Self::route_registry_url`].
    registries: HashMap<String, String>,
}

impl TarballRouter {
    pub(super) fn new(
        context: Arc<RouteContext>,
        identity: Identity,
        public_url: String,
        registries: HashMap<String, String>,
    ) -> Self {
        Self { context, identity, public_url, registries }
    }

    /// Route a registry-resolved package's tarball by the **registry** it came
    /// from, not its `dist.tarball` URL. A split-domain registry serves the
    /// tarball from a different host than the packument, so classifying by the
    /// tarball URL would misread a private package as public and leak its raw
    /// upstream URL. Classifying by the registry origin keeps a private
    /// package on its `/~<name>/` endpoint; a public one still emits its real
    /// (anonymously fetchable) tarball URL for a direct CDN download.
    fn route_registry_url(&self, package: &str, version: &str, tarball_url: &str) -> String {
        let registry = pick_registry_for_package(&self.registries, package, None);
        match self.context.classify(&self.identity, &registry, Some(package)) {
            // The `dist.tarball` is untrusted upstream metadata, so sanitize it
            // before emitting/caching: drop inline `user:pass@host` userinfo and
            // any query/fragment a registry could use to carry a signed-URL
            // token. A genuinely public tarball is anonymously fetchable, so the
            // sanitized URL still works.
            RouteClass::Public => sanitize_registry_tarball_url(tarball_url),
            RouteClass::Hosted { .. } => pnpr_tarball_url(
                &self.public_url,
                package,
                &tarball_filename(package, version, tarball_url),
            ),
            RouteClass::Proxied { alias, .. } => upstream_endpoint_tarball_url(
                &self.public_url,
                &alias,
                package,
                &tarball_filename(package, version, tarball_url),
            ),
        }
    }

    pub(super) fn route_lockfile(&self, config: &PacquetConfig, lockfile: &Lockfile) -> Lockfile {
        let mut routed = lockfile.clone();
        let Some(packages) = routed.packages.as_mut() else {
            return routed;
        };
        for (package_key, metadata) in packages {
            if !matches!(
                metadata.resolution,
                LockfileResolution::Registry(_) | LockfileResolution::Tarball(_),
            ) {
                continue;
            }
            // A resolution that pins no integrity keeps its original URL:
            // routing it through the endpoint would hand the client a
            // mirrored tarball it has no hash to check.
            let Ok((tarball_url, Some(integrity))) =
                tarball_url_and_integrity(&metadata.resolution, package_key, config)
            else {
                continue;
            };
            if !is_http_tarball_url(&tarball_url) || is_git_hosted_tarball_url(&tarball_url) {
                continue;
            }
            let name = package_key.name.to_string();
            let version = package_key.suffix.version().to_string();
            let routed_url = self.route_url(&name, &version, &tarball_url);
            if routed_url == tarball_url.as_ref() {
                continue;
            }
            metadata.resolution = LockfileResolution::Tarball(TarballResolution {
                tarball: routed_url,
                integrity: Some(integrity.clone()),
                revision: None,
                git_hosted: None,
                path: None,
            });
        }
        routed
    }

    pub(super) fn verification_lockfile(&self, lockfile: &Lockfile) -> Lockfile {
        let mut upstream = lockfile.clone();
        let Some(packages) = upstream.packages.as_mut() else {
            return upstream;
        };
        for metadata in packages.values_mut() {
            let LockfileResolution::Tarball(resolution) = &mut metadata.resolution else {
                continue;
            };
            if let Some(tarball_url) = self.upstream_endpoint_tarball_url(&resolution.tarball) {
                resolution.tarball = tarball_url;
            }
        }
        upstream
    }

    fn route_url(&self, package: &str, version: &str, tarball_url: &str) -> String {
        match self.context.classify(&self.identity, tarball_url, Some(package)) {
            // A public route keeps its upstream URL: it was fetched
            // anonymously, so its tarball is anonymously fetchable and pnpr
            // never mints a per-tarball gateway URL. Any inline userinfo a
            // malicious/compromised registry embedded in `dist.tarball` is
            // stripped first, so pnpr never streams or caches it.
            RouteClass::Public => strip_url_credentials(tarball_url),
            RouteClass::Hosted { .. } => pnpr_tarball_url(
                &self.public_url,
                package,
                &tarball_filename(package, version, tarball_url),
            ),
            RouteClass::Proxied { alias, .. } => upstream_endpoint_tarball_url(
                &self.public_url,
                &alias,
                package,
                &tarball_filename(package, version, tarball_url),
            ),
        }
    }

    /// Reverse a `/~<name>/<pkg>/-/<file>` endpoint tarball URL back to its
    /// upstream URL so an input lockfile carrying endpoint URLs can be verified
    /// against the real registry. Returns `None` for any other URL, and for an
    /// endpoint the caller is not authorized for (so verification cannot be
    /// used as an oracle for an upstream the caller cannot reach).
    fn upstream_endpoint_tarball_url(&self, tarball_url: &str) -> Option<String> {
        let prefix = format!("{}/~", self.public_url.trim_end_matches('/'));
        let route = tarball_url.strip_prefix(&prefix)?;
        let (upstream, rest) = route.split_once('/')?;
        let registry = self.context.upstream_registry(&self.identity, upstream)?;
        Some(format!("{}/{rest}", registry.trim_end_matches('/')))
    }
}

fn tarball_filename(package: &str, version: &str, tarball_url: &str) -> String {
    tarball_basename(tarball_url).map_or_else(
        || {
            PackageName::parse(package).map_or_else(
                |_| format!("{package}-{version}.tgz"),
                |name| name.tarball_name_for_version(version),
            )
        },
        str::to_string,
    )
}

fn pnpr_tarball_url(public_url: &str, package: &str, filename: &str) -> String {
    format!("{}/{package}/-/{filename}", public_url.trim_end_matches('/'))
}

/// The `/~<name>/<package>/-/<filename>` registry-endpoint URL a proxied
/// route's tarball is served through. Canonical for a client whose scope is
/// configured at `https://<pnpr>/~<name>/`, so the lockfile entry collapses
/// to integrity-only; the upstream URL and credential stay server-side.
fn upstream_endpoint_tarball_url(
    public_url: &str,
    upstream: &str,
    package: &str,
    filename: &str,
) -> String {
    format!("{}/~{upstream}/{package}/-/{filename}", public_url.trim_end_matches('/'))
}

/// NDJSON content type for the `/-/pnpr/v0/resolve` response. One JSON object
/// per line; the client parses frames as they arrive. Excluded from the
/// server's gzip [`CompressionLayer`](crate::server) so frames flush to
/// the client incrementally rather than being buffered by the encoder.
const NDJSON_CONTENT_TYPE: &str = "application/x-ndjson";
const PROJECT_TRANSFORMS_HEADER: &str = "pnpr-project-transforms";
const PROJECT_TRANSFORMS_VERSION: &str = "1";

/// [`ResolutionObserver`](pnpm_package_manager::ResolutionObserver)
/// that turns each resolved tarball into a `package` NDJSON frame and
/// pushes it down the response channel. `on_resolved` is best-effort: a
/// closed channel (client hung up) or a serialization failure drops the
/// frame silently — the resolve still runs to completion server-side.
pub(super) struct StreamObserver {
    pub(super) tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    pub(super) package_version_guard: Option<Arc<dyn PackageVersionGuard>>,
    pub(super) tarball_router: TarballRouter,
}

impl pnpm_package_manager::ResolutionObserver for StreamObserver {
    fn on_resolved(&self, hint: pnpm_package_manager::ResolvedPackageHint<'_>) {
        if let Ok(line) = ndjson_line(&package_frame(&self.tarball_router, &hint)) {
            let _ = self.tx.send(line);
        }
    }

    fn package_version_guard(&self) -> Option<Arc<dyn PackageVersionGuard>> {
        self.package_version_guard.clone()
    }
}

/// One `package` NDJSON frame. Optional fields are omitted (not null).
pub(super) fn package_frame(
    router: &TarballRouter,
    hint: &ResolvedPackageHint<'_>,
) -> serde_json::Value {
    // A registry-resolved package's `tarball_url` is the packument's
    // `dist.tarball`, which a split-domain registry hosts on a different origin
    // — route it by the registry, not the tarball host, so a private package
    // never leaks its raw upstream URL. Direct tarball deps keep their own URL.
    let tarball_url = if hint.from_registry {
        router.route_registry_url(hint.name, hint.version, hint.tarball_url)
    } else {
        router.route_url(hint.name, hint.version, hint.tarball_url)
    };
    let mut frame = serde_json::json!({
        "type": "package",
        "id": hint.id,
        "name": hint.name,
        "version": hint.version,
        "integrity": hint.integrity,
        "tarball": tarball_url,
    });
    if let Some(size) = hint.unpacked_size {
        frame["unpackedSize"] = serde_json::Value::from(size);
    }
    if let Some(count) = hint.file_count {
        frame["fileCount"] = serde_json::Value::from(count);
    }
    if tarball_url == hint.tarball_url
        && let Some(revision) = hint.revision
    {
        frame["revision"] = serde_json::Value::from(revision);
    }
    frame
}

/// `package` frames for every tarball-fetchable entry of a verified
/// frozen lockfile, deduplicated by tarball URL. Mirrors what the
/// streaming resolve's [`StreamObserver`] would have announced had the
/// tree walk run: the client prefetches each tarball on arrival, with
/// `unpackedSize` (from the verification fan-out's metadata, when the
/// registry published one) prioritizing the largest downloads.
///
/// Tarball URLs are derived with the same
/// [`tarball_url_and_integrity`] the client's frozen materialization
/// uses, so the announced URLs match the client's mem-cache keys
/// byte-for-byte. Non-tarball resolutions (git, directory, binary,
/// variations) are skipped — the client fetches those through their
/// own protocol paths.
pub(super) fn frozen_package_frames(
    config: &PacquetConfig,
    router: &TarballRouter,
    lockfile: &Lockfile,
    dist_stats: &ObservedDistStats,
) -> Vec<Vec<u8>> {
    let Some(packages) = lockfile.packages.as_ref() else {
        return Vec::new();
    };
    let mut seen_urls = std::collections::HashSet::new();
    let mut frames = Vec::new();
    for (package_key, snapshot) in packages {
        if !matches!(
            snapshot.resolution,
            LockfileResolution::Registry(_) | LockfileResolution::Tarball(_),
        ) {
            continue;
        }
        // The frame carries the integrity the client prefetches against;
        // an entry that pins none has no frame to announce.
        let Ok((tarball_url, Some(integrity))) =
            tarball_url_and_integrity(&snapshot.resolution, package_key, config)
        else {
            continue;
        };
        let name = package_key.name.to_string();
        let version = package_key.suffix.version().to_string();
        let upstream_tarball_url = tarball_url;
        let tarball_url = router.route_url(&name, &version, &upstream_tarball_url);
        if !seen_urls.insert(tarball_url.clone()) {
            continue;
        }
        let id = format!("{name}@{version}");
        let integrity = integrity.to_string();
        let revision = if tarball_url == upstream_tarball_url {
            match &snapshot.resolution {
                LockfileResolution::Tarball(tarball) => tarball.revision.map(TarballRevision::get),
                LockfileResolution::Registry(registry) => {
                    registry.revision.map(TarballRevision::get)
                }
                _ => None,
            }
        } else {
            None
        };
        let stats = dist_stats.get(&(name.clone(), version.clone())).map(|entry| *entry.value());
        let frame = package_frame(
            router,
            &ResolvedPackageHint {
                id: &id,
                name: &name,
                version: &version,
                integrity: &integrity,
                tarball_url: &tarball_url,
                unpacked_size: stats.and_then(|stats| stats.unpacked_size),
                file_count: stats.and_then(|stats| stats.file_count),
                revision,
                // The URL is already routed (canonical → endpoint above), so
                // re-routing by registry would be redundant; route_url is a
                // no-op on an already-routed URL.
                from_registry: false,
            },
        );
        if let Ok(line) = ndjson_line(&frame) {
            frames.push(line);
        }
    }
    frames
}

/// Terminal `done` frame: the full resolved lockfile + stats. The client
/// writes the lockfile and fetches every tarball itself.
pub(super) fn done_frame(lockfile: &Lockfile) -> Vec<u8> {
    let total_packages = lockfile.packages.as_ref().map_or(0, std::collections::HashMap::len);
    let frame = serde_json::json!({
        "type": "done",
        "lockfile": serde_json::to_value(lockfile).unwrap_or(serde_json::Value::Null),
        "stats": { "totalPackages": total_packages },
    });
    ndjson_line(&frame).unwrap_or_else(|_| {
        br#"{"type":"error","message":"failed to serialize lockfile"}"#.to_vec()
    })
}

/// Terminal `error` frame for a resolution that aborted mid-stream,
/// after one or more `package` frames may already have been sent (so the
/// HTTP status is locked at 200 — the failure has to ride in the body).
pub(super) fn error_frame(message: &str) -> Vec<u8> {
    let frame = serde_json::json!({ "type": "error", "message": message });
    ndjson_line(&frame)
        .unwrap_or_else(|_| br#"{"type":"error","message":"resolution failed"}"#.to_vec())
}

/// Terminal `violations` frame: the input lockfile failed the client's
/// policy. Each entry mirrors the local runner's rendered violation so
/// the client rebuilds the identical `VerifyError` and aborts the same
/// way the local gate would.
pub(super) fn violations_frame(violations: &[serde_json::Value]) -> Vec<u8> {
    let frame = serde_json::json!({ "type": "violations", "violations": violations });
    ndjson_line(&frame)
        .unwrap_or_else(|_| br#"{"type":"error","message":"verification failed"}"#.to_vec())
}

fn verify_done_frame() -> Vec<u8> {
    ndjson_line(&serde_json::json!({ "type": "done" }))
        .unwrap_or_else(|_| br#"{"type":"error","message":"verification failed"}"#.to_vec())
}

const OSV_VULNERABILITY_CODE: &str = "ERR_PNPM_OSV_VULNERABILITY";

pub(super) fn verify_done_or_osv_violations(
    osv_index: Option<&Arc<OsvIndex>>,
    lockfile: &Lockfile,
) -> Response {
    let Some(osv_index) = osv_index else {
        return ndjson_single_frame(&verify_done_frame());
    };
    let violations = osv_violations_for_lockfile(osv_index, lockfile);
    if violations.is_empty() {
        ndjson_single_frame(&verify_done_frame())
    } else {
        ndjson_single_frame(&violations_frame(&violations))
    }
}

pub(super) fn osv_violations_for_lockfile(
    index: &OsvIndex,
    lockfile: &Lockfile,
) -> Vec<serde_json::Value> {
    let Some(packages) = lockfile.packages.as_ref() else {
        return Vec::new();
    };

    let mut seen = std::collections::HashSet::new();
    let mut violations = Vec::new();
    for (package_key, snapshot) in packages {
        if !is_osv_checkable_resolution(&snapshot.resolution) {
            continue;
        }
        let name = package_key.name.to_string();
        let version = package_key.suffix.version().to_string();
        let mut ids = index.vulnerability_ids(&name, &version);
        // For a tarball resolution the fetched artifact's identity is its
        // URL, not the lockfile key. Under `trustLockfile` a tampered
        // lockfile could key a safe `name@version` while pointing the
        // tarball at a vulnerable artifact, so also screen the version in
        // the tarball filename. This is additive — a mismatch alone is
        // never a violation (custom registries may name tarballs
        // differently), only an actually-vulnerable version is.
        if let LockfileResolution::Tarball(tarball) = &snapshot.resolution
            && let Some(url_version) = tarball_url_version(&tarball.tarball, &name)
            && url_version != version
        {
            ids.extend(index.vulnerability_ids(&name, url_version));
            ids.sort_unstable();
            ids.dedup();
        }
        if ids.is_empty() {
            continue;
        }
        // Dedup only the rare vulnerable hits — several lockfile keys can
        // share one name@version via peer suffixes — so the common
        // (non-vulnerable) entry never pays for the set.
        if !seen.insert((name.clone(), version.clone())) {
            continue;
        }
        violations.push(serde_json::json!({
            "name": name,
            "version": version,
            "code": OSV_VULNERABILITY_CODE,
            "reason": format!(
                "is listed in the local OSV database as vulnerable ({})",
                format_advisory_ids(&ids),
            ),
        }));
    }
    violations
}

/// Best-effort extraction of the version from a registry tarball URL of
/// the conventional `<unscoped-name>-<version>.tgz` shape. Returns `None`
/// for non-standard naming so a legitimate custom registry isn't
/// misjudged. Never parses the URL strictly — the lockfile is untrusted.
pub(super) fn tarball_url_version<'a>(url: &'a str, name: &str) -> Option<&'a str> {
    let last = url.rsplit('/').next()?;
    let last = last.split(['?', '#']).next().unwrap_or(last);
    let stem = strip_tarball_suffix(last)?;
    let unscoped = name.rsplit('/').next().unwrap_or(name);
    let version = stem.strip_prefix(unscoped)?.strip_prefix('-')?;
    (!version.is_empty()).then_some(version)
}

/// Strip a `.tgz` / `.tar.gz` tarball suffix case-insensitively, so a
/// tampered lockfile can't dodge the URL-version cross-check with a
/// `.TGZ` or `.tar.gz` variant. Returns `None` for any other suffix.
fn strip_tarball_suffix(name: &str) -> Option<&str> {
    [".tar.gz", ".tgz"].into_iter().find_map(|suffix| {
        let head_len = name.len().checked_sub(suffix.len())?;
        let (head, tail) = (name.get(..head_len)?, name.get(head_len..)?);
        tail.eq_ignore_ascii_case(suffix).then_some(head)
    })
}

pub(super) fn is_osv_checkable_resolution(resolution: &LockfileResolution) -> bool {
    match resolution {
        LockfileResolution::Registry(_) => true,
        // A frozen lockfile is attacker-controlled, so gate on the tarball
        // URL rather than the tamper-prone `git_hosted` flag or strict URL
        // parsing — otherwise `gitHosted: true` or a barely-malformed URL
        // would let a vulnerable package opt out of the OSV scan. Mirrors
        // the npm verifier's URL-based gate.
        LockfileResolution::Tarball(tarball) => {
            is_http_tarball_url(&tarball.tarball) && !is_git_hosted_tarball_url(&tarball.tarball)
        }
        // Custom resolutions are not registry artifacts, so OSV has
        // no `name@version` advisory coordinates for them.
        LockfileResolution::Directory(_)
        | LockfileResolution::Git(_)
        | LockfileResolution::Binary(_)
        | LockfileResolution::Variations(_)
        | LockfileResolution::Custom(_) => false,
    }
}

/// Whether a tarball URL uses an http(s) scheme — the only schemes a
/// registry artifact is served over. Case-insensitive (so a tampered
/// uppercase scheme can't slip past) without allocating a lowercased copy.
fn is_http_tarball_url(url: &str) -> bool {
    let bytes = url.as_bytes();
    bytes.get(..8).is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"https://"))
        || bytes.get(..7).is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"http://"))
}

/// Serialize one frame to a newline-terminated NDJSON line.
fn ndjson_line(value: &serde_json::Value) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// A 200 NDJSON response carrying a single, already-serialized terminal
/// frame (the short-circuit and violation paths, which never stream
/// `package` frames).
pub(super) fn ndjson_single_frame(frame: &[u8]) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, NDJSON_CONTENT_TYPE)
        .header(PROJECT_TRANSFORMS_HEADER, PROJECT_TRANSFORMS_VERSION)
        .body(Body::from(frame.to_vec()))
        .expect("binary response is always valid")
}

/// A 200 NDJSON response carrying several already-serialized frames in
/// one fixed body. Used by the frozen fast path, where every frame is
/// known up front — no channel to stream from.
pub(super) fn ndjson_frames(frames: &[Vec<u8>]) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, NDJSON_CONTENT_TYPE)
        .header(PROJECT_TRANSFORMS_HEADER, PROJECT_TRANSFORMS_VERSION)
        .body(Body::from(frames.concat()))
        .expect("binary response is always valid")
}

/// A 200 NDJSON response whose body drains the frame channel as the
/// detached resolve task produces frames. Closing the channel (the task
/// dropped its sender) ends the body.
pub(super) fn ndjson_stream_response(
    rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
) -> Response {
    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|line| (Ok::<_, std::io::Error>(axum::body::Bytes::from(line)), rx))
    });
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, NDJSON_CONTENT_TYPE)
        .header(PROJECT_TRANSFORMS_HEADER, PROJECT_TRANSFORMS_VERSION)
        .body(Body::from_stream(stream))
        .expect("streaming response is always valid")
}
