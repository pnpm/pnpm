pub(crate) mod applied_policies;

pub use applied_policies::AppliedPolicies;

use super::{
    BTreeMap, Deserialize, HashSet, IndexMap, Lockfile, PnprClient, PnprClientError,
    ResolveOptions, ResolveOutcome, ResolveProjectsOptions, ResolvedPackage, Stats,
    TarballRevision, WireViolation, build_verify_error, hash_object_nullable_with_prefix,
    lockfile_format::{PNPM_LOCK_V9, expect_lockfile_format},
};
use applied_policies::assert_applied_policies;
use futures_util::StreamExt as _;

/// Cap on one NDJSON line in a streaming resolve response. Matches the
/// terminal lockfile frame limit (`32 MiB`) while preventing a server from
/// growing the line buffer without bound by withholding `\n`.
pub(super) const MAX_NDJSON_LINE_BYTES: usize = 32 * 1024 * 1024;

/// Cap on NDJSON lines read from `/-/pnpr/v0/resolve` before a terminal frame
/// settles the stream.
const MAX_RESOLVE_LINES: usize = 100_000;

#[derive(Clone, Copy)]
pub(super) struct NdjsonLimits {
    pub(super) max_line_bytes: usize,
    pub(super) max_lines: usize,
}

const RESOLVE_NDJSON_LIMITS: NdjsonLimits =
    NdjsonLimits { max_line_bytes: MAX_NDJSON_LINE_BYTES, max_lines: MAX_RESOLVE_LINES };

/// Read the response body as NDJSON, handing each non-empty line to
/// `on_line` until one of them settles the stream. `None` when the stream
/// ended without a terminal frame.
///
/// reqwest's `gzip` feature transparently inflates the byte stream if a
/// proxy compressed it, so the frames arrive as plain JSON lines. Every
/// line is bounded to `limits.max_line_bytes` before buffering, and the
/// stream is bounded to `limits.max_lines` newline-delimited lines.
pub(super) async fn read_ndjson_frames<Outcome, OnLine>(
    response: reqwest::Response,
    limits: NdjsonLimits,
    mut on_line: OnLine,
) -> Result<Option<Outcome>, PnprClientError>
where
    OnLine: FnMut(&[u8]) -> Result<Option<Outcome>, PnprClientError>,
{
    let mut stream = response.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    let mut lines_seen: usize = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let mut rest = chunk.as_ref();
        while let Some(newline) = rest
            .iter()
            .position(|&byte| byte == b'\n')
        {
            ensure_ndjson_line_count(&mut lines_seen, limits.max_lines)?;
            let segment = &rest[..newline];
            rest = &rest[newline + 1..];
            if let Some(outcome) =
                consume_ndjson_line(&mut buf, segment, limits.max_line_bytes, &mut on_line)?
            {
                return Ok(Some(outcome));
            }
        }
        append_ndjson_segment(&mut buf, rest, limits.max_line_bytes)?;
    }
    Ok(None)
}

fn ensure_ndjson_line_count(
    lines_seen: &mut usize,
    max_lines: usize,
) -> Result<(), PnprClientError> {
    *lines_seen = lines_seen.saturating_add(1);
    if *lines_seen > max_lines {
        return Err(PnprClientError::Protocol(format!(
            "NDJSON stream exceeds the {max_lines}-line limit",
        )));
    }
    Ok(())
}

fn consume_ndjson_line<Outcome, OnLine>(
    buf: &mut Vec<u8>,
    segment: &[u8],
    max_line_bytes: usize,
    on_line: &mut OnLine,
) -> Result<Option<Outcome>, PnprClientError>
where
    OnLine: FnMut(&[u8]) -> Result<Option<Outcome>, PnprClientError>,
{
    if buf.is_empty() {
        ensure_ndjson_line_fits(segment.len(), max_line_bytes)?;
        deliver_ndjson_line(segment, on_line)
    } else {
        append_ndjson_segment(buf, segment, max_line_bytes)?;
        let outcome = deliver_ndjson_line(buf, on_line);
        buf.clear();
        outcome
    }
}

