use super::{
    BTreeMap, Deserialize, HashSet, IndexMap, Lockfile, PnprClient, PnprClientError,
    ResolveOptions, ResolveOutcome, ResolveProjectsOptions, ResolvedPackage, Stats,
    TarballRevision, WireViolation, build_verify_error, hash_object_nullable_with_prefix,
};
use futures_util::StreamExt as _;

/// Reject unsupported project transforms before consuming any package
/// frame, so an older server cannot trigger downloads for this request.
fn verify_transform_support(
    response: &reqwest::Response,
    requested: bool,
) -> Result<(), PnprClientError> {
    if requested
        && response.headers().get(PROJECT_TRANSFORMS_HEADER).and_then(|value| value.to_str().ok())
            != Some(PROJECT_TRANSFORMS_VERSION)
    {
        return Err(PnprClientError::Protocol(
            "pnpr server /-/pnpr/v0/resolve does not advertise project-transform support"
                .to_string(),
        ));
    }
    Ok(())
}

/// Read the response body as NDJSON, handing each non-empty line to
/// `on_line` until one of them settles the stream. `None` when the stream
/// ended without a terminal frame.
///
/// reqwest's `gzip` feature transparently inflates the byte stream if a
/// proxy compressed it, so the frames arrive as plain JSON lines.
pub(super) async fn read_ndjson_frames<Outcome, OnLine>(
    response: reqwest::Response,
    mut on_line: OnLine,
) -> Result<Option<Outcome>, PnprClientError>
where
    OnLine: FnMut(&[u8]) -> Result<Option<Outcome>, PnprClientError>,
{
    let mut stream = response.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        buf.extend_from_slice(&chunk?);
        while let Some(newline) = buf.iter().position(|&byte| byte == b'\n') {
            let line: Vec<u8> = buf.drain(..=newline).collect();
            let line = &line[..line.len() - 1];
            if line.is_empty() {
                continue;
            }
            if let Some(outcome) = on_line(line)? {
                return Ok(Some(outcome));
            }
        }
    }
    Ok(None)
}

/// The request's own projects plus the importers its input lockfile already
/// carries: the only importers the response may name.
fn permitted_importers(opts: &ResolveProjectsOptions) -> HashSet<String> {
    opts.projects
        .iter()
        .map(|project| project.dir.clone())
        .chain(opts.lockfile.iter().flat_map(|lockfile| lockfile.importers.keys().cloned()))
        .collect()
}

fn resolve_request_body(opts: &ResolveProjectsOptions) -> serde_json::Value {
    serde_json::json!({
        "projects": opts.projects,
        "registry": opts.registry,
        "registries": opts.registries,
        "overrides": opts.overrides,
        "patchedDependencies": opts.patched_dependencies,
        "packageExtensions": opts.package_extensions,
        "allowUnusedPatches": opts.allow_unused_patches,
        "catalogs": opts.catalogs,
        "autoInstallPeers": opts.auto_install_peers,
        "dedupePeers": opts.dedupe_peers,
        "excludeLinksFromLockfile": opts.exclude_links_from_lockfile,
        "lockfile": opts.lockfile,
        "frozenLockfile": opts.frozen_lockfile,
        "preferFrozenLockfile": opts.prefer_frozen_lockfile,
        "updatePatches": opts.update_patches,
        "fixLockfile": opts.fix_lockfile,
        "ignoreManifestCheck": opts.ignore_manifest_check,
        "trustLockfile": opts.trust_lockfile,
        "resolutionMode": opts.resolution_mode,
        "minimumReleaseAge": opts.minimum_release_age,
        "minimumReleaseAgeExclude": opts.minimum_release_age_exclude,
        "minimumReleaseAgeIgnoreMissingTime": opts.minimum_release_age_ignore_missing_time,
        "trustPolicy": opts.trust_policy,
        "trustPolicyExclude": opts.trust_policy_exclude,
        "trustPolicyIgnoreAfter": opts.trust_policy_ignore_after,
    })
}

fn handle_resolve_frame(
    frame: Frame,
    on_package: &mut impl FnMut(ResolvedPackage),
    permitted_importers: &HashSet<String>,
    opts: &ResolveProjectsOptions,
) -> Result<Option<ResolveOutcome>, PnprClientError> {
    match frame {
        Frame::Package {
            id,
            name,
            version,
            integrity,
            tarball,
            unpacked_size,
            file_count,
            revision,
        } => {
            on_package(ResolvedPackage {
                id,
                name,
                version,
                integrity,
                tarball,
                unpacked_size,
                file_count,
                revision,
            });
            Ok(None)
        }
        Frame::Done { lockfile, stats } => {
            assert_requested_importers(&lockfile, permitted_importers)?;
            assert_transform_metadata(&lockfile, opts)?;
            Ok(Some(ResolveOutcome { lockfile: *lockfile, stats }))
        }
        Frame::Error { message } => Err(PnprClientError::Server(message)),
        Frame::Violations { violations } => {
            Err(PnprClientError::Verification(build_verify_error(violations)))
        }
    }
}

