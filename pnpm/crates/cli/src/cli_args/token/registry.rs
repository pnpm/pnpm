use crate::cli_args::{
    registry_client::{apply_auth_and_otp, join_registry_endpoint},
    sanitize::{DEFAULT_ERROR_BODY_LIMIT, read_sanitized_error_body},
    token::{TOKEN_BODY_LIMIT, TokenError},
};
use miette::{Context, IntoDiagnostic};
use pnpm_network::{RetryOpts, ThrottledClient, read_limited_body, send_with_retry};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenItem {
    pub key: String,
    pub token: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub created: Option<String>,
    #[serde(default)]
    pub readonly: bool,
    #[serde(default)]
    pub cidr_whitelist: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TokenListEnvelope {
    Paged {
        #[serde(default)]
        objects: Vec<TokenItem>,
    },
    List(Vec<TokenItem>),
}

#[derive(Serialize)]
struct CreateTokenPayload<'a> {
    readonly: bool,
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    cidr_whitelist: &'a [String],
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateTokenResponse {
    pub token: String,
    #[serde(default)]
    pub readonly: bool,
    #[serde(default)]
    pub cidr_whitelist: Option<Vec<String>>,
    #[serde(default)]
    pub expires: Option<serde_json::Value>,
}

pub(super) async fn fetch_token_list(
    url: &str,
    http_client: &ThrottledClient,
    auth_header: &str,
    retry_opts: RetryOpts,
) -> miette::Result<Vec<TokenItem>> {
    let endpoint = join_registry_endpoint(url, "-/npm/v1/tokens")
        .unwrap_or_else(|_| format!("{url}-/npm/v1/tokens"));
    let (_client, response) = send_with_retry(http_client, &endpoint, retry_opts, |cli| {
        apply_auth_and_otp(cli.get(&endpoint), Some(auth_header), None)
    })
    .await
    .into_diagnostic()
    .wrap_err("requesting tokens")?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(TokenError::Unauthorized.into());
    }
    if !response.status().is_success() {
        let (status, status_text, body) =
            read_sanitized_error_body(response, DEFAULT_ERROR_BODY_LIMIT).await;
        return Err(TokenError::RegistryWriteFailed {
            action: "list tokens".to_string(),
            status: status.as_u16(),
            status_text,
            body,
        }
        .into());
    }
    let body = read_limited_body(response, TOKEN_BODY_LIMIT).await
        .map_err(|err| TokenError::RegistryOperationFailed {
            operation: "reading token list",
            reason: err.to_string(),
        })?;
    let envelope: TokenListEnvelope = serde_json::from_slice(&body.bytes)
        .into_diagnostic()
        .wrap_err("parsing tokens response")?;
    let tokens = match envelope {
        TokenListEnvelope::Paged { objects, .. } => objects,
        TokenListEnvelope::List(list) => list,
    };
    Ok(tokens)
}

pub(super) async fn delete_token(
    url: &str,
    http_client: &ThrottledClient,
    auth_header: &str,
    otp: Option<&str>,
    key: &str,
    retry_opts: RetryOpts,
) -> miette::Result<()> {
    let path = format!("-/npm/v1/tokens/token/{key}");
    let endpoint = join_registry_endpoint(url, &path).unwrap_or_else(|_| format!("{url}{path}"));
    let (_client, response) = send_with_retry(http_client, &endpoint, retry_opts, |cli| {
        apply_auth_and_otp(cli.delete(&endpoint), Some(auth_header), otp)
    })
    .await
    .into_diagnostic()
    .wrap_err("revoking token")?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(TokenError::Unauthorized.into());
    }
    if !response.status().is_success() {
        let (status, status_text, body) =
            read_sanitized_error_body(response, DEFAULT_ERROR_BODY_LIMIT).await;
        return Err(TokenError::RegistryWriteFailed {
            action: format!("revoke token {key}"),
            status: status.as_u16(),
            status_text,
            body,
        }
        .into());
    }
    Ok(())
}

pub(super) async fn create_token(
    url: &str,
    http_client: &ThrottledClient,
    auth_header: &str,
    otp: Option<&str>,
    readonly: bool,
    cidr: &[String],
    retry_opts: RetryOpts,
) -> miette::Result<CreateTokenResponse> {
    let endpoint = join_registry_endpoint(url, "-/npm/v1/tokens")
        .unwrap_or_else(|_| format!("{url}-/npm/v1/tokens"));
    let payload = CreateTokenPayload { readonly, cidr_whitelist: cidr };
    let (_client, response) = send_with_retry(http_client, &endpoint, retry_opts, |cli| {
        apply_auth_and_otp(cli.post(&endpoint).json(&payload), Some(auth_header), otp)
    })
    .await
    .into_diagnostic()
    .wrap_err("creating token")?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(TokenError::Unauthorized.into());
    }
    if !response.status().is_success() {
        let (status, status_text, body) =
            read_sanitized_error_body(response, DEFAULT_ERROR_BODY_LIMIT).await;
        return Err(TokenError::RegistryWriteFailed {
            action: "create token".to_string(),
            status: status.as_u16(),
            status_text,
            body,
        }
        .into());
    }
    let body = read_limited_body(response, TOKEN_BODY_LIMIT).await
        .map_err(|err| TokenError::RegistryOperationFailed {
            operation: "reading create token response",
            reason: err.to_string(),
        })?;
    serde_json::from_slice(&body.bytes).into_diagnostic().wrap_err("parsing create token response")
}
