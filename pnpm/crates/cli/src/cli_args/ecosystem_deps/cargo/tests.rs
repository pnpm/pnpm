use super::LockedVersions;
use crate::cli_args::ecosystem_deps::{
    cargo_manifest_ops::DeclaredCargoDep, cargo_workspace_scope::tests::scope_with,
};
use pnpm_package_manifest::DependencyGroup;
use semver::{Version, VersionReq};
use std::{collections::HashMap, path::PathBuf};

fn lib() -> DeclaredCargoDep {
    DeclaredCargoDep {
        alias: "lib".to_string(),
        package_name: "lib".to_string(),
        raw_spec: "1".to_string(),
        version_req: VersionReq::parse("1").unwrap(),
        group: DependencyGroup::Prod,
        manifest_path: PathBuf::from("Cargo.toml"),
        table: "workspace.dependencies",
    }
}

#[test]
fn a_git_entry_is_the_current_version_only_of_a_patched_crate() {
    let registry_version = Version::parse("1.0.0").unwrap();
    let git_version = Version::parse("1.5.0").unwrap();
    let locked = LockedVersions {
        registry: HashMap::from([("lib".to_string(), vec![registry_version.clone()])]),
        git: HashMap::from([("lib".to_string(), vec![git_version.clone()])]),
    };

    assert_eq!(locked.current(&scope_with(&[], &[]), &lib()), Some(&registry_version));
    assert_eq!(locked.current(&scope_with(&[], &["lib"]), &lib()), Some(&git_version));
}
