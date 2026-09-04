use super::{
    LockedCrate, MANAGED_CONFIG, add_cargo_checksum, parse_lockfile, sparse_index_path,
    update_managed_config,
};
use pnpm_store_dir::StoreDir;
use std::{collections::HashMap, fs};

#[test]
fn parses_crates_io_packages_and_ignores_workspace_packages() {
    let lockfile = r#"
version = 4

[[package]]
name = "workspace-member"
version = "0.1.0"

[[package]]
name = "serde"
version = "1.0.228"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "9a8e94ea7f378bd32cbbd37198a4a91436180c5bb472411e48b5ec2e2124ae9e"
dependencies = [
 "serde_core",
]
"#;

    assert_eq!(
        parse_lockfile(lockfile).unwrap(),
        vec![LockedCrate {
            name: "serde".to_string(),
            version: "1.0.228".to_string(),
            checksum: "9a8e94ea7f378bd32cbbd37198a4a91436180c5bb472411e48b5ec2e2124ae9e"
                .to_string(),
        }],
    );
}

#[test]
fn rejects_non_crates_io_sources() {
    let lockfile = r#"
[[package]]
name = "private"
version = "1.0.0"
source = "registry+https://registry.example/index"
checksum = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
"#;

    let error = parse_lockfile(lockfile).unwrap_err().to_string();
    assert!(error.contains("crates.io-only proof of concept"), "{error}");
}

#[test]
fn crate_store_slots_are_grouped_by_name_version_and_content() {
    let package = LockedCrate {
        name: "serde".to_string(),
        version: "1.0.228".to_string(),
        checksum: "9a8e94ea7f378bd32cbbd37198a4a91436180c5bb472411e48b5ec2e2124ae9e".to_string(),
    };

    assert_eq!(
        package.store_slot(std::path::Path::new("store/v11")),
        std::path::Path::new("store/v11")
            .join("crates")
            .join("serde")
            .join("1.0.228")
            .join("9a8e94ea7f378bd32cbbd37198a4a91436180c5bb472411e48b5ec2e2124ae9e"),
    );
}

#[test]
fn appends_the_managed_config_without_changing_user_settings() {
    let existing = "[alias]\ncodecov = \"llvm-cov\"\n";
    let updated = update_managed_config(existing).unwrap();

    assert_eq!(updated, format!("{existing}\n{MANAGED_CONFIG}\n"));
}

#[test]
fn replaces_only_the_existing_managed_config() {
    let existing = "before\n# >>> pnpm-managed cargo sources >>>\nstale\n# <<< pnpm-managed cargo sources <<<\nafter\n";
    let updated = update_managed_config(existing).unwrap();

    assert_eq!(updated, format!("before\n{MANAGED_CONFIG}\nafter\n"));
}

#[test]
fn rejects_an_incomplete_managed_config() {
    let error =
        update_managed_config("# >>> pnpm-managed cargo sources >>>\n").unwrap_err().to_string();

    assert!(error.contains("incomplete"), "{error}");
}

#[test]
fn creates_the_cargo_checksum_manifest_from_cas_files() {
    let temp_dir = tempfile::tempdir().unwrap();
    let store_dir = StoreDir::from(temp_dir.path().join("store"));
    let (cargo_toml, _) = store_dir.write_cas_file(b"[package]\nname = \"demo\"\n", false).unwrap();
    let (source, _) = store_dir.write_cas_file(b"fn main() {}\n", false).unwrap();
    let mut cas_paths = HashMap::from([
        ("Cargo.toml".to_string(), cargo_toml),
        ("src/main.rs".to_string(), source),
    ]);

    add_cargo_checksum(
        &store_dir,
        &mut cas_paths,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .unwrap();

    let manifest_path = cas_paths.get(".cargo-checksum.json").unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(manifest_path).unwrap()).unwrap();
    assert_eq!(
        manifest["package"],
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    );
    assert_eq!(
        manifest["files"]["Cargo.toml"],
        "5f55e5180ed66d818f61920fd7b0205a164b782a105610f293acd5ec68d0eacb",
    );
    assert_eq!(
        manifest["files"]["src/main.rs"],
        "536e506bb90914c243a12b397b9a998f85ae2cbd9ba02dfd03a9e155ca5ca0f4",
    );
}

#[test]
fn maps_crate_names_to_sparse_index_paths() {
    assert_eq!(sparse_index_path("a").unwrap(), "1/a");
    assert_eq!(sparse_index_path("ab").unwrap(), "2/ab");
    assert_eq!(sparse_index_path("abc").unwrap(), "3/a/abc");
    assert_eq!(sparse_index_path("Serde_JSON").unwrap(), "se/rd/serde_json");
}
