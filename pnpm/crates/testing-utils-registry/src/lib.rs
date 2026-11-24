//! Registry-backed test helpers. The shared helpers are re-exported so a test
//! crate needs one dev-dependency.

#![cfg_attr(dylint_lib = "perfectionist", feature(register_tool))]
#![cfg_attr(dylint_lib = "perfectionist", register_tool(perfectionist))]

pub mod bin;
pub mod fs;
pub mod registry;

#[cfg(windows)]
pub use pnpm_testing_utils::console;
#[cfg(target_os = "linux")]
pub use pnpm_testing_utils::open_counter;
pub use pnpm_testing_utils::{
    command_env,
    diagnostics,
    fixtures,
    git_repo,
    stall_report,
    trusted_tls_server,
    turborepo_cache,
};
#[cfg(unix)]
pub use pnpm_testing_utils::{
    process_liveness,
    spawn,
    terminal,
};
