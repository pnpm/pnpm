use super::prepend_dirs_to_path;
use std::path::PathBuf;

#[test]
fn a_delimiter_in_a_directory_is_rejected() {
    let delimiter = if cfg!(windows) { "a;b" } else { "a:b" };
    let error =
        prepend_dirs_to_path(&[PathBuf::from(delimiter)]).expect_err("must reject the delimiter");
    assert_eq!(error.dir, delimiter);
}

#[test]
fn the_directories_come_first_in_the_order_given() {
    let (first, second) =
        if cfg!(windows) { (r"C:\store\bin", r"C:\node\bin") } else { ("/store/bin", "/node/bin") };
    let separator = if cfg!(windows) { ";" } else { ":" };
    let path = prepend_dirs_to_path(&[PathBuf::from(first), PathBuf::from(second)])
        .expect("normal dirs are accepted");
    let path = path.to_string_lossy().into_owned();

    assert!(path.starts_with(&format!("{first}{separator}{second}")), "{path}");
    let inherited = std::env::var("PATH").unwrap_or_default();
    if !inherited.is_empty() {
        assert!(path.ends_with(&inherited), "{path}");
    }
}

/// <https://github.com/pnpm/pnpm/issues/16308>
#[cfg(unix)]
#[test]
fn a_path_variable_in_another_case_is_left_to_the_child() {
    use super::set_command_path;
    use std::{ffi::OsStr, process::Command};

    let mut cmd = Command::new("true");
    set_command_path(&mut cmd, OsStr::new("/store/bin"));

    let envs: Vec<_> = cmd.get_envs().collect();
    assert_eq!(envs, [(OsStr::new("PATH"), Some(OsStr::new("/store/bin")))]);
}
