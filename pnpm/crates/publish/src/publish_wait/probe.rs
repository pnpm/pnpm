use std::time::{Duration, SystemTime};

use pnpm_network::{
    ThrottledClientGuard, encode_package_name, read_limited_body, redact_url_for_display,
};
use reqwest::{Response, StatusCode, header};
use serde_json::Value;

use crate::{
    publish_packed_pkg::{PublishNetwork, join_registry},
    registry_config_keys::NormalizedRegistryUrl,
};

const MAX_METADATA_BYTES: usize = 64 * 1024 * 1024;
const INSTALL_METADATA_ACCEPT: &str = "application/vnd.npm.install-v1+json";

#[derive(Debug)]
pub(super) enum ProbeResult {
    Ready,
    Pending(Duration),
}

pub(super) async fn probe_package(
    name: &str,
    version: &str,
    registry: &NormalizedRegistryUrl,
    network: &PublishNetwork<'_>,
) -> Result<ProbeResult, String> {
    let url =
        join_registry(registry, &encode_package_name(name)).map_err(|error| error.to_string())?;
    let metadata = match request(&url, name, false, network).await {
        Ok((response, _guard)) => {
            if let Some(pending) = check_status(&response)? {
                return Ok(pending);
            }
            match read_limited_body(response, MAX_METADATA_BYTES).await {
                Ok(body) if !body.truncated => body.bytes,
                Ok(_) => return Err("Install metadata exceeds 64 MiB".to_owned()),
                Err(error) => return transport_error(error),
            }
        }
        Err(error) => return transport_error(error),
    };
    let metadata: Value = serde_json::from_slice(&metadata)
        .map_err(|_| "Registry returned invalid install metadata JSON".to_owned())?;
    let Some(tarball) = tarball_url(&metadata, version)? else {
        return Ok(ProbeResult::Pending(Duration::ZERO));
    };
    probe_tarball(tarball, name, network).await
}

fn tarball_url<'a>(metadata: &'a Value, version: &str) -> Result<Option<&'a str>, String> {
    let versions = metadata
        .get("versions")
        .and_then(Value::as_object)
        .ok_or_else(|| "Install metadata has no versions object".to_owned())?;
    let Some(manifest) = versions.get(version) else { return Ok(None) };
    let manifest = manifest
        .as_object()
        .ok_or_else(|| "Invalid version manifest in install metadata".to_owned())?;
    let Some(dist) = manifest.get("dist") else { return Ok(None) };
    let dist = dist.as_object().ok_or_else(|| "Invalid dist in install metadata".to_owned())?;
    let Some(tarball) = dist.get("tarball") else { return Ok(None) };
    let tarball =
        tarball.as_str().ok_or_else(|| "Invalid dist.tarball in install metadata".to_owned())?;
    let url = url::Url::parse(tarball).map_err(|_| "Invalid dist.tarball URL".to_owned())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("dist.tarball must be an HTTP(S) URL without embedded credentials".to_owned());
    }
    Ok(Some(tarball))
}

async fn probe_tarball(
    url: &str,
    name: &str,
    network: &PublishNetwork<'_>,
) -> Result<ProbeResult, String> {
    let (response, _guard) = match request(url, name, true, network).await {
        Ok(response) => response,
        Err(error) => return transport_error(error),
    };
    if let Some(pending) = check_status(&response)? {
        return Ok(pending);
    }
    if response.status() != StatusCode::OK {
        return Err(format!("Tarball request returned HTTP {}", response.status()));
    }
    Ok(ProbeResult::Ready)
}

async fn request<'a>(
    url: &str,
    name: &str,
    tarball: bool,
    network: &'a PublishNetwork<'_>,
) -> Result<(Response, ThrottledClientGuard<'a>), reqwest::Error> {
    let configure = |mut request: reqwest::RequestBuilder, target: &str| {
        request = request.header(header::CACHE_CONTROL, "no-cache");
        if !tarball {
            request = request.header(header::ACCEPT, INSTALL_METADATA_ACCEPT);
        }
        if let Some(auth) = network.auth_headers.for_secure_url_with_package(target, Some(name)) {
            request = request.header(header::AUTHORIZATION, auth);
        }
        request
    };
    if tarball {
        network.client.head_response_with_scoped_headers(url, configure).await
    } else {
        network.client.get_response_with_scoped_headers(url, configure).await
    }
}

fn check_status(response: &Response) -> Result<Option<ProbeResult>, String> {
    let status = response.status();
    if status.is_success() {
        return Ok(None);
    }
    if status == StatusCode::NOT_FOUND
        || status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
    {
        return Ok(Some(ProbeResult::Pending(retry_after(response))));
    }
    Err(format!("HTTP {status} from {}", redact_url_for_display(response.url().as_str())))
}

fn retry_after(response: &Response) -> Duration {
    let Some(value) = response
        .headers()
        .get(header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
    else {
        return Duration::ZERO;
    };
    if let Ok(seconds) = value.parse::<u64>() {
        return Duration::from_secs(seconds);
    }
    httpdate::parse_http_date(value)
        .ok()
        .and_then(|date| {
            date.duration_since(SystemTime::now())
                .ok()
        })
        .unwrap_or_default()
}

fn transport_error(error: reqwest::Error) -> Result<ProbeResult, String> {
    if error.is_builder() || error.is_redirect() {
        Err(error.without_url().to_string())
    } else {
        Ok(ProbeResult::Pending(Duration::ZERO))
    }
}
