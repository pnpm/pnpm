use super::{
    AccessArgs, AccessError, Arc, Config, Duration, Method, RedirectGuard, Response, RetryOpts,
    StatusCode, ThrottledClient, ThrottledClientGuard, redact_and_sanitize, send_with_retry,
};
use pnpm_network::{normalize_registry_url, read_limited_body};

const ACCESS_ERROR_BODY_LIMIT: usize = 64 * 1024;

pub(super) struct AccessContext<'a> {
    pub(super) config: &'a Config,
    pub(super) http_client: ThrottledClient,
    pub(super) retry_opts: RetryOpts,
    pub(super) registry: String,
    pub(super) json: bool,
    pub(super) otp: Option<String>,
}

pub(super) fn build_access_context<'a>(
    args: &AccessArgs,
    config: &'a Config,
) -> miette::Result<AccessContext<'a>> {
    let registry = args.registry
        .as_deref()
        .map_or_else(|| config.registry.clone(), |url| normalize_registry_url(url).into_owned());

    let redirect_guard = args.otp
        .as_ref()
        .map(|_| {
            let registry_origin: Option<(String, String, Option<u16>)> =
                reqwest::Url::parse(&registry)
                    .ok()
                    .and_then(|url| {
                        url.host_str()
                            .map(|host| (url.scheme().to_string(), host.to_string(), url.port()))
                    });
            let guard: RedirectGuard = Arc::new(move |target: &reqwest::Url| -> bool {
                registry_origin
                    .as_ref()
                    .is_some_and(|(scheme, host, port)| {
                        target.scheme() == scheme
                            && target.host_str() == Some(host.as_str())
                            && target.port() == *port
                    })
            });
            guard
        });

    Ok(AccessContext {
        config,
        http_client: crate::cli_args::registry_client::build_registry_client_with_guard(
            config,
            redirect_guard.as_ref(),
        )?,
        retry_opts: RetryOpts {
            retries: config.fetch_retries,
            factor: config.fetch_retry_factor,
            min_timeout: Duration::from_millis(config.fetch_retry_mintimeout),
            max_timeout: Duration::from_millis(config.fetch_retry_maxtimeout),
        },
        registry,
        json: args.json,
        otp: args.otp.clone(),
    })
}

/// GET `url`, carrying the registry's authorization header when there is
/// one.
pub(super) async fn send_get<'client>(
    context: &'client AccessContext<'_>,
    url: &str,
    auth_header: Option<&str>,
) -> Result<(ThrottledClientGuard<'client>, Response), reqwest::Error> {
    send_with_retry(&context.http_client, url, context.retry_opts, |client| {
        let mut builder = client.get(url);
        if let Some(auth) = auth_header {
            builder = builder.header("authorization", auth);
        }
        builder
    })
    .await
}

/// Send `body` as JSON, carrying the registry's authorization header and
/// the one-time password when the command was given one.
pub(super) async fn send_json<'client>(
    context: &'client AccessContext<'_>,
    method: Method,
    url: &str,
    auth_header: Option<&str>,
    body: &serde_json::Value,
) -> Result<(ThrottledClientGuard<'client>, Response), reqwest::Error> {
    let body_bytes = serde_json::to_vec(body).expect("a serializable object");
    send_with_retry(&context.http_client, url, context.retry_opts, |client| {
        let mut builder = client
            .request(method.clone(), url)
            .header("content-type", "application/json")
            .body(body_bytes.clone());
        if let Some(auth) = auth_header {
            builder = builder.header("authorization", auth);
        }
        if let Some(otp) = &context.otp {
            builder = builder.header("npm-otp", otp);
        }
        builder
    })
    .await
}

pub(super) async fn fetch_error_from_response(response: Response, action: &str) -> miette::Report {
    let status = response.status();
    AccessError::RegistryFetchFailed {
        action: action.to_string(),
        status: status.as_u16(),
        status_text: status
            .canonical_reason()
            .unwrap_or_default()
            .to_string(),
    }
    .into()
}

pub(super) async fn write_error_from_response(
    response: Response,
    action: String,
    package_name: &str,
) -> miette::Report {
    let status = response.status();
    let status_text = status
        .canonical_reason()
        .unwrap_or_default()
        .to_string();
    let body = match read_limited_body(response, ACCESS_ERROR_BODY_LIMIT).await {
        Ok(body) => redact_and_sanitize(&super::super::sanitize::body_display_string(&body)),
        Err(_) => String::new(),
    };

    match status {
        StatusCode::UNAUTHORIZED => AccessError::Unauthorized { action, body }.into(),
        StatusCode::FORBIDDEN => AccessError::Forbidden { action, body }.into(),
        StatusCode::NOT_FOUND => {
            AccessError::PackageNotFound { package_name: package_name.to_string() }.into()
        }
        StatusCode::UNPROCESSABLE_ENTITY => AccessError::ValidationError { body }.into(),
        _ => {
            AccessError::RegistryWriteFailed { action, status: status.as_u16(), status_text, body }
                .into()
        }
    }
}
