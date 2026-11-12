use super::{HostedTarget, OwnerRecord, claim_ownership, restore_owners};
use crate::server::routing::state_and_router;
use pnpr_auth::AuthState;
use pnpr_cargo::CrateDocument;
use pnpr_error::RegistryError;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use pnpr_policy::Identity;
use std::path::Path;

fn state(dir: &Path) -> crate::server::AppState {
    let yaml = "\
storage: ./s
auth:
  admins: [root]
registries:
  crates:
    type: hosted
    ecosystem: cargo
    packages:
      demo: {}
";
    let path = dir.join("pnpr.yaml");
    std::fs::write(&path, yaml).unwrap();
    let config = crate::Config::from_yaml(&path, "127.0.0.1:0".parse().unwrap(), None).unwrap();
    state_and_router(config, AuthState::in_memory(), None).unwrap().0
}

fn claim(
    publisher: &str,
    document: &mut CrateDocument,
    record: OwnerRecord,
) -> Result<(), RegistryError> {
    let dir = tempfile::TempDir::new().unwrap();
    let state = state(dir.path());
    let publisher = Identity::user(publisher);
    let target = HostedTarget { namespace: "", registry: "crates", publisher: &publisher };
    let key = CanonicalPackageName::parse("demo", Ecosystem::Cargo).unwrap();
    claim_ownership(&state, &target, &key, document, record)
}

#[test]
fn the_first_publish_records_its_publisher_as_the_owner() {
    let mut document = CrateDocument::new("demo");
    claim("bob", &mut document, OwnerRecord::Unclaimed).unwrap();
    assert_eq!(document.owners, ["bob"]);
}

#[test]
fn a_stored_document_admits_only_its_owners_and_admins() {
    let mut document = CrateDocument::new("demo");
    document.owners = vec!["alice".to_string()];
    let refused = claim("bob", &mut document, OwnerRecord::Recorded).unwrap_err();
    assert!(matches!(refused, RegistryError::Forbidden { .. }), "{refused:?}");
    claim("alice", &mut document, OwnerRecord::Recorded).unwrap();
    claim("root", &mut document, OwnerRecord::Recorded).unwrap();
    assert_eq!(document.owners, ["alice"]);
}

#[test]
fn a_stored_document_without_owners_admits_only_admins() {
    let mut document = CrateDocument::new("demo");
    let refused = claim("bob", &mut document, OwnerRecord::Recorded).unwrap_err();
    assert!(matches!(refused, RegistryError::Forbidden { .. }), "{refused:?}");
    claim("root", &mut document, OwnerRecord::Recorded).unwrap();
    assert!(document.owners.is_empty());
}

#[tokio::test]
async fn a_removed_package_keeps_its_owner_record() {
    for owners in [vec![], vec!["alice".to_string()]] {
        let dir = tempfile::TempDir::new().unwrap();
        let state = state(dir.path());
        let storage = state.inner.storage.for_hosted("", Ecosystem::Cargo);
        let key = CanonicalPackageName::parse("demo", Ecosystem::Cargo).unwrap();
        let mut stored = CrateDocument::new("demo");
        stored.owners.clone_from(&owners);
        storage
            .write_hosted_document_if_current(&key, &stored.to_bytes(), None, None)
            .await
            .unwrap();
        let current = storage
            .read_hosted_document_for_update(&key)
            .await
            .unwrap()
            .unwrap();
        let removal = Some(&current.version);
        storage.write_hosted_document_if_current(&key, b"", removal, None).await.unwrap();

        let mut document = CrateDocument::new("demo");
        assert_eq!(
            restore_owners(&storage, &key, &mut document).await.unwrap(),
            Some(pnpr_storage::HostedDocumentVersion::Tombstone(owners.clone())),
            "{owners:?}",
        );
        assert_eq!(document.owners, owners);
    }
}