fn ensure_ndjson_line_fits(len: usize, max_line_bytes: usize) -> Result<(), PnprClientError> {
    if len > max_line_bytes {
        return Err(PnprClientError::Protocol(format!(
            "NDJSON frame exceeds the {max_line_bytes}-byte limit",
        )));
    }
    Ok(())
}

fn append_ndjson_segment(
    buf: &mut Vec<u8>,
    segment: &[u8],
    max_line_bytes: usize,
) -> Result<(), PnprClientError> {
    if segment.is_empty() {
        return Ok(());
    }
    ensure_ndjson_line_fits(buf.len().saturating_add(segment.len()), max_line_bytes)?;
    buf.extend_from_slice(segment);
    Ok(())
}

fn deliver_ndjson_line<Outcome, OnLine>(
    line: &[u8],
    on_line: &mut OnLine,
) -> Result<Option<Outcome>, PnprClientError>
where
    OnLine: FnMut(&[u8]) -> Result<Option<Outcome>, PnprClientError>,
{
    if line.is_empty() {
        return Ok(None);
    }
    on_line(line)
}

/// The request's own projects plus the importers its input lockfile already
/// carries: the only importers the response may name.
fn permitted_importers(opts: &ResolveProjectsOptions) -> HashSet<String> {
    opts.projects
        .iter()
        .map(|project| project.dir.clone())
        .chain(
            opts.reuse.lockfile
                .iter()
                .flat_map(|lockfile| lockfile.importers.keys().cloned()),
        )
        .collect()
}

fn resolve_request_body(opts: &ResolveProjectsOptions) -> serde_json::Value {
    let mut body = serde_json::to_value(opts).expect("resolution request serializes to JSON");
    body["acceptedLockfileFormats"] = serde_json::json!([PNPM_LOCK_V9]);
    body
}

fn forward_package_frame(frame: Frame, on_package: &mut impl FnMut(ResolvedPackage)) {
    if let Frame::Package {
        id,
        name,
        version,
        integrity,
        tarball,
        unpacked_size,
        file_count,
        revision,
        requires_build,
    } = frame
    {
        on_package(ResolvedPackage {
            id,
            name,
            version,
            integrity,
            tarball,
            unpacked_size,
            file_count,
            revision,
            requires_build,
        });
    }
}

fn handle_resolve_frame(
    frame: Frame,
    on_package: &mut impl FnMut(ResolvedPackage),
    permitted_importers: &HashSet<String>,
    opts: &ResolveProjectsOptions,
) -> Result<Option<ResolveOutcome>, PnprClientError> {
    match frame {
        Frame::Package { .. } => {
            forward_package_frame(frame, on_package);
            Ok(None)
        }
        Frame::Done {
            lockfile,
            lockfile_format,
            stats,
            applied_policies,
        } => {
            expect_lockfile_format(lockfile_format.as_deref(), PNPM_LOCK_V9)?;
            assert_requested_importers(&lockfile, permitted_importers)?;
            assert_transform_metadata(&lockfile, opts)?;
            assert_publish_directories(&lockfile, opts)?;
            assert_applied_policies(applied_policies.as_ref(), opts)?;
            Ok(Some(ResolveOutcome { lockfile: *lockfile, stats, applied_policies }))
        }
        Frame::Error { message, code } => Err(PnprClientError::Server { message, code }),
        Frame::Violations { violations } => {
            Err(PnprClientError::Verification(build_verify_error(violations)))
        }
        Frame::Ignored => Ok(None),
    }
}

/// The server's response is untrusted and the caller merges the returned
/// lockfile into `pnpm-lock.yaml`, so every importer it carries must be one
/// this request was about.
fn assert_requested_importers(
    lockfile: &Lockfile,
    permitted: &HashSet<String>,
) -> Result<(), PnprClientError> {
    let Some(unexpected) = lockfile.importers
        .keys()
        .find(|importer| !permitted.contains(*importer))
    else {
        return Ok(());
    };
    Err(PnprClientError::Protocol(format!(
        "/-/pnpr/v0/resolve returned an importer that was not requested: {unexpected:?}",
    )))
}

