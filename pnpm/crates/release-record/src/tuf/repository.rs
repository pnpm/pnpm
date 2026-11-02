use super::{
    TufError,
    metadata::{
        FileList, MetaFile, RoleMetadata, Root, Signed, Snapshot, Targets, Timestamp, parse,
    },
    role::{check_file, verify_role},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::fmt;

/// The versions a client trusted last, persisted between runs so that a
/// repository cannot roll metadata back.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedVersions {
    /// The version of the trusted timestamp metadata.
    pub timestamp: Option<u64>,
    /// The version of the trusted snapshot metadata.
    pub snapshot: Option<u64>,
    /// The version of the targets metadata the trusted snapshot lists.
    pub targets: Option<u64>,
}

/// The trusted state of one TUF repository, updated role by role in the
/// order the TUF client workflow prescribes: root versions, then timestamp,
/// snapshot, and targets.
pub struct TrustedRepository {
    first_root_digest: [u8; 32],
    root: Signed<Root>,
    root_bytes: Vec<u8>,
    versions: TrustedVersions,
    timestamp: Option<Timestamp>,
    snapshot: Option<Snapshot>,
    targets: Option<Targets>,
}

impl TrustedRepository {
    /// Starts from the first root version, which must have
    /// `first_root_digest` as its SHA-256, as a workspace or pnpm pins it.
    pub fn new(first_root: &[u8], first_root_digest: [u8; 32]) -> Result<Self, TufError> {
        if <[u8; 32]>::from(Sha256::digest(first_root)) != first_root_digest {
            return Err(TufError::RootDigestMismatch);
        }
        let repository =
            TrustedRepository::resume(first_root_digest, first_root, TrustedVersions::default())?;
        let version = repository.root.body.header.version;
        if version != 1 {
            return Err(TufError::RootVersion { expected: 1, actual: version });
        }
        Ok(repository)
    }

    /// Resumes from a root this client verified earlier and the versions it
    /// persisted with it.
    pub fn resume(
        first_root_digest: [u8; 32],
        trusted_root: &[u8],
        versions: TrustedVersions,
    ) -> Result<Self, TufError> {
        let root = parse::<Root>(trusted_root)?;
        verify_role(&root.body, Root::ROLE, &root)?;
        Ok(TrustedRepository {
            first_root_digest,
            root,
            root_bytes: trusted_root.to_vec(),
            versions,
            timestamp: None,
            snapshot: None,
            targets: None,
        })
    }

    /// Moves to the next root version, which must be signed by the threshold
    /// of both the trusted root and itself. A root that rotates the timestamp
    /// or snapshot keys clears the trusted versions, as TUF prescribes to
    /// recover from a fast-forward attack.
    pub fn update_root(&mut self, next_root: &[u8]) -> Result<(), TufError> {
        let next = parse::<Root>(next_root)?;
        verify_role(&self.root.body, Root::ROLE, &next)?;
        verify_role(&next.body, Root::ROLE, &next)?;
        let expected = self.root.body.header.version + 1;
        if next.body.header.version != expected {
            return Err(TufError::RootVersion { expected, actual: next.body.header.version });
        }
        let rotated =
            |role: &str| self.root.body.roles.get(role) != next.body.roles.get(role);
        if rotated(Timestamp::ROLE) || rotated(Snapshot::ROLE) {
            self.versions = TrustedVersions::default();
        }
        self.root = next;
        self.root_bytes = next_root.to_vec();
        (self.timestamp, self.snapshot, self.targets) = (None, None, None);
        Ok(())
    }

    /// Verifies timestamp metadata after the last root update. Fails if the
    /// trusted root itself expired, which stops a freeze attack. The trusted
    /// snapshot and targets must then be verified again against it.
    pub fn update_timestamp(&mut self, bytes: &[u8], now: DateTime<Utc>) -> Result<(), TufError> {
        self.root.body.header.check_expiry(Root::ROLE, now)?;
        let timestamp = parse::<Timestamp>(bytes)?;
        verify_role(&self.root.body, Timestamp::ROLE, &timestamp)?;
        let version = timestamp.body.0.header.version;
        check_rollback(Timestamp::ROLE, self.versions.timestamp, version)?;
        let snapshot = listed(&timestamp.body.0, Timestamp::ROLE, "snapshot.json")?;
        check_rollback(Snapshot::ROLE, self.versions.snapshot, snapshot.version)?;
        timestamp.body.0.header.check_expiry(Timestamp::ROLE, now)?;
        self.versions.timestamp = Some(version);
        self.timestamp = Some(timestamp.body);
        (self.snapshot, self.targets) = (None, None);
        Ok(())
    }

