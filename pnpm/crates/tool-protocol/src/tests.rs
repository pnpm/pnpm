use super::{is_tool_specifier, split_tool_protocol, strip_tool_protocol, tool_specifiers_equal};
use pretty_assertions::assert_eq;

#[test]
fn split_keeps_the_spelling() {
    assert_eq!(split_tool_protocol("tool:22"), Some(("tool:", "22")));
    assert_eq!(split_tool_protocol("runtime:^1.3.0"), Some(("runtime:", "^1.3.0")));
    assert_eq!(split_tool_protocol("tool:"), Some(("tool:", "")));
}

#[test]
fn other_protocols_are_not_tools() {
    for specifier in ["22", "npm:tool@1", "workspace:*", "file:tool:x", "tools:1"] {
        assert_eq!(strip_tool_protocol(specifier), None, "{specifier}");
        assert!(!is_tool_specifier(specifier), "{specifier}");
    }
}

#[test]
fn spellings_compare_equal() {
    assert!(tool_specifiers_equal("tool:22", "runtime:22"));
    assert!(tool_specifiers_equal("runtime:22", "tool:22"));
    assert!(tool_specifiers_equal("tool:22", "tool:22"));
    assert!(!tool_specifiers_equal("tool:22", "runtime:24"));
    assert!(!tool_specifiers_equal("tool:22", "22"));
}
