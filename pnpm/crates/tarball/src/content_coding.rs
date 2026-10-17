//! HTTP content coding of archive responses.
//!
//! Archive requests go through clients that leave `Content-Encoding` in
//! place, so the fetcher decodes the body itself and can bound what each
//! decoder holds in memory.

use crate::TarballError;
use std::io::{self, Read};

/// The `Accept-Encoding` an archive request sends.
pub(crate) const ARCHIVE_ACCEPT_ENCODING: &str = "zstd, gzip";

/// The largest zstd window a response may use, as `log2` of its size: 8 MiB,
/// the limit RFC 9659 sets for `Content-Encoding: zstd`. It bounds the memory
/// each decoder holds across the extractions an install runs in parallel.
#[cfg(not(target_family = "wasm"))]
const ZSTD_WINDOW_LOG_MAX: u32 = 23;

/// The content coding of an archive response body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContentCoding {
    Identity,
    Gzip,
    Zstd,
}

impl ContentCoding {
    /// The coding `response` declares. A coding pnpm did not offer, or a
    /// chain of several, fails the fetch.
    ///
    /// Browser networking decodes the body before pnpm sees it, so on wasm
    /// every body is read as it arrives.
    pub(crate) fn of_response(
        response: &reqwest::Response,
        package_url: &str,
    ) -> Result<Self, TarballError> {
        if cfg!(target_family = "wasm") {
            return Ok(Self::Identity);
        }
        let Some(value) = response.headers().get(reqwest::header::CONTENT_ENCODING) else {
            return Ok(Self::Identity);
        };
        let unsupported = || TarballError::UnsupportedContentEncoding {
            url: package_url.to_string(),
            encoding: String::from_utf8_lossy(value.as_bytes()).into_owned(),
        };
        let token = value
            .to_str()
            .map_err(|_| unsupported())?
            .trim();
        match token.to_ascii_lowercase().as_str() {
            "" | "identity" => Ok(Self::Identity),
            "gzip" | "x-gzip" => Ok(Self::Gzip),
            "zstd" => Ok(Self::Zstd),
            _ => Err(unsupported()),
        }
    }

    /// `body` with this coding removed.
    pub(crate) fn decode<'body>(
        self,
        body: impl Read + 'body,
    ) -> io::Result<Box<dyn Read + 'body>> {
        Ok(match self {
            Self::Identity => Box::new(body),
            Self::Gzip => Box::new(flate2::read::MultiGzDecoder::new(body)),
            #[cfg(not(target_family = "wasm"))]
            Self::Zstd => {
                let mut decoder = zstd::stream::read::Decoder::new(body)?;
                decoder.window_log_max(ZSTD_WINDOW_LOG_MAX)?;
                Box::new(decoder)
            }
            #[cfg(target_family = "wasm")]
            Self::Zstd => unreachable!("wasm bodies arrive decoded"),
        })
    }
}
