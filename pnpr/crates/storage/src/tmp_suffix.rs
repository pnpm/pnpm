//! The suffix an atomic write appends to a file name for its temporary file:
//! `.tmp.<pid>.<counter>.<random>`.

/// What every temporary file name's suffix starts with.
pub(crate) const TMP_MARKER: &str = ".tmp.";

/// How many hex digits the random part of the suffix has.
pub(crate) const RANDOM_HEX_DIGITS: usize = 16;

/// The suffix for a temporary file written by process `pid`.
pub(crate) fn tmp_suffix(pid: u32, counter: u64, random: u64) -> String {
    format!("{TMP_MARKER}{pid}.{counter}.{random:0RANDOM_HEX_DIGITS$x}")
}

#[cfg(test)]
mod tests;
