// Copyright 2018-2025 the Deno authors. MIT license.

use std::{
    num::{
        ParseFloatError,
        ParseIntError,
    },
    string::FromUtf8Error,
};

/// A shell command error whose message is produced directly by callers.
/// Used for the lightweight "print the message via stderr" reporting most
/// builtins do.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ShellCommandError(pub String);

impl ShellCommandError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl From<String> for ShellCommandError {
    fn from(message: String) -> Self {
        Self(message)
    }
}

impl From<std::io::Error> for ShellCommandError {
    fn from(err: std::io::Error) -> Self {
        Self(err.to_string())
    }
}

impl From<FromUtf8Error> for ShellCommandError {
    fn from(err: FromUtf8Error) -> Self {
        Self(err.to_string())
    }
}

impl From<ParseFloatError> for ShellCommandError {
    fn from(err: ParseFloatError) -> Self {
        Self(err.to_string())
    }
}

impl From<ParseIntError> for ShellCommandError {
    fn from(err: ParseIntError) -> Self {
        Self(err.to_string())
    }
}

macro_rules! bail {
  ($($arg:tt)*) => {
    return Err($crate::shell::commands::error::ShellCommandError(format!($($arg)*)).into())
  };
}

pub(crate) use bail;
