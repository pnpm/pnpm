use pretty_assertions::assert_eq;

use super::{NodeVersion, filter_versions};

fn make_versions() -> Vec<NodeVersion> {
    vec![
        NodeVersion { version: "22.0.0".to_string(), lts: None },
        NodeVersion { version: "20.10.0".to_string(), lts: Some("Iron".to_string()) },
        NodeVersion { version: "20.5.0".to_string(), lts: None },
        NodeVersion { version: "18.18.0".to_string(), lts: Some("Hydrogen".to_string()) },
        NodeVersion { version: "16.20.0".to_string(), lts: Some("Gallium".to_string()) },
    ]
}

#[test]
fn lts_selector_picks_every_lts_release() {
    let (picked, range) = filter_versions(&make_versions(), "lts");
    assert_eq!(picked, vec!["20.10.0", "18.18.0", "16.20.0"]);
    assert_eq!(range, "*");
}

#[test]
fn lts_codename_is_case_insensitive() {
    let (picked, range) = filter_versions(&make_versions(), "iron");
    assert_eq!(picked, vec!["20.10.0"]);
    assert_eq!(range, "*");
}

#[test]
fn semver_range_passes_through() {
    let (picked, range) = filter_versions(&make_versions(), "^20");
    assert_eq!(picked.len(), 5);
    assert_eq!(range, "^20");
}
