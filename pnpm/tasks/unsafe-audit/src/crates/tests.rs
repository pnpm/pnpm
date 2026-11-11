use crate::{members, workspace_root};
use text_block_macros::text_block_fnl;

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

/// A directory the workspace excludes is not a member, so the policy does
/// not reach it.
///
/// A fixture rather than this workspace, where no member pattern reaches
/// the one excluded directory, so the real manifest cannot tell a walk that
/// honours `exclude` from one that ignores it.
#[test]
fn an_excluded_directory_is_not_a_member() {
    let workspace = tempfile::tempdir().expect("create the fixture workspace");
    let root = workspace.path();
    write(
        &root.join("Cargo.toml"),
        text_block_fnl! {
            "[workspace]"
            r#"members = ["crates/*"]"#
            r#"exclude = ["crates/skipped"]"#
        },
    );
    write_member(root, "kept");
    write_member(root, "skipped");

    let found = members(root);
    let names: Vec<&str> = found
        .iter()
        .map(|member| member.name.as_str())
        .collect();

    assert_eq!(names, ["kept"]);
}

/// A member crate under `root/crates`, with the one table [`members`]
/// reads and a library root for it to find.
fn write_member(root: &std::path::Path, name: &str) {
    let directory = root.join("crates").join(name);
    write(&directory.join("Cargo.toml"), &format!("[package]\nname = \"{name}\"\n"));
    write(&directory.join("src/lib.rs"), "");
}

fn write(path: &std::path::Path, contents: &str) {
    std::fs::create_dir_all(path.parent().expect("the path has a parent"))
        .expect("create the fixture directory");
    std::fs::write(path, contents).expect("write the fixture file");
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
