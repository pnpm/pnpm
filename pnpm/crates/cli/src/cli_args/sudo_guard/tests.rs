use super::{check_sudo_as, sudo_blocked_operation};
use crate::cli_args::cli_command::{CliArgs, CliCommand};
use clap::Parser;

fn command(argv: &[&str]) -> CliCommand {
    CliArgs::try_parse_from(argv).expect("parses").command
}

#[test]
fn allowed_when_not_root() {
    assert!(check_sudo_as(&command(&["pnpm", "setup"]), 1000, Some("alice")).is_ok());
}

#[test]
fn allowed_for_plain_root_without_sudo() {
    assert!(check_sudo_as(&command(&["pnpm", "setup"]), 0, None).is_ok());
}

#[test]
fn allowed_when_sudo_user_is_root() {
    assert!(check_sudo_as(&command(&["pnpm", "setup"]), 0, Some("root")).is_ok());
}

#[test]
fn setup_is_blocked_under_sudo() {
    let err = check_sudo_as(&command(&["pnpm", "setup"]), 0, Some("alice")).expect_err("blocked");
    assert_eq!(err.to_string(), r#"Running "pnpm setup" with sudo is not supported"#);
}

#[test]
fn self_update_is_blocked_under_sudo() {
    assert!(check_sudo_as(&command(&["pnpm", "self-update"]), 0, Some("alice")).is_err());
}

#[test]
fn global_add_is_blocked_under_sudo() {
    let err = check_sudo_as(&command(&["pnpm", "add", "--global", "foo"]), 0, Some("alice"))
        .expect_err("blocked");
    assert_eq!(err.to_string(), r#"Running "pnpm add --global" with sudo is not supported"#);
}

#[test]
fn local_add_is_allowed_under_sudo() {
    assert!(check_sudo_as(&command(&["pnpm", "add", "foo"]), 0, Some("alice")).is_ok());
}

#[test]
fn read_only_global_commands_are_allowed_under_sudo() {
    assert!(check_sudo_as(&command(&["pnpm", "bin", "--global"]), 0, Some("alice")).is_ok());
    assert!(check_sudo_as(&command(&["pnpm", "root", "--global"]), 0, Some("alice")).is_ok());
    assert!(check_sudo_as(&command(&["pnpm", "list", "--global"]), 0, Some("alice")).is_ok());
}

#[test]
fn config_writes_are_blocked_but_reads_allowed() {
    assert!(
        check_sudo_as(
            &command(&["pnpm", "config", "set", "--global", "store-dir", "/tmp/store"]),
            0,
            Some("alice"),
        )
        .is_err(),
    );
    assert!(
        check_sudo_as(
            &command(&["pnpm", "config", "get", "--global", "store-dir"]),
            0,
            Some("alice"),
        )
        .is_ok(),
    );
}

/// `--location` wins, otherwise config writes default to the global config
/// file — a bare `sudo pnpm config set` must not slip past the guard.
#[test]
fn config_writes_are_gated_on_the_effective_scope() {
    assert_eq!(
        sudo_blocked_operation(&command(&["pnpm", "config", "set", "store-dir", "/tmp/store"])),
        Some("pnpm config set --global".to_string()),
    );
    assert_eq!(
        sudo_blocked_operation(&command(&[
            "pnpm",
            "config",
            "set",
            "--location=global",
            "store-dir",
            "/tmp/store",
        ])),
        Some("pnpm config set --global".to_string()),
    );
    assert_eq!(
        sudo_blocked_operation(&command(&[
            "pnpm",
            "config",
            "set",
            "--location=project",
            "store-dir",
            "/tmp/store",
        ])),
        None,
    );
    assert_eq!(
        sudo_blocked_operation(&command(&["pnpm", "config", "delete", "store-dir"])),
        Some("pnpm config delete --global".to_string()),
    );
}

#[test]
fn bare_link_targets_the_global_dir_and_is_blocked() {
    assert_eq!(
        sudo_blocked_operation(&command(&["pnpm", "link"])),
        Some("pnpm link --global".to_string()),
    );
    assert_eq!(sudo_blocked_operation(&command(&["pnpm", "link", "../foo"])), None);
}
