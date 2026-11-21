use crate::{
    HttpStatusError,
    NetworkError,
    TarballError,
};
use pnpm_network::{
    AuthHeaders,
    ThrottledClient,
    ThrottledClientGuard,
    is_permanent_error,
};
use pnpm_reporter::{
    FetchingProgressLog,
    FetchingProgressMessage,
    LogEvent,
    LogLevel,
    Reporter,
};

/// The HTTP caching headers of an archive response, kept verbatim so the
/// caller can decide whether and for how long the response may be reused.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CacheHeaders {
    pub etag: Option<String>,
    pub cache_control: Option<String>,
    pub age: Option<String>,
    pub date: Option<String>,
    pub vary: Option<String>,
}

#[derive(Debug)]
pub(crate) struct ArchiveResponseMeta {
    pub not_modified: bool,
    pub cache_headers: CacheHeaders,
    pub final_url: String,
}

/// Authorize and start one archive request. The returned permit must remain
/// alive until the caller finishes consuming the body. `if_none_match` makes
/// the request conditional; a `304` answer is then returned as a success
/// with [`ArchiveResponseMeta::not_modified`] set.
#[expect(
    clippy::too_many_arguments,
    reason = "the parameters are independent request inputs; bundling them into a struct only moves the same fields into a wrapper"
)]
pub(crate) async fn request_archive<'client, Reporter: self::Reporter>(
    http_client: &'client ThrottledClient,
    package_url: &str,
    package_id: &str,
    auth_headers: &AuthHeaders,
    priority: u64,
    attempt: u32,
    revision_addressed: bool,
    if_none_match: Option<&str>,
    content_coding: ArchiveContentCoding,
) -> Result<(ThrottledClientGuard<'client>, reqwest::Response, ArchiveResponseMeta), TarballError> {
    let if_none_match = if_none_match.filter(|etag| !etag.trim().is_empty());
    if !auth_headers.allows_fetch(package_url) {
        return Err(TarballError::OffAllowlist {
            url: pnpm_network::redact_url_credentials(package_url),
        });
    }
    let options = pnpm_network::ScopedGetOptions {
        priority,
        decode_content: content_coding == ArchiveContentCoding::DecodedByClient,
        follow_redirects: !revision_addressed,
    };
    let sent = send_archive_request(
        http_client,
        options,
        ArchiveRequest { package_url, package_id, auth_headers, if_none_match, content_coding },
    )
    .await;
    // Failed connects are attempts too; the reporter's counter starts at one.
    let size = sent
        .as_ref()
        .ok()
        .and_then(|(response, _)| response.content_length());
    emit_started::<Reporter>(attempt, package_id.to_owned(), size);
    let (response, client) =
        sent.map_err(|error| TarballError::FetchTarball(NetworkError::new(package_url, error)))?;
    let meta = response_meta(&response);
    let response =
        check_archive_status(http_client, response, package_url, if_none_match.is_some()).await?;
    Ok((client, response, meta))
}

fn emit_started<Reporter: self::Reporter>(attempt: u32, package_id: String, size: Option<u64>) {
    Reporter::emit(&LogEvent::FetchingProgress(FetchingProgressLog {
        level: LogLevel::Debug,
        message: FetchingProgressMessage::Started {
            attempt: attempt.saturating_add(1),
            package_id,
            size,
        },
    }));
}

fn response_meta(response: &reqwest::Response) -> ArchiveResponseMeta {
    ArchiveResponseMeta {
        not_modified: response.status() == reqwest::StatusCode::NOT_MODIFIED,
        cache_headers: CacheHeaders {
            etag: header_string(response, reqwest::header::ETAG),
            cache_control: header_list(response, reqwest::header::CACHE_CONTROL),
            age: header_string(response, reqwest::header::AGE),
            date: header_string(response, reqwest::header::DATE),
            vary: header_list(response, reqwest::header::VARY),
        },
        final_url: response.url().to_string(),
    }
}

fn header_string(
    response: &reqwest::Response,
    name: reqwest::header::HeaderName,
) -> Option<String> {
    response
        .headers()
        .get(name)
        .map(reqwest::header::HeaderValue::to_str)
        .and_then(Result::ok)
        .map(str::to_owned)
}

