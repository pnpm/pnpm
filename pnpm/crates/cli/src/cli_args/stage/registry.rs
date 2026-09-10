use super::{
    Diagnostic, Display, Error, OtpChallenge, OtpError, OtpErrorBody, OtpSession, PER_PAGE,
    Reporter, RetryOpts, STAGE_BODY_LIMIT, STAGE_ERROR_BODY_LIMIT, STAGE_LIST_MAX_PAGES,
    STAGE_TARBALL_BODY_LIMIT, StageError, StageListResponse, ThrottledClient, Value,
    WebAuthFetchOptions, WebAuthHost, WithOtpError, body_display_string, read_limited_body,
    redact_url_credentials, send_with_retry,
};

/// A failed `-/stage` registry response
/// (`ERR_PNPM_STAGE_REGISTRY_ERROR`), with the same message shape as the
/// TypeScript CLI's stage registry error.
#[derive(Debug, Display, Error, Diagnostic)]
#[display("{message}")]
#[diagnostic(code(ERR_PNPM_STAGE_REGISTRY_ERROR))]
pub struct StageRegistryError {
    #[error(not(source))]
    pub(super) message: String,
    /// The response status, kept so a caller can tell "no such staged
    /// version" apart from a failure that applies to every request.
    #[error(not(source))]
    pub status: u16,
}

impl StageRegistryError {
    pub(super) fn new(action: &str, status: u16, status_text: &str, body: &str) -> Self {
        let status_display = if status_text.is_empty() {
            status.to_string()
        } else {
            format!("{status} {status_text}")
        };
        let trimmed = body.trim();
        let message = if trimmed.is_empty() {
            format!("Failed to {action} (status {status_display})")
        } else {
            format!("Failed to {action} (status {status_display}): {trimmed}")
        };
        StageRegistryError { message, status }
    }
}

/// Download one staged package without writing it to disk.
pub(super) async fn fetch_stage_tarball(
    context: &StageContext,
    stage_id: &str,
) -> miette::Result<Vec<u8>> {
    let url = stage_endpoint_url(&context.registry, &format!("-/stage/{stage_id}/tarball"))?;
    let action = format!("download staged package {stage_id}");
    let (_guard, response) = stage_send(context, reqwest::Method::GET, url.as_str(), None)
        .await
        .map_err(|source| request_failed(&action, source))?;
    if !response.status().is_success() {
        return Err(registry_error_from_response(response, &action).await.into());
    }
    let tarball_data = read_limited_body(response, STAGE_TARBALL_BODY_LIMIT)
        .await
        .map_err(|source| request_failed(&action, source))?;
    if tarball_data.truncated {
        return Err(StageError::RequestFailed {
            operation: action,
            reason: format!("registry response exceeded {STAGE_TARBALL_BODY_LIMIT} bytes"),
        }
        .into());
    }
    Ok(tarball_data.bytes)
}

pub(super) struct StageContext {
    pub(super) registry: String,
    pub(super) auth_header: Option<String>,
    pub(super) http_client: ThrottledClient,
    pub(super) retry_opts: RetryOpts,
    pub(super) otp: Option<String>,
    pub(super) web_auth_fetch_options: WebAuthFetchOptions,
}

/// An HTTP-level failure of a stage mutation, handed to the
/// [`OtpSession`]. Only the [`Otp`](Self::Otp) arm is a challenge it acts on;
/// the rest propagate.
#[derive(Debug, Display, Error, Diagnostic)]
enum StageHttpError {
    #[display("the registry requested a one-time password")]
    Otp {
        #[error(not(source))]
        challenge: OtpChallenge,
    },

