use super::{RecordIdentity, RecordKey};
use crate::{digest::RecordDigest, purl::Purl};
use derive_more::{Display, Error};
use std::collections::{HashMap, hash_map::Entry};

/// The first line of every log entry.
pub const LOG_ENTRY_SCHEMA: &str = "pnpr-log-entry/1";

/// A transparency log entry: a record's key and digest, one field per line.
///
/// ```text
/// pnpr-log-entry/1
/// purl pkg:npm/%40example/widgets@1.2.0
/// revision 0
/// profile npm-install-manifest/1
/// digest sha512-<base64>
/// ```
///
/// An OCI tag record has `repository` and `tag` lines in place of `purl`.
/// A record without a revision has no `revision` line. The log holds
/// identities and digests, never records.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LogEntry {
    key: RecordKey,
    digest: RecordDigest,
}

/// Why a log entry could not be built or read.
#[derive(Debug, Display, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LogEntryError {
    #[display("log entry field {field} is empty or has whitespace or control characters")]
    InvalidField {
        #[error(not(source))]
        field: &'static str,
    },
    #[display("an OCI tag log entry needs a revision")]
    MissingRevision,
    #[display("malformed log entry")]
    Malformed,
}

impl LogEntry {
    /// The entry that commits to `digest` for the record `key`.
    pub fn new(key: RecordKey, digest: RecordDigest) -> Result<Self, LogEntryError> {
        match &key.identity {
            RecordIdentity::Package(purl) => check_field("purl", purl.as_str())?,
            RecordIdentity::OciTag { repository, tag } => {
                check_field("repository", repository)?;
                check_field("tag", tag)?;
                key.revision.ok_or(LogEntryError::MissingRevision)?;
            }
        }
        Ok(LogEntry { key, digest })
    }

    /// The record's key.
    #[must_use]
    pub const fn key(&self) -> &RecordKey {
        &self.key
    }

    /// The record's digest.
    #[must_use]
    pub const fn digest(&self) -> &RecordDigest {
        &self.digest
    }

    /// The entry's bytes, as the log stores and hashes them.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let identity = match &self.key.identity {
            RecordIdentity::Package(purl) => format!("purl {purl}\n"),
            RecordIdentity::OciTag { repository, tag } => {
                format!("repository {repository}\ntag {tag}\n")
            }
        };
        let revision = self.key.revision
            .map(|revision| format!("revision {revision}\n"))
            .unwrap_or_default();
        let (profile, digest) = (&self.key.profile, &self.digest);
        format!("{LOG_ENTRY_SCHEMA}\n{identity}{revision}profile {profile}\ndigest {digest}\n")
            .into_bytes()
    }

    /// Reads an entry, accepting only the exact bytes [`LogEntry::to_bytes`]
    /// writes for it.
    pub fn parse(bytes: &[u8]) -> Result<Self, LogEntryError> {
        let text = std::str::from_utf8(bytes).map_err(|_| LogEntryError::Malformed)?;
        let entry = parse_fields(text).ok_or(LogEntryError::Malformed)??;
        if entry.to_bytes() != bytes {
            return Err(LogEntryError::Malformed);
        }
        Ok(entry)
    }
}

fn parse_fields(text: &str) -> Option<Result<LogEntry, LogEntryError>> {
    let mut lines = text.strip_suffix('\n')?.split('\n');
    if lines.next()? != LOG_ENTRY_SCHEMA {
        return None;
    }
    let fields: Vec<(&str, &str)> = lines
        .map(|line| line.split_once(' '))
        .collect::<Option<_>>()?;
    let (identity, rest) = match fields.as_slice() {
        [("purl", purl), rest @ ..] => {
            (RecordIdentity::Package(Purl::from_string_unchecked((*purl).to_owned())), rest)
        }
        [("repository", repository), ("tag", tag), rest @ ..] => {
            let (repository, tag) = ((*repository).to_owned(), (*tag).to_owned());
            (RecordIdentity::OciTag { repository, tag }, rest)
        }
        _ => return None,
    };
    let (revision, profile, digest) = match rest {
        [("revision", revision), ("profile", profile), ("digest", digest)] => {
            (Some(revision.parse().ok()?), profile, digest)
        }
        [("profile", profile), ("digest", digest)] => (None, profile, digest),
        _ => return None,
    };
    let key = RecordKey { identity, revision, profile: profile.parse().ok()? };
    Some(LogEntry::new(key, digest.parse().ok()?))
}

fn check_field(field: &'static str, value: &str) -> Result<(), LogEntryError> {
    let is_clean = |character: char| !character.is_whitespace() && !character.is_control();
    if value.is_empty() || !value.chars().all(is_clean) {
        return Err(LogEntryError::InvalidField { field });
    }
    Ok(())
}

/// Two log entries for the same record key with different digests: a
/// forged or rewritten record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// The record key logged twice.
    pub key: RecordKey,
    /// The index and digest of the first entry.
    pub first: (u64, RecordDigest),
    /// The index and digest of the conflicting entry.
    pub second: (u64, RecordDigest),
}

/// Reads a log's entries in order and reports every record key logged with
/// two digests, as a monitor of the whole witnessed log does.
///
/// An inclusion proof shows that an entry exists, not that no other entry
/// for the same record does, so only a reader of every entry catches a
/// conflict. An entry repeated with the same digest is not a conflict.
#[derive(Debug, Default)]
pub struct ConflictMonitor {
    seen: HashMap<RecordKey, (u64, RecordDigest)>,
}

impl ConflictMonitor {
    /// Records the entry at `index`, returning the conflict it creates, if
    /// any.
    pub fn observe(&mut self, index: u64, entry: &LogEntry) -> Option<Conflict> {
        match self.seen.entry(entry.key.clone()) {
            Entry::Vacant(vacant) => {
                vacant.insert((index, entry.digest));
                None
            }
            Entry::Occupied(occupied) if occupied.get().1 == entry.digest => None,
            Entry::Occupied(occupied) => Some(Conflict {
                key: entry.key.clone(),
                first: *occupied.get(),
                second: (index, entry.digest),
            }),
        }
    }
}
