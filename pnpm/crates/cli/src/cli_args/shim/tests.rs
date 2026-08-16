use super::{installed_shims, virtual_shim_package, virtual_shims};
use pnpm_cmd_shim::{Host as CmdShimHost, link_virtual_shims};
use std::{collections::HashMap, fs};
use tempfile::tempdir;

#[test]
fn a_generated_shim_names_the_package_it_stands_for() {
    let dir = tempdir().unwrap();
    link_virtual_shims::<CmdShimHost>("yarn", &["yarn", "yarnpkg"], dir.path())
        .expect("link the shims");

    let found: HashMap<String, String> = virtual_shims(dir.path()).collect();
    assert_eq!(
        found,
        HashMap::from([
            ("yarn".to_string(), "yarn".to_string()),
            ("yarnpkg".to_string(), "yarn".to_string()),
        ]),
    );
}

/// A bin whose shim points at an installed target is somebody else's —
/// removing it would break a global install.
#[test]
fn a_shim_with_a_real_target_is_not_reported() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("tsc"), "#!/bin/sh\n# cmd-shim-target=../typescript/bin/tsc\n")
        .unwrap();
    link_virtual_shims::<CmdShimHost>("yarn", &["yarn"], dir.path()).expect("link the shims");

    assert_eq!(installed_shims(dir.path(), "yarn"), ["yarn"]);
    assert!(virtual_shims(dir.path()).all(|(bin, _)| bin != "tsc"));
}

#[test]
fn only_the_named_package_s_shims_are_reported() {
    let dir = tempdir().unwrap();
    link_virtual_shims::<CmdShimHost>("yarn", &["yarn", "yarnpkg"], dir.path()).expect("link");
    link_virtual_shims::<CmdShimHost>("npm", &["npm", "npx"], dir.path()).expect("link");

    assert_eq!(installed_shims(dir.path(), "npm"), ["npm", "npx"]);
    assert_eq!(installed_shims(dir.path(), "typescript"), Vec::<String>::new());
}

/// The marker is the whole record, so a body that merely mentions one
/// must not be mistaken for a shim pnpm generated.
#[test]
fn a_body_that_only_mentions_the_marker_is_not_a_shim() {
    assert_eq!(virtual_shim_package("echo '# cmd-shim-target=pkg:yarn'\n"), None);
}

/// A globally installed package manager opts into project-aware
/// dispatch, but only when the user has not already decided for it.
#[test]
fn installing_a_package_manager_globally_records_the_opt_in() {
    use super::policy::{record_package_manager_shims, recorded_entries};
    use pnpm_config::{Config, NamedShimPolicy, ShimPolicyValue};

    let dir = tempdir().unwrap();
    let config = Config { config_dir: Some(dir.path().to_path_buf()), ..Config::default() };

    let added = record_package_manager_shims(&config, ["yarn", "typescript"]).expect("record");
    assert_eq!(added.into_iter().collect::<Vec<_>>(), ["yarn"]);
    let entries = recorded_entries(dir.path()).expect("read back");
    assert_eq!(entries.get("yarn"), Some(&ShimPolicyValue::Named(NamedShimPolicy::Auto)));
    // A package that is not a package manager is left to `pnpm shim add`.
    assert_eq!(entries.get("typescript"), None);

    // A decision already on record wins over the default.
    super::set_policy(&config, "npm", Some(ShimPolicyValue::Toggle(false))).expect("opt out");
    let added = record_package_manager_shims(&config, ["npm"]).expect("record");
    assert!(added.is_empty());
    assert_eq!(
        recorded_entries(dir.path()).expect("read back").get("npm"),
        Some(&ShimPolicyValue::Toggle(false)),
    );
}

/// Turning every shim off is a decision, so installing a package manager
/// globally must not quietly undo it.
#[test]
fn a_global_disable_is_not_undone_by_installing_a_package_manager() {
    use super::policy::record_package_manager_shims;
    use pnpm_config::Config;

    let dir = tempdir().unwrap();
    fs::write(dir.path().join("config.yaml"), "globalShims: false\n").unwrap();
    let config = Config { config_dir: Some(dir.path().to_path_buf()), ..Config::default() };

    let added = record_package_manager_shims(&config, ["yarn"]).expect("record");

    assert!(added.is_empty());
    assert_eq!(
        fs::read_to_string(dir.path().join("config.yaml")).unwrap().trim(),
        "globalShims: false",
    );
}

/// A package can publish a bin whose name carries an extension, and the
/// shim written for it is found by its marker like any other — `pnpm shim
/// ls` and `rm` would otherwise never see it.
#[test]
fn a_bin_name_with_an_extension_is_still_discovered() {
    let dir = tempdir().unwrap();
    link_virtual_shims::<CmdShimHost>("tool", &["tool.js"], dir.path()).expect("link the shim");

    assert_eq!(installed_shims(dir.path(), "tool"), ["tool.js"]);
}
