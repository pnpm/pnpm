use reqwest::Response;

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
    mut response: Response,
    limit: usize,
) -> Result<LimitedBody, reqwest::Error> {
    let header_exceeds_limit =
        response.content_length().is_some_and(|length| length > limit as u64);
    let mut bytes = Vec::new();
    let mut truncated = header_exceeds_limit;
    while let Some(chunk) = response.chunk().await? {
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
