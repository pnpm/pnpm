use super::{
    VirtualShimPublication, installed_shims, publish_virtual_shims, record_virtual_shim_state,
    remove_virtual_shim_state, virtual_shim_bins_to_restore, virtual_shim_owner,
    virtual_shim_state_path, virtual_shims,
};
use crate::shim_dispatch::{ShimTarget, native_shim::install_native_shim_from};
use std::{collections::HashMap, fs, path::Path};
use tempfile::tempdir;

/// Link the target-less shims of `package` from a stand-in executable.
fn link_virtual_shims(package: &str, bins: &[&str], bin_dir: &Path) {
    let source = bin_dir.join(".stand-in-executable");
    fs::create_dir_all(bin_dir).unwrap();
    fs::write(&source, b"stand-in executable").unwrap();
    for bin in bins {
        install_native_shim_from(&source, bin_dir, bin, &ShimTarget::Virtual(package.to_string()))
            .expect("link the shim");
    }
}

#[test]
fn a_generated_shim_names_the_package_it_stands_for() {
    let dir = tempdir().unwrap();
    link_virtual_shims("yarn", &["yarn", "yarnpkg"], dir.path());

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
    link_virtual_shims("yarn", &["yarn"], dir.path());

    assert_eq!(installed_shims(dir.path(), "yarn"), ["yarn"]);
    assert!(virtual_shims(dir.path()).all(|(bin, _)| bin != "tsc"));
}

#[test]
fn only_the_named_package_s_shims_are_reported() {
    let dir = tempdir().unwrap();
    link_virtual_shims("yarn", &["yarn", "yarnpkg"], dir.path());
    link_virtual_shims("npm", &["npm", "npx"], dir.path());

    assert_eq!(installed_shims(dir.path(), "npm"), ["npm", "npx"]);
    assert_eq!(installed_shims(dir.path(), "typescript"), Vec::<String>::new());
}

#[test]
fn a_binary_in_the_global_bin_slot_is_not_a_virtual_shim() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("tool");
    fs::write(&path, [0xff, 0xfe]).unwrap();

    assert_eq!(virtual_shim_owner(&path).unwrap(), None);
}

#[test]
fn failed_publication_retains_intent_for_a_retry() {
    let root = tempdir().unwrap();
    let bin_dir = root.path().join("bin");
    let config_dir = root.path().join("config");
    let bin_path = bin_dir.join("tool");
    fs::create_dir_all(&bin_path).unwrap();
    let config = pnpm_config::Config { config_dir: Some(config_dir.clone()), ..Default::default() };
    let bins = vec!["tool".to_string()];
    let publication = || VirtualShimPublication {
        config: &config,
        bin_dir: &bin_dir,
        package: "tool",
        bins: &bins,
    };

    publish_virtual_shims(&publication()).expect_err("the occupied bin path should fail");
    assert!(bin_path.is_dir());
    assert_eq!(virtual_shim_bins_to_restore(&bin_dir, "tool").unwrap(), bins);
    assert!(fs::read_to_string(config_dir.join("config.yaml")).unwrap().contains("tool: auto"));

    fs::remove_dir(&bin_path).unwrap();
    publish_virtual_shims(&publication()).expect("retry publication");
    assert_eq!(virtual_shim_owner(&bin_path).unwrap().as_deref(), Some("tool"));
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
    link_virtual_shims("tool", &["tool.js"], dir.path());

    assert_eq!(installed_shims(dir.path(), "tool"), ["tool.js"]);
}

#[test]
fn restoration_state_round_trips_scoped_packages_and_bins() {
    let dir = tempdir().unwrap();
    let bins = vec!["tool".to_string(), "tool.js".to_string()];

    record_virtual_shim_state(dir.path(), "@scope/tool", &bins).expect("record state");

    assert_eq!(virtual_shim_bins_to_restore(dir.path(), "@scope/tool").expect("read state"), bins);
    let state_path = virtual_shim_state_path(dir.path(), "@scope/tool");
    assert_eq!(state_path.parent(), Some(dir.path()));

    remove_virtual_shim_state(dir.path(), "@scope/tool").expect("remove state");
    assert!(
        virtual_shim_bins_to_restore(dir.path(), "@scope/tool")
            .expect("read removed state")
            .is_empty(),
    );
}

#[test]
fn restoration_state_rejects_unsafe_bin_names() {
    let dir = tempdir().unwrap();
    let state_path = virtual_shim_state_path(dir.path(), "tool");
    fs::write(
        &state_path,
        serde_json::to_vec(&serde_json::json!({
            "package": "tool",
            "bins": ["../outside"],
        }))
        .unwrap(),
    )
    .unwrap();

    let error = virtual_shim_bins_to_restore(dir.path(), "tool").unwrap_err();
    let error = error.to_string();
    assert!(error.contains(r#"invalid bin name "../outside""#), "{error}");
}

#[test]
fn restoration_state_rejects_invalid_package_owners() {
    let dir = tempdir().unwrap();
    let state_path = virtual_shim_state_path(dir.path(), "../tool");
    fs::write(
        &state_path,
        serde_json::to_vec(&serde_json::json!({
            "package": "../tool",
            "bins": ["tool"],
        }))
        .unwrap(),
    )
    .unwrap();

    let error = virtual_shim_bins_to_restore(dir.path(), "../tool").unwrap_err().to_string();
    assert!(error.contains("invalid package owner"), "{error}");
}
