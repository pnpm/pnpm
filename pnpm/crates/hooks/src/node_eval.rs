//! The `node -e` invocation shared by the pnpmfile runtimes.

use std::process::Stdio;
use tokio::process::Command;

/// A `node` process that evaluates `script` as `input_type`, with stdin
/// and stdout piped and the child killed when its handle drops. The
/// caller adds what it needs beyond that, stderr among it.
#[must_use]
pub fn node_eval_command(input_type: &str, script: &str) -> Command {
    let mut command = Command::new("node");
    command.args(["--input-type", input_type, "-e", script]);
    command.kill_on_drop(true).stdin(Stdio::piped()).stdout(Stdio::piped());
    command
}
