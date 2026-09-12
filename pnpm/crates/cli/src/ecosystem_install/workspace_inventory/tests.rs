use super::{EcosystemManifest, EcosystemWorkspaceInventory};
use std::fs;

#[tokio::test]
async fn applies_negations_to_native_discovery_without_using_npm_include_patterns() {
    let workspace = tempfile::tempdir().unwrap();
    for directory in ["rust", "fixtures/excluded"] {
        fs::create_dir_all(workspace.path().join(directory)).unwrap();
        fs::write(workspace.path().join(directory).join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(workspace.path().join(directory).join("pyproject.toml"), "[project]\n").unwrap();
    }
    let config = pnpm_config::Config {
        workspace_package_patterns: Some(vec!["npm/*".to_string(), "!fixtures/**".to_string()]),
        ..Default::default()
    };
    let inventory = EcosystemWorkspaceInventory::new(workspace.path().to_path_buf(), &config);
    assert_eq!(
        inventory.manifests(EcosystemManifest::Cargo).await.unwrap(),
        [workspace.path().join("rust/Cargo.toml")],
    );
    assert_eq!(
        inventory.manifests(EcosystemManifest::Python).await.unwrap(),
        [workspace.path().join("rust/pyproject.toml")],
    );
}

#[tokio::test]
async fn exposes_cargo_manifests_from_the_shared_inventory() {
    let workspace = tempfile::tempdir().unwrap();
    let cargo_project = workspace.path().join("rust");
    fs::create_dir_all(&cargo_project).unwrap();
    fs::write(cargo_project.join("Cargo.toml"), "[workspace]\n").unwrap();
    let inventory = EcosystemWorkspaceInventory::new(
        workspace.path().to_path_buf(),
        &pnpm_config::Config::default(),
    );

    let first = inventory.manifests(EcosystemManifest::Cargo).await.unwrap();
    let second = inventory.manifests(EcosystemManifest::Cargo).await.unwrap();

    assert_eq!(first, [cargo_project.join("Cargo.toml")]);
    assert!(std::ptr::eq(first, second));
}