/// The server's response is untrusted and the caller merges the returned
/// lockfile into `pnpm-lock.yaml`, so every importer it carries must be one
/// this request was about.
fn assert_requested_importers(
    lockfile: &Lockfile,
    permitted: &HashSet<String>,
) -> Result<(), PnprClientError> {
    let Some(unexpected) =
        lockfile.importers.keys().find(|importer| !permitted.contains(*importer))
    else {
        return Ok(());
    };
    Err(PnprClientError::Protocol(format!(
        "/-/pnpr/v0/resolve returned an importer that was not requested: {unexpected:?}",
    )))
}

fn has_project_transforms(opts: &ResolveProjectsOptions) -> bool {
    opts.patched_dependencies.as_ref().is_some_and(|patches| !patches.is_empty())
        || opts.package_extensions.as_ref().is_some_and(|extensions| !extensions.is_empty())
}

pub(super) const PROJECT_TRANSFORMS_HEADER: &str = "pnpr-project-transforms";

pub(super) const PROJECT_TRANSFORMS_VERSION: &str = "1";

fn assert_transform_metadata(
    lockfile: &Lockfile,
    opts: &ResolveProjectsOptions,
) -> Result<(), PnprClientError> {
    if let Some(expected) = opts.patched_dependencies.as_ref().filter(|patches| !patches.is_empty())
        && !equal_patch_hashes(lockfile.patched_dependencies.as_ref(), expected)
    {
        return Err(PnprClientError::Protocol(
            "/-/pnpr/v0/resolve returned patchedDependencies that do not match the request; the server may not support project transforms".to_string(),
        ));
    }

    if let Some(package_extensions) =
        opts.package_extensions.as_ref().filter(|extensions| !extensions.is_empty())
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

fn equal_patch_hashes(
    actual: Option<&BTreeMap<String, String>>,
    expected: &IndexMap<String, String>,
) -> bool {
    actual.is_some_and(|actual| {
        actual.len() == expected.len()
            && expected.iter().all(|(selector, hash)| actual.get(selector) == Some(hash))
    })
}

pub(super) fn parse_frame(line: &[u8]) -> Result<Frame, PnprClientError> {
    serde_json::from_slice(line).map_err(|err| PnprClientError::Protocol(err.to_string()))
}

/// One NDJSON frame from `/-/pnpr/v0/resolve`. `package` frames stream as the
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
    },
    /// Boxed: the lockfile dwarfs the other variants, so keeping it
    /// behind a pointer keeps the enum small.
    Done {
        lockfile: Box<Lockfile>,
        #[serde(default)]
        stats: Stats,
    },
    Error {
        message: String,
    },
    Violations {
        violations: Vec<WireViolation>,
    },
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
        // The server's response is untrusted, and the caller merges the
        // returned lockfile into `pnpm-lock.yaml`. Constrain it to the
        // importers this request is about — the requested projects plus
        // whatever the input lockfile already carried — so a hostile server
        // cannot introduce dependencies for a project that was never sent.
        // This is a containment check (every returned importer was
        // requested), which is the injection boundary; it deliberately does
        // not require every requested importer to be present. A dependency-
        // free importer is still present-but-empty (pnpm records it as
        // `{ specifiers: {} }`), and a genuinely missing importer is surfaced
        // downstream by the lockfile merge, not a way to inject dependencies.
        let permitted_importers = permitted_importers(&opts);
        let project_transforms_requested = has_project_transforms(&opts);
        let mut post = self
            .http
            .post(format!("{}-/pnpr/v0/resolve", self.base_url))
            .json(&resolve_request_body(&opts));
        if let Some(authorization) = opts.authorization.as_deref() {
            post = post.header("authorization", authorization);
        }
        let response = post.send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(PnprClientError::Server(format!(
                "/-/pnpr/v0/resolve returned {status}: {body}",
            )));
        }

        verify_transform_support(&response, project_transforms_requested)?;

        // Consume the NDJSON stream line by line. The response header above
        // proves transform support before any package frame is consumed, so
        // current servers preserve resolution/fetch overlap while older
        // servers fail without triggering downloads or buffering hints.
        // reqwest's `gzip` feature transparently inflates the byte stream if a
        // proxy compressed it, so the frames arrive as plain JSON lines.
        let outcome = read_ndjson_frames(response, |line| {
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
