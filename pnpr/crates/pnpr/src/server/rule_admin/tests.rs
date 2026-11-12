use super::edited_ecosystem;
use pnpr_config::RegistryGraphDocument;
use pnpr_error::RegistryError;
use pnpr_registry::Ecosystem;

#[test]
fn a_registry_that_changed_ecosystem_is_a_conflict() {
    let document = RegistryGraphDocument::from_yaml(
        "registries:\n  local:\n    type: hosted\n    ecosystem: cargo\n",
    )
    .unwrap();
    assert_eq!(edited_ecosystem(&document, "local", "cargo").unwrap(), Ecosystem::Cargo);
    let error = edited_ecosystem(&document, "local", "npm").unwrap_err();
    assert!(matches!(error, RegistryError::AdminConflict { .. }), "{error:?}");
    assert!(matches!(edited_ecosystem(&document, "missing", "npm"), Err(RegistryError::NotFound),));
}
