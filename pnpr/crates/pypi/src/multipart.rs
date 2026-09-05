//! A `multipart/form-data` reader for the legacy upload API.
//!
//! The upload body is one form with a handful of short text fields and one
//! file part, already bounded by the server's body limit and fully buffered,
//! so this reads the whole body at once rather than streaming parts.

use derive_more::{Display, Error};

/// One part of a `multipart/form-data` body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormPart {
    /// The `name` of the part's `Content-Disposition`.
    pub name: String,
    /// The `filename` of the part's `Content-Disposition`, for file parts.
    pub filename: Option<String>,
    pub data: Vec<u8>,
}

/// A body that is not well-formed `multipart/form-data`.
#[derive(Debug, Display, Error, Clone, PartialEq, Eq)]
pub enum MultipartError {
    #[display("request body must be multipart/form-data")]
    NotMultipart,
    #[display("multipart/form-data content type carries no boundary")]
    MissingBoundary,
    #[display("multipart body has no opening boundary")]
    MissingOpeningBoundary,
    #[display("multipart body ends without a closing boundary")]
    MissingClosingBoundary,
    #[display("multipart part has no header terminator")]
    MissingHeaderTerminator,
    #[display("multipart part headers are not valid UTF-8")]
    HeadersNotText,
    #[display("multipart part has no Content-Disposition name")]
    MissingName,
}

/// The boundary parameter of a `multipart/form-data` content type.
pub fn boundary(content_type: &str) -> Result<&str, MultipartError> {
    let mut params = content_type.split(';');
    let media_type = params.next().unwrap_or_default().trim();
    if !media_type.eq_ignore_ascii_case("multipart/form-data") {
        return Err(MultipartError::NotMultipart);
    }
    params
        .filter_map(|param| param.trim().split_once('='))
        .find(|(key, _)| key.trim().eq_ignore_ascii_case("boundary"))
        .map(|(_, value)| value.trim().trim_matches('"'))
        .filter(|value| !value.is_empty())
        .ok_or(MultipartError::MissingBoundary)
}

/// Split a `multipart/form-data` body (RFC 7578) into its parts.
pub fn parse_form(content_type: &str, body: &[u8]) -> Result<Vec<FormPart>, MultipartError> {
    let boundary = boundary(content_type)?;
    let delimiter = format!("--{boundary}");
    let delimiter = delimiter.as_bytes();
    // The first delimiter may start the body or follow a preamble line.
    let mut cursor =
        find(body, delimiter).ok_or(MultipartError::MissingOpeningBoundary)? + delimiter.len();
    let mut parts = Vec::new();
    loop {
        let rest = &body[cursor..];
        if rest.starts_with(b"--") {
            return Ok(parts);
        }
        let rest = rest
            .strip_prefix(b"\r\n")
            .filter(|rest| !rest.is_empty())
            .ok_or(MultipartError::MissingClosingBoundary)?;
        let header_end = find(rest, b"\r\n\r\n").ok_or(MultipartError::MissingHeaderTerminator)?;
        let headers =
            std::str::from_utf8(&rest[..header_end]).map_err(|_| MultipartError::HeadersNotText)?;
        let (name, filename) = content_disposition(headers)?;
        let data_start = header_end + 4;
        let data = &rest[data_start..];
        let mut next_delimiter = b"\r\n".to_vec();
        next_delimiter.extend_from_slice(delimiter);
        let data_end = find(data, &next_delimiter).ok_or(MultipartError::MissingClosingBoundary)?;
        parts.push(FormPart { name, filename, data: data[..data_end].to_vec() });
        cursor = body.len() - rest.len() + data_start + data_end + next_delimiter.len();
    }
}

fn content_disposition(headers: &str) -> Result<(String, Option<String>), MultipartError> {
    let disposition = headers
        .split("\r\n")
        .filter_map(|line| line.split_once(':'))
        .find(|(header, _)| header.trim().eq_ignore_ascii_case("Content-Disposition"))
        .map(|(_, value)| value)
        .ok_or(MultipartError::MissingName)?;
    let mut name = None;
    let mut filename = None;
    for param in disposition.split(';').skip(1) {
        let Some((key, value)) = param.trim().split_once('=') else { continue };
        let value = value.trim().trim_matches('"').to_string();
        match key.trim() {
            "name" => name = Some(value),
            "filename" => filename = Some(value),
            _ => {}
        }
    }
    Ok((name.ok_or(MultipartError::MissingName)?, filename))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|window| window == needle)
}

#[cfg(test)]
pub(crate) mod tests;
