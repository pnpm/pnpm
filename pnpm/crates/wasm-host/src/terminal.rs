use serde_json::{Value, json};
use std::io;

/// Confirms through the host terminal, preserving an optional Enter-key default.
/// Interrupted input and end of input return an error, including when the default is yes.
pub fn confirm(message: &str, default: Option<bool>) -> io::Result<bool> {
    let response = crate::request_blocking(&json!({
        "operation": "terminal.confirm", "message": message, "default": default,
    }))
    .map_err(prompt_error)?;
    response.as_bool().ok_or_else(|| io::Error::other("Invalid terminal confirmation response"))
}

/// Returns whether the host stdin is attached to a terminal.
pub fn stdin_is_terminal() -> io::Result<bool> {
    let response = crate::request_blocking(&json!({"operation": "terminal.status"}))
        .map_err(io::Error::other)?;
    response
        .get("stdin")
        .and_then(Value::as_bool)
        .ok_or_else(|| io::Error::other("Invalid terminal status response"))
}

/// Reads visible terminal input, optionally accepting an empty line.
pub fn input(message: &str, allow_empty: bool) -> io::Result<String> {
    read_text("terminal.input", message, allow_empty)
}

/// Reads a terminal password without echoing its characters.
pub fn password(message: &str, allow_empty: bool) -> io::Result<String> {
    read_text("terminal.password", message, allow_empty)
}

fn read_text(operation: &str, message: &str, allow_empty: bool) -> io::Result<String> {
    let response = crate::request_blocking(&json!({
        "operation": operation, "message": message, "allowEmpty": allow_empty,
    }))
    .map_err(prompt_error)?;
    response
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| io::Error::other("Invalid terminal input response"))
}

fn prompt_error(error: crate::HostError) -> io::Error {
    let kind = if error.code.as_deref() == Some("EINTR") {
        io::ErrorKind::Interrupted
    } else {
        io::ErrorKind::Other
    };
    io::Error::new(kind, error)
}
