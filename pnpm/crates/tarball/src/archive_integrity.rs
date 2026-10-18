//! The integrities one archive extraction establishes, and which of them a
//! pinned integrity is checked against.

use crate::{BodyHasher, TarballError, VerifyChecksumError};
use pnpm_lockfile::IntegrityTarget;
use ssri::{Algorithm, Integrity};
use std::io::Read;

/// Which hash of an archive a pinned integrity names.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum IntegrityKind {
    /// The resource at the tarball URL after HTTP content decoding.
    #[default]
    Resource,
    /// The uncompressed tar.
    Tar,
    /// Either of them: the archive matches when its resource or its tar
    /// does. For a resolution that holds an integrity without knowing which
    /// hash it is. Only a sha512 pin can name a tar.
    ResourceOrTar,
}

impl From<IntegrityTarget> for IntegrityKind {
    fn from(target: IntegrityTarget) -> Self {
        match target {
            IntegrityTarget::Resource => IntegrityKind::Resource,
            IntegrityTarget::Tar => IntegrityKind::Tar,
        }
    }
}

/// What an extraction checks the archive against: the pinned integrity, if
/// any, and which hash of the archive it names.
#[derive(Debug, Default, Clone)]
pub(crate) struct ArchiveVerification {
    pub(crate) expected: Option<Integrity>,
    pub(crate) kind: IntegrityKind,
}

impl ArchiveVerification {
    pub(crate) fn new(expected: Option<&Integrity>, kind: IntegrityKind) -> Self {
        let kind = match (kind, expected) {
            (IntegrityKind::ResourceOrTar, Some(expected))
                if expected.pick_algorithm() != Algorithm::Sha512 =>
            {
                IntegrityKind::Resource
            }
            (kind, _) => kind,
        };
        Self { expected: expected.cloned(), kind }
    }

    /// Whether a `.tgz` has to be decoded in full, every gzip member, which
    /// only the streaming decoder does: when the pin may name the tar, or the
    /// hash being learned is the tar's.
    pub(crate) fn needs_tar_stream(&self) -> bool {
        self.kind != IntegrityKind::Resource
    }

    /// The hashers for a resource that is a `.tgz` when `gzip` holds and a
    /// tar otherwise. A tar is its own tar, so it gets no second hasher.
    pub(crate) fn hashers(&self, gzip: bool) -> ArchiveHashers {
        let pinned = || BodyHasher::new(self.expected.as_ref());
        let computed = || BodyHasher::new(None);
        match (gzip, self.kind) {
            (false, _) => ArchiveHashers { resource: pinned(), tar: None },
            (true, IntegrityKind::Resource) => {
                ArchiveHashers { resource: pinned(), tar: Some(computed()) }
            }
            (true, IntegrityKind::Tar) => {
                ArchiveHashers { resource: computed(), tar: Some(pinned()) }
            }
            (true, IntegrityKind::ResourceOrTar) => {
                ArchiveHashers { resource: computed(), tar: Some(computed()) }
            }
        }
    }

    /// Pair the resource's hash with the tar's. `tar` is `None` for a tar
    /// resource, and the outcome of hashing the decoded stream for a `.tgz`.
    ///
    /// A `.tgz` that does not decode in full has no tar. That fails the
    /// archive only when the pin may name the tar.
    pub(crate) fn finish(
        &self,
        resource: Result<Integrity, TarballError>,
        tar: Option<Result<Integrity, TarballError>>,
        package_url: &str,
    ) -> Result<ArchiveIntegrity, TarballError> {
        let Some(tar) = tar else {
            let resource = resource?;
            return Ok(ArchiveIntegrity { tar: Some(resource.clone()), resource });
        };
        let resource = resource?;
        match self.kind {
            IntegrityKind::Resource => {
                let tar = tar
                    .inspect_err(|error| {
                        tracing::debug!(
                            target: "pacquet::download",
                            %error,
                            "the .tgz does not decode to a complete tar; recording no tar integrity",
                        );
                    })
                    .ok();
                Ok(ArchiveIntegrity { resource, tar })
            }
            IntegrityKind::Tar => Ok(ArchiveIntegrity { resource, tar: Some(tar?) }),
            IntegrityKind::ResourceOrTar => {
                let tar = tar?;
                if let Some(expected) = &self.expected
                    && expected.matches(&resource).is_none()
                    && expected.matches(&tar).is_none()
                {
                    return Err(TarballError::Checksum(VerifyChecksumError {
                        url: package_url.to_string(),
                        error: ssri::Error::IntegrityCheckError(expected.clone(), tar),
                    }));
                }
                Ok(ArchiveIntegrity { resource, tar: Some(tar) })
            }
        }
    }
}

