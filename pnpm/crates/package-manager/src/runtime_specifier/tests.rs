use super::{node_runtime_version_spec, runtime_save_specifier};

#[test]
fn node_runtime_version_spec_matches_only_node_runtime_specifiers() {
    assert_eq!(node_runtime_version_spec("node", "runtime:26"), Some("26"));
    assert_eq!(node_runtime_version_spec("node", "tool:26"), Some("26"));
    assert_eq!(node_runtime_version_spec("node", "runtime:"), Some(""));
    // A registry-range `node` specifier is owned by the npm resolver.
    assert_eq!(node_runtime_version_spec("node", "^26"), None);
    // Deno and bun echo the requested spec back, so they save verbatim.
    assert_eq!(node_runtime_version_spec("deno", "runtime:2"), None);
    assert_eq!(node_runtime_version_spec("deno", "tool:2"), None);
    assert_eq!(node_runtime_version_spec("bun", "runtime:latest"), None);
}

#[test]
fn runtime_save_specifier_writes_runtime_for_either_spelling() {
    assert_eq!(runtime_save_specifier("bun", "tool:1.3.0").as_deref(), Some("runtime:1.3.0"));
    assert_eq!(runtime_save_specifier("deno", "tool:^2").as_deref(), Some("runtime:^2"));
    assert_eq!(runtime_save_specifier("deno", "runtime:2").as_deref(), Some("runtime:2"));
    assert_eq!(runtime_save_specifier("bun", "^1.3.0"), None);
    assert_eq!(runtime_save_specifier("typescript", "tool:5"), None);
}
