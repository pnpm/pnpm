use super::*;

#[test]
fn startup_error_report_redacts_dsn_credentials() {
    let err = RegistryError::Internal {
        reason: "startup failed for postgres://admin:secret@[::1]/pnpr?sslmode=require".to_string(),
    };
    let report = redacted_report(&err).to_string();

    assert!(report.contains("postgres://redacted@[::1]/pnpr?sslmode=require"));
    assert!(!report.contains("admin"));
    assert!(!report.contains("secret"));
}
