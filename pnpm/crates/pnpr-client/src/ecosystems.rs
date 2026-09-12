use super::{
    CargoResolveOptions, Deserialize, MAX_ERROR_BODY_SIZE, PnprClient, PnprClientError,
    PypiResolveOptions, RenderedViolation, VerifyError, VerifyLockfileOptions, read_ndjson_frames,
    response_body_bounded,
};

fn parse_pypi_frame(line: &[u8]) -> Result<PypiFrame, PnprClientError> {
    serde_json::from_slice(line).map_err(|err| PnprClientError::Protocol(err.to_string()))
}

fn parse_cargo_frame(line: &[u8]) -> Result<CargoFrame, PnprClientError> {
    serde_json::from_slice(line).map_err(|err| PnprClientError::Protocol(err.to_string()))
}

fn parse_verify_frame(line: &[u8]) -> Result<VerifyFrame, PnprClientError> {
    serde_json::from_slice(line).map_err(|err| PnprClientError::Protocol(err.to_string()))
}

/// The Cargo ecosystem's name in a resolve request body and in the
/// handshake's `ecosystems` list.
pub const CARGO_ECOSYSTEM: &str = "cargo";

/// The Python ecosystem's name in a resolve request body and in the
/// handshake's `ecosystems` list.
pub const PYPI_ECOSYSTEM: &str = "pypi";

/// Cap on a resolve response that is one terminal frame: a `Cargo.lock`
/// or a `pylock.toml`, each a few megabytes for the largest projects.
const MAX_TERMINAL_RESPONSE_SIZE: usize = 32 * 1024 * 1024;

/// One NDJSON frame from a Cargo `/-/pnpr/v0/resolve`. Cargo resolution
/// yields nothing before it yields everything, so the response is a
/// single terminal frame: the rendered `Cargo.lock`, or the failure.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum CargoFrame {
    Done { lockfile: String },
    Error { message: String },
}

/// One NDJSON frame from a Python `/-/pnpr/v0/resolve`: the `pylock.toml`
/// the project resolved to, or the failure.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PypiFrame {
    /// Boxed: the lockfile dwarfs the other variant.
    Done {
        lockfile: Box<pnpm_python_resolver::Lockfile>,
    },
    Error {
        message: String,
    },
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum VerifyFrame {
    Done,
    Error { message: String },
    Violations { violations: Vec<WireViolation> },
}

#[derive(Deserialize)]
pub(super) struct WireViolation {
    pub(super) name: String,
    pub(super) version: String,
    pub(super) code: String,
    pub(super) reason: String,
}

/// Rebuild the [`VerifyError`] the local gate would have raised from
/// the server's rendered violations. Sorting by `name@version` before
/// [`VerifyError::from_rendered`] reproduces the same breakdown order
/// the local runner produces, so the abort is byte-identical.
pub(super) fn build_verify_error(mut violations: Vec<WireViolation>) -> VerifyError {
    violations.sort_by(|left, right| {
        format!("{}@{}", left.name, left.version).cmp(&format!("{}@{}", right.name, right.version))
    });
    let rendered: Vec<RenderedViolation> = violations
        .into_iter()
        .map(|violation| RenderedViolation {
            name: violation.name,
            version: violation.version,
            code: intern_violation_code(&violation.code),
            reason: violation.reason,
        })
        .collect();
    VerifyError::from_rendered(&rendered)
}

/// Map a wire violation code back to the `&'static str` constant
/// [`VerifyError::from_rendered`] matches on. Values are byte-identical
/// to `pnpm_resolving_npm_resolver`'s violation codes; an unknown
/// code falls back to the generic envelope rather than fabricating a
/// variant. Kept inline (rather than depending on the npm resolver)
/// for the same reason the verification crate aliases them.
fn intern_violation_code(code: &str) -> &'static str {
    match code {
        "MINIMUM_RELEASE_AGE_VIOLATION" => "MINIMUM_RELEASE_AGE_VIOLATION",
        "TRUST_DOWNGRADE" => "TRUST_DOWNGRADE",
        "TARBALL_URL_MISMATCH" => "TARBALL_URL_MISMATCH",
        _ => "LOCKFILE_RESOLUTION_VERIFICATION",
    }
}

impl PnprClient {
    /// Resolve a single project against the server and return the
    /// resolved lockfile, ignoring the streamed per-package frames.
    /// Resolve a Python project against the server and return the
    /// `pylock.toml` document it produced. The client still downloads the
    /// wheels the document names and re-solves the project against their
    /// own metadata, so this answer is a proposal the install verifies,
    /// not something it takes on trust.
    pub async fn resolve_pypi(
        &self,
        opts: PypiResolveOptions,
    ) -> Result<pnpm_python_resolver::Lockfile, PnprClientError> {
        let request = serde_json::json!({
            "ecosystem": PYPI_ECOSYSTEM,
            "requirements": opts.requirements,
            "target": opts.target,
            "index": opts.index,
            "requiresPython": opts.requires_python,
        });
        let frame = self.terminal_frame(&request, opts.authorization.as_deref()).await?;
        match parse_pypi_frame(&frame)? {
            PypiFrame::Done { lockfile } => Ok(*lockfile),
            PypiFrame::Error { message } => Err(PnprClientError::Server(message)),
        }
    }

