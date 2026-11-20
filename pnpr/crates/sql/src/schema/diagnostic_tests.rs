#[test]
fn future_schema_requires_an_upgrade() {
    let error =
        super::refused_error("libsql://database.example.test", &super::Stored::FutureVersion(999));
    assert!(matches!(error, pnpr_error::RegistryError::InvalidConfig { .. }));
    let message = error.to_string();
    assert!(message.contains("Upgrade pnpr"), "{message}");
    assert!(message.contains("schema version 999"), "{message}");
    assert!(message.contains("libsql://database.example.test"), "{message}");
}
