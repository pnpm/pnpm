use super::{workspace_dependency_spec, workspace_includes_member};
use crate::cli_args::ecosystem_deps::cargo_manifest_ops::find_cargo_workspace_root;
use std::fs;

#[test]
fn workspace_membership_respects_single_segment_globs_and_literal_bang_paths() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let member = root.join("crates/alpha");
    let nested = root.join("crates/tools/nested");
    let bang_member = root.join("!special");
    let excluded = root.join("crates/excluded");
    for path in [&member, &nested, &bang_member, &excluded] {
        fs::create_dir_all(path).unwrap();
        fs::write(path.join("Cargo.toml"), "[package]\nname = \"x\"\nversion = \"0.1.0\"\n")
            .unwrap();
    }
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/*\", \"!special\"]\nexclude = [\"crates/excluded\"]\n",
    )
    .unwrap();

    assert!(workspace_includes_member(root, &member));
    assert!(!workspace_includes_member(root, &nested));
    assert!(workspace_includes_member(root, &bang_member));
    assert!(!workspace_includes_member(root, &excluded));

    assert_eq!(find_cargo_workspace_root(&member, Some(root)), root);
    assert_eq!(find_cargo_workspace_root(&nested, Some(root)), nested);
    assert_eq!(find_cargo_workspace_root(&bang_member, Some(root)), root);
    assert_eq!(find_cargo_workspace_root(&excluded, Some(root)), excluded);

    let repeated_glob_root = root.join("repeated-glob-ws");
    let deep_non_member =
        repeated_glob_root.join("a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/r/s/t/u/v/w/x/y/z");
    fs::create_dir_all(&deep_non_member).unwrap();
    fs::write(
        repeated_glob_root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"**/**/**/**/**/**/**/**/**/never-matches\"]\n",
    )
    .unwrap();
    assert!(!workspace_includes_member(&repeated_glob_root, &deep_non_member));
}

#[test]
fn workspace_membership_discovers_implicit_path_dependency_members() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let member = root.join("crates/alpha");
    let implicit_lib = root.join("support/implicit-lib");
    let inherited_lib = root.join("support/inherited-lib");
    let excluded_lib = root.join("support/excluded-lib");
    for path in [&member, &implicit_lib, &inherited_lib, &excluded_lib] {
        fs::create_dir_all(path).unwrap();
        fs::write(path.join("Cargo.toml"), "[package]\nname = \"x\"\nversion = \"0.1.0\"\n")
            .unwrap();
    }
    fs::write(
        implicit_lib.join("Cargo.toml"),
        "[package]\nname = \"implicit-lib\"\nversion = \"0.1.0\"\n\n[dependencies]\ninherited-alias = { workspace = true }\n",
    )
    .unwrap();
    fs::write(
        member.join("Cargo.toml"),
        "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\n\n[dependencies]\nimplicit-lib = { path = \"../../support/implicit-lib\" }\nexcluded-lib = { path = \"../../support/excluded-lib\" }\n",
    )
    .unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/*\"]\nexclude = [\"support/excluded-lib\"]\n\n[workspace.dependencies]\ninherited-alias = { package = \"inherited-lib\", path = \"support/inherited-lib\" }\nitoa = \"1.0\"\nfoo = \"1.0\"\n",
    )
    .unwrap();

    assert!(workspace_includes_member(root, &implicit_lib));
    assert!(workspace_includes_member(root, &inherited_lib));
    assert!(!workspace_includes_member(root, &excluded_lib));
    assert_eq!(find_cargo_workspace_root(&implicit_lib, Some(root)), root);
    assert_eq!(find_cargo_workspace_root(&inherited_lib, Some(root)), root);
    assert_eq!(find_cargo_workspace_root(&excluded_lib, Some(root)), excluded_lib);

    let by_alias = workspace_dependency_spec(&member, "inherited-alias", Some("inherited-lib"))
        .expect("resolve workspace dependency when both workspace and member spell package");
    assert_eq!(by_alias.package_name, "inherited-lib");
    assert_eq!(by_alias.path.as_deref(), Some("support/inherited-lib"));

    let by_package_fallback = workspace_dependency_spec(&member, "my_itoa", Some("itoa"))
        .expect("resolve workspace dependency via member package fallback");
    assert_eq!(by_package_fallback.package_name, "itoa");

    let by_key_takes_precedence = workspace_dependency_spec(&member, "foo", Some("bar"))
        .expect("matched workspace key determines crate name");
    assert_eq!(by_key_takes_precedence.package_name, "foo");
}
