use super::{
    AuthType, Config, DEPRECATION_ERROR_BODY_LIMIT, DeprecateContext, DeprecateError, Diagnostic,
    Display, Error, Method, OtpChallenge, OtpError, OtpSession, Reporter, StatusCode,
    UnpublishHost, WebAuthFetchOptions, WebAuthRetryOptions, WithOtpError,
    otp_challenge_from_unauthorized_body, read_limited_body, registry_operation_failed,
    send_with_retry, write_error_for_status,
};

/// Everything a registry mutation of one package needs. The OTP session is
/// shared by every mutation of the run, so a partial unpublish (one `PUT`
/// plus a tarball `DELETE` per removed version) authenticates once.
pub(super) struct MutationContext<'a> {
    pub(super) registry: &'a DeprecateContext<'a>,
    pub(super) auth_header: Option<&'a str>,
    pub(super) auth_type: AuthType,
    pub(super) session: OtpSession,
}

#[derive(Clone, Copy)]
pub(super) struct MutationRequest<'a> {
    pub(super) method: &'a Method,
    pub(super) url: &'a str,
    pub(super) json_body: Option<&'a str>,
}

/// An HTTP-level failure of an unpublish mutation, handed to the
/// [`OtpSession`]. Only the [`Otp`](Self::Otp) arm is a challenge it acts on;
/// the rest propagate.
#[derive(Debug, Display, Error, Diagnostic)]
enum UnpublishHttpError {
    #[display("the registry requested a one-time password")]
    Otp {
        #[error(not(source))]
        challenge: OtpChallenge,
    },

    #[diagnostic(transparent)]
    Registry(#[error(not(source))] DeprecateError),
}

impl OtpError for UnpublishHttpError {
    fn as_otp_challenge(&self) -> Option<OtpChallenge> {
        match self {
            UnpublishHttpError::Otp { challenge } => Some(challenge.clone()),
            UnpublishHttpError::Registry(_) => None,
        }
    }
}

/// Send one mutation through the OTP session: the first attempt carries any
/// configured `--otp`; a 401 OTP challenge drives the interactive flow and
/// the request is retried with the obtained password, while any other 401 is
/// a plain authentication failure. Every other status is returned for the
/// caller to classify.
pub(super) async fn send_mutation<Sys: UnpublishHost, Reporter: self::Reporter>(
    mutation: &mut MutationContext<'_>,
    request: MutationRequest<'_>,
) -> miette::Result<reqwest::Response> {
    let MutationContext { registry, auth_header, auth_type, session } = mutation;
    let (registry, auth_header, auth_type) = (*registry, *auth_header, *auth_type);
    session
        .run::<Sys, Reporter, reqwest::Response, UnpublishHttpError, _, _>(
            // A plain `FnMut` returning an `async move` block (not an
            // `AsyncFnMut`) so the produced future carries an ordinary `Send`
            // obligation — see `with_otp_handling`'s `Operation` bound.
            move |challenge_otp: Option<String>| {
                // The web-auth-provided OTP (a fresh challenge) takes precedence
                // over any statically configured one.
                let effective_otp = challenge_otp.or_else(|| registry.otp.clone());
                async move {
                    send_once(registry, auth_header, auth_type, request, effective_otp.as_deref())
                        .await
                }
            },
        )
        .await
        .map_err(|error| match error {
            // Unwrap the operation's own failure so the user sees the registry
            // error once, not re-narrated through the OTP wrapper.
            WithOtpError::Operation(UnpublishHttpError::Registry(registry_error)) => {
                miette::Report::new(registry_error)
            }
            other => miette::Report::new(other),
        })
}

/// Perform a single mutation request and classify a 401: an OTP challenge
/// or a plain authentication failure.
async fn send_once(
    registry: &DeprecateContext<'_>,
    auth_header: Option<&str>,
    auth_type: AuthType,
    request: MutationRequest<'_>,
    otp: Option<&str>,
) -> Result<reqwest::Response, UnpublishHttpError> {
    let (_guard, response) =
        send_with_retry(&registry.http_client, request.url, registry.retry_opts, |client| {
            let mut builder = client
                .request(request.method.clone(), request.url)
                .header("npm-auth-type", auth_type.header_value());
            if let Some(json_body) = request.json_body {
                builder =
                    builder.header("content-type", "application/json").body(json_body.to_owned());
            }
            if let Some(auth_header) = auth_header {
                builder = builder.header("authorization", auth_header);
            }
            if let Some(otp) = otp {
                builder = builder.header("npm-otp", otp);
            }
            builder
        })
        .await
        .map_err(|source| {
            UnpublishHttpError::Registry(registry_operation_failed(
                "requesting the registry",
                source,
            ))
        })?;
    if response.status() != StatusCode::UNAUTHORIZED {
        return Ok(response);
    }
    unauthorized_unpublish(response).await
}

pub(super) fn web_auth_fetch_options(config: &Config) -> WebAuthFetchOptions {
    WebAuthFetchOptions {
        timeout: Some(config.fetch_timeout),
        retry: Some(WebAuthRetryOptions {
            factor: Some(f64::from(config.fetch_retry_factor)),
            max_timeout: Some(config.fetch_retry_maxtimeout),
            min_timeout: Some(config.fetch_retry_mintimeout),
            randomize: None,
            retries: Some(config.fetch_retries),
        }),
    }
}

async fn unauthorized_unpublish(
    response: reqwest::Response,
) -> Result<reqwest::Response, UnpublishHttpError> {
    let body =
        read_limited_body(response, DEPRECATION_ERROR_BODY_LIMIT).await.map_err(|source| {
            UnpublishHttpError::Registry(registry_operation_failed(
                "reading the registry error response",
                source,
            ))
        })?;
    if let Some(challenge) = otp_challenge_from_unauthorized_body(&body.bytes) {
        return Err(UnpublishHttpError::Otp { challenge });
    }
    Err(UnpublishHttpError::Registry(write_error_for_status(
        StatusCode::UNAUTHORIZED,
        &body,
        "unpublish".to_string(),
    )))
}
