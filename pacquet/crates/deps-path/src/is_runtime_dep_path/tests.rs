use super::is_runtime_dep_path;

#[test]
fn matches_pnpm_test_cases() {
    assert!(is_runtime_dep_path("node@runtime:20.1.0"));
    assert!(!is_runtime_dep_path("node@20.1.0"));
}

#[test]
fn matches_all_three_runtime_prefixes() {
    assert!(is_runtime_dep_path("node@runtime:22.0.0"));
    assert!(is_runtime_dep_path("bun@runtime:1.1.0"));
    assert!(is_runtime_dep_path("deno@runtime:1.46.0"));
}

#[test]
fn rejects_unrelated_runtimes_and_partials() {
    assert!(!is_runtime_dep_path("python@runtime:3.12.0"));
    // The prefix matcher requires a non-empty body so a stray
    // `node@runtime:` with nothing after the colon doesn't
    // masquerade as a runtime entry.
    assert!(!is_runtime_dep_path("node@runtime:"));
    // Anchored at the start of the string — a name that happens
    // to contain `node@runtime:` as a substring is not a match.
    assert!(!is_runtime_dep_path("@scope/node@runtime:1.0.0"));
}
