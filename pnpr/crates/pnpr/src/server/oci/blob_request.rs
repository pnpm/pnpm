use super::{
    Body, DOCKER_CONTENT_DIGEST, Digest, ErrorCode, Method, Request, Response, StatusCode, error,
    header, method_not_allowed, ranged_blob_response, registry_error, server_error,
};
impl Request {
    fn blob_response(
        &self,
        body: Body,
        size: Option<u64>,
        digest: &Digest,
        etag: &str,
    ) -> Response {
        let mut response = Response::builder()
            .status(StatusCode::OK)
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::ETAG, etag)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(DOCKER_CONTENT_DIGEST, digest.to_string());
        if let Some(size) = size {
            response = response.header(header::CONTENT_LENGTH, size);
        }
        let body = if self.method == Method::HEAD {
            Body::empty()
        } else {
            body
        };
        response
            .body(body)
            .unwrap_or_else(|_| server_error())
    }

    pub(super) async fn blob(&self, name: &str, digest: &str) -> Response {
        let Ok(digest) = Digest::parse(digest) else {
            return error(ErrorCode::DigestInvalid, "not a supported digest");
        };
        match self.method {
            Method::GET | Method::HEAD => self.read_blob(name, &digest).await,
            Method::DELETE => self.delete_blob(name, &digest).await,
            _ => method_not_allowed(),
        }
    }

    async fn read_blob(&self, name: &str, digest: &Digest) -> Response {
        if let Some((key, source)) = self.upstream_source(name) {
            return self.proxy_blob(&key, &source, digest).await;
        }
        let repo = match self.hosted_repo(name) {
            Ok(repo) => repo,
            Err(refusal) => return refusal.respond(),
        };
        let etag = format!(r#""{digest}""#);
        if let Some(range) = self.requested_download_range(&etag) {
            let ranged =
                repo.storage.open_hosted_blob_range(&repo.key, &digest.blob_filename(), &range)
                    .await;
            let response = match ranged {
                Ok(Some(blob)) => ranged_blob_response(blob, digest, &etag),
                Ok(None) => return error(ErrorCode::BlobUnknown, "no such blob"),
                Err(err) => return registry_error(err),
            };
            return self.caller_scoped(Some(repo.key.as_str()), response);
        }
        let (body, size) = match repo.storage.open_hosted_blob(&repo.key, &digest.blob_filename())
            .await
        {
            Ok(Some(blob)) => blob,
            Ok(None) => return error(ErrorCode::BlobUnknown, "no such blob"),
            Err(err) => return registry_error(err),
        };
        let response = self.blob_response(body, size, digest, &etag);
        self.caller_scoped(Some(repo.key.as_str()), response)
    }

    /// The byte range a `GET` asks for, when it asks for exactly one and its
    /// `If-Range` still matches.
    pub(super) fn requested_download_range(&self, etag: &str) -> Option<pnpr_storage::GetRange> {
        if self.method != Method::GET
            || self.headers
                .get(header::IF_RANGE)
                .is_some_and(|value| value != etag)
            || self.headers
                .get_all(header::RANGE)
                .iter()
                .count()
                != 1
        {
            return None;
        }
        self.headers
            .get(header::RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_download_range)
    }
}

pub(super) fn parse_download_range(value: &str) -> Option<pnpr_storage::GetRange> {
    let (unit, bounds) = value.trim().split_once('=')?;
    if !unit.eq_ignore_ascii_case("bytes") {
        return None;
    }
    let (start, end) = bounds.split_once('-')?;
    let number = |value: &str| {
        (!value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit()))
        .then(|| value.parse::<u64>().ok())
        .flatten()
    };
    if start.is_empty() {
        return number(end).map(pnpr_storage::GetRange::Suffix);
    }
    let start = number(start)?;
    if end.is_empty() {
        return Some(pnpr_storage::GetRange::Offset(start));
    }
    let end = number(end)?;
    if start > end {
        return None;
    }
    Some(match end.checked_add(1) {
        Some(end) => pnpr_storage::GetRange::Bounded(start..end),
        None => pnpr_storage::GetRange::Offset(start),
    })
}
