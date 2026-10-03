use super::parse_git_version;

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
