// Copyright 2018-2025 the Deno authors. MIT license.

use std::ffi::{
    OsStr,
    OsString,
};

use super::error::ShellCommandError;

#[derive(Debug, PartialEq, Eq)]
pub enum ArgKind<'a> {
    ShortFlag(char),
    LongFlag(&'a str),
    Arg(&'a OsStr),
}

impl ArgKind<'_> {
    pub fn bail_unsupported(&self) -> Result<(), ShellCommandError> {
        match self {
            ArgKind::Arg(arg) => Err(ShellCommandError::new(format!(
                "unsupported argument: {}",
                arg.to_string_lossy()
            ))),
            ArgKind::LongFlag(name) => {
                Err(ShellCommandError::new(format!("unsupported flag: --{name}")))
            }
            ArgKind::ShortFlag(name) => {
                Err(ShellCommandError::new(format!("unsupported flag: -{name}")))
            }
        }
    }
}

pub fn parse_arg_kinds(flags: &[OsString]) -> Vec<ArgKind<'_>> {
    let mut result = Vec::new();
    let mut had_dash_dash = false;
    for arg in flags {
        if had_dash_dash {
            result.push(ArgKind::Arg(arg));
        } else if arg == "-" {
            result.push(ArgKind::Arg(OsStr::new("-")));
        } else if arg == "--" {
            had_dash_dash = true;
        } else if let Some(arg_str) = arg.to_str() {
            if let Some(flag) = arg_str.strip_prefix("--") {
                result.push(ArgKind::LongFlag(flag));
            } else if let Some(flags) = arg_str.strip_prefix('-') {
                if flags.parse::<f64>().is_ok() {
                    result.push(ArgKind::Arg(arg));
                } else {
                    for c in flags.chars() {
                        result.push(ArgKind::ShortFlag(c));
                    }
                }
            } else {
                result.push(ArgKind::Arg(arg));
            }
        } else {
            result.push(ArgKind::Arg(arg));
        }
    }
    result
}

#[cfg(test)]
mod test {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn parses() {
        let data =
            vec!["-f", "-ab", "--force", "testing", "other", "-1", "-6.4", "--", "--test", "-t"];
        let data = data
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        let args = parse_arg_kinds(&data);
        assert_eq!(
            args,
            vec![
                ArgKind::ShortFlag('f'),
                ArgKind::ShortFlag('a'),
                ArgKind::ShortFlag('b'),
                ArgKind::LongFlag("force"),
                ArgKind::Arg(OsStr::new("testing")),
                ArgKind::Arg(OsStr::new("other")),
                ArgKind::Arg(OsStr::new("-1")),
                ArgKind::Arg(OsStr::new("-6.4")),
                ArgKind::Arg(OsStr::new("--test")),
                ArgKind::Arg(OsStr::new("-t")),
            ]
        )
    }
}
