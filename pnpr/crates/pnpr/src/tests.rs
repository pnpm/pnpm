use super::{Args, Command, RegistryError, redacted_report};
use clap::Parser as _;

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

#[test]
fn parses_revision_backfill_dry_run() {
    let args =
        Args::try_parse_from(["pnpr", "--storage", "/tmp/pnpr", "backfill-revisions", "--dry-run"])
            .unwrap();

    assert!(matches!(args.command, Some(Command::BackfillRevisions { dry_run: true })));
    assert_eq!(args.storage.as_deref(), Some(std::path::Path::new("/tmp/pnpr")));
}
