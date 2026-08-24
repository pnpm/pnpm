//! The subprocess capability the git queries run through, and the
//! production [`Host`] provider for it.
//!
//! See the "Dependency injection for tests" section of
//! `pnpm/CODE_STYLE_GUIDE.md` for the convention.

use std::{io, path::Path, process::Command};

/// Captured output of a spawned subprocess: the `stdout`, `stderr`, and
/// exit-status fields the callers read.
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Run a subprocess and capture its output.
pub trait RunCommand {
    fn run(program: &str, args: &[&str], cwd: Option<&Path>) -> io::Result<CommandOutput>;
}

/// Production implementation of [`RunCommand`], spawning the real
/// process through [`std::process::Command`].
pub struct Host;

impl RunCommand for Host {
    fn run(program: &str, args: &[&str], cwd: Option<&Path>) -> io::Result<CommandOutput> {
        let mut command = Command::new(program);
        command.args(args);
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        let output = command.output()?;
        Ok(CommandOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}
