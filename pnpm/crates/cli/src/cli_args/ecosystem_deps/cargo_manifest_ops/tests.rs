use super::{apply_manifest_edits, declared_cargo_deps, format_cargo_saved_spec};
use crate::cli_args::ecosystem_deps::{
    cargo::validate_explicit_cargo_requests, selectors::CargoUpdateSelector,
};
use semver::Version;
use std::fs;

#[test]
fn format_cargo_saved_spec_preserves_exact_requirements() {
    let saved = format_cargo_saved_spec("=1.3.1", &Version::parse("1.3.6").unwrap(), None);

    assert_eq!(saved, "=1.3.6");
}

#[test]
fn format_cargo_saved_spec_prefers_an_explicit_range_over_an_exact_requirement() {
    let saved =
        format_cargo_saved_spec("=1.3.1", &Version::parse("2.0.0").unwrap(), Some("^2.0.0"));

    assert_eq!(saved, "^2.0.0");
}

#[test]
fn format_cargo_saved_spec_omits_build_metadata_from_an_explicit_range() {
    let saved = format_cargo_saved_spec(
        "1.1.8",
        &Version::parse("1.1.9+spec-1.1.1").unwrap(),
        Some("^1.1.9+spec-1.1.1"),
    );

    assert_eq!(saved, "^1.1.9");
}

#[test]
fn format_cargo_saved_spec_omits_build_metadata() {
    let saved = format_cargo_saved_spec(
        "1.1.8+spec-1.1.0",
        &Version::parse("1.1.9+spec-1.1.1").unwrap(),
        None,
    );

    assert_eq!(saved, "1.1.9");
}

#[test]
fn format_cargo_saved_spec_keeps_the_existing_operator() {
    let resolved = Version::parse("1.4.0").unwrap();

    assert_eq!(format_cargo_saved_spec("~1.3.1", &resolved, None), "~1.4.0");
    assert_eq!(format_cargo_saved_spec("1.3.1", &resolved, None), "1.4.0");
    assert_eq!(format_cargo_saved_spec("1.3.0+exp", &resolved, None), "1.4.0");
    assert_eq!(format_cargo_saved_spec(">=1.3, <2", &resolved, None), ">=1.4.0, <2");
}

#[test]
fn format_cargo_saved_spec_keeps_a_requirement_without_a_lower_bound() {
    let resolved = Version::parse("2.0.0").unwrap();

    assert_eq!(format_cargo_saved_spec("<=2.0.0", &resolved, None), "<=2.0.0");
    assert_eq!(format_cargo_saved_spec("<=2.0.0+spec", &resolved, None), "<=2.0.0");
    assert_eq!(format_cargo_saved_spec("2.*", &resolved, None), "2.*");
    assert_eq!(format_cargo_saved_spec("<2", &resolved, None), "^2.0.0");
}

#[test]
fn format_cargo_saved_spec_writes_a_bare_explicit_requirement_as_given() {
    let resolved = Version::parse("2.0.0").unwrap();

    assert_eq!(format_cargo_saved_spec("~1.0.0", &resolved, Some("2.0.0")), "2.0.0");
    assert_eq!(format_cargo_saved_spec("~1.0.0", &resolved, Some("2")), "2");
}

#[test]
fn declared_cargo_deps_and_manifest_edits_resolve_renamed_workspace_dependencies() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let member = root.join("crates/app");
    fs::create_dir_all(&member).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        concat!(
            "[workspace]\n",
            "members = [\"crates/*\"]\n\n",
            "[workspace.dependencies]\n",
            "itoa = \"1.0.0\"\n",
            "my_ryu = { package = \"ryu\", version = \"1.0.0\" }\n",
        ),
    )
    .unwrap();
    fs::write(
        member.join("Cargo.toml"),
        concat!(
            "[package]\n",
            "name = \"app\"\n",
            "version = \"0.1.0\"\n\n",
            "[dependencies]\n",
            "my_itoa = { package = \"itoa\", workspace = true }\n",
            "my_ryu = { workspace = true }\n",
            "other_ryu = { package = \"my_ryu\", workspace = true }\n",
        ),
    )
    .unwrap();

    let deps = declared_cargo_deps(&member, Some(root)).unwrap();
    assert_eq!(deps.len(), 2);

    let itoa = deps
        .iter()
        .find(|dep| dep.has_alias("my_itoa"))
        .expect("my_itoa resolved");
    assert_eq!(itoa.manifest_key, "itoa");
    assert_eq!(itoa.package_name, "itoa");
    assert_eq!(itoa.raw_spec, "1.0.0");
    assert_eq!(itoa.table, "workspace.dependencies");

    let ryu = deps
        .iter()
        .find(|dep| dep.manifest_key == "my_ryu")
        .expect("my_ryu resolved");
    assert_eq!(ryu.package_name, "ryu");
    assert_eq!(ryu.raw_spec, "1.0.0");
    assert_eq!(ryu.table, "workspace.dependencies");
    assert!(ryu.has_alias("my_ryu"));
    assert!(ryu.has_alias("other_ryu"));
    assert!(ryu.matches_name("other_ryu"));
    validate_explicit_cargo_requests(
        std::slice::from_ref(&member),
        Some(root),
        &[CargoUpdateSelector {
            name: "other_ryu".to_string(),
            version_spec: None,
            explicit: true,
        }],
    )
    .expect("secondary inherited alias remains selectable");

    let edits: Vec<_> = deps
        .iter()
        .map(|dep| {
            (dep.manifest_path.clone(), dep.table, dep.manifest_key.clone(), "1.0.18".to_string())
        })
        .collect();
    apply_manifest_edits(&edits).unwrap();

    let updated_root = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let doc: toml::Table = toml::from_str(&updated_root).unwrap();
    let ws_deps = doc["workspace"]["dependencies"].as_table().unwrap();
    assert_eq!(ws_deps["itoa"].as_str(), Some("1.0.18"));
    assert_eq!(ws_deps["my_ryu"]["version"].as_str(), Some("1.0.18"));
    assert_eq!(ws_deps["my_ryu"]["package"].as_str(), Some("ryu"));
    assert_eq!(ws_deps.get("my_itoa"), None);
    assert_eq!(ws_deps.get("other_ryu"), None);
}
