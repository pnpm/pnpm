#![allow(
    clippy::disallowed_methods,
    reason = "test setup starts processes the executor does not run"
)]
use super::{
    PNP_FILENAME,
    ReadPnpFileError,
    package_dirs_of_loader,
    read_pnp_package_dirs,
    render_pnp_loader,
};
use command_extra::CommandExtra;

#[test]
fn template_markers_inside_values_are_not_replaced() {
    let loader =
        render_pnp_loader("registry __PNPM_MODULES_DIR__", "modules __PNPM_PACKAGE_REGISTRY__");

    assert_eq!(loader.matches("registry __PNPM_MODULES_DIR__").count(), 1);
    assert_eq!(loader.matches("modules __PNPM_PACKAGE_REGISTRY__").count(), 1);
}

#[test]
fn loader_respects_exports_and_resolution_paths() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for directory in [".hidden", "second", "app", "other"] {
        std::fs::create_dir(root.join(directory)).unwrap();
    }
    for (directory, value) in [(".hidden", "first"), ("second", "second")] {
        std::fs::write(
            root.join(directory).join("package.json"),
            r#"{"exports":{".":{"require":"./entry.cjs"},"./sub":"./sub.cjs","./pattern/*":"./*.cjs","./blocked":null,"./conditional":{"custom":"./sub.cjs","require":"./entry.cjs"},"./env":{"from-env":"./sub.cjs","default":"./entry.cjs"},"./addons":{"node-addons":"./sub.cjs","default":"./entry.cjs"},"./sync":{"module-sync":"./sub.cjs","default":"./entry.cjs"}}}"#,
        ).unwrap();
        std::fs::write(
            root.join(directory).join("entry.cjs"),
            format!("module.exports = '{value}'"),
        )
        .unwrap();
        std::fs::write(root.join(directory).join("sub.cjs"), "module.exports = 'sub'").unwrap();
        std::fs::write(root.join(directory).join("private.cjs"), "module.exports = 'private'")
            .unwrap();
    }
    let registry = serde_json::json!({"packages": {
        ".": {"url":"./app", "dependencies":{"alias":"first"}},
        "other": {"url":"./other", "dependencies":{"alias":"second"}},
        "first": {"url":"./.hidden", "dependencies":{}},
        "second": {"url":"./second", "dependencies":{}}
    }});
    std::fs::write(root.join(".pnp.cjs"), render_pnp_loader(&registry.to_string(), r#"".""#))
        .unwrap();
    let script = r"
const assert = require('assert');
const path = require('path');
const api = require('../.pnp.cjs');
assert.strictEqual(require('alias'), 'first');
assert.strictEqual(require('alias/sub'), 'sub');
assert.strictEqual(require('alias/env'), 'sub');
assert.strictEqual(require('alias/addons'), process.argv[2] === 'disabled' ? 'first' : 'sub');
assert.strictEqual(require('alias/sync'), process.features.require_module ? 'sub' : 'first');
assert.strictEqual(require('alias/conditional'), 'sub');
assert.strictEqual(require(api.resolveRequest('alias/conditional', __filename, {conditions: new Set(['require'])})), 'first');
assert.strictEqual(require('alias/pattern/sub'), 'sub');
for (const specifier of ['alias/private.cjs', 'alias/blocked']) {
  assert.throws(() => require(specifier), {code: 'ERR_PACKAGE_PATH_NOT_EXPORTED'});
}
assert.throws(() => require('alias/../second/entry.cjs'), {code: 'MODULE_NOT_FOUND'});
assert.strictEqual(api.resolveRequest('alias', __filename), require.resolve('alias'));
assert.strictEqual(api.resolveToUnqualified('alias', __filename), path.resolve('../.hidden'));
assert.throws(() => api.resolveRequest('alias/private.cjs', __filename), {code: 'ERR_PACKAGE_PATH_NOT_EXPORTED'});
assert.strictEqual(require(require.resolve('alias', {paths: [path.resolve('../other')]})), 'second');
assert.strictEqual(require(require.resolve('alias', {paths: [path.resolve('../.hidden'), path.resolve('../other')]})), 'second');
assert.throws(() => require.resolve('alias', {paths: []}), {code: 'MODULE_NOT_FOUND'});
assert.throws(() => require.resolve('undeclared'), {code: 'MODULE_NOT_FOUND'});
";
    std::fs::write(root.join("app/check.cjs"), script).unwrap();
    for flags in [vec![], vec!["--no-addons", "--no-experimental-require-module"], vec!["--addons"]]
    {
        let disabled = flags.contains(&"--no-addons");
        let output = std::process::Command::new("node")
            .with_current_dir(root.join("app"))
            .with_env(
                "NODE_OPTIONS",
                if flags.contains(&"--addons") {
                    "--conditions=from-env --no-addons"
                } else {
                    "--conditions=from-env"
                },
            )
            .with_args(flags)
            .with_args([
                "--conditions=custom",
                "check.cjs",
                if disabled { "disabled" } else { "enabled" },
            ])
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    }
}

#[test]
fn execution_uses_the_configured_lockfile_directory() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    let lockfile_dir = temp.path().join("lockfiles");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&lockfile_dir).unwrap();
    std::fs::write(workspace.join(super::PNP_FILENAME), "stale").unwrap();
    std::fs::write(lockfile_dir.join(super::PNP_FILENAME), "configured").unwrap();
    let config = pnpm_config::Config {
        workspace_dir: Some(workspace.clone()),
        lockfile_dir: Some(lockfile_dir.clone()),
        ..Default::default()
    };
    assert_eq!(
        super::pnp_path_for_execution(&config, &workspace),
        Some(lockfile_dir.join(super::PNP_FILENAME)),
    );
    std::fs::remove_file(lockfile_dir.join(super::PNP_FILENAME)).unwrap();
    assert_eq!(super::pnp_path_for_execution(&config, &workspace), None);
}

