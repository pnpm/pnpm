//! Persistent URL → integrity record for remote tarball dependencies.
//!
//! A lockfile-less install has nothing else that maps a tarball URL to the
//! archive already in the store. The record keeps the response's caching
//! headers, so resolution skips the network while `Cache-Control` says the
//! response is fresh and revalidates it with `If-None-Match` afterwards.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use pnpm_tarball::CacheHeaders;

pub(crate) const TARBALL_RESOLUTION_CACHE_DIR: &str = "v11/tarball-resolutions";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TarballResolutionRecord {
    pub url: String,
    pub tarball: String,
    /// The URL that served the archive, after redirects. A `304` from a
    /// different URL does not vouch for this archive.
    pub final_url: String,
    pub integrity: String,
    pub etag: Option<String>,
    pub cache_control: Option<String>,
    /// When this cache received the response, in milliseconds since the
    /// Unix epoch.
    pub fetched_at: u64,
    /// How old the response already was when it arrived, from its `Age`
    /// and `Date` headers.
    pub initial_age_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Freshness {
    /// `Cache-Control` says the stored response is still fresh.
    Fresh,
    /// A stored response exists and must be revalidated before reuse.
    Revalidate,
    /// `no-store`, or a record that must not be served.
    Unusable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CacheControl {
    pub(crate) no_store: bool,
    pub(crate) no_cache: bool,
    pub(crate) immutable: bool,
    pub(crate) max_age: Option<u64>,
}

impl TarballResolutionRecord {
    /// Build a record for a response received at the given time.
    pub(crate) fn from_response(
        url: String,
        tarball: String,
        final_url: String,
        integrity: String,
        headers: &CacheHeaders,
        now_ms: u64,
    ) -> Self {
        Self {
            url,
            tarball,
            final_url,
            integrity,
            etag: headers.etag.clone(),
            cache_control: headers.cache_control.clone(),
            fetched_at: now_ms,
            initial_age_ms: initial_age_ms(headers, now_ms),
        }
    }

    /// Freshness per RFC 9111: the response's current age, which counts the
    /// age it arrived with, is compared with `max-age`.
    pub(crate) fn freshness(&self, now_ms: u64) -> Freshness {
        let directives = CacheControl::parse(self.cache_control.as_deref().unwrap_or(""));
        if directives.no_store {
            return Freshness::Unusable;
        }
        if directives.no_cache {
            return Freshness::Revalidate;
        }
        let Some(max_age) = directives.max_age else {
            return Freshness::Revalidate;
        };
        let age_ms = self.initial_age_ms.saturating_add(now_ms.saturating_sub(self.fetched_at));
        if age_ms < max_age.saturating_mul(1000) { Freshness::Fresh } else { Freshness::Revalidate }
    }

    /// The record after a `304 Not Modified` received at the given time. Headers
    /// the `304` carries replace the stored ones.
    pub(crate) fn renewed(&self, headers: &CacheHeaders, now_ms: u64) -> Self {
        Self {
            url: self.url.clone(),
            tarball: self.tarball.clone(),
            final_url: self.final_url.clone(),
            integrity: self.integrity.clone(),
            etag: headers.etag.clone().or_else(|| self.etag.clone()),
            cache_control: headers.cache_control
                .clone()
                .or_else(|| self.cache_control.clone()),
            fetched_at: now_ms,
            initial_age_ms: initial_age_ms(headers, now_ms),
        }
    }
}

impl CacheControl {
    /// Directive names are case-insensitive. A `max-age` that is not a
    /// non-negative integer is ignored, so the response is revalidated.
    pub(crate) fn parse(header: &str) -> Self {
        let mut parsed = Self { no_store: false, no_cache: false, immutable: false, max_age: None };
        for directive in header.split(',') {
            let (name, value) = directive
                .split_once('=')
                .map_or((directive, None), |(name, value)| (name, Some(value)));
            match name
                .trim()
                .to_ascii_lowercase()
                .as_str()
            {
                "no-store" => parsed.no_store = true,
                "no-cache" => parsed.no_cache = true,
                "immutable" => parsed.immutable = true,
                "max-age" => parsed.max_age = value.and_then(parse_delta_seconds),
                _ => {}
            }
        }
        parsed
    }
}

fn parse_delta_seconds(value: &str) -> Option<u64> {
    let value = value.trim();
    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value);
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

/// The larger of the `Age` header and the apparent age from `Date`.
fn initial_age_ms(headers: &CacheHeaders, now_ms: u64) -> u64 {
    let age_ms = headers.age
        .as_deref()
        .and_then(parse_delta_seconds)
        .map_or(0, |seconds| seconds.saturating_mul(1000));
    let apparent_ms = headers.date
        .as_deref()
        .and_then(|date| httpdate::parse_http_date(date).ok())
        .and_then(|date| date.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |date| now_ms.saturating_sub(duration_ms(date)));
    age_ms.max(apparent_ms)
}

pub(crate) fn should_store(cache_control: Option<&str>, etag: Option<&str>) -> bool {
    let directives = CacheControl::parse(cache_control.unwrap_or(""));
    if directives.no_store {
        return false;
    }
    directives.max_age.is_some() || etag.is_some()
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, duration_ms)
}

fn duration_ms(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

pub(crate) fn load(cache_dir: &Path, url: &str) -> Option<TarballResolutionRecord> {
    let text = fs::read_to_string(record_path(cache_dir, url)).ok()?;
    parse_record(&text).filter(|record| record.url == url)
}

/// Persist `record`, or drop any stored record for its URL when the
/// response must not be cached. The cache is an optimization, so a write
/// failure only costs a request on the next install.
pub(crate) fn store(cache_dir: &Path, record: &TarballResolutionRecord) {
    if !should_store(record.cache_control.as_deref(), record.etag.as_deref()) {
        remove(cache_dir, &record.url);
        return;
    }
    let body = serde_json::json!({
        "url": record.url,
        "tarball": record.tarball,
        "finalUrl": record.final_url,
        "integrity": record.integrity,
        "etag": record.etag,
        "cacheControl": record.cache_control,
        "fetchedAt": record.fetched_at,
        "initialAgeMs": record.initial_age_ms,
    });
    let _ =
        pnpm_fs::write_atomic(&record_path(cache_dir, &record.url), body.to_string().as_bytes());
}

pub(crate) fn remove(cache_dir: &Path, url: &str) {
    let _ = fs::remove_file(record_path(cache_dir, url));
}

fn record_path(cache_dir: &Path, url: &str) -> PathBuf {
    cache_dir
        .join(TARBALL_RESOLUTION_CACHE_DIR)
        .join(format!("{}.json", pnpm_crypto_hash::create_hex_hash(url)))
}

fn parse_record(text: &str) -> Option<TarballResolutionRecord> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let string = |key: &str| {
        value
            .get(key)?
            .as_str()
            .map(str::to_owned)
    };
    Some(TarballResolutionRecord {
        url: string("url")?,
        tarball: string("tarball")?,
        final_url: string("finalUrl")?,
        integrity: string("integrity")?,
        etag: string("etag"),
        cache_control: string("cacheControl"),
        fetched_at: value.get("fetchedAt")?.as_u64()?,
        initial_age_ms: value.get("initialAgeMs")?.as_u64()?,
    })
}

#[cfg(test)]
mod tests;
