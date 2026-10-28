use super::{
    DEP, FOO, PARENT, add_workspace_package, assert_eq, dep_spec, lockfile_package_keys, pacquet,
    setup_with_own_registry, write_manifest,
};
use assert_cmd::assert::OutputAssertExt;
use std::path::Path;

#[test]
fn latest_selectors_refresh_matching_transitive_dependencies() {
    for selectors in [vec!["!@pnpm.e2e/foo"], vec!["*", "!@pnpm.e2e/foo"], vec![PARENT, DEP]] {
        assert_transitive_update(&selectors, "100.1.0");
    }
}

#[test]
fn latest_wildcard_refreshes_transitive_dependencies() {
    assert_transitive_update(&["*"], "100.1.0");
}

#[test]
fn latest_exclusion_refreshes_transitive_dependencies_without_saving() {
    assert_transitive_update(&["!@pnpm.e2e/foo", "--no-save"], "100.1.0");
}

#[test]
fn latest_exclusion_refreshes_transitive_dependencies_with_explicit_depth() {
    assert_transitive_update(&["!@pnpm.e2e/foo", "--depth", "Infinity"], "100.1.0");
}

#[test]
fn latest_selectors_preserve_unmatched_transitive_dependencies() {
    for selectors in [vec![PARENT], vec!["*", "!@pnpm.e2e/dep-of-pkg-with-1-dep"]] {
        assert_transitive_update(&selectors, "100.0.0");
    }
}

#[test]
fn latest_selectors_preserve_transitive_dependencies_at_depth_zero() {
    assert_transitive_update(&["!@pnpm.e2e/foo", "--depth", "0"], "100.0.0");
}

fn assert_transitive_update(selectors: &[&str], child_version: &str) {
    for mode in
        [&[][..], &["--recursive"], &["--lockfile-only"], &["--recursive", "--lockfile-only"]]
    {
        let (root, workspace, anchor) = setup_with_own_registry();
        anchor.set_dist_tag(PARENT, "100.0.0", "latest");
        add_workspace_package(&workspace, "app", "1.0.0");
        let app = workspace.join("app");
        write_manifest(
            &app,
            &format!(r#"{{ "{PARENT}": "100.0.0", "{DEP}": "100.0.0", "{FOO}": "1.0.0" }}"#),
        );
        pacquet(&app, ["install", "--lockfile-only"]).assert().success();
        write_manifest(&app, &format!(r#"{{ "{PARENT}": "100.0.0", "{FOO}": "^1.0.0" }}"#));
        pacquet(&app, ["install", "--lockfile-only"]).assert().success();
        assert!(lockfile_package_keys(&workspace).contains(&format!("{DEP}@100.0.0")));
        let lockfile_only = mode.contains(&"--lockfile-only");
        if !lockfile_only {
            pacquet(&app, ["install", "--frozen-lockfile"]).assert().success();
            assert_installed_child(&app, "100.0.0");
        }

        let mut args = vec!["update", "--latest"];
        args.extend_from_slice(selectors);
        args.extend_from_slice(mode);
        pacquet(&app, &args).assert().success();

        let packages = lockfile_package_keys(&workspace);
        assert!(packages.contains(&format!("{DEP}@{child_version}")), "{args:?}: {packages:?}");
        assert!(!packages.contains(&format!("{DEP}@101.0.0")), "{packages:?}");
        assert!(packages.contains(&format!("{PARENT}@100.0.0")), "{packages:?}");
        assert_eq!(dep_spec(&app, PARENT).as_deref(), Some("100.0.0"));
        assert_eq!(dep_spec(&app, DEP), None);
        if selectors.contains(&"!@pnpm.e2e/foo")
            || selectors == [PARENT, DEP]
            || selectors == [PARENT]
        {
            assert!(packages.contains(&format!("{FOO}@1.0.0")), "{packages:?}");
            assert_eq!(dep_spec(&app, FOO).as_deref(), Some("^1.0.0"));
        }

        if !lockfile_only {
            assert_installed_child(&app, child_version);
        }

        drop((root, anchor));
    }
}

fn assert_installed_child(app: &Path, version: &str) {
    let script = format!(
        "const parent = require.resolve('{PARENT}/package.json');
         const child = require.resolve('{DEP}/package.json', {{ paths: [parent] }});
         console.log(require(child).version);",
    );
    pacquet(app, ["exec", "node", "-e", &script])
        .assert()
        .success()
        .stdout(format!("{version}\n"));
}
