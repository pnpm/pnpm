use super::{
    AccessArgs, AccessError, Arc, Config, Context, Duration, IntoDiagnostic, Method, RedirectGuard,
    Response, RetryOpts, StatusCode, ThrottledClient, ThrottledClientGuard, encode_uri_component,
    redact_and_sanitize, send_with_retry,
};
use futures_util::StreamExt as _;

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
    let registry =
        args.registry.as_deref().map_or_else(|| config.registry.clone(), normalize_registry_url);

    let redirect_guard = args.otp.as_ref().map(|_| {
        let registry_origin: Option<(String, String, Option<u16>)> =
            reqwest::Url::parse(&registry).ok().and_then(|url| {
                url.host_str().map(|host| (url.scheme().to_string(), host.to_string(), url.port()))
            });
        let guard: RedirectGuard = Arc::new(move |target: &reqwest::Url| -> bool {
            registry_origin.as_ref().is_some_and(|(scheme, host, port)| {
                target.scheme() == scheme
                    && target.host_str() == Some(host.as_str())
                    && target.port() == *port
            })
        });
        guard
    });

    Ok(AccessContext {
        config,
        http_client: build_http_client(config, redirect_guard.as_ref())?,
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

fn build_http_client(
    config: &Config,
    redirect_guard: Option<&RedirectGuard>,
) -> miette::Result<ThrottledClient> {
    ThrottledClient::for_installs_with_guard(
        &config.proxy,
        &config.tls,
        &config.tls_by_uri,
        &config.network_settings(),
        redirect_guard,
    )
    .into_diagnostic()
    .wrap_err("create the network client for access command")
}

pub(super) fn normalize_registry_url(registry_url: &str) -> String {
    if registry_url.ends_with('/') { registry_url.to_string() } else { format!("{registry_url}/") }
}

pub(super) fn escaped_package_name(package_name: &str) -> String {
    match package_name.strip_prefix('@') {
        Some(rest) => format!("@{}", encode_uri_component(rest).replace("%2F", "%2f")),
        None => encode_uri_component(package_name),
    }
}

pub(super) async fn fetch_error_from_response(response: Response, action: &str) -> miette::Report {
    let status = response.status();
    AccessError::RegistryFetchFailed {
        action: action.to_string(),
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or_default().to_string(),
    }
    .into()
}

pub(super) async fn write_error_from_response(
    response: Response,
    action: String,
    package_name: &str,
) -> miette::Report {
    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or_default().to_string();
    let body = redact_and_sanitize(&read_error_body(response).await);

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

async fn read_error_body(response: Response) -> String {
    let limit = ACCESS_ERROR_BODY_LIMIT;
    let header_exceeds_limit =
        response.content_length().is_some_and(|length| length > limit as u64);
    let mut bytes = Vec::new();
    let mut truncated = header_exceeds_limit;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else { break };
        let remaining = limit.saturating_sub(bytes.len());
        if chunk.len() > remaining {
            bytes.extend_from_slice(&chunk[..remaining]);
            truncated = true;
            break;
        }
        bytes.extend_from_slice(&chunk);
    }
    let mut body = String::from_utf8_lossy(&bytes).into_owned();
    if truncated {
        if !body.is_empty() && !body.chars().next_back().is_some_and(char::is_whitespace) {
            body.push(' ');
        }
        body.push_str("(response body truncated)");
    }
    body
}
