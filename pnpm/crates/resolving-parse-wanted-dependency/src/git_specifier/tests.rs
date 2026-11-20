use super::git_specifiers_are_equivalent;

#[test]
fn recognizes_equivalent_git_specifiers() {
    for (left, right) in [
        (
            "git://github.com/kevva/is-positive.git#97edff6",
            "git+https://github.com/kevva/is-positive.git#97edff6",
        ),
        ("git://github.com/org/lsp-mcp#main", "git+https://github.com/org/lsp-mcp.git#main"),
        (
            "github:kevva/is-positive#97edff6",
            "git+https://github.com/kevva/is-positive.git#97edff6",
        ),
        ("gitlab:group/repository#main", "git+https://gitlab.com/group/repository.git#main"),
        ("bitbucket:group/repository#main", "git+https://bitbucket.org/group/repository.git#main"),
        ("kevva/is-positive#97edff6", "https://github.com/kevva/is-positive.git#97edff6"),
        ("github:kevva/is-positive", "GIT+HTTPS://github.com/kevva/is-positive.git"),
        (
            "git+https://GitHub.com/kevva/is-positive.git#97edff6",
            "git+https://github.com/kevva/is-positive.git#97edff6",
        ),
    ] {
        assert!(git_specifiers_are_equivalent(left, right), "LEFT: {left}\nRIGHT: {right}");
    }
}

#[test]
fn distinguishes_different_git_specifiers() {
    let canonical = "git+https://github.com/kevva/is-positive.git#97edff6";
    for different in [
        "git+https://gitlab.com/kevva/is-positive.git#97edff6",
        "git+https://github.com/other/is-positive.git#97edff6",
        "git+https://github.com/kevva/other.git#97edff6",
        "git+https://github.com/kevva/is-positive.git#different",
        "git+ssh://git@github.com/kevva/is-positive.git#97edff6",
        "https://example.com/not-a-git-dependency",
        "git+https://github.com/kevva/Is-Positive.git#97edff6",
        "^1.0.0",
    ] {
        assert!(
            !git_specifiers_are_equivalent(canonical, different),
            "LEFT: {canonical}\nRIGHT: {different}",
        );
        assert!(
            !git_specifiers_are_equivalent(different, canonical),
            "LEFT: {different}\nRIGHT: {canonical}",
        );
    }
}
