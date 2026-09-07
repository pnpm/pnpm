use pnpm_reporter::SilentReporter;

use super::{ApproveBuildsArgs, ApproveBuildsError, partition_params, sort_unique};

fn pending(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_string()).collect()
}

fn params(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_string()).collect()
}

fn args(packages: &[&str]) -> ApproveBuildsArgs {
    ApproveBuildsArgs { packages: params(packages), all: false, global: false }
}

fn approve_builds_error(report: miette::Report) -> ApproveBuildsError {
    report.downcast::<ApproveBuildsError>().expect("an approve-builds error")
}

#[test]
fn splits_approved_and_denied() {
    let partition = partition_params(&params(&["foo", "!bar"]), &pending(&["foo", "bar"]));
    assert_eq!(partition.approved, vec!["foo".to_string()]);
    assert_eq!(partition.denied, vec!["bar".to_string()]);
    assert!(partition.unknown.is_empty());
}

#[test]
fn reports_an_unknown_approved_package_as_pre_emptive() {
    let partition = partition_params(&params(&["nope"]), &pending(&["foo"]));
    assert_eq!(partition.approved, vec!["nope".to_string()]);
    assert!(partition.denied.is_empty());
    assert_eq!(partition.unknown, vec!["nope".to_string()]);
}

#[test]
fn reports_an_unknown_denied_package_as_pre_emptive() {
    let partition = partition_params(&params(&["!nope"]), &pending(&["foo"]));
    assert!(partition.approved.is_empty());
    assert_eq!(partition.denied, vec!["nope".to_string()]);
    assert_eq!(partition.unknown, vec!["nope".to_string()]);
}

// Ports pnpm's `contradictory arguments throw error`.
#[test]
fn rejects_contradictory_arguments() {
    let err = args(&["foo", "!foo"])
        .decide::<SilentReporter>(&pending(&["foo"]))
        .err()
        .expect("contradicting arguments are rejected");
    let ApproveBuildsError::ContradictingArgs(names) = approve_builds_error(err) else {
        panic!("expected ContradictingArgs");
    };
    assert_eq!(names, vec!["foo".to_string()]);
}

#[test]
fn rejects_an_argument_that_names_no_package() {
    for packages in [&["!"][..], &[""][..], &["foo", "!"][..]] {
        let err = args(packages).validate().unwrap_err();
        assert!(
            matches!(approve_builds_error(err), ApproveBuildsError::MissingPackage),
            "expected MissingPackage for {packages:?}",
        );
    }
}

#[test]
fn rejects_positional_arguments_with_all() {
    let err = ApproveBuildsArgs { packages: params(&["foo"]), all: true, global: false }
        .validate()
        .unwrap_err();
    assert!(matches!(approve_builds_error(err), ApproveBuildsError::AllWithArgs));
}

#[test]
fn sort_unique_dedupes_and_sorts() {
    assert_eq!(sort_unique(params(&["b", "a", "b"])), vec!["a".to_string(), "b".to_string()]);
}
