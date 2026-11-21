use super::render_pnp_loader;
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