/// Hashers for one resource; see [`ArchiveVerification::hashers`].
pub(crate) struct ArchiveHashers {
    pub(crate) resource: BodyHasher,
    pub(crate) tar: Option<BodyHasher>,
}

/// The integrities of one extracted archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveIntegrity {
    /// The resource at the tarball URL after HTTP content decoding.
    pub resource: Integrity,
    /// The tar the resource holds: the resource itself when it is a tar, the
    /// whole gzip-decoded stream of a `.tgz`. `None` when a `.tgz` does not
    /// decode in full.
    pub tar: Option<Integrity>,
}

impl ArchiveIntegrity {
    /// The integrity a pin of `kind` resolves to: the tar's when the pin may
    /// name it.
    #[must_use]
    pub fn of(&self, kind: IntegrityKind) -> Option<&Integrity> {
        match kind {
            IntegrityKind::Resource => Some(&self.resource),
            IntegrityKind::Tar | IntegrityKind::ResourceOrTar => self.tar.as_ref(),
        }
    }

    /// The tar's integrity when it differs from the resource's: the second
    /// key a store row is reachable under.
    #[must_use]
    pub fn distinct_tar(&self) -> Option<&Integrity> {
        self.tar
            .as_ref()
            .filter(|tar| **tar != self.resource)
    }

    /// Record the tar's integrity on the row filed under the resource's,
    /// and return the key to alias to that row, if the two differ.
    pub(crate) fn tag_row(
        &self,
        row: &mut pnpm_store_dir::PackageFilesIndex,
        alias_key: impl FnOnce(&Integrity) -> String,
    ) -> Option<String> {
        row.tar_integrity = self.tar.as_ref().map(Integrity::to_string);
        self.distinct_tar().map(alias_key)
    }
}

/// Hash the tar `archive` holds, decoding every gzip member of a `.tgz`,
/// and check it against `expected` when there is one. A tar decoding past
/// the default [`TarLimits`](crate::TarLimits) fails.
pub(crate) fn check_tar_integrity(
    archive: impl Read,
    expected: Option<&Integrity>,
    package_url: &str,
) -> Result<Integrity, TarballError> {
    let limit = crate::TarLimits::default().max_decoded();
    let mut archive = std::io::BufReader::new(archive);
    let gzip = std::io::BufRead::fill_buf(&mut archive)
        .map_err(TarballError::ReadTarballEntries)?
        .starts_with(&crate::extract::GZIP_MAGIC);
    let mut hasher = BodyHasher::new(expected);
    let mut sink = HasherSink(&mut hasher);
    let copied = if gzip {
        std::io::copy(&mut flate2::read::MultiGzDecoder::new(archive).take(limit + 1), &mut sink)
    } else {
        std::io::copy(&mut archive.take(limit + 1), &mut sink)
    };
    let copied = copied.map_err(|error| TarballError::IncompleteTar {
        url: package_url.to_string(),
        reason: error.to_string(),
    })?;
    if copied > limit {
        return Err(TarballError::ArchiveLimit(crate::ArchiveLimit::Payload(limit)));
    }
    hasher.finish(package_url)
}

struct HasherSink<'a>(&'a mut BodyHasher);

impl std::io::Write for HasherSink<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.input(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
