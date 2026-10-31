use super::{declared_cargo_deps, format_cargo_saved_spec};
use semver::Version;
use std::fs;

#[test]
fn format_cargo_saved_spec_preserves_exact_requirements() {
    let saved = format_cargo_saved_spec(
        "=1.3.1",
        &Version::parse("1.3.6").unwrap(),
        None,
        false,
        Some("^"),
    )
    .unwrap();

    assert_eq!(saved, "=1.3.6");
}

#[test]
fn format_cargo_saved_spec_prefers_an_explicit_range_over_an_exact_requirement() {
    let saved = format_cargo_saved_spec(
        "=1.3.1",
        &Version::parse("2.0.0").unwrap(),
        Some("^2.0.0"),
        false,
        Some("^"),
    )
    .unwrap();

    assert_eq!(saved, "^2.0.0");
}

#[test]
fn format_cargo_saved_spec_omits_build_metadata() {
    let saved = format_cargo_saved_spec(
        "1.1.8+spec-1.1.0",
        &Version::parse("1.1.9+spec-1.1.1").unwrap(),
        None,
        false,
        None,
    )
    .unwrap();

    assert_eq!(saved, "1.1.9");
}

#[test]
fn declared_cargo_deps_omits_crates_replaced_by_workspace_patches() {
    let workspace = tempfile::tempdir().unwrap();
    let member = workspace.path().join("member");
    fs::create_dir(&member).unwrap();
    fs::write(
        workspace.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"member\"]\n\n[patch.crates-io]\nlibsql = { git = \"https://example.test/libsql\" }\n",
    )
    .unwrap();
    fs::write(
        member.join("Cargo.toml"),
        "[package]\nname = \"member\"\nversion = \"0.1.0\"\n\n[dependencies]\nlibsql = \"0.9.30\"\nserde = \"1.0.0\"\n",
    )
    .unwrap();

    let declared = declared_cargo_deps(&member, Some(workspace.path())).unwrap();

    assert_eq!(
        declared
            .iter()
            .map(|dep| dep.package_name.as_str())
            .collect::<Vec<_>>(),
        ["serde"],
    );
}
