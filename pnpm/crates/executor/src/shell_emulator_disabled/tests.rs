use super::{EmulatedOutput, ShellEmulatorError, execute_emulated};
use pretty_assertions::assert_eq;
use std::{collections::HashMap, path::Path};

#[test]
fn reports_when_shell_emulation_is_not_compiled_in() {
    let error = execute_emulated(
        "echo hello",
        Path::new("."),
        &HashMap::new(),
        EmulatedOutput::Inherit,
        None,
    )
    .unwrap_err();

    assert_eq!(error.to_string(), "Shell emulation is not enabled in this build.");
    assert!(matches!(error, ShellEmulatorError::Unsupported));
}
