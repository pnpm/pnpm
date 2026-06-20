use super::{
    CatalogDecision, CatalogEntry, CatalogModeDep, CatalogVersionMismatchError, decide_catalog,
};
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

fn decide(
    mode: CatalogMode,
    catalogs: &Catalogs,
    dep: &CatalogModeDep<'_>,
) -> Result<CatalogDecision, CatalogVersionMismatchError> {
    decide_catalog::<SilentReporter>(mode, None, catalogs, dep, "/repo")
}

#[test]
fn manual_mode_keeps_the_direct_version() {
    let catalogs = catalogs(&[("default", &[("is-positive", "1.0.0")])]);
    let decision = decide(CatalogMode::Manual, &catalogs, &dep("is-positive", "2.0.0")).unwrap();
    assert_eq!(decision, CatalogDecision::KeepDirect, "manual mode never catalogs");
}

#[test]
fn strict_errors_on_a_concrete_version_mismatch() {
    let catalogs = catalogs(&[("default", &[("is-positive", "1.0.0")])]);
    let err = decide(CatalogMode::Strict, &catalogs, &dep("is-positive", "2.0.0"))
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

#[test]
fn strict_errors_when_the_catalog_entry_is_a_range() {
    let catalogs = catalogs(&[("default", &[("is-positive", "^2.0.0")])]);
    let err = decide(CatalogMode::Strict, &catalogs, &dep("is-positive", "1.0.0"))
        .expect_err("a range catalog entry must error, not panic, under strict mode");
    assert_eq!(
        err,
        CatalogVersionMismatchError {
            catalog_dep: "is-positive@^2.0.0".to_string(),
            wanted_dep: "is-positive@1.0.0".to_string(),
        },
    );
}

#[test]
fn strict_errors_when_the_wanted_specifier_is_a_range() {
    let catalogs = catalogs(&[("default", &[("is-positive", "1.0.0")])]);
    let err = decide(CatalogMode::Strict, &catalogs, &dep("is-positive", "^2.0.0"))
        .expect_err("a wanted range that disagrees with the catalog must error");
    assert_eq!(err.wanted_dep, "is-positive@^2.0.0");
    assert_eq!(err.catalog_dep, "is-positive@1.0.0");
}

#[test]
fn strict_uses_the_catalog_on_a_matching_concrete_version() {
    let catalogs = catalogs(&[("default", &[("is-positive", "1.0.0")])]);
    let decision = decide(CatalogMode::Strict, &catalogs, &dep("is-positive", "1.0.0")).unwrap();
    assert_eq!(
        decision,
        CatalogDecision::Catalog {
            manifest_specifier: "catalog:".to_string(),
            updated_entry: None
        },
        "an exact match reuses the existing catalog entry",
    );
}

#[test]
fn strict_catalogs_a_dependency_absent_from_the_catalog() {
    let catalogs = catalogs(&[("default", &[("is-negative", "1.0.0")])]);
    let decision = decide(CatalogMode::Strict, &catalogs, &dep("is-positive", "2.0.0")).unwrap();
    assert_eq!(
        decision,
        CatalogDecision::Catalog {
            manifest_specifier: "catalog:".to_string(),
            updated_entry: Some(CatalogEntry {
                catalog_name: "default".to_string(),
                specifier: "2.0.0".to_string(),
            }),
        },
        "a dependency with no catalog entry yet is added to the default catalog",
    );
}

#[test]
fn strict_skips_runtime_specifiers() {
    let catalogs = catalogs(&[("default", &[("node", "1.0.0")])]);
    let decision = decide(CatalogMode::Strict, &catalogs, &dep("node", "runtime:22.0.0")).unwrap();
    assert_eq!(decision, CatalogDecision::KeepDirect, "a runtime: specifier is never cataloged");
}

#[test]
fn reinstalling_a_catalog_dependency_reuses_the_existing_entry() {
    let catalogs = catalogs(&[("default", &[("is-positive", "^1.0.0")])]);
    let dep = CatalogModeDep {
        alias: "is-positive",
        bare_specifier: "catalog:",
        prev_specifier: Some("catalog:"),
    };
    let decision = decide(CatalogMode::Strict, &catalogs, &dep).unwrap();
    assert_eq!(
        decision,
        CatalogDecision::Catalog {
            manifest_specifier: "catalog:".to_string(),
            updated_entry: None
        },
        "re-adding a `catalog:` dependency keeps the catalog entry untouched",
    );
}

#[test]
fn strict_resolves_a_named_catalog_via_the_previous_specifier() {
    let catalogs = catalogs(&[
        ("default", &[("is-positive", "9.9.9")]),
        ("my-catalog", &[("is-positive", "1.0.0")]),
    ]);
    let dep = CatalogModeDep {
        alias: "is-positive",
        bare_specifier: "2.0.0",
        prev_specifier: Some("catalog:my-catalog"),
    };
    let err = decide(CatalogMode::Strict, &catalogs, &dep)
        .expect_err("the named catalog's entry must drive the comparison");
    assert_eq!(
        err.catalog_dep, "is-positive@1.0.0",
        "the mismatch is against the named catalog, not `default`",
    );
}

#[test]
fn save_catalog_name_catalogs_even_in_manual_mode() {
    let catalogs = catalogs(&[]);
    let decision = decide_catalog::<SilentReporter>(
        CatalogMode::Manual,
        Some("default"),
        &catalogs,
        &dep("is-positive", "1.0.0"),
        "/repo",
    )
    .unwrap();
    assert_eq!(
        decision,
        CatalogDecision::Catalog {
            manifest_specifier: "catalog:".to_string(),
            updated_entry: Some(CatalogEntry {
                catalog_name: "default".to_string(),
                specifier: "1.0.0".to_string(),
            }),
        },
        "--save-catalog-name engages cataloging even under manual mode",
    );
}

#[test]
fn save_catalog_name_targets_a_named_catalog() {
    let catalogs = catalogs(&[]);
    let decision = decide_catalog::<SilentReporter>(
        CatalogMode::Manual,
        Some("frontend"),
        &catalogs,
        &dep("is-positive", "1.0.0"),
        "/repo",
    )
    .unwrap();
    assert_eq!(
        decision,
        CatalogDecision::Catalog {
            manifest_specifier: "catalog:frontend".to_string(),
            updated_entry: Some(CatalogEntry {
                catalog_name: "frontend".to_string(),
                specifier: "1.0.0".to_string(),
            }),
        },
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
    let decision = decide_catalog::<RecordingReporter>(
        CatalogMode::Prefer,
        None,
        &catalogs,
        &dep("is-positive", "2.0.0"),
        "/repo",
    )
    .unwrap();
    assert_eq!(
        decision,
        CatalogDecision::KeepDirect,
        "prefer mode keeps the direct version rather than erroring",
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
