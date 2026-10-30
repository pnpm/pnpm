use super::{CargoWorkspaceScope, RootManifest, read_root_manifest};
use crate::{
    cargo_deps::CargoWorkspaceDependency,
    cli_args::ecosystem_deps::cargo_manifest_ops::DeclaredCargoDep,
};
use pnpm_package_manifest::DependencyGroup;
use semver::VersionReq;
use std::{collections::BTreeSet, path::PathBuf};

const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";

fn declaration(req: &str, features: &[&str], source: Option<&str>) -> CargoWorkspaceDependency {
    CargoWorkspaceDependency {
        name: "hash".to_string(),
        rename: None,
        req: req.to_string(),
        features: features
            .iter()
            .map(ToString::to_string)
            .collect(),
        source: source.map(str::to_string),
        registry: None,
    }
}

fn declared(req: &str) -> DeclaredCargoDep {
    DeclaredCargoDep {
        alias: "hash".to_string(),
        package_name: "hash".to_string(),
        raw_spec: req.to_string(),
        version_req: VersionReq::parse(req).unwrap(),
        group: DependencyGroup::Prod,
        manifest_path: PathBuf::from("Cargo.toml"),
        table: "workspace.dependencies",
    }
}

/// A scope declaring each `(package name, alias)` with no features, and
/// with `patched` replaced through `[patch.crates-io]`.
pub(in crate::cli_args::ecosystem_deps) fn scope_with(
    aliases: &[(&str, &str)],
    patched: &[&str],
) -> CargoWorkspaceScope {
    CargoWorkspaceScope {
        root: PathBuf::new(),
        declared: Vec::new(),
        declarations: aliases
            .iter()
            .map(|(name, alias)| ((name.to_string(), alias.to_string()), Vec::new()))
            .collect(),
        root_manifest: RootManifest {
            patched: patched
                .iter()
                .map(ToString::to_string)
                .collect(),
            workspace_aliases: BTreeSet::new(),
        },
    }
}

#[test]
fn enabled_features_unite_registry_declarations_with_the_same_requirement() {
    let mut scope = scope_with(&[], &[]);
    let alternate = CargoWorkspaceDependency {
        registry: Some("https://alt.example/index".to_string()),
        ..declaration("^0.10.9", &["alternate"], Some("registry+https://alt.example/index"))
    };
    scope.add_declarations(vec![
        declaration("^0.10.9", &[], Some(CRATES_IO)),
        declaration("^0.10.9", &["asm"], Some(CRATES_IO)),
        declaration("^0.9", &["older"], Some(CRATES_IO)),
        declaration("^0.10.9", &["local"], None),
        alternate,
    ]);

    assert_eq!(scope.enabled_features(&declared("0.10.9")), ["asm"]);
}

#[test]
fn only_a_renamed_alias_of_a_package_declared_twice_has_a_sibling() {
    let mut scope = scope_with(&[], &[]);
    scope.add_declarations(vec![
        declaration("^0.5", &[], Some(CRATES_IO)),
        CargoWorkspaceDependency {
            rename: Some("hash-0-4".to_string()),
            ..declaration("^0.4", &[], Some(CRATES_IO))
        },
    ]);
    let renamed = DeclaredCargoDep { alias: "hash-0-4".to_string(), ..declared("0.4") };
    let lone_rename = DeclaredCargoDep {
        alias: "digest".to_string(),
        package_name: "hash-rs".to_string(),
        ..declared("0.4")
    };

    assert!(scope.has_sibling_alias(&renamed));
    assert!(!scope.has_sibling_alias(&declared("0.5")));
    assert!(!scope.has_sibling_alias(&lone_rename));
}

#[test]
fn the_root_manifest_yields_crates_io_patches_and_workspace_aliases() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path().join("Cargo.toml");
    std::fs::write(
        &manifest,
        concat!(
            "[workspace.dependencies]\n",
            "tower = \"0.5\"\n",
            "tower-0-4 = { package = \"tower\", version = \"0.4\" }\n",
            "local = { path = \"crates/local\" }\n",
            "\n[patch.crates-io]\n",
            "lib = { path = \"vendor/lib\" }\n",
            "fork = { package = \"upstream\", git = \"https://example.test/fork\" }\n",
            "\n[patch.\"https://github.com/rust-lang/crates.io-index\"]\n",
            "by-url = { path = \"vendor/by-url\" }\n",
            "\n[patch.\"https://example.test/index\"]\n",
            "other = { path = \"vendor/other\" }\n",
        ),
    )
    .unwrap();

    let root = read_root_manifest(&manifest).unwrap();

    assert_eq!(
        root.patched,
        BTreeSet::from(["by-url", "lib", "upstream"].map(ToString::to_string)),
    );
    assert_eq!(
        root.workspace_aliases,
        BTreeSet::from([("tower", "tower"), ("tower", "tower-0-4")].map(|(name, alias)| (
            name.to_string(),
            alias.to_string()
        )),),
    );
}

#[test]
fn an_uninherited_workspace_declaration_is_a_sibling() {
    let mut scope = scope_with(&[("hash", "hash-0-4")], &[]);
    scope.root_manifest.workspace_aliases.insert(("hash".to_string(), "hash".to_string()));
    let renamed = DeclaredCargoDep { alias: "hash-0-4".to_string(), ..declared("0.4") };

    assert!(scope.has_sibling_alias(&renamed));
}
