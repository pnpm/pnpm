pub(super) mod osv;

pub(super) use frozen_frames::frozen_package_frames_until;
pub(super) use osv::{osv_violations_for_lockfile_until, verify_done_frame};
pub(super) use tarball_router::TarballRouter;

mod frozen_frames;
mod tarball_router;

use std::{collections::HashMap, sync::Arc};

use axum::{
    body::Body,
    http::{StatusCode, header},
    response::Response,
};
use osv::is_http_tarball_url;
use pnpm_config::Config as PacquetConfig;
use pnpm_lockfile::{
    Lockfile, LockfileResolution, PackageKey, PackageMetadata, TarballResolution,
    is_git_hosted_tarball_url, pick_registry_for_package,
};
use pnpm_package_manager::{ResolvedPackageHint, tarball_url_and_integrity};
use pnpm_resolving_resolver_base::PackageVersionGuard;
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_route::{RouteClass, RouteContext, sanitize_registry_tarball_url, strip_url_credentials};
use pnpr_upstream::{
    tarball_basename, tarball_basename_for_package, version_route_tarball_version,
};

use super::{
    limits::{GRAPH_TOO_LARGE_CODE, GraphLimitTracker},
    protocol::LockfileFormat,
};

/// NDJSON content type for the `/-/pnpr/v0/resolve` response. One JSON object
/// per line; the client parses frames as they arrive. Excluded from the
/// server's gzip [`CompressionLayer`](crate::server) so frames flush to
/// the client incrementally rather than being buffered by the encoder.
const NDJSON_CONTENT_TYPE: &str = "application/x-ndjson";

/// Maximum serialized byte length of a single terminal NDJSON frame, aligned
/// with `pnpm-pnpr-client`'s 32 MiB line cap.
pub(super) const MAX_NDJSON_FRAME_BYTES: usize = 32 * 1024 * 1024;

/// [`ResolutionObserver`](pnpm_package_manager::ResolutionObserver)
/// that turns each resolved tarball into a `package` NDJSON frame and
/// pushes it down the bounded response channel.
///
/// `package` frames are opportunistic prefetch hints (the terminal `done`
/// frame carries the full lockfile), so a full channel drops excess hints
/// rather than blocking resolution. When the graph exceeds the package cap,
/// [`GraphLimitTracker`] wakes the outer task to abort resolution.
pub(super) struct StreamObserver {
    pub(super) tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    pub(super) package_version_guard: Option<Arc<dyn PackageVersionGuard>>,
    pub(super) tarball_router: TarballRouter,
    pub(super) graph_limit: Arc<GraphLimitTracker>,
}

impl pnpm_package_manager::ResolutionObserver for StreamObserver {
    fn on_resolution_id(&self, _id: &str) {
        self.graph_limit.record_package();
    }

    fn on_resolved(&self, hint: pnpm_package_manager::ResolvedPackageHint<'_>) {
        if self.graph_limit.is_exceeded() {
            return;
        }
        if let Ok(line) = ndjson_line(&package_frame(&self.tarball_router, &hint)) {
            let _ = self.tx.try_send(line);
        }
    }

    fn package_version_guard(&self) -> Option<Arc<dyn PackageVersionGuard>> {
        self.package_version_guard.as_ref().map(Arc::clone)
    }

