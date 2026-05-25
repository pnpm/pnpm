use super::{
    RunError, get_specified_scripts, print_project_commands, throw_or_filter_hidden_scripts,
};
use serde_json::json;

#[test]
fn specified_scripts_exact_match() {
    let manifest = json!({ "scripts": { "build": "tsc", "test": "jest" } });
    assert_eq!(get_specified_scripts(&manifest, "build"), vec!["build".to_string()]);
    assert_eq!(get_specified_scripts(&manifest, "test"), vec!["test".to_string()]);
}

#[test]
fn specified_scripts_start_fallback() {
    // `start` is selectable even without a declared script.
    let manifest = json!({ "scripts": { "build": "tsc" } });
    assert_eq!(get_specified_scripts(&manifest, "start"), vec!["start".to_string()]);
}

#[test]
fn specified_scripts_missing_is_empty() {
    let manifest = json!({ "scripts": { "build": "tsc" } });
    assert!(get_specified_scripts(&manifest, "nonexistent").is_empty());
}

#[test]
fn hidden_filter_passes_visible_scripts() {
    let scripts = vec!["build".to_string()];
    assert_eq!(throw_or_filter_hidden_scripts(scripts.clone(), "build").unwrap(), scripts);
}

#[test]
fn hidden_filter_rejects_exact_hidden_request() {
    let scripts = vec![".secret".to_string()];
    let err = throw_or_filter_hidden_scripts(scripts, ".secret").unwrap_err();
    assert!(matches!(err, RunError::HiddenScript { .. }), "got {err:?}");
}

#[test]
fn print_commands_groups_lifecycle_and_other() {
    let manifest = json!({
        "scripts": { "test": "jest", "build": "tsc", ".hidden": "secret" },
    });
    let output = print_project_commands(&manifest);
    assert!(output.contains("Lifecycle scripts:"), "lifecycle header present:\n{output}");
    assert!(output.contains("  test\n    jest"), "test under lifecycle:\n{output}");
    assert!(
        output.contains("Commands available via \"pnpm run\":"),
        "other header present:\n{output}",
    );
    assert!(output.contains("  build\n    tsc"), "build under other:\n{output}");
    assert!(!output.contains("hidden"), "hidden scripts are omitted:\n{output}");
}

#[test]
fn print_commands_empty_when_no_scripts() {
    let manifest = json!({ "name": "x" });
    assert_eq!(print_project_commands(&manifest), "There are no scripts specified.");
}
