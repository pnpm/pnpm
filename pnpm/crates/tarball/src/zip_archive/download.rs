use super::{NetworkError, Reporter, TarballError, allocate_tarball_buffer};

pub(super) async fn download_zip_body<Reporter: self::Reporter>(
    response_head: reqwest::Response,
    package_url: &str,
    package_id: &str,
    max_bytes: Option<usize>,
) -> Result<Vec<u8>, TarballError> {
    use futures_util::StreamExt;
    let expected_size = response_head.content_length();
    check_zip_size(expected_size.unwrap_or(0), max_bytes, package_url)?;
    let mut buf = allocate_tarball_buffer(expected_size, package_url)?;
    let mut stream = response_head.bytes_stream();
    let mut progress = crate::download::BodyProgress::new(expected_size, package_id);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| {
            TarballError::FetchTarball(NetworkError::new(package_url, error))
        })?;
        check_zip_size(
            (buf.len() as u64).saturating_add(chunk.len() as u64),
            max_bytes,
            package_url,
        )?;
        buf.extend_from_slice(&chunk);
        progress.on_chunk::<Reporter>(chunk.len());
    }
    progress.finish::<Reporter>();
    Ok(buf)
}

pub(crate) fn check_zip_size(
    size: u64,
    max_bytes: Option<usize>,
    url: &str,
) -> Result<(), TarballError> {
    if max_bytes.is_some_and(|limit| size > limit as u64) {
        return Err(TarballError::TarballTooLarge { url: url.to_string(), advertised_size: size });
    }
    Ok(())
}
