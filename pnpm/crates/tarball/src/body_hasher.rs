use crate::{TarballError, VerifyChecksumError};
use ssri::{Algorithm, Integrity, IntegrityChecker, IntegrityOpts};

/// Hashes an archive resource chunk by chunk as it is read: verifying the
/// pinned integrity when the resolution carries one, computing a fresh
/// sha512 when it does not.
///
/// Same two outcomes [`verify_tarball_integrity`](crate::download::verify_tarball_integrity) produces over a fully
/// buffered body. Doing it incrementally is what lets a body be
/// extracted while it downloads without the whole of it being held to
/// hash at the end.
pub(crate) enum BodyHasher {
    Pinned { expected: Integrity, checker: IntegrityChecker },
    Computed(IntegrityOpts),
}

impl BodyHasher {
    pub(crate) fn new(expected_integrity: Option<&Integrity>) -> Self {
        match expected_integrity {
            Some(expected) => BodyHasher::Pinned {
                expected: expected.clone(),
                checker: IntegrityChecker::new(expected.clone()),
            },
            None => BodyHasher::Computed(IntegrityOpts::new().algorithm(Algorithm::Sha512)),
        }
    }

    pub(crate) fn input(&mut self, bytes: &[u8]) {
        match self {
            BodyHasher::Pinned { checker, .. } => checker.input(bytes),
            BodyHasher::Computed(opts) => opts.input(bytes),
        }
    }

    pub(crate) fn finish(self, package_url: &str) -> Result<Integrity, TarballError> {
        match self {
            BodyHasher::Pinned { expected, checker } => checker
                .result()
                .map(|_| expected)
                .map_err(|error| {
                    TarballError::Checksum(VerifyChecksumError {
                        url: package_url.to_string(),
                        error,
                    })
                }),
            BodyHasher::Computed(opts) => Ok(opts.result()),
        }
    }
}
