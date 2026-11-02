//! Entry bundles: the entries of one level-0 tile, each prefixed with its
//! length as a big-endian 16-bit integer.

use crate::tiles::TILE_WIDTH;
use derive_more::{Display, Error};

/// The largest entry a bundle can hold.
pub const MAX_ENTRY_SIZE: usize = u16::MAX as usize;

/// Why an entry bundle could not be encoded or decoded.
#[derive(Debug, Display, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BundleError {
    #[display("log entry of {size} bytes exceeds the {MAX_ENTRY_SIZE}-byte limit")]
    EntryTooLarge { size: usize },
    #[display("an entry bundle holds at most {TILE_WIDTH} entries")]
    TooManyEntries,
    #[display("truncated entry bundle")]
    Truncated,
}

/// Encodes `entries` as one entry bundle.
pub fn encode_bundle<Entry: AsRef<[u8]>>(entries: &[Entry]) -> Result<Vec<u8>, BundleError> {
    if entries.len() > TILE_WIDTH as usize {
        return Err(BundleError::TooManyEntries);
    }
    let mut bundle = Vec::new();
    for entry in entries {
        let entry = entry.as_ref();
        let size = u16::try_from(entry.len())
            .map_err(|_| BundleError::EntryTooLarge { size: entry.len() })?;
        bundle.extend_from_slice(&size.to_be_bytes());
        bundle.extend_from_slice(entry);
    }
    Ok(bundle)
}

/// Splits an entry bundle into its entries.
pub fn decode_bundle(mut bundle: &[u8]) -> Result<Vec<&[u8]>, BundleError> {
    let mut entries = Vec::new();
    while !bundle.is_empty() {
        let (size, rest) = bundle.split_first_chunk::<2>().ok_or(BundleError::Truncated)?;
        let size = usize::from(u16::from_be_bytes(*size));
        let (entry, rest) = rest.split_at_checked(size).ok_or(BundleError::Truncated)?;
        entries.push(entry);
        bundle = rest;
    }
    if entries.len() > TILE_WIDTH as usize {
        return Err(BundleError::TooManyEntries);
    }
    Ok(entries)
}

#[cfg(test)]
mod tests;
