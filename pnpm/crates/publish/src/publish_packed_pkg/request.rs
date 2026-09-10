use super::{
    Diagnostic, EnterKeyListener, OidcHttpOptions, OpenUrl, OtpChallenge, OtpError, OtpErrorBody,
    PromptOtp, Reporter, Sleep, StdinIsTty, StdoutIsTty, ThrottledClient, Value, WebAuthClock,
    WebAuthFetch, WebAuthFetchOptions, WebAuthRetryOptions, WithOtpError, with_otp_handling,
};

/// One completed publish response.
#[derive(Debug)]
pub(crate) struct PublishResponse {
    pub(crate) ok: bool,
    pub(crate) status: u16,
    pub(crate) status_text: String,
    pub(crate) body: String,
    pub(super) stage_id: Option<String>,
}

/// An HTTP-level publish failure handed to [`with_otp_handling`]. Only the
/// [`Otp`](Self::Otp) arm is a challenge it acts on; a transport failure
/// propagates.
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
pub enum PublishHttpError {
    #[display("the registry requested a one-time password")]
    Otp {
        #[error(not(source))]
        challenge: OtpChallenge,
    },

    #[display("the publish request failed: {reason}")]
    Transport {
        #[error(not(source))]
        reason: String,
    },
}

impl OtpError for PublishHttpError {
    fn as_otp_challenge(&self) -> Option<OtpChallenge> {
        match self {
            PublishHttpError::Otp { challenge } => Some(challenge.clone()),
            PublishHttpError::Transport { .. } => None,
        }
    }
}

/// Send the publish PUT, retrying once under OTP through the web-auth flow.
/// The operation returns `Ok` for every completed HTTP response (the caller
/// inspects `ok`) and `Err` only for an OTP challenge or a transport failure.
///
/// `Sys` is the web-auth [host](pnpm_network_web_auth::Host): production
/// passes the real one, tests pass a fake so the poll / clock / prompt are
/// scripted while the PUT still goes through a mocked registry.
#[expect(
    clippy::too_many_arguments,
    reason = "a single registry request legitimately needs the URL, auth, command, body, OTP, stage flag and retry options"
)]
pub(crate) async fn publish_with_otp_handling<Sys, Reporter>(
    client: &ThrottledClient,
    put_url: &str,
    authorization: Option<&str>,
    npm_command: &str,
    body: bytes::Bytes,
    otp: Option<&str>,
    is_stage: bool,
    fetch_options: WebAuthFetchOptions,
) -> Result<PublishResponse, WithOtpError<PublishHttpError>>
where
    Sys: WebAuthClock
        + Sleep
        + WebAuthFetch
        + StdinIsTty
        + StdoutIsTty
        + EnterKeyListener
        + OpenUrl
        + PromptOtp,
    Reporter: self::Reporter,
{
    with_otp_handling::<Sys, Reporter, PublishResponse, PublishHttpError, _, _>(
        fetch_options,
        // A plain `FnMut` returning an `async move` block (not an `AsyncFnMut`),
        // so the produced future is a concrete type with an ordinary `Send`
        // obligation — see `with_otp_handling`'s `Operation` bound.
        move |challenge_otp: Option<String>| {
            // The web-auth-provided OTP (a fresh challenge) takes precedence
            // over any statically configured one.
            let effective_otp = challenge_otp.or_else(|| otp.map(str::to_owned));
            // `Bytes::clone` is a cheap refcount bump, so the megabytes-large
            // body is not re-copied when the OTP retry re-invokes this closure.
            let body = body.clone();
            async move {
                put_publish(
                    client,
                    put_url,
                    authorization,
                    npm_command,
                    body,
                    effective_otp.as_deref(),
                    is_stage,
                )
                .await
            }
        },
    )
    .await
}

/// Perform a single publish PUT and classify the response.
pub(super) async fn put_publish(
    client: &ThrottledClient,
    put_url: &str,
    authorization: Option<&str>,
    npm_command: &str,
    body: bytes::Bytes,
    otp: Option<&str>,
    is_stage: bool,
) -> Result<PublishResponse, PublishHttpError> {
    let guard = client.acquire_for_url(put_url).await;
    // A staged publish POSTs to `-/stage/package/:pkg` (libnpmpublish's stage
    // route); a regular publish PUTs to `/:pkg`.
    let builder = if is_stage { guard.post(put_url) } else { guard.put(put_url) };
    let mut request = builder
        .header("content-type", "application/json")
        .header("npm-auth-type", "web")
        .header("npm-command", npm_command)
        .body(body);
    if let Some(authorization) = authorization {
        request = request.header("authorization", authorization);
    }
    if let Some(otp) = otp {
        request = request.header("npm-otp", otp);
    }

    let response = request
        .send()
        .await
        .map_err(|error| PublishHttpError::Transport { reason: error.to_string() })?;
    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or_default().to_owned();
    let www_authenticate = www_authenticate_header(&response);
    let body = response.text().await.unwrap_or_default();

    // The registry signals an OTP / web-auth challenge with a 401 that either
    // advertises the `otp` token in `WWW-Authenticate` or carries a
    // `one-time pass` body (npm-registry-fetch's two detection paths). For web
    // auth the body also carries `authUrl` / `doneUrl`.
    if status.as_u16() == 401 && is_otp_challenge(www_authenticate.as_deref(), &body) {
        return Err(PublishHttpError::Otp { challenge: parse_otp_challenge(&body) });
    }

    let stage_id = is_stage.then(|| stage_id_from_body(&body)).flatten();
    Ok(PublishResponse {
        ok: status.is_success(),
        status: status.as_u16(),
        status_text,
        body,
        stage_id,
    })
}

fn www_authenticate_header(response: &reqwest::Response) -> Option<String> {
    response
        .headers()
        .get(reqwest::header::WWW_AUTHENTICATE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

/// Whether a 401 response is an OTP / two-factor challenge: the
/// `WWW-Authenticate` header lists `otp` as a comma-separated token, or the
/// body mentions `one-time pass`.
pub(super) fn is_otp_challenge(www_authenticate: Option<&str>, body: &str) -> bool {
    let header_lists_otp = www_authenticate.is_some_and(|value| {
        value.split(',').any(|token| token.trim().eq_ignore_ascii_case("otp"))
    });
    header_lists_otp || body.to_lowercase().contains("one-time pass")
}

/// Read `authUrl` / `doneUrl` out of a challenge body for the web-auth flow.
pub(super) fn parse_otp_challenge(body: &str) -> OtpChallenge {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let read =
        |field: &str| parsed.as_ref().and_then(|json| json.get(field)?.as_str().map(str::to_owned));
    OtpChallenge {
        body: Some(OtpErrorBody { auth_url: read("authUrl"), done_url: read("doneUrl") }),
    }
}

fn stage_id_from_body(body: &str) -> Option<String> {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|json| json.get("stageId")?.as_str().map(str::to_owned))
}

pub(crate) fn web_auth_fetch_options(http: &OidcHttpOptions) -> WebAuthFetchOptions {
    WebAuthFetchOptions {
        timeout: http.fetch_timeout,
        retry: Some(WebAuthRetryOptions {
            factor: http.fetch_retry_factor,
            max_timeout: http.fetch_retry_maxtimeout,
            min_timeout: http.fetch_retry_mintimeout,
            randomize: None,
            retries: http.fetch_retries,
        }),
    }
}
