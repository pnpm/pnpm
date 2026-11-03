use super::pin_unchanged_deps;
use crate::cli_args::ecosystem_deps::{
    cargo_manifest_ops::DeclaredCargoDep, cargo_workspace_scope::tests::scope_with,
};
use pnpm_package_manifest::DependencyGroup;
use semver::{Version, VersionReq};
use std::{collections::HashMap, path::PathBuf};

fn declared(name: &str) -> DeclaredCargoDep {
    DeclaredCargoDep {
        alias: name.to_string(),
        package_name: name.to_string(),
        raw_spec: "1".to_string(),
        version_req: VersionReq::parse("1").unwrap(),
        group: DependencyGroup::Prod,
        manifest_path: PathBuf::from("Cargo.toml"),
        table: "workspace.dependencies",
    }
}

#[test]
fn unchanged_deps_are_pinned_except_updated_and_patched_crates() {
    let mut scope = scope_with(&[], &["patched"]);
    scope.declared = vec![declared("kept"), declared("updated"), declared("patched")];
    let locked = ["kept", "updated", "patched"]
        .map(|name| (name.to_string(), vec![Version::parse("1.2.0").unwrap()]))
        .into_iter()
        .collect::<HashMap<_, _>>();
    let updated = declared("updated");

    let pins = pin_unchanged_deps(&scope, &locked, &[&updated]);

    assert_eq!(
        pins,
        [(
            PathBuf::from("Cargo.toml"),
            "workspace.dependencies",
            "kept".to_string(),
            "=1.2.0".to_string(),
        )],
    );
}