fn has_project_transforms(opts: &ResolveProjectsOptions) -> bool {
    opts.transforms.patched_dependencies
        .as_ref()
        .is_some_and(|patches| !patches.is_empty())
        || opts.transforms.package_extensions
            .as_ref()
            .is_some_and(|extensions| !extensions.is_empty())
}

fn assert_transform_metadata(
    lockfile: &Lockfile,
    opts: &ResolveProjectsOptions,
) -> Result<(), PnprClientError> {
    if let Some(expected) = opts.transforms.patched_dependencies
        .as_ref()
        .filter(|patches| !patches.is_empty())
        && !equal_patch_hashes(lockfile.patched_dependencies.as_ref(), expected)
    {
        return Err(PnprClientError::Protocol(
            "/-/pnpr/v0/resolve returned patchedDependencies that do not match the request; the server may not support project transforms".to_string(),
        ));
    }

    if let Some(package_extensions) = opts.transforms.package_extensions
        .as_ref()
        .filter(|extensions| !extensions.is_empty())
    {
        let value = serde_json::to_value(package_extensions)
            .map_err(|err| PnprClientError::Protocol(err.to_string()))?;
        let expected = hash_object_nullable_with_prefix(&value)
            .expect("a non-empty packageExtensions map has a checksum");
        if lockfile.package_extensions_checksum.as_deref() != Some(expected.as_str()) {
            return Err(PnprClientError::Protocol(
                "/-/pnpr/v0/resolve returned packageExtensionsChecksum that does not match the request; the server may not support project transforms".to_string(),
            ));
        }
    }

    Ok(())
}

/// A project that publishes from a subdirectory must come back with that
/// directory on its importer. The server rebuilds each project's manifest from
/// the request, so a server that ignored `publishConfig` would have the caller
/// link the project root — and install other code than a local install does.
fn assert_publish_directories(
    lockfile: &Lockfile,
    opts: &ResolveProjectsOptions,
) -> Result<(), PnprClientError> {
    for project in &opts.projects {
        let Some(directory) = project.publish_config
            .as_ref()
            .map(|publish_config| publish_config.directory.as_str())
        else {
            continue;
        };
        let Some(importer) = lockfile.importers.get(&project.dir) else {
            continue;
        };
        if importer.publish_directory.as_deref() != Some(directory) {
            return Err(PnprClientError::Protocol(format!(
                "/-/pnpr/v0/resolve returned importer {:?} linked at {:?} instead of its publishConfig.directory {directory:?}; the server may not forward project publishConfig",
                project.dir, importer.publish_directory,
            )));
        }
    }
    Ok(())
}

fn equal_patch_hashes(
    actual: Option<&BTreeMap<String, String>>,
    expected: &IndexMap<String, String>,
) -> bool {
    actual.is_some_and(|actual| {
        actual.len() == expected.len()
            && expected
                .iter()
                .all(|(selector, hash)| actual.get(selector) == Some(hash))
    })
}

pub(super) fn parse_frame(line: &[u8]) -> Result<Frame, PnprClientError> {
    serde_json::from_slice(line).map_err(|err| PnprClientError::Protocol(err.to_string()))
}

/// One NDJSON frame from resolve endpoint. `package` frames stream as the
/// server resolves; exactly one terminal frame (`done` / `error` /
/// `violations`) closes the response.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum Frame {
    Package {
        id: String,
        name: String,
        version: String,
        integrity: String,
        tarball: String,
        #[serde(rename = "unpackedSize", default)]
        unpacked_size: Option<usize>,
        #[serde(rename = "fileCount", default)]
        file_count: Option<usize>,
        #[serde(default)]
        revision: Option<TarballRevision>,
        #[serde(rename = "requiresBuild", default)]
        requires_build: Option<bool>,
    },
    /// Boxed: the lockfile dwarfs the other variants, so keeping it
    /// behind a pointer keeps the enum small.
    Done {
        lockfile: Box<Lockfile>,
        #[serde(rename = "lockfileFormat", default)]
        lockfile_format: Option<String>,
        #[serde(default)]
        stats: Stats,
        #[serde(default, rename = "appliedPolicies")]
        applied_policies: Option<AppliedPolicies>,
    },
    Error {
        message: String,
        #[serde(default)]
        code: Option<String>,
    },
    Violations {
        violations: Vec<WireViolation>,
    },
    #[serde(other)]
    Ignored,
}