#[test]
fn package_dirs_are_read_back_as_the_loader_resolves_them() {
    let root = tempfile::tempdir().unwrap();
    let lockfile_dir = root.path().join("project");
    let store_package = root.path().join("store dir/links/@/a/1.0.0/hash/node_modules/a");
    let absolute_url = url::Url::from_directory_path(&store_package).unwrap();
    let registry = serde_json::json!({ "packages": {
        "a@1.0.0": { "url": "../../store%20dir/links/@/a/1.0.0/hash/node_modules/a/", "dependencies": {} },
        "b@1.0.0": { "url": absolute_url.as_str(), "dependencies": {} },
        ".": { "url": "../", "dependencies": {} },
    }});
    let loader = render_pnp_loader(&registry.to_string(), r#""node_modules""#);

    let mut dirs: Vec<_> = package_dirs_of_loader(&loader, &lockfile_dir)
        .unwrap()
        .into_iter()
        .map(|dir| pnpm_fs::lexical_normalize(&dir))
        .collect();
    dirs.sort();

    let mut expected = vec![lockfile_dir, store_package.clone(), store_package];
    expected.sort();
    assert_eq!(dirs, expected);
}

#[test]
fn a_loader_without_the_registry_is_unrecognized() {
    assert_eq!(package_dirs_of_loader("module.exports = {}\n", std::path::Path::new("/p")), None);
}

#[test]
fn reading_a_loader_skips_foreign_and_rejects_unreadable_pnpm_loaders() {
    let dir = tempfile::tempdir().unwrap();
    let pnp = dir.path().join(PNP_FILENAME);

    assert_eq!(read_pnp_package_dirs(dir.path()).unwrap(), Vec::<std::path::PathBuf>::new());

    std::fs::write(&pnp, "#!/usr/bin/env node\n/* yarn */\n").unwrap();
    assert_eq!(read_pnp_package_dirs(dir.path()).unwrap(), Vec::<std::path::PathBuf>::new());

    let bundled = format!("#!/usr/bin/env node\n{}", super::PNP_EXPORTS_RESOLVER);
    std::fs::write(&pnp, bundled).unwrap();
    assert_eq!(read_pnp_package_dirs(dir.path()).unwrap(), Vec::<std::path::PathBuf>::new());

    let registry = serde_json::json!({ "packages": { ".": { "url": "../", "dependencies": {} } } });
    let loader = render_pnp_loader(&registry.to_string(), r#""node_modules""#);
    std::fs::write(&pnp, &loader).unwrap();
    assert_eq!(read_pnp_package_dirs(dir.path()).unwrap().len(), 1);

    std::fs::write(&pnp, loader.replace("const registry = ", "const registry =\n  ")).unwrap();
    assert!(matches!(read_pnp_package_dirs(dir.path()), Err(ReadPnpFileError::Unreadable { .. }),));
}

#[cfg(unix)]
#[test]
fn reading_a_loader_does_not_follow_a_symlink() {
    let dir = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink("/dev/zero", dir.path().join(PNP_FILENAME)).unwrap();

    assert_eq!(read_pnp_package_dirs(dir.path()).unwrap(), Vec::<std::path::PathBuf>::new());
}
