pub(super) use tarball_router::TarballRouter;

mod tarball_router;

use std::{collections::HashMap, sync::Arc};

use axum::{
    body::Body,
    http::{StatusCode, header},
    response::Response,
};
use pnpm_config::Config as PacquetConfig;
use pnpm_lockfile::{
    Lockfile, LockfileResolution, PackageKey, PackageMetadata, TarballResolution, TarballRevision,
    is_git_hosted_tarball_url, pick_registry_for_package,
};
use pnpm_package_manager::{ResolvedPackageHint, tarball_url_and_integrity};
use pnpm_resolving_npm_resolver::ObservedDistStats;
use pnpm_resolving_resolver_base::PackageVersionGuard;

use pnpr_osv::{OsvIndex, format_advisory_ids};
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_route::{RouteClass, RouteContext, sanitize_registry_tarball_url, strip_url_credentials};
use pnpr_upstream::tarball_basename;

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
        if let Some(line) = frozen_package_frame(
            FrozenFrameInputs { config, router, dist_stats, package_key, snapshot },
            &mut seen_urls,
        ) {
            frames.push(line);
        }
    }
    frames
}

#[derive(Clone, Copy)]
struct FrozenFrameInputs<'a> {
    config: &'a PacquetConfig,
    router: &'a TarballRouter,
    dist_stats: &'a ObservedDistStats,
    package_key: &'a PackageKey,
    snapshot: &'a PackageMetadata,
}

/// The `package` frame for one frozen lockfile entry, or `None` when the
/// entry is not a registry or tarball package or its URL was announced
/// already.
fn frozen_package_frame(
    inputs: FrozenFrameInputs<'_>,
    seen_urls: &mut std::collections::HashSet<String>,
) -> Option<Vec<u8>> {
    if !matches!(
        inputs.snapshot.resolution,
        LockfileResolution::Registry(_) | LockfileResolution::Tarball(_),
    ) {
        return None;
    }
    // The frame carries the integrity the client prefetches against;
    // an entry that pins none has no frame to announce.
    let Ok((tarball_url, Some(integrity))) =
        tarball_url_and_integrity(&inputs.snapshot.resolution, inputs.package_key, inputs.config)
    else {
        return None;
    };
    let name = inputs.package_key.name.to_string();
    let version = inputs.package_key.suffix.version().to_string();
    let upstream_tarball_url = tarball_url;
    let tarball_url = inputs.router.route_url(&name, &version, &upstream_tarball_url);
    if !seen_urls.insert(tarball_url.clone()) {
        return None;
    }
    let id = format!("{name}@{version}");
    let integrity = integrity.to_string();
    let revision =
        pnpr_served_revision(&inputs.snapshot.resolution, &tarball_url, &upstream_tarball_url);
    let stats = inputs.dist_stats.get(&(name.clone(), version.clone())).map(|entry| *entry.value());
    let frame = package_frame(
        inputs.router,
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
    ndjson_line(&frame).ok()
}

/// The tarball revision a frame announces. Only a URL still pointing at the
/// upstream carries one: a routed URL is served by pnpr, which addresses the
/// tarball by integrity rather than by revision.
fn pnpr_served_revision(
    resolution: &LockfileResolution,
    tarball_url: &str,
    upstream_tarball_url: &str,
) -> Option<u64> {
    if tarball_url != upstream_tarball_url {
        return None;
    }
    match resolution {
        LockfileResolution::Tarball(tarball) => tarball.revision.map(TarballRevision::get),
        LockfileResolution::Registry(registry) => registry.revision.map(TarballRevision::get),
        _ => None,
    }
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

/// Terminal `done` frame of a Cargo resolve: the rendered `Cargo.lock`
/// the client writes verbatim. Cargo's lockfile is a TOML document rather
/// than a structure the server rewrites, so it rides the frame as text.
pub(super) fn cargo_done_frame(lockfile: &str) -> Vec<u8> {
    let frame = serde_json::json!({ "type": "done", "lockfile": lockfile });
    ndjson_line(&frame).unwrap_or_else(|_| {
        br#"{"type":"error","message":"failed to serialize lockfile"}"#.to_vec()
    })
}

/// Terminal `done` frame of a Python resolve: the `pylock.toml` document
/// the client writes. It rides the frame as JSON, which is the shape the
/// client's own lockfile type reads.
pub(super) fn pypi_done_frame(lockfile: &pnpm_python_resolver::Lockfile) -> Vec<u8> {
    let Ok(lockfile) = serde_json::to_value(lockfile) else {
        return br#"{"type":"error","message":"failed to serialize lockfile"}"#.to_vec();
    };
    let frame = serde_json::json!({ "type": "done", "lockfile": lockfile });
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
        let ids = vulnerability_ids_for_entry(index, &snapshot.resolution, &name, &version);
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

/// The advisory ids one lockfile entry matches.
///
/// For a tarball resolution the fetched artifact's identity is its URL, not the
/// lockfile key. Under `trustLockfile` a tampered lockfile could key a safe
/// `name@version` while pointing the tarball at a vulnerable artifact, so the
/// version in the tarball filename is screened too. This is additive — a
/// mismatch alone is never a violation (custom registries may name tarballs
/// differently), only an actually-vulnerable version is.
fn vulnerability_ids_for_entry(
    index: &OsvIndex,
    resolution: &LockfileResolution,
    name: &str,
    version: &str,
) -> Vec<String> {
    let mut ids = index.vulnerability_ids(name, version);
    if let LockfileResolution::Tarball(tarball) = resolution
        && let Some(url_version) = tarball_url_version(&tarball.tarball, name)
        && url_version != version
    {
        ids.extend(index.vulnerability_ids(name, url_version));
        ids.sort_unstable();
        ids.dedup();
    }
    ids
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
