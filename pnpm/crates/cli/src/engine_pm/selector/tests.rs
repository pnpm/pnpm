use super::tool_install_selector;

#[test]
fn a_line_published_under_its_own_name_needs_no_rewrite() {
    assert_eq!(tool_install_selector("yarn@1.22.22"), None);
    assert_eq!(tool_install_selector("yarn@^1"), None);
    assert_eq!(tool_install_selector("npm@11"), None);
    assert_eq!(tool_install_selector("npm"), None);
}

/// Yarn Berry is published as `@yarnpkg/cli-dist`, so asking for `yarn@4`
/// has to install that package under the name the user typed.
#[test]
fn yarn_berry_installs_the_cli_dist_package_under_the_yarn_name() {
    assert_eq!(
        tool_install_selector("yarn@4.9.2").as_deref(),
        Some("yarn@npm:@yarnpkg/cli-dist@4.9.2"),
    );
    let berry = "yarn@npm:@yarnpkg/cli-dist@latest";
    assert_eq!(tool_install_selector("yarn").as_deref(), Some(berry));
}

/// The tools that ship as platform archives go through the protocol that
/// resolves archives.
#[test]
fn archive_published_tools_go_through_the_runtime_protocol() {
    assert_eq!(tool_install_selector("yarn@6").as_deref(), Some("yarn@runtime:6"));
    assert_eq!(tool_install_selector("bun@1.3.0").as_deref(), Some("bun@runtime:1.3.0"));
    assert_eq!(tool_install_selector("node@22").as_deref(), Some("node@runtime:22"));
    assert_eq!(tool_install_selector("deno").as_deref(), Some("deno@runtime:latest"));
}

/// Everything else is an ordinary package, including a scoped name whose
/// leading `@` must not be read as a version separator.
#[test]
fn other_packages_are_left_alone() {
    assert_eq!(tool_install_selector("typescript@5"), None);
    assert_eq!(tool_install_selector("@yarnpkg/cli-dist@4.9.2"), None);
    assert_eq!(tool_install_selector("nodemon"), None);
    // pnpm installs itself through `self-update`, never through this path.
    assert_eq!(tool_install_selector("pnpm@11"), None);
}

/// A request that already locates a package says what it wants; the
/// rewrite would nest one locator inside another.
#[test]
fn a_request_that_locates_a_package_is_left_alone() {
    assert_eq!(tool_install_selector("node@runtime:22"), None);
    assert_eq!(tool_install_selector("bun@runtime:1.3.0"), None);
    assert_eq!(tool_install_selector("yarn@npm:@yarnpkg/cli-dist@4.9.2"), None);
    assert_eq!(tool_install_selector("node@github:nodejs/node"), None);
    // The GitHub shorthand locates one without spelling out a protocol.
    assert_eq!(tool_install_selector("yarn@yarnpkg/berry"), None);
    assert_eq!(tool_install_selector("node@nodejs/node#main"), None);
}
