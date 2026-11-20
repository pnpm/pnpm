use std::{
    borrow::Cow,
    io::Read,
};

use axum::{
    body::Bytes,
    http::{
        HeaderMap,
        StatusCode,
        header,
    },
    response::Response,
};

use super::{
    MAX_AUDIT_BODY_BYTES,
    audit_error_response,
};

const INITIAL_DECODE_CAPACITY_CAP: usize = 64 * 1024;

#[derive(Clone, Copy)]
enum ContentCoding {
    Identity,
    Gzip,
}

pub(super) fn decode_request_body<'body>(
    headers: &HeaderMap,
    body: &'body Bytes,
) -> Result<Cow<'body, [u8]>, Response> {
    match parse_content_coding(headers)? {
        ContentCoding::Identity => Ok(Cow::Borrowed(body.as_ref())),
        ContentCoding::Gzip => {
            decompress_gzip_bounded(body.as_ref(), MAX_AUDIT_BODY_BYTES).map(Cow::Owned)
        }
    }
}

fn parse_content_coding(headers: &HeaderMap) -> Result<ContentCoding, Response> {
    let mut values = headers.get_all(header::CONTENT_ENCODING).iter();
    let Some(first) = values.next() else {
        return Ok(ContentCoding::Identity);
    };
    if values.next().is_some() {
        return Err(unsupported_encoding_response());
    }
    let Ok(raw) = first.to_str() else {
        return Err(unsupported_encoding_response());
    };
    let token = raw.trim();
    if token.eq_ignore_ascii_case("identity") {
        return Ok(ContentCoding::Identity);
    }
    if token.eq_ignore_ascii_case("gzip") || token.eq_ignore_ascii_case("x-gzip") {
        return Ok(ContentCoding::Gzip);
    }
    Err(unsupported_encoding_response())
}

pub(super) fn decompress_gzip_bounded(
    compressed: &[u8],
    limit: usize,
) -> Result<Vec<u8>, Response> {
    if compressed.is_empty() {
        return Err(invalid_gzip_response());
    }
    let mut decompressed = Vec::with_capacity(compressed.len().min(INITIAL_DECODE_CAPACITY_CAP));
    let mut remaining = compressed;
    while !remaining.is_empty() {
        let before_len = remaining.len();
        let allowance = (limit - decompressed.len()) as u64;
        let mut decoder = flate2::bufread::GzDecoder::new(&mut remaining);
        if decoder
            .by_ref()
            .take(allowance + 1)
            .read_to_end(&mut decompressed)
            .is_err()
        {
            return Err(invalid_gzip_response());
        }
        if decompressed.len() > limit {
            return Err(payload_too_large_response());
        }
        if remaining.len() == before_len {
            return Err(invalid_gzip_response());
        }
    }
    Ok(decompressed)
}

fn unsupported_encoding_response() -> Response {
    audit_error_response(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported content-encoding")
}

fn invalid_gzip_response() -> Response {
    audit_error_response(StatusCode::BAD_REQUEST, "invalid gzip payload")
}

fn payload_too_large_response() -> Response {
    audit_error_response(StatusCode::PAYLOAD_TOO_LARGE, "request body too large")
}
