//! The environment for a git invocation that must fail fast instead of
//! waiting on the terminal. pnpm runs git behind a live-updating reporter
//! that repaints over anything git or ssh prints, so a credential,
//! passphrase, or host-key prompt would be invisible and the install would
//! look hung.
//!
//! `GIT_TERMINAL_PROMPT=0` covers git's own prompts. ssh prompts on the
//! terminal directly, so it is run with `BatchMode=yes`, unless the user
//! configured the ssh command themselves through `GIT_SSH_COMMAND` or
//! `GIT_SSH`.

use std::{env, process::Command};

/// Disable the terminal prompts of `cmd`, a git invocation, and of the ssh it
/// spawns.
pub fn disable_git_prompts(cmd: &mut Command) {
    for (name, value) in non_interactive_git_env(|name| env::var_os(name).is_some()) {
        cmd.env(name, value);
    }
}

/// The variables [`disable_git_prompts`] sets. `is_set` reports whether the
/// inherited environment already defines a variable.
pub fn non_interactive_git_env(is_set: impl Fn(&str) -> bool) -> Vec<(&'static str, &'static str)> {
    let mut vars = vec![("GIT_TERMINAL_PROMPT", "0")];
    if !is_set("GIT_SSH_COMMAND") && !is_set("GIT_SSH") {
        vars.push(("GIT_SSH_COMMAND", "ssh -o BatchMode=yes"));
    }
    vars
}

#[cfg(test)]
mod tests;
