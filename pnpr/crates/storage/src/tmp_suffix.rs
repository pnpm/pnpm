//! The suffix an atomic write appends to a file name for its temporary file:
//! `.tmp.<pid>.<counter>.<random>`. One module owns its format and its longest
//! length, so a check that a name still fits with the suffix cannot drift from
//! the suffix the write appends.

/// What every temporary file name's suffix starts with.
pub(crate) const TMP_MARKER: &str = ".tmp.";

/// How many hex digits the random part of the suffix has.
pub(crate) const RANDOM_HEX_DIGITS: usize = 16;

/// The most bytes [`tmp_suffix`] returns: the longest `u32` pid and `u64`
/// counter, with their separators.
pub(crate) const MAX_TMP_SUFFIX_BYTES: usize = TMP_MARKER.len()
    + u32::MAX.ilog10() as usize
    + 1
    + ".".len()
    + u64::MAX.ilog10() as usize
    + 1
    + ".".len()
    + RANDOM_HEX_DIGITS;

/// The suffix for a temporary file written by process `pid`.
pub(crate) fn tmp_suffix(pid: u32, counter: u64, random: u64) -> String {
    format!("{TMP_MARKER}{pid}.{counter}.{random:0RANDOM_HEX_DIGITS$x}")
}

#[cfg(test)]
mod tests;
