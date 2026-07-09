use super::{
    StaleConvergenceOverride, find_stale_convergence_overrides, stale_convergence_override_warning,
};
use node_semver::Version;
use pacquet_catalogs_types::Catalogs;
use pacquet_config_parse_overrides::{VersionOverride, parse_overrides};
use std::collections::{HashMap, HashSet};

fn converge_override(name: &str, value: &str) -> Vec<VersionOverride> {
    let input = HashMap::from([(format!("{name}@"), value.to_string())]);
    parse_overrides(&input, &Catalogs::new()).expect("parse_overrides fixture")
}

fn ranges(name: &str, declared: &[&str]) -> HashMap<String, HashSet<String>> {
    HashMap::from([(name.to_string(), declared.iter().map(|range| (*range).to_string()).collect())])
}

/// Canned per-range registry answers standing in for the resolver: a
/// missing entry models a range that fails to resolve.
fn canned(answers: &[(&str, &str)]) -> impl Fn(String, String) -> BestVersionFuture {
    let answers: HashMap<String, Version> = answers
        .iter()
        .map(|(range, version)| ((*range).to_string(), Version::parse(version).unwrap()))
        .collect();
    move |_name, range| {
        let version = answers.get(&range).cloned();
        Box::pin(async move { version })
    }
}

type BestVersionFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Option<Version>>>>;

#[tokio::test]
async fn reports_the_best_newer_version_admitted_by_every_range() {
    let overrides = converge_override("foo", "4.0.6");
    let declared = ranges("foo", &["^4.0.5", "^4.0.0"]);
    let stale = find_stale_convergence_overrides(
        &overrides,
        &declared,
        canned(&[("^4.0.5", "4.0.9"), ("^4.0.0", "4.0.9")]),
    )
    .await;

    assert_eq!(stale.len(), 1);
    let StaleConvergenceOverride { name, current_value, best } = &stale[0];
    assert_eq!(name, "foo");
    assert_eq!(current_value, "4.0.6");
    assert_eq!(best.to_string(), "4.0.9");
}

#[tokio::test]
async fn silent_when_no_candidate_satisfies_every_range() {
    let overrides = converge_override("foo", "4.0.6");
    let declared = ranges("foo", &["^4.0.5", "^3.0.0"]);
    let stale = find_stale_convergence_overrides(
        &overrides,
        &declared,
        canned(&[("^4.0.5", "4.0.9"), ("^3.0.0", "3.5.0")]),
    )
    .await;

    assert!(stale.is_empty(), "no single version converges ^4.0.5 and ^3.0.0");
}

#[tokio::test]
async fn silent_when_no_candidate_is_newer_than_the_override_value() {
    let overrides = converge_override("foo", "4.0.6");
    let declared = ranges("foo", &["^4.0.0"]);
    let stale =
        find_stale_convergence_overrides(&overrides, &declared, canned(&[("^4.0.0", "4.0.6")]))
            .await;

    assert!(stale.is_empty(), "the override already pins the best admitted version");
}

#[tokio::test]
async fn unresolved_range_stays_in_the_satisfies_check() {
    let overrides = converge_override("foo", "4.0.6");
    // `4.0.6` (an exact declared range) fails to resolve — it yields no
    // candidate, but the `^4.0.5` candidate must still satisfy it.
    let declared = ranges("foo", &["^4.0.5", "4.0.6"]);
    let stale =
        find_stale_convergence_overrides(&overrides, &declared, canned(&[("^4.0.5", "4.0.9")]))
            .await;

    assert!(stale.is_empty(), "4.0.9 does not satisfy the declared exact range 4.0.6");
}

#[tokio::test]
async fn silent_when_no_declared_range_was_collected() {
    let overrides = converge_override("foo", "4.0.6");
    let stale = find_stale_convergence_overrides(&overrides, &HashMap::new(), canned(&[])).await;

    assert!(stale.is_empty(), "nothing declared the package, so nothing can be converged");
}

#[test]
fn warning_message_matches_the_shared_wording() {
    let entry = StaleConvergenceOverride {
        name: "form-data".to_string(),
        current_value: "4.0.6".to_string(),
        best: Version::parse("4.0.9").unwrap(),
    };
    let message = stale_convergence_override_warning(&entry);
    eprintln!("MESSAGE:\n{message}\n");
    assert_eq!(
        message,
        r#"The convergence override "form-data@": "4.0.6" is stale: every declared range of form-data also admits 4.0.9. Change the override's value to 4.0.9 in pnpm-workspace.yaml, or remove the override and run "pnpm dedupe"."#,
    );
}
