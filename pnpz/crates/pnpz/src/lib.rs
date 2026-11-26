//! pnpz installs system packages from a Nix binary cache into `/nix/store`,
//! without Nix. See `pnpz/README.md` for what this proof of concept covers.
#![cfg(unix)]

pub mod binary_cache;
pub mod catalog;
pub mod closure;
pub mod environment;
pub mod install;
pub mod lockfile;
pub mod manifest;
pub mod project;
#[cfg(target_os = "linux")]
pub mod sandbox;
pub mod store;
pub mod system;

pub use cli::run;

mod cli;
mod commands;
