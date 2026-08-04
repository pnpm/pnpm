use super::{TargetKind, TargetSpec};
use std::str::FromStr;

#[test]
fn target_spec_pacquet_prefix() {
    let spec = TargetSpec::from_str("pacquet@main").unwrap();
    assert_eq!(spec.kind, TargetKind::Pacquet);
    assert_eq!(spec.rev, "main");
}

#[test]
fn target_spec_pnpm_prefix() {
    let spec = TargetSpec::from_str("pnpm@v9.0.0").unwrap();
    assert_eq!(spec.kind, TargetKind::Pnpm);
    assert_eq!(spec.rev, "v9.0.0");
}

#[test]
fn target_spec_pnpr_prefix() {
    let spec = TargetSpec::from_str("pnpr@main").unwrap();
    assert_eq!(spec.kind, TargetKind::Pnpr);
    assert_eq!(spec.rev, "main");
}

#[test]
fn target_spec_unprefixed_is_rejected() {
    let err = TargetSpec::from_str("HEAD").unwrap_err();
    assert!(err.contains("`pacquet@<rev>`, `pnpm@<rev>`, or `pnpr@<rev>`"), "err = {err}");
}

#[test]
fn target_spec_unknown_prefix_is_rejected() {
    let err = TargetSpec::from_str("yarn@main").unwrap_err();
    assert!(err.contains("unknown kind"), "err = {err}");
}

#[test]
fn target_spec_empty_rev_is_rejected() {
    let err = TargetSpec::from_str("pacquet@").unwrap_err();
    assert!(err.contains("<rev> must not be empty"), "err = {err}");
    let err = TargetSpec::from_str("pnpm@").unwrap_err();
    assert!(err.contains("<rev> must not be empty"), "err = {err}");
    let err = TargetSpec::from_str("pnpr@").unwrap_err();
    assert!(err.contains("<rev> must not be empty"), "err = {err}");
}