    /// Resolve a Cargo workspace against the server and return the
    /// rendered `Cargo.lock`. The server walks the sparse index and runs
    /// the same resolver the client would have run locally, so the
    /// lockfile is written verbatim.
    pub async fn resolve_cargo(
        &self,
        opts: CargoResolveOptions,
    ) -> Result<String, PnprClientError> {
        let request = serde_json::json!({
            "ecosystem": CARGO_ECOSYSTEM,
            "metadata": opts.metadata,
            "registry": opts.registry,
        });
        let frame = self.terminal_frame(&request, opts.authorization.as_deref()).await?;
        match parse_cargo_frame(&frame)? {
            CargoFrame::Done { lockfile } => Ok(lockfile),
            CargoFrame::Error { message } => Err(PnprClientError::Server(message)),
        }
    }

    /// Post a resolve request whose answer is one terminal frame, and read
    /// that frame.
    ///
    /// A Cargo or Python resolve yields nothing before it yields
    /// everything, so there is no stream to consume as it arrives. The
    /// read is bounded, which keeps a compromised server from growing the
    /// install's memory without limit, and a second frame is refused: a
    /// lockfile from a response that also reports a failure is not one to
    /// write.
    pub(super) async fn terminal_frame(
        &self,
        request: &serde_json::Value,
        authorization: Option<&str>,
    ) -> Result<Vec<u8>, PnprClientError> {
        let mut post = self.http.post(format!("{}-/pnpr/v0/resolve", self.base_url)).json(request);
        if let Some(authorization) = authorization {
            post = post.header("authorization", authorization);
        }
        let response = post.send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response_body_bounded(response, MAX_ERROR_BODY_SIZE).await?;
            return Err(PnprClientError::Server(format!(
                "/-/pnpr/v0/resolve returned {status}: {}",
                String::from_utf8_lossy(&body),
            )));
        }
        let body = response_body_bounded(response, MAX_TERMINAL_RESPONSE_SIZE).await?;
        let mut frames = body.split(|&byte| byte == b'\n').filter(|line| !line.is_empty());
        let Some(frame) = frames.next() else {
            return Err(PnprClientError::Protocol(
                "/-/pnpr/v0/resolve returned no terminal frame".to_string(),
            ));
        };
        if frames.next().is_some() {
            return Err(PnprClientError::Protocol(
                "/-/pnpr/v0/resolve returned more than one terminal frame".to_string(),
            ));
        }
        Ok(frame.to_vec())
    }

    /// Ask the server to verify a lockfile under the client's registry
    /// and policy settings, without resolving or echoing the lockfile
    /// back.
    pub async fn verify_lockfile(
        &self,
        opts: VerifyLockfileOptions,
    ) -> Result<(), PnprClientError> {
        let request = serde_json::json!({
            "registry": opts.registry,
            "registries": opts.registries,
            "overrides": opts.overrides,
            "lockfile": opts.lockfile,
            "trustLockfile": opts.trust_lockfile,
            "minimumReleaseAge": opts.minimum_release_age,
            "minimumReleaseAgeExclude": opts.minimum_release_age_exclude,
            "minimumReleaseAgeIgnoreMissingTime": opts.minimum_release_age_ignore_missing_time,
            "trustPolicy": opts.trust_policy,
            "trustPolicyExclude": opts.trust_policy_exclude,
            "trustPolicyIgnoreAfter": opts.trust_policy_ignore_after,
        });

        let mut post =
            self.http.post(format!("{}-/pnpr/v0/verify-lockfile", self.base_url)).json(&request);
        if let Some(authorization) = opts.authorization.as_deref() {
            post = post.header("authorization", authorization);
        }
        let response = post.send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(PnprClientError::Server(format!(
                "/-/pnpr/v0/verify-lockfile returned {status}: {body}",
            )));
        }

        let done = read_ndjson_frames(response, |line| match parse_verify_frame(line)? {
            VerifyFrame::Done => Ok(Some(())),
            VerifyFrame::Error { message } => Err(PnprClientError::Server(message)),
            VerifyFrame::Violations { violations } => {
                Err(PnprClientError::Verification(build_verify_error(violations)))
            }
        })
        .await?;
        done.ok_or_else(|| {
            PnprClientError::Protocol(
                "/-/pnpr/v0/verify-lockfile stream ended without a terminal frame".to_string(),
            )
        })
    }
}
