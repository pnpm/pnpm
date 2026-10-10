use crate::cli_args::{
    profile::ProfileError,
    registry_client::{apply_auth_and_otp, join_registry_endpoint},
    sanitize::{DEFAULT_ERROR_BODY_LIMIT, read_sanitized_error_body},
};
use miette::{Context, IntoDiagnostic};
use pnpm_network::{RetryOpts, ThrottledClient, read_limited_body, send_with_retry};
use serde_json::{Value, json};

pub(super) const PROFILE_BODY_LIMIT: usize = 1024 * 1024;

pub(super) async fn fetch_profile(
    registry_url: &str,
    http_client: &ThrottledClient,
    auth_header: &str,
    otp: Option<&str>,
    retry_opts: RetryOpts,
) -> miette::Result<Value> {
    let endpoint = join_registry_endpoint(registry_url, "-/npm/v1/user")
        .into_diagnostic()
        .wrap_err("resolving profile endpoint")?;
    let (client, response) = send_with_retry(http_client, &endpoint, retry_opts, |cli| {
        apply_auth_and_otp(cli.get(&endpoint), Some(auth_header), otp)
    })
    .await
    .into_diagnostic()
    .wrap_err("requesting user profile")?;

    let status = response.status();
    if status.as_u16() == 401 {
        return Err(ProfileError::Unauthorized.into());
    }
    if !status.is_success() {
        return Err(error_from_response(response, "fetch profile").await);
    }

    let body = response
        .json::<Value>()
        .await
        .into_diagnostic()
        .wrap_err("parsing profile response")?;
    drop(client);
    Ok(body)
}

pub(super) async fn update_profile(
    registry_url: &str,
    http_client: &ThrottledClient,
    auth_header: &str,
    otp: Option<&str>,
    retry_opts: RetryOpts,
    body: &Value,
) -> miette::Result<Value> {
    let endpoint = join_registry_endpoint(registry_url, "-/npm/v1/user")
        .into_diagnostic()
        .wrap_err("resolving profile endpoint")?;
    let body_bytes =
        serde_json::to_vec(body).into_diagnostic().wrap_err("serializing profile request body")?;

    let (client, response) = send_with_retry(http_client, &endpoint, retry_opts, |cli| {
        let builder = cli
            .post(&endpoint)
            .header("content-type", "application/json")
            .body(body_bytes.clone());
        apply_auth_and_otp(builder, Some(auth_header), otp)
    })
    .await
    .into_diagnostic()
    .wrap_err("updating user profile")?;

    let status = response.status();
    if status.as_u16() == 401 {
        return Err(ProfileError::Unauthorized.into());
    }
    if !status.is_success() {
        return Err(error_from_response(response, "update profile").await);
    }

    let Ok(bytes) = read_limited_body(response, PROFILE_BODY_LIMIT).await else {
        return Ok(json!({ "ok": true }));
    };
    let res = serde_json::from_slice::<Value>(&bytes.bytes).unwrap_or(json!({ "ok": true }));
    drop(client);
    Ok(res)
}

async fn error_from_response(
    response: reqwest::Response,
    operation: &'static str,
) -> miette::Report {
    let (status, status_text, body) =
        read_sanitized_error_body(response, DEFAULT_ERROR_BODY_LIMIT).await;
    match status {
        reqwest::StatusCode::FORBIDDEN => ProfileError::Forbidden { body }.into(),
        _ => ProfileError::RegistryFailed { operation, status: status.as_u16(), status_text, body }
            .into(),
    }
}