    fn max_snapshots(&self) -> Option<usize> {
        Some(self.graph_limit.max_snapshots())
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
        router.route_registry_url(hint.identity.name, hint.identity.version, hint.tarball_url)
    } else {
        router.route_url(hint.identity.name, hint.identity.version, hint.tarball_url)
    };
    let mut frame = serde_json::json!({
        "type": "package",
        "id": hint.identity.id,
        "name": hint.identity.name,
        "version": hint.identity.version,
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
    if let Some(requires_build) = hint.requires_build {
        frame["requiresBuild"] = serde_json::Value::from(requires_build);
    }
    frame
}

const CANCEL_CHECK_INTERVAL_BYTES: usize = 64 * 1024;

fn oversized_terminal_frame() -> Vec<u8> {
    error_frame_with_code(
        &format!("resolved response exceeds maximum frame size of {MAX_NDJSON_FRAME_BYTES} bytes"),
        Some(GRAPH_TOO_LARGE_CODE),
    )
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DoneFrame<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    applied_policies: &'a super::protocol::AppliedPolicies,
    lockfile_format: &'static str,
    lockfile: &'a Lockfile,
    stats: DoneStats,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DoneStats {
    total_packages: usize,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CargoDoneFrame<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    lockfile_format: &'static str,
    lockfile: &'a str,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PypiDoneFrame<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    lockfile_format: &'static str,
    lockfile: &'a pnpm_python_resolver::Lockfile,
}

#[derive(serde::Serialize)]
struct ViolationsFrame<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    violations: &'a [serde_json::Value],
}

struct BoundedFrameWriter<'a> {
    buf: Vec<u8>,
    since_check: usize,
    cancelled: bool,
    exceeded: bool,
    should_cancel: &'a dyn Fn() -> bool,
}

impl std::io::Write for BoundedFrameWriter<'_> {
    fn write(&mut self, chunk: &[u8]) -> std::io::Result<usize> {
        if (self.buf.is_empty() || self.since_check >= CANCEL_CHECK_INTERVAL_BYTES)
            && (self.should_cancel)()
        {
            self.cancelled = true;
            return Err(std::io::Error::other("cancelled"));
        }
        if self.since_check >= CANCEL_CHECK_INTERVAL_BYTES {
            self.since_check = 0;
        }
        if self.buf.len().saturating_add(chunk.len()) >= MAX_NDJSON_FRAME_BYTES {
            self.exceeded = true;
            return Err(std::io::Error::other("frame exceeds limit"));
        }
        self.buf.extend_from_slice(chunk);
        self.since_check = self.since_check.saturating_add(chunk.len());
        Ok(chunk.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) fn serialize_bounded_frame<Frame: serde::Serialize>(
    value: &Frame,
    fallback_message: &'static str,
    should_cancel: &dyn Fn() -> bool,
) -> Option<Result<Vec<u8>, Vec<u8>>> {
    if should_cancel() {
        return None;
    }
    let mut writer = BoundedFrameWriter {
        buf: Vec::new(),
        since_check: 0,
        cancelled: false,
        exceeded: false,
        should_cancel,
    };
    let written = serde_json::to_writer(&mut writer, value);
    if writer.cancelled || should_cancel() {
        return None;
    }
    if writer.exceeded {
        return Some(Err(oversized_terminal_frame()));
    }
    if written.is_err() {
        return Some(Err(error_frame(fallback_message)));
    }
    writer.buf.push(b'\n');
    Some(Ok(writer.buf))
}

pub(super) fn try_done_frame_until(
    lockfile: &Lockfile,
    applied_policies: &super::protocol::AppliedPolicies,
    should_cancel: &dyn Fn() -> bool,
) -> Option<Result<Vec<u8>, Vec<u8>>> {
    let total_packages = lockfile.packages
        .as_ref()
        .map(HashMap::len)
        .unwrap_or(0);
    let frame = DoneFrame {
        kind: "done",
        applied_policies,
        lockfile_format: LockfileFormat::PnpmLockV9.wire_name(),
        lockfile,
        stats: DoneStats { total_packages },
    };
    serialize_bounded_frame(&frame, "failed to serialize lockfile", should_cancel)
}

/// Terminal `done` frame of a Cargo resolve: the rendered `Cargo.lock`
/// the client writes verbatim. Cargo's lockfile is a TOML document rather
/// than a structure the server rewrites, so it rides the frame as text.
pub(super) fn cargo_done_frame_until(
    lockfile: &str,
    should_cancel: &dyn Fn() -> bool,
) -> Option<Vec<u8>> {
    serialize_bounded_frame(
        &CargoDoneFrame {
            kind: "done",
            lockfile_format: LockfileFormat::CargoLock.wire_name(),
            lockfile,
        },
        "failed to serialize lockfile",
        should_cancel,
    )
    .map(|result| result.unwrap_or_else(|err| err))
}

/// Terminal `done` frame of a Python resolve: the `pylock.toml` document
/// the client writes. It rides the frame as JSON, which is the shape the
/// client's own lockfile type reads.
pub(super) fn pypi_done_frame(lockfile: &pnpm_python_resolver::Lockfile) -> Vec<u8> {
    serialize_bounded_frame(
        &PypiDoneFrame {
            kind: "done",
            lockfile_format: LockfileFormat::Pylock.wire_name(),
            lockfile,
        },
        "failed to serialize lockfile",
        &|| false,
    )
    .expect("unconditional frame serialization cannot cancel")
    .unwrap_or_else(|err| err)
}

/// Non-terminal `progress` frame. A streamed resolve sends one every
/// [`KEEPALIVE_INTERVAL`] while it runs, so a proxy with an idle timeout does
/// not cut a long resolution. Clients ignore it.
pub(super) fn progress_frame() -> Vec<u8> {
    b"{\"type\":\"progress\"}\n".to_vec()
}

/// Below the common 30 and 60 second proxy idle timeouts.
pub(super) const KEEPALIVE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15);

/// Terminal `error` frame for a resolution that aborted mid-stream,
/// after one or more `package` frames may already have been sent (so the
/// HTTP status is locked at 200 — the failure has to ride in the body).
pub(super) fn error_frame(message: &str) -> Vec<u8> {
    error_frame_with_code(message, None)
}

/// Terminal `error` frame with a structured error code.
pub(super) fn error_frame_with_code(message: &str, code: Option<&str>) -> Vec<u8> {
    let code = code.unwrap_or("ERR_PNPR_RESOLUTION_FAILED");
    let code = code
        .strip_prefix("ERR_PNPM_")
        .map(|kind| format!("ERR_PNPR_{kind}"))
        .unwrap_or_else(|| code.to_string());
    let mut frame = pnpr_error::error_body(message, &code);
    frame["type"] = serde_json::json!("error");
    frame["message"] = serde_json::json!(message);
    ndjson_line(&frame).expect("error frames contain only JSON strings")
}

/// Terminal `violations` frame: the input lockfile failed the client's
/// policy. Each entry mirrors the local runner's rendered violation so
/// the client rebuilds the identical `VerifyError` and aborts the same
/// way the local gate would.
pub(super) fn violations_frame(violations: &[serde_json::Value]) -> Vec<u8> {
    serialize_bounded_frame(
        &ViolationsFrame { kind: "violations", violations },
        "verification failed",
        &|| false,
    )
    .expect("unconditional frame serialization cannot cancel")
    .unwrap_or_else(|err| err)
}

/// Serialize one frame to a newline-terminated NDJSON line.
pub(super) fn ndjson_line(value: &serde_json::Value) -> Result<Vec<u8>, serde_json::Error> {
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
        .body(Body::from(frame.to_vec()))
        .expect("binary response is always valid")
}

/// A 200 NDJSON response streaming already-serialized frames without copying
/// them into a single contiguous buffer. Used by the frozen fast path, where
/// every frame is known up front.
pub(super) fn ndjson_frames_until(
    frames: Vec<Vec<u8>>,
    should_cancel: &dyn Fn() -> bool,
) -> Option<Response> {
    if should_cancel() {
        return None;
    }
    let stream = futures_util::stream::iter(
        frames
            .into_iter()
            .map(|line| Ok::<_, std::convert::Infallible>(axum::body::Bytes::from(line))),
    );
    Some(
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, NDJSON_CONTENT_TYPE)
            .body(Body::from_stream(stream))
            .expect("streaming response is always valid"),
    )
}

pub(super) struct AbortOnDrop(pub(super) tokio::task::AbortHandle);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub(super) fn ndjson_stream_response(
    rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    abort_on_drop: AbortOnDrop,
) -> Response {
    let stream =
        futures_util::stream::unfold((rx, abort_on_drop), |(mut rx, abort_on_drop)| async move {
            rx.recv().await
                .map(|line| {
                    (Ok::<_, std::io::Error>(axum::body::Bytes::from(line)), (rx, abort_on_drop))
                })
        });
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, NDJSON_CONTENT_TYPE)
        .body(Body::from_stream(stream))
        .expect("streaming response is always valid")
}
