use crate::{members, workspace_root};

/// Pins that the member walk finds the crates the workspace actually has,
/// since an audit that found none would pass vacuously.
#[test]
fn every_member_pattern_is_expanded() {
    let members = members(&workspace_root());
    assert!(members.len() > 100, "found only {} members", members.len());
    for expected in ["pnpm-cli", "pnpm-fs", "pnpr", "pnpm-unsafe-audit"] {
        assert!(
            members
                .iter()
                .any(|member| member.name == expected),
            "{expected} is missing from the member list",
        );
    }
}

/// The vendored `tar` fork is excluded from the workspace, so the policy
/// does not reach it and the walk must not pick it up.
#[test]
fn an_excluded_directory_is_not_a_member() {
    let members = members(&workspace_root());
    assert!(
        members
            .iter()
            .all(|member| !member.directory.ends_with("vendor/tar")),
    );
}

/// Pins that a crate's roots include the ones Cargo discovers as well as
/// the ones its manifest names, because a missing root is a crate the
/// attribute check skips.
#[test]
fn a_members_roots_cover_its_targets() {
    let members = members(&workspace_root());
    let cli = members
        .iter()
        .find(|member| member.name == "pnpm-cli")
        .expect("pnpm-cli is a member");
    // The manifest names the binary's path; the suite is discovered.
    assert!(
        cli.roots
            .iter()
            .any(|root| root.ends_with("src/bin/main.rs")),
    );
    assert!(
        cli.roots
            .iter()
            .any(|root| root.ends_with("tests/suite/main.rs")),
    );
    assert!(
        cli.roots
            .iter()
            .any(|root| root.ends_with("src/lib.rs")),
    );
    assert!(cli.sources.len() > cli.roots.len());
}
