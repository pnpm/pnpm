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
pub mod known_failure;
#[cfg(target_os = "linux")]
pub mod open_counter;
#[cfg(unix)]
pub mod pending_signal;
#[cfg(windows)]
pub mod privileges;
#[cfg(unix)]
pub mod process_liveness;
pub mod registry;
#[cfg(unix)]
pub mod signal_delivery;
#[cfg(unix)]
pub mod spawn;
#[cfg(feature = "libsql")]
pub mod sqlite_misuse;
pub mod stall_report;
#[cfg(unix)]
pub mod terminal;
pub mod trusted_tls_server;
pub mod turborepo_cache;
pub mod untrusted_tls_server;

mod registry_activity;
