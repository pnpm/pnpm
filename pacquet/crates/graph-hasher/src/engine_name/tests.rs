use super::{detect_node_major, detect_node_version, engine_name, parse_node_version_output};
use pretty_assertions::assert_eq;

/// Format matches pnpm's `${platform};${arch};node${major}`
/// — required for the side-effects cache to interop.
#[test]
fn engine_name_matches_pnpm_format() {
    assert_eq!(engine_name(20, Some("darwin"), Some("arm64")), "darwin;arm64;node20");
    assert_eq!(engine_name(22, Some("linux"), Some("x64")), "linux;x64;node22");
    assert_eq!(engine_name(24, Some("win32"), Some("x64")), "win32;x64;node24");
}

#[test]
fn parse_node_version_handles_common_shapes() {
    assert_eq!(parse_node_version_output("v22.11.0"), Some(22));
    assert_eq!(parse_node_version_output("v20.18.1"), Some(20));
    assert_eq!(parse_node_version_output("18.20.4"), Some(18));
    assert_eq!(parse_node_version_output("v25.0.0-nightly"), Some(25));
    assert_eq!(parse_node_version_output(""), None);
    assert_eq!(parse_node_version_output("not a version"), None);
    assert_eq!(parse_node_version_output("v.broken"), None);
}

/// Defaults route through the host mapping. Just assert the
/// shape (three semicolon-separated parts ending in
/// `node<digits>`) — the exact OS/arch depends on where the
/// test is run.
#[test]
fn engine_name_host_default_has_expected_shape() {
    let name = engine_name(20, None, None);
    let parts: Vec<&str> = name.split(';').collect();
    assert_eq!(parts.len(), 3, "expected three parts, got {name:?}");
    assert!(parts[2].starts_with("node"), "third part must start with `node`: {name:?}");
    assert!(parts[2][4..].parse::<u32>().is_ok(), "node version must be numeric: {name:?}");
}

/// `node` is a hard prerequisite for the test suite — if it isn't on
/// `PATH` that's a test-env bug, so we `expect` rather than skip.
#[test]
fn detect_node_version_strips_leading_v() {
    let version = detect_node_version().expect("`node` must be on PATH for the test suite");
    assert!(!version.starts_with('v'), "leading `v` must be stripped: {version:?}");
    let major = version.split('.').next().expect("at least one component");
    assert!(major.parse::<u32>().is_ok(), "major must be numeric: {version:?}");
}

#[test]
fn detect_node_major_matches_detect_node_version_leading_component() {
    let major = detect_node_major().expect("`node` must be on PATH for the test suite");
    let version = detect_node_version().expect("`node` must be on PATH for the test suite");
    let leading: u32 =
        version.split('.').next().expect("non-empty version").parse().expect("major numeric");
    assert_eq!(major, leading);
}
