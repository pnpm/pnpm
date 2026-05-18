#![cfg_attr(dylint_lib = "perfectionist", feature(register_tool))]
#![cfg_attr(dylint_lib = "perfectionist", register_tool(perfectionist))]
mod local_tracing;

pub use miette;
pub use tracing;

pub use local_tracing::enable_tracing_by_env;
