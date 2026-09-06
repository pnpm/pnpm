//! Cargo-compatible dependency resolution for pnpm.

#![cfg_attr(dylint_lib = "perfectionist", feature(register_tool))]
#![cfg_attr(dylint_lib = "perfectionist", register_tool(perfectionist))]

mod features;
mod lockfile;
mod metadata;
mod model;
mod registry;
mod resolution;

pub use metadata::resolve_inputs;
pub use registry::{CRATES_IO_SPARSE_INDEX, latest_version};
pub use resolution::{missing_index_names, resolve_lockfile};

#[cfg(test)]
mod tests;
