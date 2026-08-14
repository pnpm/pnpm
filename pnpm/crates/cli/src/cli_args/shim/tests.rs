use super::{installed_shims, virtual_shim_package, virtual_shims};
use pacquet_cmd_shim::{Host as CmdShimHost, link_virtual_shims};
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
