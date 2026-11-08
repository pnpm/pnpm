use super::{is_unambiguous_local_file_spec, is_workspace_path_spec};

#[test]
fn a_workspace_path_is_told_from_a_workspace_range() {
    for spec in [
        "workspace:../lib",
        "workspace:./lib",
        "workspace:/lib",
        "workspace:~/lib",
        r"workspace:~\lib",
        r"workspace:\lib",
        r"workspace:C:\lib",
        r"workspace:\\server\share\lib",
    ] {
        assert!(is_workspace_path_spec(spec), "{spec} names a path");
    }
    for spec in [
        "workspace:*",
        "workspace:^",
        "workspace:~",
        "workspace:^1.0.0",
        "workspace:lib@*",
        "file:../lib",
        "../lib",
        "catalog:",
    ] {
        assert!(!is_workspace_path_spec(spec), "{spec} names no path");
    }
}

#[test]
fn an_unambiguous_local_file_spec_carries_a_protocol_or_a_path() {
    for spec in
        ["file:../lib", "file:lib.tgz", "../lib", "/lib", "~/lib", r"~\lib", r"c:\lib", "c:lib"]
    {
        assert!(is_unambiguous_local_file_spec(spec), "{spec}");
    }
    for spec in ["lib", "lib.tgz", "user/repo", "^1.0.0", "workspace:../lib", "~", "~1.0.0"] {
        assert!(!is_unambiguous_local_file_spec(spec), "{spec}");
    }
}
