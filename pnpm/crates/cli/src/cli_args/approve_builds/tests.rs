use super::{ApproveBuildsError, partition_params, sort_unique};

fn pending(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_string()).collect()
}

fn params(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_string()).collect()
}

#[test]
fn splits_approved_and_denied() {
    let (approved, denied) =
        partition_params(&params(&["foo", "!bar"]), &pending(&["foo", "bar"])).unwrap();
    assert_eq!(approved, vec!["foo".to_string()]);
    assert_eq!(denied, vec!["bar".to_string()]);
}

#[test]
fn accepts_unknown_approved_package() {
    let (approved, denied) = partition_params(&params(&["nope"]), &pending(&["foo"])).unwrap();
    assert_eq!(approved, vec!["nope".to_string()]);
    assert!(denied.is_empty());
}

#[test]
fn accepts_unknown_denied_package() {
    let (approved, denied) = partition_params(&params(&["!nope"]), &pending(&["foo"])).unwrap();
    assert!(approved.is_empty());
    assert_eq!(denied, vec!["nope".to_string()]);
}

// Ports pnpm's `contradictory arguments throw error`.
#[test]
fn rejects_contradictory_arguments() {
    let err = partition_params(&params(&["foo", "!foo"]), &pending(&["foo"])).unwrap_err();
    let ApproveBuildsError::ContradictingArgs(names) = err else {
        panic!("expected ContradictingArgs, got {err:?}");
    };
    assert_eq!(names, vec!["foo".to_string()]);
}

#[test]
fn sort_unique_dedupes_and_sorts() {
    assert_eq!(sort_unique(params(&["b", "a", "b"])), vec!["a".to_string(), "b".to_string()]);
}
