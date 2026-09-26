//! The npm commands pnpm registers but has not implemented.
//!
//! They are part of the command table rather than left to the
//! external-subcommand fallback so `pnpm token` names the npm CLI instead
//! of being taken for a package script and failing as a missing binary.

use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;

/// Everything after the command name is swallowed, so
/// `pnpm token create --read-only` reaches the same error as a bare
/// `pnpm token` rather than an argument-parsing one.
#[derive(Debug, Args)]
pub struct NotImplementedArgs {
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub params: Vec<String>,
}

/// The one outcome of an unimplemented command. `command` is the name pnpm
/// registered, not an alias the user typed — these commands have none.
#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    r#"The "{command}" command is not yet implemented in pnpm. Use the npm CLI directly: npm {command}"#
)]
#[diagnostic(code(ERR_PNPM_NOT_IMPLEMENTED))]
pub struct NotImplementedError {
    #[error(not(source))]
    pub command: &'static str,
}
