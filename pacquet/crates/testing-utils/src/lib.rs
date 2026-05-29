#![cfg_attr(dylint_lib = "perfectionist", feature(register_tool))]
#![cfg_attr(dylint_lib = "perfectionist", register_tool(perfectionist))]

pub mod bin;
pub mod env_guard;
pub mod fixtures;
pub mod fs;
pub mod known_failure;
pub mod registry;
