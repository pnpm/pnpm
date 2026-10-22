//! Holds the workspace to the [`unsafe` policy](../../../UNSAFE_POLICY.md).
//!
//! The policy's rule is that a crate either abstracts a foreign or
//! raw-pointer API or forbids `unsafe` outright. [`exempt`] records which
//! crates are the former, and [`audit`] fails the rest.
//!
//! The record lives in code rather than in the policy document so the two
//! cannot drift: a crate added to one tier without an entry here is caught
//! by [`audit`], and an entry whose crate no longer holds any `unsafe` is
//! caught as stale.

#![forbid(unsafe_code)]

pub use crates::{Member, members};
pub use exemption::{Exemption, Tier, exempt};
pub use scan::{Finding, Occurrence, audit};

mod crates;
mod exemption;
mod scan;

/// The repository root, found by walking up from this crate's directory.
///
/// `CARGO_MANIFEST_DIR` is where this crate is, and the root is where the
/// workspace manifest is, so the walk stops at the first directory whose
/// `Cargo.toml` holds a `workspace` table.
#[must_use]
pub fn workspace_root() -> std::path::PathBuf {
    let mut directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    loop {
        if crates::declares_a_workspace(&directory.join("Cargo.toml")) {
            return directory.to_path_buf();
        }
        directory = directory.parent().expect("this crate sits under the workspace root");
    }
}
