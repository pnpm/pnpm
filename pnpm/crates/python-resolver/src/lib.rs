//! Python dependency resolution for pnpm.
//!
//! Resolution reads two things about a project: which versions of a
//! distribution an index offers ([`candidates_from_page`]), and what each
//! of those versions requires ([`WheelMetadata`]). Both are gathered by
//! the caller — the pnpm CLI downloads a wheel and asks the interpreter,
//! a pnpr server reads the index's own metadata files — and handed to
//! [`step`], which runs one pubgrub pass and either solves the project or
//! names the one thing it still needs.
//!
//! The caller therefore owns every fetch, and this crate owns the rules:
//! which wheel of a version is the one for a target, how a requirement's
//! markers and extras constrain a solution, and what the resulting
//! `pylock.toml` says.

#![cfg_attr(dylint_lib = "perfectionist", feature(register_tool))]
#![cfg_attr(dylint_lib = "perfectionist", register_tool(perfectionist))]

pub use candidates::{
    WheelFilename, candidates_from_page, parse_requirement, source_version, validate_url,
    wheel_identity,
};
pub use lockfile::{
    Inputs, LockedDirectory, LockedPackage, LockedSdist, LockedVcs, LockedWheel, Lockfile,
    Metadata, Solved, Target, ToolMetadata, environment_marker,
};
pub use metadata::WheelMetadata;
pub use packages::{
    Candidate, Excluded, IndexCandidate, NAMED_RELEASES, Offered, Packages, Releases,
};
pub use resolve::{Step, active_locked_sources, locked_solution, step, validate_locked};
pub use source::Source;

mod candidates;
mod lockfile;
mod metadata;
mod packages;
mod requires_python;
mod resolve;
mod source;

#[cfg(test)]
mod tests;
