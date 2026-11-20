use super::{
    Command,
    CommandExtra,
    CommandTempCwd,
    TempDir,
    assert_eq,
    cache_foo_index_versions,
    cargo_add_project,
    exec_pacquet_in_temp_cwd,
    get_filenames_in_folder,
    prod_spec,
};
use crate::_utils::flatten_report;
use assert_cmd::{
    assert::OutputAssertExt,
    cargo::CommandCargoExt,
};

#[test]
fn add_npm_purl_saves_the_scoped_package_it_names() {
    let (root, dir, anchor) =
        exec_pacquet_in_temp_cwd(["add", "pkg:npm/%40pnpm.e2e/hello-world-js-bin@1.0.0"]);

    assert_eq!(prod_spec(&dir, "@pnpm.e2e/hello-world-js-bin"), "1.0.0");
    assert_eq!(
        get_filenames_in_folder(&dir.join("node_modules/@pnpm.e2e")),
        ["hello-world-js-bin"],
    );
    drop((root, anchor)); // cleanup
}

#[test]
fn add_cargo_purl_writes_the_crate_to_the_cargo_manifest() {
    let (root, cache_dir) = cargo_add_project();
    // `1.1.0` is what a caret range would reach and `2.0.0` is what a
    // latest-version lookup would pick, so neither can pass for `1.0.0`.
    cache_foo_index_versions(&cache_dir, &["1.0.0", "1.1.0", "2.0.0"]);
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(root.path())
        .with_env("PNPM_CONFIG_CACHE_DIR", &cache_dir)
        .with_args(["add", "pkg:cargo/foo@1.0.0", "--offline", "--lockfile-only"])
        .assert()
        .success();

    let manifest = std::fs::read_to_string(root.path().join("Cargo.toml"))
        .expect("read updated Cargo manifest");
    assert!(manifest.contains("[dependencies]\nfoo = \"=1.0.0\""), "{manifest}");
    assert!(
        !root
            .path()
            .join("package.json")
            .exists(),
    );
}

#[test]
fn add_rejects_an_unsupported_purl_type() {
    let root = TempDir::new().expect("create project directory");
    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(root.path())
        .with_args(["add", "pkg:maven/org.apache.commons/io@1.3.4"])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    eprintln!("stderr:\n{stderr}");
    assert!(
        flatten_report(&stderr)
            .contains(&flatten_report(
                "has purl type `maven`, but pnpm can add only `npm`, `cargo`, and `pypi` packages",
            )),
        "{stderr}",
    );
    assert!(
        !root
            .path()
            .join("package.json")
            .exists(),
    );
}

/// `pnpm add npm@11.0.0` declares which package manager the project uses,
/// and `pnpm add node@22.0.0` records a runtime. A purl names a package in
/// the npm registry, so the same version becomes a dependency instead.
#[test]
fn an_npm_purl_that_names_a_tool_is_installed_rather_than_declared() {
    for (selector, name, version) in
        [("pkg:npm/npm@11.0.0", "npm", "11.0.0"), ("pkg:npm/node@22.0.0", "node", "22.0.0")]
    {
        let CommandTempCwd {
            pacquet,
            root,
            workspace,
            npmrc_info,
            ..
        } = CommandTempCwd::init().add_mocked_registry();
        pacquet
            .with_args(["add", selector, "--lockfile-only"])
            .assert()
            .success();

        assert_eq!(prod_spec(&workspace, name), version, "{selector}");
        let manifest = std::fs::read_to_string(workspace.join("package.json"))
            .expect("read the updated manifest");
        let declarations: serde_json::Value =
            serde_json::from_str(&manifest).expect("parse the updated manifest");
        assert_eq!(declarations.get("devEngines"), None, "{selector}: {manifest}");
        assert_eq!(declarations.get("engines"), None, "{selector}: {manifest}");
        drop((root, npmrc_info)); // cleanup
    }
}

/// The mark a purl carries belongs to the request rather than to its text,
/// so one command can carry both spellings of one selector and each keeps
/// its own meaning.
#[test]
fn a_bare_package_manager_request_beside_its_purl_keeps_its_own_meaning() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    pacquet
        .with_args(["add", "npm@11.0.0", "pkg:npm/npm@11.0.0", "--lockfile-only"])
        .assert()
        .success();

    assert_eq!(prod_spec(&workspace, "npm"), "11.0.0");
    let manifest =
        std::fs::read_to_string(workspace.join("package.json")).expect("read the updated manifest");
    let declarations: serde_json::Value =
        serde_json::from_str(&manifest).expect("parse the updated manifest");
    assert_eq!(
        declarations["devEngines"]["packageManager"],
        serde_json::json!({ "name": "npm", "version": "11.0.0" }),
        "{manifest}",
    );
    drop((root, npmrc_info)); // cleanup
}

#[test]
fn add_refuses_a_cargo_or_python_dependency_by_naming_its_ecosystem() {
    for (selector, target, message) in [
        ("crate:serde", "--global", "Cargo dependencies cannot be installed globally"),
        ("pkg:cargo/serde", "--global", "Cargo dependencies cannot be installed globally"),
        ("pypi:requests", "--global", "Python dependencies cannot be installed globally"),
        ("pkg:pypi/requests", "--global", "Python dependencies cannot be installed globally"),
        ("crate:serde", "--config", "Cargo dependencies cannot be configuration dependencies"),
        ("pkg:cargo/serde", "--config", "Cargo dependencies cannot be configuration dependencies"),
        ("pypi:requests", "--config", "Python dependencies cannot be configuration dependencies"),
        (
            "pkg:pypi/requests",
            "--config",
            "Python dependencies cannot be configuration dependencies",
        ),
    ] {
        let root = TempDir::new().expect("create project directory");
        let output = Command::cargo_bin("pnpm")
            .expect("find the pnpm binary")
            .with_current_dir(root.path())
            .with_args(["add", selector, target])
            .assert()
            .failure();

        let stderr = String::from_utf8_lossy(&output.get_output().stderr);
        eprintln!("{selector} {target} stderr:\n{stderr}");
        assert!(
            flatten_report(&stderr).contains(&flatten_report(message)),
            "{selector} {target}: {stderr}",
        );
    }
}