    #[diagnostic(transparent)]
    Registry(#[error(not(source))] StageRegistryError),

    #[diagnostic(transparent)]
    Request(#[error(not(source))] Box<StageError>),
}

impl OtpError for StageHttpError {
    fn as_otp_challenge(&self) -> Option<OtpChallenge> {
        match self {
            StageHttpError::Otp { challenge } => Some(challenge.clone()),
            StageHttpError::Registry(_) | StageHttpError::Request(_) => None,
        }
    }
}

/// Send one stage mutation (approve / reject) with OTP / web-auth handling:
/// the first attempt carries any configured `--otp`; a 401 OTP challenge
/// drives the interactive flow and retries with the obtained password.
pub(super) async fn stage_request_with_otp<Reporter: self::Reporter>(
    context: &StageContext,
    method: reqwest::Method,
    url: &str,
    action: &str,
) -> miette::Result<()> {
    let mut session = OtpSession::new(context.web_auth_fetch_options.clone());
    stage_request_in_session::<Reporter>(context, &mut session, method, url, action).await
}

/// Send one stage mutation through `session`, so a series of mutations
/// shares a single one-time password. See [`stage_request_with_otp`] for the
/// standalone form.
pub(super) async fn stage_request_in_session<Reporter: self::Reporter>(
    context: &StageContext,
    session: &mut OtpSession,
    method: reqwest::Method,
    url: &str,
    action: &str,
) -> miette::Result<()> {
    session
        .run::<WebAuthHost, Reporter, (), StageHttpError, _, _>(
            // A plain `FnMut` returning an `async move` block (not an
            // `AsyncFnMut`) so the produced future carries an ordinary `Send`
            // obligation — see `with_otp_handling`'s `Operation` bound.
            move |challenge_otp: Option<String>| {
                // The web-auth-provided OTP (a fresh challenge) takes precedence
                // over any statically configured one.
                let effective_otp = challenge_otp.or_else(|| context.otp.clone());
                let method = method.clone();
                async move {
                    stage_mutation(context, method, url, action, effective_otp.as_deref()).await
                }
            },
        )
        .await
        .map_err(|error| match error {
            // Unwrap the operation's own failure so the user sees the registry
            // error once, not re-narrated through the OTP wrapper.
            WithOtpError::Operation(StageHttpError::Registry(registry_error)) => {
                miette::Report::new(registry_error)
            }
            WithOtpError::Operation(StageHttpError::Request(request_error)) => {
                miette::Report::new(*request_error)
            }
            other => miette::Report::new(other),
        })
}

/// Perform a single stage mutation request and classify the response.
async fn stage_mutation(
    context: &StageContext,
    method: reqwest::Method,
    url: &str,
    action: &str,
    otp: Option<&str>,
) -> Result<(), StageHttpError> {
    let (_guard, response) = stage_send(context, method, url, otp).await.map_err(|source| {
        StageHttpError::Request(Box::new(request_failed_error(action, source)))
    })?;
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let status_text = status.canonical_reason().unwrap_or_default().to_owned();
    let www_authenticate = response
        .headers()
        .get(reqwest::header::WWW_AUTHENTICATE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = read_limited_body(response, STAGE_ERROR_BODY_LIMIT).await.map_err(|source| {
        StageHttpError::Request(Box::new(request_failed_error(action, source)))
    })?;
    if status.as_u16() == 401
        && let Some(challenge) = parse_stage_otp_challenge(www_authenticate.as_deref(), &body.bytes)
    {
        return Err(StageHttpError::Otp { challenge });
    }
    Err(StageHttpError::Registry(StageRegistryError::new(
        action,
        status.as_u16(),
        &status_text,
        &body_display_string(&body),
    )))
}

/// Every staged version the registry reports, optionally narrowed to one
/// package name.
pub(super) async fn fetch_stage_items(
    context: &StageContext,
    package_filter: Option<&str>,
) -> miette::Result<Vec<Value>> {
    let mut items: Vec<Value> = Vec::new();
    let mut page: usize = 0;
    loop {
        let mut url = stage_endpoint_url(&context.registry, "-/stage")?;
        url.query_pairs_mut()
            .append_pair("page", &page.to_string())
            .append_pair("perPage", &PER_PAGE.to_string());
        if let Some(package) = package_filter {
            url.query_pairs_mut().append_pair("package", package);
        }
        let response: StageListResponse =
            stage_json_request(context, url.as_str(), "list staged packages").await?;
        let page_len = response.items.len();
        items.extend(response.items);
        if items.len() >= response.total || page_len < PER_PAGE {
            break;
        }
        page += 1;
        if page >= STAGE_LIST_MAX_PAGES {
            break;
        }
    }
    Ok(items)
}

/// GET a `-/stage` endpoint and parse its JSON body.
pub(super) async fn stage_json_request<Body: serde::de::DeserializeOwned>(
    context: &StageContext,
    url: &str,
    action: &str,
) -> miette::Result<Body> {
    let (_guard, response) = stage_send(context, reqwest::Method::GET, url, None)
        .await
        .map_err(|source| request_failed(action, source))?;
    if !response.status().is_success() {
        return Err(registry_error_from_response(response, action).await.into());
    }
    let body = read_limited_body(response, STAGE_BODY_LIMIT)
        .await
        .map_err(|source| request_failed(action, source))?;
    if body.truncated {
        return Err(StageError::RequestFailed {
            operation: action.to_owned(),
            reason: format!("registry response exceeded {STAGE_BODY_LIMIT} bytes"),
        }
        .into());
    }
    serde_json::from_slice(&body.bytes).map_err(|source| request_failed(action, source))
}

/// Send one request to a `-/stage` endpoint with the stage headers
/// (`npm-auth-type: web`, `npm-command: stage`, auth, optional OTP),
/// retrying transient failures.
async fn stage_send<'client>(
    context: &'client StageContext,
    method: reqwest::Method,
    url: &str,
    otp: Option<&str>,
) -> Result<(pnpm_network::ThrottledClientGuard<'client>, reqwest::Response), reqwest::Error> {
    send_with_retry(&context.http_client, url, context.retry_opts, |client| {
        let mut builder = client
            .request(method.clone(), url)
            .header("npm-auth-type", "web")
            .header("npm-command", "stage");
        if let Some(auth_header) = &context.auth_header {
            builder = builder.header("authorization", auth_header);
        }
        if let Some(otp) = otp {
            builder = builder.header("npm-otp", otp);
        }
        builder
    })
    .await
}

/// Map a failed (non-2xx) stage response to a [`StageRegistryError`].
async fn registry_error_from_response(
    response: reqwest::Response,
    action: &str,
) -> StageRegistryError {
    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or_default().to_owned();
    let body = match read_limited_body(response, STAGE_ERROR_BODY_LIMIT).await {
        Ok(body) => body_display_string(&body),
        Err(_) => String::new(),
    };
    StageRegistryError::new(action, status.as_u16(), &status_text, &body)
}

/// Identify a 401 stage response as an OTP / web-auth challenge: a JSON body
/// carrying `authUrl` + `doneUrl` (the browser-based flow), or a
/// `www-authenticate` header mentioning `otp` (classic TOTP).
fn parse_stage_otp_challenge(www_authenticate: Option<&str>, body: &[u8]) -> Option<OtpChallenge> {
    let parsed: Option<Value> = serde_json::from_slice(body).ok();
    let read =
        |field: &str| parsed.as_ref().and_then(|json| json.get(field)?.as_str().map(str::to_owned));
    let auth_url = read("authUrl");
    let done_url = read("doneUrl");
    let has_web_auth_urls = auth_url.is_some() && done_url.is_some();
    let header_mentions_otp =
        www_authenticate.is_some_and(|value| value.to_lowercase().contains("otp"));
    if !has_web_auth_urls && !header_mentions_otp {
        return None;
    }
    Some(OtpChallenge { body: Some(OtpErrorBody { auth_url, done_url }) })
}

fn request_failed(action: &str, source: impl std::fmt::Display) -> miette::Report {
    request_failed_error(action, source).into()
}

fn request_failed_error(action: &str, source: impl std::fmt::Display) -> StageError {
    StageError::RequestFailed {
        operation: action.to_owned(),
        reason: redact_url_credentials(&source.to_string()),
    }
}

/// Resolve a `-/stage` path against the registry base URL.
pub(super) fn stage_endpoint_url(registry: &str, path: &str) -> miette::Result<reqwest::Url> {
    reqwest::Url::parse(registry)
        .and_then(|url| url.join(path))
        .map_err(|source| request_failed("build the registry staging URL", source))
}
