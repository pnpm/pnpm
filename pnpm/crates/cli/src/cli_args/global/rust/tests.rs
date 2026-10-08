use super::with_tools;
use pnpm_global::{GlobalTool, ListReportAs};
use pretty_assertions::assert_eq;
use std::path::PathBuf;

fn rust() -> Vec<GlobalTool> {
    vec![GlobalTool {
        name: "rust".to_string(),
        version: "1.95.0".to_string(),
        location: PathBuf::from("/store/rust/1.95.0"),
    }]
}

#[test]
fn a_deep_listing_keeps_its_tree_and_lists_the_toolchain() {
    assert_eq!(
        with_tools("tree\n".to_string(), &rust(), &[], ListReportAs::Tree),
        "tree\n\nrust@1.95.0",
    );
    assert_eq!(
        with_tools("/global/pkg\n".to_string(), &rust(), &[], ListReportAs::Parseable),
        "/global/pkg\n/store/rust/1.95.0",
    );
    let json = with_tools(
        r#"[{"path":"/global","dependencies":{"typescript":{"version":"5.9.3"}}}]"#.to_string(),
        &rust(),
        &[],
        ListReportAs::Json,
    );
    let json: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(json[0]["dependencies"]["typescript"]["version"], "5.9.3");
    assert_eq!(json[0]["dependencies"]["rust"]["version"], "1.95.0");
}

#[test]
fn params_select_the_toolchain_as_wildcards() {
    assert_eq!(
        with_tools("tree".to_string(), &rust(), &["rus*".to_string()], ListReportAs::Tree),
        "tree\n\nrust@1.95.0",
    );
    assert_eq!(
        with_tools("tree".to_string(), &rust(), &["typescript".to_string()], ListReportAs::Tree),
        "tree",
    );
}
