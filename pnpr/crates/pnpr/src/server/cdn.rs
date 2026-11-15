use super::{AppState, RegistryError, ecosystem::sha256_hex};
use pnpr_config::CloudflareCdnConfig;
use pnpr_storage::{Storage, database::CdnInvalidation};
use std::{sync::Arc, time::Duration};

pub(super) fn public_tarball_response(
    mut response: axum::response::Response,
) -> axum::response::Response {
    use axum::http::{HeaderValue, header};
    if response.status().is_success()
        && response
            .headers()
            .get(header::CONTENT_TYPE)
            .is_some_and(|value| {
                value == "application/octet-stream" || value == "application/x-tar"
            })
        && !response.headers().contains_key(header::CACHE_CONTROL)
    {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=300"));
    }
    response
}

const RETRY_INTERVAL: Duration = Duration::from_secs(5);

pub(super) fn start(state: &AppState) -> Result<(), RegistryError> {
    let Some(config) = state.inner.config.http.cloudflare_cdn.clone() else { return Ok(()) };
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| RegistryError::Internal {
            reason: "could not initialize CDN invalidation".to_string(),
        })?;
    let weak = Arc::downgrade(&state.inner);
    tokio::spawn(async move {
        let mut enabled = false;
        loop {
            let Some(inner) = weak.upgrade() else { break };
            let storage = inner.storage.clone();
            drop(inner);
            if !enabled {
                enabled = subscribe(&storage, &config).await;
            }
            if enabled && let Err(reason) = deliver(&storage, &client, &config).await {
                tracing::error!(%reason, "CDN purge failed; pending invalidations will be retried");
            }
            tokio::time::sleep(RETRY_INTERVAL).await;
        }
    });
    Ok(())
}

async fn subscribe(storage: &Storage, config: &CloudflareCdnConfig) -> bool {
    let target = format!("{}:{}", config.zone_id, config.cache_tag);
    match storage.enable_cdn_invalidations(&target).await {
        Ok(()) => true,
        Err(error) => {
            tracing::error!(%error, "could not record CDN startup invalidation; retrying");
            false
        }
    }
}

async fn deliver(
    storage: &Storage,
    client: &reqwest::Client,
    config: &CloudflareCdnConfig,
) -> Result<(), String> {
    let pending = storage
        .pending_cdn_invalidations()
        .await
        .map_err(|_| "could not read pending CDN invalidations".to_string())?;
    if pending.is_empty() {
        return Ok(());
    }
    let tags = purge_tags(config, &pending);
    let endpoint =
        format!("https://api.cloudflare.com/client/v4/zones/{}/purge_cache", config.zone_id);
    purge(client, &endpoint, &config.api_token, tags).await?;
    storage
        .acknowledge_cdn_invalidations(pending)
        .await
        .map_err(|_| "could not acknowledge CDN invalidations".to_string())?;
    Ok(())
}

fn purge_tags(config: &CloudflareCdnConfig, pending: &[CdnInvalidation]) -> Vec<String> {
    if pending
        .iter()
        .any(|event| event.package == "*")
    {
        return vec![config.cache_tag.clone()];
    }
    pending
        .iter()
        .map(|event| format!("{}-{}", config.cache_tag, sha256_hex(event.package.as_bytes())))
        .collect()
}

async fn purge(
    client: &reqwest::Client,
    endpoint: &str,
    token: &str,
    tags: Vec<String>,
) -> Result<(), String> {
    let response = client
        .post(endpoint)
        .bearer_auth(token)
        .json(&serde_json::json!({ "tags": tags }))
        .send()
        .await
        .map_err(|error| format!("Cloudflare request failed: {}", error.without_url()))?;
    if !response.status().is_success() {
        return Err(format!("Cloudflare returned HTTP {}", response.status()));
    }
    #[derive(serde::Deserialize)]
    struct Purged {
        success: bool,
    }
    let body = pnpm_network::read_limited_body(response, 64 * 1024).await
        .map_err(|_| "could not read Cloudflare response".to_string())?;
    if body.truncated {
        return Err("Cloudflare response exceeded 64 KiB".to_string());
    }
    let result: Purged =
        serde_json::from_slice(&body.bytes).map_err(|_| "invalid Cloudflare response".to_string())?;
    if !result.success {
        return Err("Cloudflare rejected the purge".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
