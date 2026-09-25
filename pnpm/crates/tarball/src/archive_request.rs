use crate::{HttpStatusError, NetworkError, TarballError, auth_header_for_package_download};
use pnpm_network::{AuthHeaders, ThrottledClient, ThrottledClientGuard, is_certificate_error};
use pnpm_reporter::{FetchingProgressLog, FetchingProgressMessage, LogEvent, LogLevel, Reporter};

/// Authorize and start one archive request. The returned permit must remain
/// alive until the caller finishes consuming the body.
pub(crate) async fn request_archive<'client, Reporter: self::Reporter>(
    http_client: &'client ThrottledClient,
    package_url: &str,
    package_id: &str,
    auth_headers: &AuthHeaders,
    priority: u64,
    attempt: u32,
    revision_addressed: bool,
) -> Result<(ThrottledClientGuard<'client>, reqwest::Response), TarballError> {
    if !auth_headers.allows_fetch(package_url) {
        return Err(TarballError::OffAllowlist {
            url: pnpm_network::redact_url_credentials(package_url),
        });
    }
    let client = if revision_addressed {
        http_client.acquire_for_url_without_redirects_with_priority(package_url, priority).await
    } else {
        http_client.acquire_for_url_with_priority(package_url, priority).await
    };
    let sent = send_archive_request(&client, package_url, package_id, auth_headers).await;
    // Failed connects are attempts too; the reporter's counter starts at one.
    let size = sent
        .as_ref()
        .ok()
        .and_then(reqwest::Response::content_length);
    Reporter::emit(&LogEvent::FetchingProgress(FetchingProgressLog {
        level: LogLevel::Debug,
        message: FetchingProgressMessage::Started {
            attempt: attempt.saturating_add(1),
            package_id: package_id.to_owned(),
            size,
        },
    }));
    let response =
        sent.map_err(|error| TarballError::FetchTarball(NetworkError::new(package_url, error)))?;
    let response = check_archive_status(response, package_url).await?;
    Ok((client, response))
}

async fn check_archive_status(
    response: reqwest::Response,
    package_url: &str,
) -> Result<reqwest::Response, TarballError> {
    let status = response.status();
    if !status.is_success() {
        // Fully draining a small error body lets the connection be reused.
        const DRAIN_CAP: u64 = 64 * 1024;
        if response
            .content_length()
            .is_some_and(|len| len <= DRAIN_CAP)
        {
            let _ = response.bytes().await;
        }
        return Err(TarballError::HttpStatus(HttpStatusError {
            url: package_url.to_string(),
            status: status.as_u16(),
        }));
    }
    Ok(response)
}

async fn send_archive_request(
    client: &ThrottledClientGuard<'_>,
    package_url: &str,
    package_id: &str,
    auth_headers: &AuthHeaders,
) -> Result<reqwest::Response, reqwest::Error> {
    let mut attempt = 0;
    loop {
        let request = build_archive_request(client, package_url, package_id, auth_headers);
        let error = match request.send().await {
            Ok(response) => return Ok(response),
            Err(error) => error,
        };
        let delay = retry_backoff_for_error(&error, attempt).ok_or(error)?;
        attempt += 1;
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
    }
}

fn build_archive_request(
    client: &ThrottledClientGuard<'_>,
    package_url: &str,
    package_id: &str,
    auth_headers: &AuthHeaders,
) -> reqwest::RequestBuilder {
    let mut request = client.get(package_url);
    if let Some(value) = auth_header_for_package_download(auth_headers, package_url, package_id) {
        request = request.header("authorization", value);
    }
    request
}

fn retry_backoff_for_error(error: &reqwest::Error, attempt: usize) -> Option<std::time::Duration> {
    const MAX_QUICK_RETRIES: usize = 2;
    if attempt >= MAX_QUICK_RETRIES || is_certificate_error(error) {
        return None;
    }
    if error.is_connect() {
        Some(std::time::Duration::from_millis(50))
    } else if error.is_request() {
        Some(std::time::Duration::ZERO)
    } else {
        None
    }
}
