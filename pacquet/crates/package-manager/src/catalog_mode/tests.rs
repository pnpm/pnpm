use super::{CatalogModeDep, CatalogVersionMismatchError, check_catalog_mode};
use miette::Diagnostic;
use pacquet_catalogs_types::Catalogs;
use pacquet_config::CatalogMode;
use pacquet_reporter::SilentReporter;

/// Build a [`Catalogs`] map from `(catalog name, [(alias, specifier)])`
/// tuples.
fn catalogs(entries: &[(&str, &[(&str, &str)])]) -> Catalogs {
    entries
        .iter()
        .map(|(name, deps)| {
            let catalog =
                deps.iter().map(|(alias, spec)| ((*alias).to_string(), (*spec).to_string()));
            ((*name).to_string(), catalog.collect())
        })
        .collect()
}

/// A freshly-added direct dependency (no previous specifier).
fn dep<'a>(alias: &'a str, bare_specifier: &'a str) -> CatalogModeDep<'a> {
    CatalogModeDep { alias, bare_specifier, prev_specifier: None }
}

#[test]
fn manual_mode_never_checks_the_catalog() {
    let catalogs = catalogs(&[("default", &[("is-positive", "1.0.0")])]);
    let deps = [dep("is-positive", "2.0.0")];
    let result =
        check_catalog_mode::<SilentReporter>(CatalogMode::Manual, &catalogs, &deps, "/repo");
    assert!(result.is_ok(), "manual mode is a no-op even on a mismatch: {result:?}");
}

#[test]
fn strict_errors_on_a_concrete_version_mismatch() {
    let catalogs = catalogs(&[("default", &[("is-positive", "1.0.0")])]);
    let deps = [dep("is-positive", "2.0.0")];
    let err = check_catalog_mode::<SilentReporter>(CatalogMode::Strict, &catalogs, &deps, "/repo")
        .expect_err("a concrete-vs-concrete mismatch must error under strict mode");
    assert_eq!(
        err,
        CatalogVersionMismatchError {
            catalog_dep: "is-positive@1.0.0".to_string(),
            wanted_dep: "is-positive@2.0.0".to_string(),
        },
    );
    assert_eq!(
        err.code().expect("error carries a diagnostic code").to_string(),
        "ERR_PNPM_CATALOG_VERSION_MISMATCH",
    );
}

/// The ported fix: a catalog entry that is a *range* must not crash the
/// comparison. It falls through to the strict mismatch error, never
/// reaching an exact-version parse of the range.
#[test]
fn strict_errors_when_the_catalog_entry_is_a_range() {
    let catalogs = catalogs(&[("default", &[("is-positive", "^2.0.0")])]);
    let deps = [dep("is-positive", "1.0.0")];
    let err = check_catalog_mode::<SilentReporter>(CatalogMode::Strict, &catalogs, &deps, "/repo")
        .expect_err("a range catalog entry must error, not panic, under strict mode");
    assert_eq!(
        err,
        CatalogVersionMismatchError {
            catalog_dep: "is-positive@^2.0.0".to_string(),
            wanted_dep: "is-positive@1.0.0".to_string(),
        },
    );
}

/// The symmetric case: the wanted specifier is the range (e.g.
/// `update --latest` rewrites `^<latest>`). Still a clean mismatch.
#[test]
fn strict_errors_when_the_wanted_specifier_is_a_range() {
    let catalogs = catalogs(&[("default", &[("is-positive", "1.0.0")])]);
    let deps = [dep("is-positive", "^2.0.0")];
    let err = check_catalog_mode::<SilentReporter>(CatalogMode::Strict, &catalogs, &deps, "/repo")
        .expect_err("a wanted range that disagrees with the catalog must error");
    assert_eq!(err.wanted_dep, "is-positive@^2.0.0");
    assert_eq!(err.catalog_dep, "is-positive@1.0.0");
}

#[test]
fn strict_allows_a_matching_concrete_version() {
    let catalogs = catalogs(&[("default", &[("is-positive", "1.0.0")])]);
    let deps = [dep("is-positive", "1.0.0")];
    let result =
        check_catalog_mode::<SilentReporter>(CatalogMode::Strict, &catalogs, &deps, "/repo");
    assert!(result.is_ok(), "an exact match agrees with the catalog: {result:?}");
}

#[test]
fn strict_allows_a_dependency_absent_from_the_catalog() {
    let catalogs = catalogs(&[("default", &[("is-negative", "1.0.0")])]);
    let deps = [dep("is-positive", "2.0.0")];
    let result =
        check_catalog_mode::<SilentReporter>(CatalogMode::Strict, &catalogs, &deps, "/repo");
    assert!(result.is_ok(), "no catalog entry means nothing to reconcile: {result:?}");
}

#[test]
fn strict_skips_runtime_specifiers() {
    let catalogs = catalogs(&[("default", &[("node", "1.0.0")])]);
    let deps = [dep("node", "runtime:22.0.0")];
    let result =
        check_catalog_mode::<SilentReporter>(CatalogMode::Strict, &catalogs, &deps, "/repo");
    assert!(result.is_ok(), "a runtime: specifier is never cataloged: {result:?}");
}

#[test]
fn strict_resolves_a_named_catalog_via_the_previous_specifier() {
    let catalogs = catalogs(&[
        ("default", &[("is-positive", "9.9.9")]),
        ("my-catalog", &[("is-positive", "1.0.0")]),
    ]);
    let deps = [CatalogModeDep {
        alias: "is-positive",
        bare_specifier: "2.0.0",
        prev_specifier: Some("catalog:my-catalog"),
    }];
    let err = check_catalog_mode::<SilentReporter>(CatalogMode::Strict, &catalogs, &deps, "/repo")
        .expect_err("the named catalog's entry must drive the comparison");
    assert_eq!(
        err.catalog_dep, "is-positive@1.0.0",
        "the mismatch is against the named catalog, not `default`",
    );
}

#[test]
fn prefer_warns_and_keeps_the_direct_version_on_mismatch() {
    use pacquet_reporter::{LogEvent, LogLevel, Reporter};
    use std::sync::Mutex;

    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }
    // The reporter sink is a process-global `static`; clear it so a prior
    // run in the same process can't leak events into this assertion.
    EVENTS.lock().unwrap().clear();

    let catalogs = catalogs(&[("default", &[("is-positive", "1.0.0")])]);
    let deps = [dep("is-positive", "2.0.0")];
    let result =
        check_catalog_mode::<RecordingReporter>(CatalogMode::Prefer, &catalogs, &deps, "/repo");
    assert!(
        result.is_ok(),
        "prefer mode keeps the direct version rather than erroring: {result:?}",
    );

    let events = EVENTS.lock().unwrap();
    let warning = events
        .iter()
        .find_map(|event| match event {
            LogEvent::Pnpm(log) if log.level == LogLevel::Warn => Some(log),
            _ => None,
        })
        .expect("prefer mode emits a warning on a mismatch");
    assert_eq!(
        warning.message,
        r#"Catalog version mismatch for "is-positive": using direct version "2.0.0" instead of catalog version "1.0.0"."#,
    );
    assert_eq!(warning.prefix, "/repo");
}