/// Every field of a list-valued header, joined as one comma-separated value.
fn header_list(response: &reqwest::Response, name: reqwest::header::HeaderName) -> Option<String> {
    let values: Vec<&str> = response
        .headers()
        .get_all(name)
        .iter()
        .map(reqwest::header::HeaderValue::to_str)
        .filter_map(Result::ok)
        .collect();
    (!values.is_empty()).then(|| values.join(", "))
}

async fn check_archive_status(
    http_client: &ThrottledClient,
    response: reqwest::Response,
    package_url: &str,
    conditional: bool,
) -> Result<reqwest::Response, TarballError> {
    let status = response.status();
    if conditional && status == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(response);
    }
    if !status.is_success() {
        // Fully draining a small error body lets the connection be reused.
        const DRAIN_CAP: u64 = 64 * 1024;
        if response
            .content_length()
            .is_some_and(|len| len <= DRAIN_CAP)
            && let Err(error) = response.bytes().await
        {
            http_client.downscale_on_timeout(package_url, &error);
        }
        return Err(TarballError::HttpStatus(HttpStatusError {
            url: package_url.to_string(),
            status: status.as_u16(),
        }));
    }
    Ok(response)
}

/// Who removes an archive response's content coding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArchiveContentCoding {
    /// The fetcher offers its own `Accept-Encoding` and decodes the body.
    Negotiated,
    /// The HTTP client decodes the body, as it does for metadata.
    DecodedByClient,
}

#[derive(Clone, Copy)]
struct ArchiveRequest<'a> {
    package_url: &'a str,
    package_id: &'a str,
    auth_headers: &'a AuthHeaders,
    if_none_match: Option<&'a str>,
    content_coding: ArchiveContentCoding,
}

async fn send_archive_request<'client>(
    http_client: &'client ThrottledClient,
    options: pnpm_network::ScopedGetOptions,
    archive: ArchiveRequest<'_>,
) -> Result<(reqwest::Response, ThrottledClientGuard<'client>), reqwest::Error> {
    if let Some(response) = archive.auth_headers.fetch_local(
        archive.package_url,
        Some(archive.package_id),
        reqwest::header::HeaderMap::new(),
    ) {
        let client =
            http_client.acquire_for_url_with_priority(archive.package_url, options.priority).await;
        return Ok((response.await, client));
    }
    let authorize = pnpm_network::scoped_redirect_auth(archive.package_url, |url| {
        archive.auth_headers.for_secure_url_with_package(url, Some(archive.package_id))
    });
    let mut attempt = 0;
    loop {
        let sent = http_client.scoped_get(archive.package_url, options, |request, url| {
            build_archive_request(request, archive, authorize(url))
        })
        .await;
        let error = match sent {
            Ok(response) => return Ok(response),
            Err(error) => error,
        };
        http_client.downscale_on_timeout(archive.package_url, &error);
        let delay = retry_backoff_for_error(&error, attempt).ok_or(error)?;
        attempt += 1;
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
    }
}

fn build_archive_request(
    mut request: reqwest::RequestBuilder,
    archive: ArchiveRequest<'_>,
    authorization: Option<String>,
) -> reqwest::RequestBuilder {
    let ArchiveRequest { if_none_match, content_coding, .. } = archive;
    if let Some(value) = authorization {
        request = request.header("authorization", value);
    }
    if let Some(etag) = if_none_match {
        request = request.header(reqwest::header::IF_NONE_MATCH, etag);
    }
    // Browser networking owns `Accept-Encoding` and decodes the body itself.
    if content_coding == ArchiveContentCoding::Negotiated && cfg!(not(target_family = "wasm")) {
        request = request.header(
            reqwest::header::ACCEPT_ENCODING,
            crate::content_coding::ARCHIVE_ACCEPT_ENCODING,
        );
    }
    request
}

fn retry_backoff_for_error(error: &reqwest::Error, attempt: usize) -> Option<std::time::Duration> {
    const MAX_QUICK_RETRIES: usize = 2;
    if attempt >= MAX_QUICK_RETRIES || is_permanent_error(error) {
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