    /// Verifies snapshot metadata against the trusted timestamp. The trusted
    /// targets must then be verified again against it.
    pub fn update_snapshot(&mut self, bytes: &[u8], now: DateTime<Utc>) -> Result<(), TufError> {
        let timestamp = self.timestamp
            .as_ref()
            .ok_or(TufError::NotLoaded { role: Timestamp::ROLE })?;
        let listing = listed(&timestamp.0, Timestamp::ROLE, "snapshot.json")?;
        check_file("snapshot.json", bytes, listing.length, &listing.hashes, false)?;
        let snapshot = parse::<Snapshot>(bytes)?;
        verify_role(&self.root.body, Snapshot::ROLE, &snapshot)?;
        check_version(Snapshot::ROLE, listing.version, snapshot.body.0.header.version)?;
        let targets = listed(&snapshot.body.0, Snapshot::ROLE, "targets.json")?;
        check_rollback(Targets::ROLE, self.versions.targets, targets.version)?;
        snapshot.body.0.header.check_expiry(Snapshot::ROLE, now)?;
        self.versions.snapshot = Some(listing.version);
        self.versions.targets = Some(targets.version);
        self.snapshot = Some(snapshot.body);
        self.targets = None;
        Ok(())
    }

    /// Verifies top-level targets metadata against the trusted snapshot.
    pub fn update_targets(&mut self, bytes: &[u8], now: DateTime<Utc>) -> Result<(), TufError> {
        let snapshot = self.snapshot
            .as_ref()
            .ok_or(TufError::NotLoaded { role: Snapshot::ROLE })?;
        let listing = listed(&snapshot.0, Snapshot::ROLE, "targets.json")?;
        check_file("targets.json", bytes, listing.length, &listing.hashes, false)?;
        let targets = parse::<Targets>(bytes)?;
        verify_role(&self.root.body, Targets::ROLE, &targets)?;
        check_version(Targets::ROLE, listing.version, targets.body.header.version)?;
        targets.body.header.check_expiry(Targets::ROLE, now)?;
        self.targets = Some(targets.body);
        Ok(())
    }

    /// Verifies a target's bytes against the length and hashes the trusted
    /// targets metadata lists for `path`.
    pub fn verify_target(&self, path: &str, bytes: &[u8]) -> Result<(), TufError> {
        let targets = self.targets
            .as_ref()
            .ok_or(TufError::NotLoaded { role: Targets::ROLE })?;
        let target = targets.targets
            .get(path)
            .ok_or_else(|| TufError::MissingTarget { path: path.to_owned() })?;
        check_file(path, bytes, Some(target.length), &target.hashes, true)
    }

    /// The SHA-256 of the first root version this repository is pinned to.
    #[must_use]
    pub const fn first_root_digest(&self) -> &[u8; 32] {
        &self.first_root_digest
    }

    /// The bytes of the trusted root, to persist for [`TrustedRepository::resume`].
    #[must_use]
    pub fn root_bytes(&self) -> &[u8] {
        &self.root_bytes
    }

    /// The trusted root's version.
    #[must_use]
    pub fn root_version(&self) -> u64 {
        self.root.body.header.version
    }

    /// The versions to persist for [`TrustedRepository::resume`].
    #[must_use]
    pub const fn versions(&self) -> TrustedVersions {
        self.versions
    }
}

impl fmt::Debug for TrustedRepository {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TrustedRepository")
            .field("root_version", &self.root_version())
            .field("versions", &self.versions)
            .finish_non_exhaustive()
    }
}

fn listed<'a>(
    list: &'a FileList,
    role: &'static str,
    file: &'static str,
) -> Result<&'a MetaFile, TufError> {
    list.meta
        .get(file)
        .ok_or(TufError::MissingMeta { role, file })
}

fn check_rollback(role: &'static str, trusted: Option<u64>, offered: u64) -> Result<(), TufError> {
    match trusted {
        Some(trusted) if offered < trusted => Err(TufError::Rollback { role, trusted, offered }),
        _ => Ok(()),
    }
}

fn check_version(role: &'static str, expected: u64, actual: u64) -> Result<(), TufError> {
    if expected != actual {
        return Err(TufError::VersionMismatch { role, expected, actual });
    }
    Ok(())
}
