use super::{FormPart, MultipartError, boundary, parse_form};

/// Encode `parts` the way `twine` (via `requests`) does: CRLF line endings,
/// a `Content-Disposition` per part, and a closing `--boundary--`.
pub(crate) fn encode_form(boundary: &str, parts: &[(&str, Option<&str>, &[u8])]) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, filename, data) in parts {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!(r#"Content-Disposition: form-data; name="{name}""#).as_bytes(),
        );
        if let Some(filename) = filename {
            body.extend_from_slice(format!(r#"; filename="{filename}""#).as_bytes());
            body.extend_from_slice(b"\r\nContent-Type: application/octet-stream");
        }
        body.extend_from_slice(b"\r\n\r\n");
        body.extend_from_slice(data);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

#[test]
fn boundary_is_read_from_the_content_type() {
    assert_eq!(boundary("multipart/form-data; boundary=abc123"), Ok("abc123"));
    assert_eq!(boundary(r#"Multipart/Form-Data; charset=utf-8; boundary="quoted""#), Ok("quoted"));
    assert_eq!(boundary("multipart/form-data"), Err(MultipartError::MissingBoundary));
    assert_eq!(boundary("application/json"), Err(MultipartError::NotMultipart));
}

#[test]
fn parses_text_and_file_parts() {
    let body = encode_form(
        "xyz",
        &[
            (":action", None, b"file_upload"),
            ("name", None, b"demo"),
            (
                "content",
                Some("demo-1.0.0-py3-none-any.whl"),
                b"PK\x03\x04binary\r\n--not-a-boundary",
            ),
        ],
    );
    let parts = parse_form("multipart/form-data; boundary=xyz", &body).unwrap();
    assert_eq!(
        parts,
        vec![
            FormPart { name: ":action".into(), filename: None, data: b"file_upload".to_vec() },
            FormPart { name: "name".into(), filename: None, data: b"demo".to_vec() },
            FormPart {
                name: "content".into(),
                filename: Some("demo-1.0.0-py3-none-any.whl".into()),
                data: b"PK\x03\x04binary\r\n--not-a-boundary".to_vec(),
            },
        ],
    );
}

#[test]
fn accepts_a_preamble_and_empty_parts() {
    let mut body = b"preamble text\r\n".to_vec();
    body.extend_from_slice(&encode_form("b", &[("empty", None, b""), ("k", None, b"v")]));
    let parts = parse_form("multipart/form-data; boundary=b", &body).unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].data, b"");
    assert_eq!(parts[1].data, b"v");
}

#[test]
fn rejects_malformed_bodies() {
    let content_type = "multipart/form-data; boundary=b";
    assert_eq!(
        parse_form(content_type, b"no boundary here"),
        Err(MultipartError::MissingOpeningBoundary),
    );
    assert_eq!(
        parse_form(content_type, b"--b\r\nContent-Disposition: form-data; name=\"x\"\r\n\r\ndata"),
        Err(MultipartError::MissingClosingBoundary),
    );
    assert_eq!(
        parse_form(
            content_type,
            b"--b\r\nContent-Disposition: form-data; name=\"x\"\r\ndata\r\n--b--"
        ),
        Err(MultipartError::MissingHeaderTerminator),
    );
    assert_eq!(
        parse_form(content_type, b"--b\r\nContent-Type: text/plain\r\n\r\ndata\r\n--b--"),
        Err(MultipartError::MissingName),
    );
    assert_eq!(
        parse_form(
            content_type,
            b"--b\r\nContent-Disposition: form-data; name=\"x\"\r\n\r\ndata\r\n--b\r\n"
        ),
        Err(MultipartError::MissingClosingBoundary),
    );
}

#[test]
fn preserves_boundary_prefixes_in_binary_data() {
    let data = b"--xyz\r\nstart\0\xff\r\n--xyzordinary\r\n--xyz-\r\nend";
    let body = encode_form("xyz", &[("content", Some("demo.whl"), data)]);
    let parts = parse_form("multipart/form-data; boundary=xyz", &body).unwrap();
    assert_eq!(parts[0].data, data);
}

#[test]
fn rejects_invalid_boundaries_before_scanning() {
    for boundary in
        ["a".repeat(71), "bad\r\nboundary".to_string(), "trailing ".to_string(), "é".to_string()]
    {
        assert_eq!(
            parse_form(&format!(r#"multipart/form-data; boundary="{boundary}""#), b""),
            Err(MultipartError::InvalidBoundary),
        );
    }
    assert!(boundary(&format!("multipart/form-data; boundary={}", "a".repeat(70))).is_ok());
}
