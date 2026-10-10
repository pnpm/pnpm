use super::node_runtime_version_spec;

#[test]
fn node_runtime_version_spec_matches_only_node_runtime_specifiers() {
    assert_eq!(node_runtime_version_spec("node", "runtime:26"), Some(("runtime:", "26")));
    assert_eq!(node_runtime_version_spec("node", "tool:26"), Some(("tool:", "26")));
    assert_eq!(node_runtime_version_spec("node", "runtime:"), Some(("runtime:", "")));
    // A registry-range `node` specifier is owned by the npm resolver.
    assert_eq!(node_runtime_version_spec("node", "^26"), None);
    // Deno and bun echo the requested spec back, so they save verbatim.
    assert_eq!(node_runtime_version_spec("deno", "runtime:2"), None);
    assert_eq!(node_runtime_version_spec("deno", "tool:2"), None);
    assert_eq!(node_runtime_version_spec("bun", "runtime:latest"), None);
}
