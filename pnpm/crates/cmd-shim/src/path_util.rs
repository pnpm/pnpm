//! Path-manipulation helpers shared across [`crate::bin_resolver`] and
//! [`crate::shim`]. Kept private to the crate.

pub(crate) use pnpm_fs::lexical_normalize;
