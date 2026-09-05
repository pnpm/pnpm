use super::{EcosystemManifest, EcosystemWorkspaceInventory};
use std::fs;

#[tokio::test]
async fn exposes_cargo_manifests_from_the_shared_inventory() {
    let workspace = tempfile::tempdir().unwrap();
    let cargo_project = workspace.path().join("rust");
    fs::create_dir_all(&cargo_project).unwrap();
    fs::write(cargo_project.join("Cargo.toml"), "[workspace]\n").unwrap();
    let inventory = EcosystemWorkspaceInventory::new(workspace.path().to_path_buf());

    let first = inventory.manifests(EcosystemManifest::Cargo).await.unwrap();
    let second = inventory.manifests(EcosystemManifest::Cargo).await.unwrap();

    assert_eq!(first, [cargo_project.join("Cargo.toml")]);
    assert!(std::ptr::eq(first, second));
}
