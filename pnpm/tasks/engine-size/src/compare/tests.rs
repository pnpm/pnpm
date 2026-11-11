use super::{SizeChange, compare_sizes};
use crate::cli_args::Lto;
use pretty_assertions::assert_eq;

#[test]
fn reports_absolute_and_percentage_changes_in_baseline_order() {
    let current = [(Lto::Fat, 120), (Lto::Thin, 75)];
    let baseline = [(Lto::Thin, 100), (Lto::Fat, 100)];

    assert_eq!(
        compare_sizes(&current, &baseline).unwrap(),
        [
            SizeChange {
                lto: Lto::Thin,
                baseline_bytes: 100,
                current_bytes: 75,
                change_bytes: -25,
                change_percent: -25.0,
            },
            SizeChange {
                lto: Lto::Fat,
                baseline_bytes: 100,
                current_bytes: 120,
                change_bytes: 20,
                change_percent: 20.0,
            },
        ],
    );
}

#[test]
fn rejects_a_missing_baseline_mode() {
    assert_eq!(
        compare_sizes(&[(Lto::Thin, 100)], &[(Lto::Fat, 100)]),
        Err("baseline and current reports must contain the same LTO modes".to_string()),
    );
}

#[test]
fn rejects_duplicate_modes() {
    assert_eq!(
        compare_sizes(&[(Lto::Thin, 100), (Lto::Thin, 101)], &[(Lto::Thin, 100)]),
        Err("each report must contain each LTO mode once".to_string()),
    );
}

#[test]
fn rejects_an_empty_baseline_size() {
    assert_eq!(
        compare_sizes(&[(Lto::Thin, 1)], &[(Lto::Thin, 0)]),
        Err("baseline executable size must be greater than zero".to_string()),
    );
}

#[test]
fn rejects_reports_without_measurements() {
    assert_eq!(
        compare_sizes(&[], &[]),
        Err("reports must contain at least one LTO mode".to_string()),
    );
}

#[test]
fn rejects_reports_for_different_targets() {
    let current =
        r#"{"target":"aarch64-apple-darwin","probe_features":"default","measurements":[]}"#;
    let baseline =
        r#"{"target":"x86_64-unknown-linux-gnu","probe_features":"default","measurements":[]}"#;
    assert_eq!(
        super::render_comparison(current, baseline),
        Err("cannot compare reports for different targets: x86_64-unknown-linux-gnu and aarch64-apple-darwin".to_string()),
    );
}

#[test]
fn comparison_uses_stripped_size_from_json_reports() {
    let current = r#"{"target":"x86_64-unknown-linux-gnu","probe_features":"no-default-features","measurements":[{"lto":"thin","stripped_size":90}]}"#;
    let baseline = r#"{"target":"x86_64-unknown-linux-gnu","probe_features":"default","measurements":[{"lto":"thin","stripped_size":100}]}"#;
    assert_eq!(
        super::render_comparison(current, baseline).unwrap(),
        "Baseline features: `default`; current features: `no-default-features`\n\n| LTO | Baseline | Current | Change | Change % |\n| --- | ---: | ---: | ---: | ---: |\n| thin | 100 | 90 | -10 | -10.00% |\n",
    );
}

#[test]
fn comparison_accepts_a_baseline_report_without_feature_metadata() {
    let current = r#"{"target":"x86_64-unknown-linux-gnu","probe_features":"no-default-features","measurements":[{"lto":"thin","stripped_size":90}]}"#;
    let baseline = r#"{"target":"x86_64-unknown-linux-gnu","measurements":[{"lto":"thin","stripped_size":100}]}"#;
    assert_eq!(
        super::render_comparison(current, baseline).unwrap(),
        "Baseline features: `unknown`; current features: `no-default-features`\n\n| LTO | Baseline | Current | Change | Change % |\n| --- | ---: | ---: | ---: | ---: |\n| thin | 100 | 90 | -10 | -10.00% |\n",
    );
}
