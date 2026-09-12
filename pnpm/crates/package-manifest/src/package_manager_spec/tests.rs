use super::{
    dev_engines_package_managers, engine_name_version, is_version_request, split_spec,
    version_without_build,
};
use serde_json::json;

#[test]
fn a_scoped_name_keeps_its_own_at_sign() {
    assert_eq!(split_spec("yarn@4.9.2"), ("yarn", Some("4.9.2")));
    assert_eq!(split_spec("@scope/pm@1.2.3"), ("@scope/pm", Some("1.2.3")));
    assert_eq!(split_spec("yarn"), ("yarn", None));
    assert_eq!(split_spec("@scope/pm"), ("@scope/pm", None));
}

/// The separator is the first `@`, so a reference holding one of its own
/// arrives whole.
#[test]
fn a_reference_holding_an_at_sign_stays_intact() {
    assert_eq!(
        split_spec("pnpm@https://user@example.test/pnpm.tgz"),
        ("pnpm", Some("https://user@example.test/pnpm.tgz")),
    );
}

#[test]
fn the_build_is_not_part_of_the_version() {
    assert_eq!(version_without_build("1.22.22+sha512.abc"), "1.22.22");
    assert_eq!(version_without_build("4.9.2"), "4.9.2");
}

#[test]
fn only_a_released_version_is_a_version_request() {
    for reference in ["4", "^4.9.2", ">=2 <5", "latest", "1.22.22+sha512.abc"] {
        assert!(is_version_request(reference), "{reference}");
    }
    for reference in [
        "npm:@yarnpkg/cli-dist@4.9.2",
        "yarnpkg/berry",
        "yarnpkg/berry#main",
        "https://example.test/yarn.js",
    ] {
        assert!(!is_version_request(reference), "{reference}");
    }
}

#[test]
fn dev_engines_reads_one_entry_or_a_list() {
    let single = json!({ "devEngines": { "packageManager": { "name": "yarn", "version": "4" } } });
    let names: Vec<_> =
        dev_engines_package_managers(&single).filter_map(engine_name_version).collect();
    assert_eq!(names, [("yarn", Some("4"))]);

    let list = json!({
        "devEngines": {
            "packageManager": [
                { "name": "pnpm", "version": "12" },
                { "name": "yarn" },
            ],
        },
    });
    let names: Vec<_> =
        dev_engines_package_managers(&list).filter_map(engine_name_version).collect();
    assert_eq!(names, [("pnpm", Some("12")), ("yarn", None)]);

    assert_eq!(dev_engines_package_managers(&json!({})).count(), 0);
}
