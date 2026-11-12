pub use crate::shell_emulator_types::{EmulatedOutput, ShellEmulatorError};

use crate::ProcessTracker;
use std::{collections::HashMap, path::Path};

pub fn execute_emulated(
    _script: &str,
    _cwd: &Path,
    _env: &HashMap<String, String>,
    _output: EmulatedOutput<'_>,
    _process_tracker: Option<&ProcessTracker>,
) -> Result<i32, ShellEmulatorError> {
    Err(ShellEmulatorError::Unsupported)
}

#[cfg(test)]
#[path = "shell_emulator_disabled/tests.rs"]
mod tests;
