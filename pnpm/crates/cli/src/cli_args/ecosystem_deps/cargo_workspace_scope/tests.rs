use super::CargoWorkspaceScope;
use crate::{
    cargo_deps::CargoWorkspaceDependency,
    cli_args::ecosystem_deps::cargo_manifest_ops::DeclaredCargoDep,
};
use pnpm_package_manifest::DependencyGroup;
use semver::VersionReq;
use std::{collections::HashMap, path::PathBuf};

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

#[test]
fn enabled_features_unite_registry_declarations_with_the_same_requirement() {
    let mut scope = CargoWorkspaceScope {
        root: PathBuf::new(),
        declared: Vec::new(),
        declarations: HashMap::new(),
    };
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
