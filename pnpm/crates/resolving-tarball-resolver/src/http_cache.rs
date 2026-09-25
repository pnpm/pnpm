//! Persistent URL → integrity record for remote tarball dependencies.

use std::{
    fmt::Write,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use sha2::{Digest, Sha256};

static WRITE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) const TARBALL_RESOLUTION_CACHE_DIR: &str = "v11/tarball-resolutions";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TarballResolutionRecord {
    pub url: String,
    pub tarball: String,
    pub integrity: String,
    pub etag: Option<String>,
    pub cache_control: Option<String>,
    pub fetched_at: u64,
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
        let age_ms = now_ms.saturating_sub(self.fetched_at);
        if age_ms < max_age.saturating_mul(1000) { Freshness::Fresh } else { Freshness::Revalidate }
    }

    pub(crate) fn renewed(
        &self,
        cache_control: Option<String>,
        etag: Option<String>,
        now_ms: u64,
    ) -> Self {
        Self {
            url: self.url.clone(),
            tarball: self.tarball.clone(),
            integrity: self.integrity.clone(),
            etag: etag.or_else(|| self.etag.clone()),
            cache_control: cache_control.or_else(|| self.cache_control.clone()),
            fetched_at: now_ms,
        }
    }
}

impl CacheControl {
    pub(crate) fn parse(header: &str) -> Self {
        let mut parsed = Self { no_store: false, no_cache: false, immutable: false, max_age: None };
        for directive in header.split(',') {
            let directive = directive.trim();
            if directive.is_empty() {
                continue;
            }
            let (name, value) = directive
                .split_once('=')
                .map_or((directive, None), |(name, value)| (name.trim(), Some(value.trim())));
            match name {
                "no-store" => parsed.no_store = true,
                "no-cache" => parsed.no_cache = true,
                "immutable" => parsed.immutable = true,
                "max-age" => {
                    parsed.max_age = value.and_then(|value| value.trim_matches('"').parse().ok());
                }
                _ => {}
            }
        }
        parsed
    }
}

pub(crate) fn should_store(cache_control: Option<&str>, etag: Option<&str>) -> bool {
    let directives = CacheControl::parse(cache_control.unwrap_or(""));
    if directives.no_store {
        return false;
    }
    directives.max_age.is_some() || etag.is_some()
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

pub(crate) fn load(cache_dir: &Path, url: &str) -> Option<TarballResolutionRecord> {
    let path = record_path(cache_dir, url);
    let text = fs::read_to_string(path).ok()?;
    parse_record(&text).filter(|record| record.url == url)
}

pub(crate) fn store(cache_dir: &Path, record: &TarballResolutionRecord) {
    if !should_store(record.cache_control.as_deref(), record.etag.as_deref()) {
        remove(cache_dir, &record.url);
        return;
    }
    let Some(path) = record_path_checked(cache_dir, &record.url) else { return };
    let Some(parent) = path.parent() else { return };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let body = serde_json::json!({
        "url": record.url,
        "tarball": record.tarball,
        "integrity": record.integrity,
        "etag": record.etag,
        "cacheControl": record.cache_control,
        "fetchedAt": record.fetched_at,
    });
    let Ok(body) = serde_json::to_vec(&body) else { return };
    let counter = WRITE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_extension(format!("json.tmp.{}.{counter}", std::process::id()));
    if fs::write(&tmp, body).is_ok() {
        if fs::rename(&tmp, path).is_err() {
            let _ = fs::remove_file(&tmp);
        }
    } else {
        let _ = fs::remove_file(&tmp);
    }
}

pub(crate) fn remove(cache_dir: &Path, url: &str) {
    if let Some(path) = record_path_checked(cache_dir, url) {
        let _ = fs::remove_file(path);
    }
}

fn record_path(cache_dir: &Path, url: &str) -> PathBuf {
    cache_dir
        .join(TARBALL_RESOLUTION_CACHE_DIR)
        .join(format!("{}.json", url_digest(url)))
}

fn record_path_checked(cache_dir: &Path, url: &str) -> Option<PathBuf> {
    let path = record_path(cache_dir, url);
    path.parent().is_some().then_some(path)
}

fn url_digest(url: &str) -> String {
    let digest = Sha256::digest(url.as_bytes());
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn parse_record(text: &str) -> Option<TarballResolutionRecord> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    Some(TarballResolutionRecord {
        url: value.get("url")?.as_str()?.to_owned(),
        tarball: value
            .get("tarball")?
            .as_str()?
            .to_owned(),
        integrity: value
            .get("integrity")?
            .as_str()?
            .to_owned(),
        etag: value
            .get("etag")
            .and_then(|etag| etag.as_str())
            .map(str::to_owned),
        cache_control: value
            .get("cacheControl")
            .and_then(|header| header.as_str())
            .map(str::to_owned),
        fetched_at: value.get("fetchedAt")?.as_u64()?,
    })
}

#[cfg(test)]
mod tests;
