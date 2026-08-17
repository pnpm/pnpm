//! Provisioning package managers.
//!
//! pnpm installs and runs the package managers it knows about — itself,
//! npm, Yarn and Bun — at the version a project or a git-hosted dependency
//! asks for. [`channel`] decides where an engine's bytes come from;
//! [`install`] materializes one into the shared engine store and returns
//! the directory holding its bins.

pub(crate) mod channel;
pub(crate) mod error;
pub(crate) mod install;
pub(crate) mod pin;
pub(crate) mod provision;
pub(crate) mod resolve;
pub(crate) mod selector;
