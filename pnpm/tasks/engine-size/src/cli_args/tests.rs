use super::CliArgs;
use clap::Parser;
use pretty_assertions::assert_eq;
use std::path::Path;

#[test]
fn runs_rejects_zero() {
    let error = CliArgs::try_parse_from(["engine-size", "--runs", "0"]).unwrap_err();
    assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
}

#[test]
fn runs_accepts_one() {
    let args = CliArgs::try_parse_from(["engine-size", "--runs", "1"]).unwrap();
    assert_eq!(args.runs, 1);
}

#[test]
fn target_dir_resolves_a_relative_path_against_the_current_dir() {
    let args = CliArgs::try_parse_from(["engine-size", "--target-dir", "target/probe"]).unwrap();
    let expected = std::env::current_dir().unwrap().join("target/probe");
    assert_eq!(args.target_dir().unwrap(), expected);
    assert!(Path::new(&expected).is_absolute());
}
