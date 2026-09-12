use super::{
    API_VERSION_HEADER, Body, CHALLENGE, DOCKER_CONTENT_DIGEST, DOCKER_UPLOAD_UUID, Digest,
    ErrorBody, ErrorCode, HeaderValue, ManifestEntry, RegistryError, Response, Serialize,
    StatusCode, header,
};
use axum::response::IntoResponse;

pub(super) fn insert_header(response: &mut Response, name: &'static str, value: &str) {
    let value = HeaderValue::from_str(value).expect("generated OCI response header is valid");
    response.headers_mut().insert(name, value);
}

/// Render a ranged blob read as its response.
pub(super) fn ranged_blob_response(
    blob: pnpr_storage::RangedBlob,
    digest: &Digest,
    etag: &str,
) -> Response {
    match blob {
        pnpr_storage::RangedBlob::Read { body, range, size } => Response::builder()
            .status(StatusCode::PARTIAL_CONTENT)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(header::CONTENT_LENGTH, range.end - range.start)
            .header(
                header::CONTENT_RANGE,
                format!("bytes {}-{}/{size}", range.start, range.end - 1),
            )
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::ETAG, etag)
            .header(DOCKER_CONTENT_DIGEST, digest.to_string())
            .body(body)
            .unwrap_or_else(|_| server_error()),
        pnpr_storage::RangedBlob::Unsatisfiable { size } => {
            let mut response = respond(
                StatusCode::RANGE_NOT_SATISFIABLE,
                ErrorCode::SizeInvalid,
                "range is outside the blob",
            );
            insert_header(&mut response, "content-range", &format!("bytes */{size}"));
            response
        }
    }
}

pub(in super::super) struct Refusal {
    pub(super) status: StatusCode,
    pub(super) code: ErrorCode,
    pub(super) message: String,
    pub(super) original: Option<Box<RegistryError>>,
}

impl Refusal {
    pub(super) fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { status: status_for(code), code, message: message.into(), original: None }
    }

    pub(in super::super) fn respond(self) -> Response {
        let status = self.original.map_or(self.status, |err| err.into_response().status());
        respond(status, self.code, self.message)
    }
}

/// A pnpr error in the distribution spec's own vocabulary, keeping the status
/// the error chose.
impl From<RegistryError> for Refusal {
    fn from(err: RegistryError) -> Self {
        let upload_conflict = matches!(err, RegistryError::BlobUploadConflict { .. });
        let message = err.public_message();
        let status = err.status_code();
        let code = match status {
            StatusCode::CONFLICT if upload_conflict => ErrorCode::BlobUploadInvalid,
            StatusCode::UNAUTHORIZED => ErrorCode::Unauthorized,
            StatusCode::FORBIDDEN => ErrorCode::Denied,
            StatusCode::NOT_FOUND => ErrorCode::NameUnknown,
            StatusCode::TOO_MANY_REQUESTS => ErrorCode::TooManyRequests,
            // No spec code means "the registry refused this"; the status is
            // what a client acts on.
            _ => ErrorCode::Unsupported,
        };
        Self { status, code, message, original: Some(Box::new(err)) }
    }
}

pub(super) fn registry_error(err: RegistryError) -> Response {
    Refusal::from(err).respond()
}

pub(in super::super) fn error(code: ErrorCode, message: impl Into<String>) -> Response {
    Refusal::new(code, message).respond()
}

pub(super) fn status_for(code: ErrorCode) -> StatusCode {
    match code {
        ErrorCode::Unauthorized => StatusCode::UNAUTHORIZED,
        ErrorCode::Denied => StatusCode::FORBIDDEN,
        ErrorCode::NameUnknown
        | ErrorCode::ManifestUnknown
        | ErrorCode::BlobUnknown
        | ErrorCode::BlobUploadUnknown => StatusCode::NOT_FOUND,
        ErrorCode::Unsupported => StatusCode::METHOD_NOT_ALLOWED,
        ErrorCode::TooManyRequests => StatusCode::TOO_MANY_REQUESTS,
        ErrorCode::BlobUploadInvalid
        | ErrorCode::DigestInvalid
        | ErrorCode::ManifestBlobUnknown
        | ErrorCode::ManifestInvalid
        | ErrorCode::NameInvalid
        | ErrorCode::SizeInvalid => StatusCode::BAD_REQUEST,
    }
}

/// Every failing response carries the spec's error body, and a 401 carries
/// the challenge as well: without it a Docker client treats the refusal as
/// final instead of retrying with its stored credentials.
pub(super) fn respond(status: StatusCode, code: ErrorCode, message: impl Into<String>) -> Response {
    let mut response = json(status, &ErrorBody::new(code, message));
    if status == StatusCode::UNAUTHORIZED {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static(CHALLENGE));
    }
    api_version(response)
}

pub(super) fn method_not_allowed() -> Response {
    error(ErrorCode::Unsupported, "unsupported method for this endpoint")
}

pub(super) fn unknown_repository(name: &str) -> Refusal {
    Refusal::new(ErrorCode::NameUnknown, format!("no repository named {name:?} is served here"))
}

pub(super) fn api_version(mut response: Response) -> Response {
    response.headers_mut().insert(API_VERSION_HEADER, HeaderValue::from_static("registry/2.0"));
    response
}

pub(super) fn json<Payload: Serialize>(status: StatusCode, payload: &Payload) -> Response {
    let bytes = serde_json::to_vec(payload).unwrap_or_else(|_| b"{}".to_vec());
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| server_error())
}

pub(super) fn server_error() -> Response {
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

pub(super) fn no_content(status: StatusCode) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_LENGTH, 0)
        .body(Body::empty())
        .unwrap_or_else(|_| server_error())
}

/// `202 Accepted` with where the upload continues and how much of it landed.
pub(super) fn accepted(base: &str, name: &str, id: &str, offset: u64) -> Response {
    // `Range` here is the inclusive span already stored, and an empty upload
    // has none, which the spec spells `0-0`.
    let range = if offset == 0 { "0-0".to_string() } else { format!("0-{}", offset - 1) };
    Response::builder()
        .status(StatusCode::ACCEPTED)
        .header(header::LOCATION, format!("{base}/{name}/blobs/uploads/{id}"))
        .header(header::RANGE, range)
        .header(DOCKER_UPLOAD_UUID, id)
        .header(header::CONTENT_LENGTH, 0)
        .body(Body::empty())
        .unwrap_or_else(|_| server_error())
}

pub(super) fn range_not_satisfiable(base: &str, name: &str, id: &str, offset: u64) -> Response {
    let mut response = accepted(base, name, id, offset);
    *response.status_mut() = StatusCode::RANGE_NOT_SATISFIABLE;
    response
}

pub(super) fn created(location: &str, digest: &Digest) -> Response {
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::LOCATION, location)
        .header(DOCKER_CONTENT_DIGEST, digest.to_string())
        .header(header::CONTENT_LENGTH, 0)
        .body(Body::empty())
        .unwrap_or_else(|_| server_error())
}

/// HEAD advertises the manifest's full length while sending no body.
pub(super) fn hosted_manifest_response(
    bytes: Vec<u8>,
    entry: ManifestEntry,
    head: bool,
) -> Response {
    let length = bytes.len();
    let body = if head { Body::empty() } else { Body::from(bytes) };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, entry.media_type)
        .header(header::CONTENT_LENGTH, length)
        .header(DOCKER_CONTENT_DIGEST, entry.digest.to_string())
        .body(body)
        .unwrap_or_else(|_| server_error())
}
