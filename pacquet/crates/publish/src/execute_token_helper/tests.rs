use super::{execute_token_helper, strip_bearer_prefix};
use crate::capabilities::{CommandOutput, RunCommand};
use pacquet_reporter::SilentReporter;
use pretty_assertions::assert_eq;
use std::{io, path::Path};

#[test]
fn strips_bearer_prefix_case_insensitively() {
    assert_eq!(strip_bearer_prefix("Bearer abc"), "abc");
    assert_eq!(strip_bearer_prefix("bearer   xyz"), "xyz");
}

#[test]
fn keeps_token_without_whitespace_after_scheme() {
    assert_eq!(strip_bearer_prefix("Bearertoken"), "Bearertoken");
    assert_eq!(strip_bearer_prefix("plain"), "plain");
}

#[test]
fn returns_trimmed_stdout() {
    struct Helper;
    impl RunCommand for Helper {
        fn run(_: &str, _: &[&str], _: Option<&Path>) -> io::Result<CommandOutput> {
            Ok(CommandOutput {
                success: true,
                stdout: "  Bearer secret-token\n".to_owned(),
                stderr: String::new(),
            })
        }
    }
    let token = execute_token_helper::<Helper, SilentReporter>(&["helper".to_owned()]).unwrap();
    assert_eq!(token, "secret-token");
}

#[test]
fn empty_command_yields_empty_token() {
    struct Unreachable;
    impl RunCommand for Unreachable {
        fn run(_: &str, _: &[&str], _: Option<&Path>) -> io::Result<CommandOutput> {
            unreachable!("an empty tokenHelper must not spawn a process")
        }
    }
    let token = execute_token_helper::<Unreachable, SilentReporter>(&[]).unwrap();
    assert_eq!(token, "");
}
