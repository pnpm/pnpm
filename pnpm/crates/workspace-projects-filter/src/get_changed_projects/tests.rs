use super::{check_git_version, git_supports_no_relative, parse_git_version};

#[test]
fn parse_git_version_reads_major_and_minor() {
    assert_eq!(parse_git_version("git version 2.25.1\n"), Some((2, 25)));
    assert_eq!(parse_git_version("git version 2.39.3 (Apple Git-145)\n"), Some((2, 39)));
    assert_eq!(parse_git_version("git version 2.45.1.windows.1\n"), Some((2, 45)));
}

#[test]
fn parse_git_version_rejects_unknown_output() {
    assert_eq!(parse_git_version(""), None);
    assert_eq!(parse_git_version("hub version 2.14.2\n"), None);
    assert_eq!(parse_git_version("git version unknown\n"), None);
}

#[test]
fn check_git_version_rejects_git_older_than_2_24() {
    let err = check_git_version(Some((2, 17))).expect_err("git 2.17 is too old");
    assert_eq!(
        err.to_string(),
        "Filtering by changed packages failed. The [<since>] selector requires git 2.24 or newer, but git 2.17 is installed.",
    );
}

#[test]
fn check_git_version_accepts_git_2_24_and_unknown_versions() {
    assert!(check_git_version(Some((2, 24))).is_ok());
    assert!(check_git_version(Some((3, 0))).is_ok());
    assert!(check_git_version(None).is_ok());
}

#[test]
fn git_supports_no_relative_from_git_2_28_and_for_unknown_versions() {
    assert!(!git_supports_no_relative(Some((2, 27))));
    assert!(git_supports_no_relative(Some((2, 28))));
    assert!(git_supports_no_relative(None));
}
