//! The environment for a git invocation that must fail fast instead of
//! waiting on the terminal. pnpm runs git behind a live-updating reporter
//! that repaints over anything git or ssh prints, so a credential,
//! passphrase, or host-key prompt would be invisible and the install would
//! look hung.
//!
//! `GIT_TERMINAL_PROMPT=0` covers git's own prompts. ssh prompts on the
//! terminal directly, so it is run with `BatchMode=yes`, unless the user
//! selected the ssh command themselves through `GIT_SSH_COMMAND`,
//! `GIT_SSH`, or the `core.sshCommand` git setting in effect in the
//! directory the invocation runs in.

use std::{env, path::Path, process::Command};

use crate::RunCommand;

/// Disable the terminal prompts of `cmd`, a git invocation that may reach a
/// remote and runs in `cwd`, and of the ssh it spawns.
pub fn disable_git_prompts<Sys: RunCommand>(cmd: &mut Command, cwd: Option<&Path>) {
    let vars = non_interactive_git_env(
        |name| env::var_os(name).is_some(),
        || has_configured_ssh_command::<Sys>(cwd),
    );
    for (name, value) in vars {
        cmd.env(name, value);
    }
}

/// The variables [`disable_git_prompts`] sets. `is_set` reports whether the
/// inherited environment already defines a variable; `ssh_command_configured`
/// whether git configuration selects the ssh command, and is consulted only
/// when the environment does not.
pub fn non_interactive_git_env(
    is_set: impl Fn(&str) -> bool,
    ssh_command_configured: impl FnOnce() -> bool,
) -> Vec<(&'static str, &'static str)> {
    let mut vars = vec![("GIT_TERMINAL_PROMPT", "0")];
    if !is_set("GIT_SSH_COMMAND") && !is_set("GIT_SSH") && !ssh_command_configured() {
        vars.push(("GIT_SSH_COMMAND", "ssh -o BatchMode=yes"));
    }
    vars
}

/// Whether the git configuration in effect in `cwd` selects the ssh command
/// through `core.sshCommand`. A missing git reads as not configured; the
/// invocation that follows fails on the missing executable with its own
/// error.
#[must_use]
pub fn has_configured_ssh_command<Sys: RunCommand>(cwd: Option<&Path>) -> bool {
    Sys::run("git", &["config", "--get", "core.sshCommand"], cwd).is_ok_and(|output| output.success)
}

#[cfg(test)]
mod tests;
