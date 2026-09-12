use crate::add::specifier::node_runtime_version_spec;

#[test]
fn node_runtime_version_spec_matches_only_explicit_node_runtime_requests() {
    assert_eq!(node_runtime_version_spec("node", Some("runtime:26")), Some("26"));
    assert_eq!(node_runtime_version_spec("node", Some("runtime:")), Some(""));
    // A registry-range `node` request is owned by the npm resolver.
    assert_eq!(node_runtime_version_spec("node", Some("^26")), None);
    assert_eq!(node_runtime_version_spec("node", None), None);
    // Deno and bun echo the requested spec back, so they save verbatim.
    assert_eq!(node_runtime_version_spec("deno", Some("runtime:2")), None);
    assert_eq!(node_runtime_version_spec("bun", Some("runtime:latest")), None);
}
