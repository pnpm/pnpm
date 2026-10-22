#![allow(unsafe_code, reason = "exempt by UNSAFE_POLICY.md, enforced by pnpm-unsafe-audit")]
#![cfg_attr(dylint_lib = "perfectionist", feature(register_tool))]
#![cfg_attr(dylint_lib = "perfectionist", register_tool(perfectionist))]

pub mod bin;
pub mod command_env;
#[cfg(windows)]
pub mod console;
pub mod diagnostics;
pub mod env_guard;
pub mod fixtures;
pub mod fs;
pub mod git_repo;
#[cfg(windows)]
pub mod handles;
pub mod known_failure;
#[cfg(windows)]
pub mod privileges;
pub mod registry;
#[cfg(unix)]
pub mod spawn;
#[cfg(unix)]
pub mod terminal;
pub mod trusted_tls_server;
pub mod turborepo_cache;
pub mod untrusted_tls_server;
