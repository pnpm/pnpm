use super::{Args, RegistryError, redacted_report};
use clap::Parser as _;

#[test]
fn disable_artifacts_sets_the_config_override() {
    let args = Args::try_parse_from(["pnpr", "--disable-artifacts"]).unwrap();

    let overrides = args.feature_overrides();

    assert!(overrides.disable_artifacts);
    assert!(!overrides.disable_registry);
    assert!(!overrides.disable_resolver);
}

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
