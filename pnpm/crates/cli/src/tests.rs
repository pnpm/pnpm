use super::{inject_alias_subcommand, parse_cli_args, prepare_cli_argv};
use std::ffi::OsString;

fn argv(parts: &[&str]) -> Vec<OsString> {
    parts
        .iter()
        .map(OsString::from)
        .collect()
}

#[test]
fn pnpx_and_pnx_inject_dlx_after_the_program_name() {
    for alias in ["pnpx", "pnx"] {
        let result = inject_alias_subcommand(Some(alias), argv(&[alias, "create-vite", "app"]));
        assert_eq!(result, argv(&[alias, "dlx", "create-vite", "app"]));
    }
}

#[test]
fn pnpm_pn_and_pacquet_names_are_left_untouched() {
    for name in ["pnpm", "pn", "pacquet", "PNPX-not-exact"] {
        let original = argv(&[name, "install"]);
        assert_eq!(inject_alias_subcommand(Some(name), original.clone()), original);
    }
}

#[test]
fn an_unknown_executable_name_is_left_untouched() {
    let original = argv(&["whatever", "install"]);
    assert_eq!(inject_alias_subcommand(None, original.clone()), original);
}

fn recursive_from_command_line(parts: &[&str]) -> bool {
    let (command, argv) = prepare_cli_argv(argv(parts));
    parse_cli_args(command, argv).expect("argv should parse").workspace.recursive_from_command_line
}

#[test]
fn an_explicit_recursive_flag_is_recorded_as_coming_from_the_command_line() {
    assert!(recursive_from_command_line(&["pnpm", "install", "-r"]));
    assert!(recursive_from_command_line(&["pnpm", "--recursive", "install"]));
}

#[test]
fn a_negated_recursive_flag_is_not_recorded_as_coming_from_the_command_line() {
    assert!(!recursive_from_command_line(&["pnpm", "install", "-r", "--no-recursive"]));
    assert!(!recursive_from_command_line(&["pnpm", "install"]));
}
