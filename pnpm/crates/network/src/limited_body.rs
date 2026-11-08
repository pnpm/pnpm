use crate::error_chain::is_unclean_tls_close;
use bytes::Bytes;
use reqwest::{
    Response,
    header::{CONTENT_LENGTH, HeaderMap, TRANSFER_ENCODING},
};

/// A response body read through [`read_limited_body`]: at most the requested
/// number of bytes, with `truncated` recording whether the wire body was
/// longer.
pub struct LimitedBody {
    pub bytes: Vec<u8>,
    pub truncated: bool,
}

/// Read a response body, capping it at `limit` bytes. A registry response
/// that a command buffers whole (an error body, a metadata object) must not
/// let a hostile or broken server exhaust memory, so reading stops — and the
/// body is marked truncated — once the cap is reached.
pub async fn read_limited_body(
    response: Response,
    limit: usize,
) -> Result<LimitedBody, reqwest::Error> {
    read_capped_body(response, limit, BodyEnd::Framed).await
}

/// [`read_limited_body`] for a body whose format proves its own
/// completeness, ending a close-delimited body at an unclean TLS close as
/// [`read_self_delimiting_body`] does.
pub async fn read_limited_self_delimiting_body(
    response: Response,
    limit: usize,
) -> Result<LimitedBody, reqwest::Error> {
    let end = body_end(response.headers());
    read_capped_body(response, limit, end).await
}

async fn read_capped_body(
    mut response: Response,
    limit: usize,
    end: BodyEnd,
) -> Result<LimitedBody, reqwest::Error> {
    let header_exceeds_limit = response
        .content_length()
        .is_some_and(|length| length > limit as u64);
    let mut bytes = Vec::new();
    let mut truncated = header_exceeds_limit;
    while let Some(chunk) = next_chunk(&mut response, end).await? {
        let remaining = limit.saturating_sub(bytes.len());
        if chunk.len() > remaining {
            bytes.extend_from_slice(&chunk[..remaining]);
            truncated = true;
            break;
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(LimitedBody { bytes, truncated })
}

/// A response body as the chunks it arrived in.
///
/// Joining them is left to [`Self::into_text`], so a caller decides which
/// thread allocates the whole body.
pub struct BodyChunks(Vec<Bytes>);

impl BodyChunks {
    /// Join the chunks, decoding invalid UTF-8 lossily.
    pub fn into_text(self) -> String {
        let mut bytes = Vec::with_capacity(self.0.iter().map(Bytes::len).sum());
        for chunk in self.0 {
            bytes.extend_from_slice(&chunk);
        }
        String::from_utf8(bytes)
            .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned())
    }
}

/// Read a whole response body when the body's format proves its own
/// completeness, such as a JSON document.
///
/// A server or a TLS-intercepting proxy can end a body that has neither a
/// `Content-Length` nor chunked framing by closing the connection without a
/// TLS `close_notify` alert. Node.js reads that close as the end of the body,
/// rustls reports it as an error. For a response without either framing
/// header, this returns the bytes received before such a close
/// (<https://github.com/pnpm/pnpm/issues/16704>). Decompression drops
/// `Content-Length`, so a cut-short compressed body is left to the caller's
/// parse to reject.
pub async fn read_self_delimiting_body(
    mut response: Response,
) -> Result<BodyChunks, reqwest::Error> {
    let end = body_end(response.headers());
    let mut chunks = Vec::new();
    while let Some(chunk) = next_chunk(&mut response, end).await? {
        chunks.push(chunk);
    }
    Ok(BodyChunks(chunks))
}

/// How the end of a response body is recognized.
#[derive(Clone, Copy)]
enum BodyEnd {
    /// Only by the HTTP framing, so an unclean TLS close is an error.
    Framed,
    /// The body has no framing and the caller's parse proves completeness,
    /// so an unclean TLS close ends it.
    CloseDelimited,
}

fn body_end(headers: &HeaderMap) -> BodyEnd {
    if has_body_framing(headers) { BodyEnd::Framed } else { BodyEnd::CloseDelimited }
}

async fn next_chunk(
    response: &mut Response,
    end: BodyEnd,
) -> Result<Option<Bytes>, reqwest::Error> {
    match (response.chunk().await, end) {
        (Err(error), BodyEnd::CloseDelimited) if is_unclean_tls_close(&error) => Ok(None),
        (result, _) => result,
    }
}

fn has_body_framing(headers: &HeaderMap) -> bool {
    headers.contains_key(CONTENT_LENGTH)
        || headers
            .get_all(TRANSFER_ENCODING)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(','))
            .any(|coding| coding.trim().eq_ignore_ascii_case("chunked"))
}
