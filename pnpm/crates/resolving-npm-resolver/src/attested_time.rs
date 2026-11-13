//! Publish times read from the attestation endpoint, kept on disk.
//!
//! An attestation answers for one version, so no packument mirror records it.
//! Without this record an offline verification could not repeat a check the
//! attestation endpoint settled online.
//!
//! One file per `(registry, name)` under
//! [`ATTESTED_TIME_DIR`](crate::mirror::ATTESTED_TIME_DIR), one JSON
//! line per version. A record is appended whole, so concurrent installs
//! never rewrite each other's records. It is written between two newlines,
//! so a record that an interrupted write left unfinished costs only itself.
//! A line that does not parse is skipped.

use crate::lookup_context::PublishedAtTimeMap;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
};

#[derive(Serialize, Deserialize)]
struct AttestedTime {
    version: String,
    #[serde(rename = "publishedAt")]
    published_at: String,
}

/// The attested publish times recorded in `path`, keyed by version. Empty
/// when the file is missing or unreadable.
pub(crate) fn load_attested_times(path: &Path) -> PublishedAtTimeMap {
    let Ok(contents) = fs::read_to_string(path) else {
        return PublishedAtTimeMap::new();
    };
    contents
        .lines()
        .filter_map(|line| serde_json::from_str::<AttestedTime>(line).ok())
        .map(|record| (record.version, record.published_at))
        .collect()
}

pub(crate) fn append_attested_time(
    path: &Path,
    version: &str,
    published_at: &str,
) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let record =
        AttestedTime { version: version.to_string(), published_at: published_at.to_string() };
    let line = format!("\n{}\n", serde_json::to_string(&record)?);
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(line.as_bytes())
}

#[cfg(test)]
mod tests;
