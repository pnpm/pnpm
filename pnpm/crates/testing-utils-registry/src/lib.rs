//! Registry-backed test helpers. The shared helpers are re-exported so a test
//! crate needs one dev-dependency.

#![cfg_attr(dylint_lib = "perfectionist", feature(register_tool))]
#![cfg_attr(dylint_lib = "perfectionist", register_tool(perfectionist))]

pub mod bin;
pub mod fs;
pub mod registry;

pub use pnpm_testing_utils::*;
