use crate::cli_args::{
    profile::ProfileError,
    registry_client::{apply_auth_and_otp, join_registry_endpoint},
    sanitize::{DEFAULT_ERROR_BODY_LIMIT, read_sanitized_error_body},
};
use miette::{Context, IntoDiagnostic};
use pnpm_network::{RetryOpts, ThrottledClient, UNPRIORITIZED, read_limited_body, send_with_retry};
use pnpm_network_web_auth::otp_challenge_from_unauthorized_body;
use serde_json::{Map, Value};

const PROFILE_BODY_LIMIT: usize = 1024 * 1024;

/// The registry's `-/npm/v1/user` endpoint, with the credentials every
/// profile request carries.
pub(super) struct ProfileRegistry<'a> {
    pub url: &'a str,
    pub client: &'a ThrottledClient,
    pub auth_header: &'a str,
    pub otp: Option<&'a str>,
    pub retry_opts: RetryOpts,
}

impl ProfileRegistry<'_> {
    pub(super) async fn fetch(&self) -> miette::Result<Value> {
        let endpoint = self.endpoint()?;
        let (client, response) = send_with_retry(self.client, &endpoint, self.retry_opts, |cli| {
            apply_auth_and_otp(cli.get(&endpoint), Some(self.auth_header), self.otp)
        })
        .await
        .into_diagnostic()
        .wrap_err("requesting user profile")?;
        let profile = read_json_response(response, "fetch profile").await?;
        drop(client);
        Ok(profile)
    }

    /// POST `body` and return the registry's reply. An empty reply is an
    /// empty object.
    ///
    /// The body can carry the account password or an authenticator code, so
    /// the request is sent once and never follows a redirect.
    pub(super) async fn update(&self, body: &Value) -> miette::Result<Value> {
        let endpoint = self.endpoint()?;
        let body_bytes = serde_json::to_vec(body)
            .into_diagnostic()
            .wrap_err("serializing profile request body")?;
        let client =
            self.client.acquire_for_url_without_redirects_with_priority(&endpoint, UNPRIORITIZED)
                .await;
        let request = client
            .post(&endpoint)
            .header("content-type", "application/json")
            .body(body_bytes);
        let response = apply_auth_and_otp(request, Some(self.auth_header), self.otp)
            .send()
            .await
            .into_diagnostic()
            .wrap_err("updating user profile")?;
        let reply = read_json_response(response, "update profile").await?;
        drop(client);
        Ok(reply)
    }

    fn endpoint(&self) -> miette::Result<String> {
        join_registry_endpoint(self.url, "-/npm/v1/user")
            .into_diagnostic()
            .wrap_err("resolving profile endpoint")
    }
}

async fn read_json_response(
    response: reqwest::Response,
    operation: &'static str,
) -> miette::Result<Value> {
    if !response.status().is_success() {
        return Err(error_from_response(response, operation).await);
    }
    let body = read_limited_body(response, PROFILE_BODY_LIMIT).await
        .into_diagnostic()
        .wrap_err("reading profile response")?;
    if body.truncated {
        return Err(ProfileError::ResponseTooLarge { limit: PROFILE_BODY_LIMIT }.into());
    }
    if body.bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(Value::Object(Map::new()));
    }
    serde_json::from_slice(&body.bytes).into_diagnostic().wrap_err("parsing profile response")
}

async fn error_from_response(
    response: reqwest::Response,
    operation: &'static str,
) -> miette::Report {
    let (status, status_text, body) =
        read_sanitized_error_body(response, DEFAULT_ERROR_BODY_LIMIT).await;
    match status {
        reqwest::StatusCode::UNAUTHORIZED
            if otp_challenge_from_unauthorized_body(body.as_bytes()).is_some() =>
        {
            ProfileError::OtpRequired.into()
        }
        reqwest::StatusCode::UNAUTHORIZED => ProfileError::Unauthorized.into(),
        reqwest::StatusCode::FORBIDDEN => ProfileError::Forbidden { body }.into(),
        _ => ProfileError::RegistryFailed { operation, status: status.as_u16(), status_text, body }
            .into(),
    }
}
