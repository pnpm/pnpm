use super::{
    PACKAGE_BODY, VersionSelectorEntry, VersionSelectorType, VersionSelectors, parse_cutoff,
    range_spec,
};
use crate::pick_package::version_pick::{PickerOpts, pick_matching_version_fast};

#[test]
fn fallback_preserves_the_first_pick_and_preferred_versions() {
    let mut document: serde_json::Value = serde_json::from_str(PACKAGE_BODY).unwrap();
    document["time"]["1.0.0"] = "2022-06-01T00:00:00Z".into();
    document["time"]["1.1.0"] = "2022-05-01T00:00:00Z".into();
    let mut later = document["versions"]["1.1.0"].clone();
    later["version"] = "1.2.0".into();
    document["versions"]["1.2.0"] = later;
    document["time"]["1.2.0"] = "2022-05-02T00:00:00Z".into();
    document["dist-tags"]["latest"] = "1.2.0".into();
    let meta = serde_json::from_value(document).unwrap();
    let preferred = VersionSelectors::from_iter([(
        "1.2.0".to_string(),
        VersionSelectorEntry::Plain(VersionSelectorType::Version),
    )]);
    for (cutoff, fallback, selectors, expected) in [
        ("2022-04-01", "2022-05-15", None, "1.1.0"),
        ("2022-04-01", "2022-05-15", Some(&preferred), "1.2.0"),
        ("2022-04-01", "2022-04-15", None, "1.0.0"),
        ("2022-05-15", "2022-06-15", None, "1.2.0"),
    ] {
        let opts = PickerOpts {
            preferred_version_selectors: selectors,
            published_by: Some(parse_cutoff(&format!("{cutoff}T00:00:00Z"))),
            fallback_published_by: Some(parse_cutoff(&format!("{fallback}T00:00:00Z"))),
            published_by_exclude: None,
            pick_lowest_version: false,
            include_latest_tag: false,
            ignore_missing_time_field: false,
        };
        let picked = pick_matching_version_fast(&opts, &range_spec("acme", "^1.0.0"), &meta)
            .expect("pick succeeds")
            .expect("matching version");
        assert_eq!(picked.version.to_string(), expected, "cutoff {cutoff}, fallback {fallback}");
    }
}
