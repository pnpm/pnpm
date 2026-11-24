//! Registry-backed test helpers. The shared helpers are re-exported so a test
//! crate needs one dev-dependency.

#![allow(unsafe_code, reason = "exempt by UNSAFE_POLICY.md, enforced by pnpm-unsafe-audit")]
#![cfg_attr(dylint_lib = "perfectionist", feature(register_tool))]
#![cfg_attr(dylint_lib = "perfectionist", register_tool(perfectionist))]

pub use pnpm_testing_utils::*;

pub mod bin;
pub mod fs;
pub mod registry;
