//! The per-archive unpacked-size budget of the `maxUnpackedSize` setting.

use super::TarballError;

/// Running total of the bytes one archive's regular files declare,
/// checked against the `maxUnpackedSize` setting as each file header is
/// read.
///
/// Header sizes stand in for written bytes: the tar reader never yields
/// more payload than a header declares, so their sum bounds what can
/// reach the store, and checking it before the payload is read keeps the
/// crossing entry out of the store entirely. Entries the ignore filter
/// drops count too, as they do in the registry's `unpackedSize`.
pub(crate) struct UnpackedSizeBudget {
    limit: u64,
    declared: u64,
}

impl UnpackedSizeBudget {
    pub(crate) fn new(limit: u64) -> Self {
        Self { limit, declared: 0 }
    }

    pub(crate) fn declare(&mut self, size: u64) -> Result<(), TarballError> {
        self.declared = self.declared.saturating_add(size);
        if self.declared > self.limit {
            return Err(TarballError::UnpackedSizeExceeded { limit: self.limit });
        }
        Ok(())
    }
}