impl PnprClient {
    /// Equivalent to [`Self::resolve_streaming`] with a no-op callback.
    pub async fn resolve(&self, opts: ResolveOptions) -> Result<ResolveOutcome, PnprClientError> {
        self.resolve_projects(opts.into()).await
    }

    /// Resolve workspace projects against the server and return the resolved
    /// lockfile, ignoring the streamed per-package frames.
    pub async fn resolve_projects(
        &self,
        opts: ResolveProjectsOptions,
    ) -> Result<ResolveOutcome, PnprClientError> {
        self.resolve_projects_streaming(opts, |_| {}).await
    }

    /// Resolve a single project, invoking `on_package` once per resolved
    /// tarball as its `package` frame streams in — *before* the full
    /// lockfile arrives — so the caller can begin fetching each tarball
    /// while the server is still resolving. Returns the resolved lockfile
    /// from the terminal `done` frame.
    pub async fn resolve_streaming(
        &self,
        opts: ResolveOptions,
        on_package: impl FnMut(ResolvedPackage),
    ) -> Result<ResolveOutcome, PnprClientError> {
        self.resolve_projects_streaming(opts.into(), on_package).await
    }

    async fn handshake_project_transforms(
        &self,
        opts: &ResolveProjectsOptions,
    ) -> Result<(), PnprClientError> {
        let project_transforms_requested = has_project_transforms(opts);
        if project_transforms_requested {
            let capability = self.ensure_handshake().await?;
            Self::require_resolver_protocol(capability)?;
            if !capability
                .protocol("resolver")
                .features
                .iter()
                .any(|feature| feature == "projectTransforms")
            {
                return Err(PnprClientError::Protocol(
                    "pnpr server /-/pnpr/v0/resolve does not advertise project-transform support"
                        .to_string(),
                ));
            }
        }
        Ok(())
    }

    /// Resolve workspace projects, invoking `on_package` once per resolved
    /// tarball before the terminal lockfile frame arrives.
    pub async fn resolve_projects_streaming(
        &self,
        opts: ResolveProjectsOptions,
        mut on_package: impl FnMut(ResolvedPackage),
    ) -> Result<ResolveOutcome, PnprClientError> {
        if opts.fix_lockfile {
            self.handshake_fix_lockfile().await?;
        }
        // The importer check is a containment check (every returned importer
        // was requested), which is the injection boundary; it deliberately does
        // not require every requested importer to be present. A dependency-
        // free importer is still present-but-empty (pnpm records it as
        // `{ specifiers: {} }`), and a genuinely missing importer is surfaced
        // downstream by the lockfile merge, not a way to inject dependencies.
        let permitted_importers = permitted_importers(&opts);
        self.handshake_project_transforms(&opts).await?;
        let mut post = self.http
            .post(format!("{}-/pnpr/v0/resolve", self.base_url))
            .json(&resolve_request_body(&opts));
        if let Some(authorization) = opts.routing.authorization.as_deref() {
            post = post.header("authorization", authorization);
        }
        let response = post.send().await?;
        let response = super::check_status(response, "/-/pnpr/v0/resolve").await?;

        let outcome = read_ndjson_frames(response, RESOLVE_NDJSON_LIMITS, |line| {
            handle_resolve_frame(parse_frame(line)?, &mut on_package, &permitted_importers, &opts)
        })
        .await?;
        outcome.ok_or_else(|| {
            PnprClientError::Protocol(
                "/-/pnpr/v0/resolve stream ended without a terminal frame".to_string(),
            )
        })
    }
}
