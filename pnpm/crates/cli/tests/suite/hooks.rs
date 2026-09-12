use crate::_utils::pacquet_in;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::{EnvLockfile, PackageKey};
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path};

#[test]
fn filter_log_is_ignored_with_a_warning() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").expect("write package.json");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { filterLog: () => false } }",
    )
    .expect("write filterLog hook");
    fs::write(workspace.join("pnpm-lock.yaml"), "not: [valid").expect("write broken lockfile");

    let output = pacquet_in(&workspace).with_arg("install").assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("filterLog hook is deprecated"), "STDOUT:\n{stdout}");
    assert!(stdout.contains("Ignoring broken lockfile"), "STDOUT:\n{stdout}");

    drop(root);
}

const EXTRA_ENV_PNPMFILE: &str = "module.exports = { hooks: { updateConfig (config) { config.extraEnv = { ...config.extraEnv, PNPM_HOOK_MARKER: 'from-hook' }; return config } } }";

/// A script that records `PNPM_HOOK_MARKER` — the variable
/// [`EXTRA_ENV_PNPMFILE`] exports — in `marker.txt` next to the manifest.
const WRITE_MARKER_SCRIPT: &str =
    r#"node -e "require('fs').writeFileSync('marker.txt', process.env.PNPM_HOOK_MARKER || '')""#;

const CATALOG_DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";

/// A registry URL nothing serves, so reaching it is a test failure.
const DEAD_REGISTRY: &str = "registry=http://127.0.0.1:1/";

fn write_catalog_hook_project(workspace: &Path) {
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "catalog-hook-project",
            "version": "1.0.0",
            "dependencies": { (CATALOG_DEP): "catalog:" },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        format!(
            "module.exports = {{ hooks: {{ updateConfig (config) {{ config.catalogs = {{ default: {{ '{CATALOG_DEP}': '^100.0.0' }} }}; return config }} }} }}",
        ),
    )
    .expect("write pnpmfile");
}

#[test]
fn update_config_catalog_applies_to_link() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_catalog_hook_project(&workspace);
    let target = root.path().join("other-pkg");
    fs::create_dir_all(&target).expect("create link target");
    fs::write(target.join("package.json"), r#"{ "name": "other-pkg", "version": "1.0.0" }"#)
        .expect("write link target manifest");

    pacquet_in(&workspace).with_args(["link", "../other-pkg"]).assert().success();

    drop((root, mock_instance));
}

#[test]
fn update_config_catalog_applies_to_outdated() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_catalog_hook_project(&workspace);

    pacquet_in(&workspace).with_arg("install").assert().success();
    let output = pacquet_in(&workspace).with_arg("outdated").output().expect("run outdated");

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("STDOUT:\n{stdout}\n");
    assert!(stdout.contains(CATALOG_DEP), "outdated should report the catalog dependency");

    drop((root, mock_instance));
}

#[test]
fn update_config_catalog_applies_to_import() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_catalog_hook_project(&workspace);
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut settings = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    settings.push_str("\nconfigDependencies:\n  '@pnpm/plugin-pnpmfile': 1.0.0\n");
    fs::write(workspace_yaml, settings).expect("write configDependencies");
    // `import` needs a foreign lockfile to read. Pinning a version the
    // hook's `^100.0.0` catalog range excludes keeps the assertion below
    // about the catalog: an imported pin is only a preference, so it
    // cannot pull resolution outside the range the catalog set.
    fs::write(
        workspace.join("package-lock.json"),
        serde_json::json!({
            "lockfileVersion": 1,
            "dependencies": {
                (CATALOG_DEP): { "version": "101.0.0" },
            },
        })
        .to_string(),
    )
    .expect("write package-lock.json");

    pacquet_in(&workspace).with_arg("import").assert().success();

    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(lockfile.starts_with("---\n"), "env document must lead pnpm-lock.yaml");
    assert!(lockfile.contains("configDependencies:"), "env document must retain config deps");
    let env_lockfile = EnvLockfile::read(&workspace)
        .expect("read env lockfile")
        .expect("env lockfile should be present");
    let config_dependency = &env_lockfile.importers[EnvLockfile::ROOT_IMPORTER_KEY]
        .config_dependencies["@pnpm/plugin-pnpmfile"];
    let config_dependency_key: PackageKey =
        format!("@pnpm/plugin-pnpmfile@{}", config_dependency.version)
            .parse()
            .expect("parse config dependency package key");
    assert!(
        env_lockfile
            .packages
            .get(&config_dependency_key)
            .expect("config dependency package must be retained")
            .resolution
            .checkable_integrity()
            .is_some(),
        "env document must retain the config dependency integrity",
    );
    assert!(
        lockfile.contains("@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0"),
        "the imported lockfile should resolve the hook-provided catalog entry:\n{lockfile}",
    );
    pacquet_in(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    drop((root, mock_instance));
}

/// `updateConfig` applies to `pnpm run`, not just to the install family:
/// the settings a hook returns — `extraEnv` here — reach the environment of
/// the script it spawns.
#[test]
fn update_config_applies_to_run() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let manifest = serde_json::json!({
        "name": "run-reads-extra-env",
        "version": "0.0.0",
        "scripts": { "write-marker": WRITE_MARKER_SCRIPT },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
    fs::write(workspace.join(".pnpmfile.cjs"), EXTRA_ENV_PNPMFILE).expect("write pnpmfile");

    pacquet_in(&workspace).with_arg("run").with_arg("write-marker").assert().success();

    assert_eq!(fs::read_to_string(workspace.join("marker.txt")).expect("read marker"), "from-hook");

    drop(root);
}

/// The same for `pnpm exec`, which spawns its command through the same
/// environment.
#[test]
fn update_config_applies_to_exec() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{"name":"exec-reads-extra-env"}"#)
        .expect("write package.json");
    fs::write(workspace.join(".pnpmfile.cjs"), EXTRA_ENV_PNPMFILE).expect("write pnpmfile");

    pacquet_in(&workspace)
        .with_arg("exec")
        .with_arg("node")
        .with_arg("-e")
        .with_arg("require('fs').writeFileSync('marker.txt', process.env.PNPM_HOOK_MARKER || '')")
        .assert()
        .success();

    assert_eq!(fs::read_to_string(workspace.join("marker.txt")).expect("read marker"), "from-hook");

    drop(root);
}

/// A recursive `pnpm run` applies the workspace-root hook to every
/// project's script environment.
#[test]
fn update_config_applies_to_recursive_run() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{"name":"root","private":true}"#)
        .expect("write root package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join(".pnpmfile.cjs"), EXTRA_ENV_PNPMFILE).expect("write pnpmfile");
    let project = workspace.join("packages").join("a");
    fs::create_dir_all(&project).expect("create project dir");
    let manifest = serde_json::json!({
        "name": "a",
        "version": "0.0.0",
        "scripts": { "write-marker": WRITE_MARKER_SCRIPT },
    })
    .to_string();
    fs::write(project.join("package.json"), manifest).expect("write project package.json");

    pacquet_in(&workspace)
        .with_arg("--recursive")
        .with_arg("run")
        .with_arg("write-marker")
        .assert()
        .success();

    assert_eq!(fs::read_to_string(project.join("marker.txt")).expect("read marker"), "from-hook");

    drop(root);
}

/// The recursive-run defaults `bail`, `sort` and `reverse` are read after
/// the hook ran: a hook that turns `bail` off keeps a recursive run
/// dispatching past a failing project.
#[test]
fn update_config_applies_to_recursive_run_defaults() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{"name":"root","private":true}"#)
        .expect("write root package.json");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nworkspaceConcurrency: 1\n",
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { updateConfig (config) { config.bail = false; return config } } }",
    )
    .expect("write pnpmfile");
    // One project at a time, in name order: the failure comes first, so
    // the default `bail` would never dispatch the second project.
    let failing = workspace.join("packages").join("a-fails");
    fs::create_dir_all(&failing).expect("create failing project dir");
    fs::write(
        failing.join("package.json"),
        serde_json::json!({
            "name": "a-fails",
            "version": "0.0.0",
            "scripts": { "check": r#"node -e "process.exit(1)""# },
        })
        .to_string(),
    )
    .expect("write failing project package.json");
    let next = workspace.join("packages").join("b-writes-marker");
    fs::create_dir_all(&next).expect("create next project dir");
    fs::write(
        next.join("package.json"),
        serde_json::json!({
            "name": "b-writes-marker",
            "version": "0.0.0",
            "scripts": { "check": WRITE_MARKER_SCRIPT },
        })
        .to_string(),
    )
    .expect("write next project package.json");

    pacquet_in(&workspace).with_args(["--recursive", "run", "check"]).assert().failure();

    assert!(
        next.join("marker.txt").exists(),
        "with the hook's `bail: false`, the run must go on to the project after the failure",
    );

    drop(root);
}

/// `pnpm rebuild` re-runs install scripts with the hook's settings too.
#[test]
fn update_config_applies_to_rebuild() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let manifest = serde_json::json!({
        "name": "rebuild-reads-extra-env",
        "version": "0.0.0",
        "scripts": { "install": WRITE_MARKER_SCRIPT },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
    fs::write(workspace.join(".pnpmfile.cjs"), EXTRA_ENV_PNPMFILE).expect("write pnpmfile");
    let marker = workspace.join("marker.txt");

    pacquet_in(&workspace).with_args(["install", "--ignore-scripts"]).assert().success();
    assert!(!marker.exists(), "--ignore-scripts must leave the install script for rebuild");

    pacquet_in(&workspace).with_args(["rebuild", "--pending"]).assert().success();

    assert_eq!(fs::read_to_string(&marker).expect("read marker"), "from-hook");

    drop(root);
}

/// A hook that changes an install-affecting setting must be applied before
/// the verify-deps-before-run check compares the live settings with the ones
/// the last install recorded. Without the hook, `pnpm run` sees the
/// pre-hook value, reports the setting as changed, and — under
/// `verifyDepsBeforeRun: error` — refuses to run any script at all.
#[test]
fn update_config_applies_before_the_verify_deps_check() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let manifest = serde_json::json!({
        "name": "run-under-verify-deps",
        "private": true,
        "scripts": { "foo": r#"node -e "console.log('ran')""# },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages: []\nverifyDepsBeforeRun: error\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { updateConfig (config) { config.dedupePeers = true; return config } } }",
    )
    .expect("write pnpmfile");

    pacquet_in(&workspace).with_arg("install").assert().success();
    let output = pacquet_in(&workspace).with_arg("run").with_arg("foo").assert().success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("ran"), "the script should have run\nSTDOUT:\n{stdout}");

    drop(root);
}

/// Records the whole config the `updateConfig` hook is handed, so a test can
/// assert on what a hook can read.
const DUMP_CONFIG_PNPMFILE: &str = "const fs = require('fs');\nconst path = require('path');\nmodule.exports = { hooks: { updateConfig (config) {\n  fs.writeFileSync(path.join(__dirname, 'seen.json'), JSON.stringify(config));\n  return config;\n} } }";

fn config_seen_by_hook(workspace: &Path) -> serde_json::Value {
    let seen = fs::read_to_string(workspace.join("seen.json")).expect("read the config seen");
    serde_json::from_str(&seen).expect("the config seen parses as JSON")
}

/// A hook reads the configuration the install runs with, so a scope routed
/// by `.npmrc` is visible to it (pnpm/pnpm#14676). `.npmrc` is the only
/// source of scoped registry routing for many projects, and it never reaches
/// `pnpm-workspace.yaml`.
#[test]
fn update_config_sees_npmrc_scoped_registries() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").expect("write package.json");
    fs::write(workspace.join(".npmrc"), "@acme:registry=https://acme.example.com/npm/\n")
        .expect("write .npmrc");
    fs::write(workspace.join(".pnpmfile.cjs"), DUMP_CONFIG_PNPMFILE).expect("write pnpmfile");

    pacquet_in(&workspace).with_arg("install").assert().success();

    let seen = config_seen_by_hook(&workspace);
    dbg!(&seen["registriesByScope"]);
    assert_eq!(
        seen["registriesByScope"]["@acme"],
        serde_json::json!("https://acme.example.com/npm/"),
    );
    // Every unscoped package resolves through the default registry, so it is
    // reported whether or not a source named it.
    assert!(seen["registry"].as_str().is_some_and(|url| !url.is_empty()));
    assert_eq!(seen["registriesByScope"]["default"], seen["registry"]);

    drop(root);
}

/// A hook branching on a setting needs its effective value, whether a CLI
/// flag or a default supplied it.
#[test]
fn update_config_sees_cli_flags_and_resolved_defaults() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").expect("write package.json");
    fs::write(workspace.join(".pnpmfile.cjs"), DUMP_CONFIG_PNPMFILE).expect("write pnpmfile");

    pacquet_in(&workspace)
        .with_args(["install", "--registry=https://cli.example.com/"])
        .assert()
        .success();

    let seen = config_seen_by_hook(&workspace);
    dbg!(&seen["registry"], &seen["nodeLinker"], &seen["autoInstallPeers"]);
    assert_eq!(seen["registry"], serde_json::json!("https://cli.example.com/"));
    assert_eq!(seen["registriesByScope"]["default"], serde_json::json!("https://cli.example.com/"));
    // Unset everywhere, so only the resolved default can answer.
    assert_eq!(seen["nodeLinker"], serde_json::json!("isolated"));
    assert_eq!(seen["autoInstallPeers"], serde_json::json!(true));

    drop(root);
}

/// A setting nothing set is absent rather than `null`, as it is on pnpm 11,
/// so a hook testing for a key gets the same answer in both versions.
/// `registries` is a shape only `pnpm-workspace.yaml` has, and its resolved
/// form is reported as `registriesByScope`.
#[test]
fn update_config_omits_the_settings_nothing_set() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").expect("write package.json");
    fs::write(workspace.join(".pnpmfile.cjs"), DUMP_CONFIG_PNPMFILE).expect("write pnpmfile");

    pacquet_in(&workspace).with_arg("install").assert().success();

    let seen = config_seen_by_hook(&workspace);
    let settings = seen.as_object().expect("the configuration seen is an object");
    let reported_as_null: Vec<&String> =
        settings.iter().filter(|(_, value)| value.is_null()).map(|(key, _)| key).collect();
    dbg!(&reported_as_null);
    assert!(reported_as_null.is_empty());
    assert!(!settings.contains_key("registries"));

    // The pnpmfiles being run, and the cache directory pnpm chose for the
    // host, neither of which anything here set.
    dbg!(&seen["pnpmfile"], &seen["cacheDir"]);
    let pnpmfiles: Vec<&str> = seen["pnpmfile"]
        .as_array()
        .expect("the pnpmfiles seen are an array")
        .iter()
        .map(|path| path.as_str().expect("a pnpmfile path is a string"))
        .collect();
    assert_eq!(pnpmfiles.len(), 1);
    assert!(pnpmfiles[0].ends_with(".pnpmfile.cjs"));
    assert!(Path::new(pnpmfiles[0]).is_absolute());
    assert!(seen["cacheDir"].as_str().is_some_and(|dir| Path::new(dir).is_absolute()));

    drop(root);
}

/// The registry credentials a hook reads, the way pnpm 11 exposes them: a
/// pnpmfile runs as unrestricted Node in both versions, so this is nothing
/// it could not already read from `.npmrc` itself.
#[test]
fn update_config_sees_registry_credentials() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").expect("write package.json");
    fs::write(
        workspace.join(".npmrc"),
        "@acme:registry=https://acme.example.com/npm/\n//acme.example.com/npm/:_authToken=hook-visible-token\n//acme.example.com/npm/:@acme:_authToken=scoped-token\n",
    )
    .expect("write .npmrc");
    fs::write(workspace.join(".pnpmfile.cjs"), DUMP_CONFIG_PNPMFILE).expect("write pnpmfile");

    pacquet_in(&workspace).with_arg("install").assert().success();

    let seen = config_seen_by_hook(&workspace);
    dbg!(&seen["authConfig"], &seen["configByUri"]);
    assert_eq!(
        seen["authConfig"]["//acme.example.com/npm/:_authToken"],
        serde_json::json!("hook-visible-token"),
    );
    assert_eq!(
        seen["configByUri"]["//acme.example.com/npm/"]["@"]["authToken"],
        serde_json::json!("hook-visible-token"),
    );
    assert_eq!(
        seen["configByUri"]["//acme.example.com/npm/"]["@acme"]["authToken"],
        serde_json::json!("scoped-token"),
    );
    // The registry rows resolved across every source, so a hook reading
    // `authConfig` finds the URL the install fetches from.
    assert_eq!(
        seen["authConfig"]["@acme:registry"],
        serde_json::json!("https://acme.example.com/npm/"),
    );
    assert_eq!(seen["authConfig"]["registry"], seen["registry"]);

    drop(root);
}

/// A hook rewrites registry routing under the name it reads it as, so
/// `registriesByScope` is writable and the install resolves through the
/// route the hook chose. The `.npmrc` here points the default registry at a
/// port nothing listens on, so only the hook's rewrite can make the install
/// succeed.
#[test]
fn update_config_can_rewrite_registry_routing() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;

    let mocked = mock_instance.url();
    let npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let dead = npmrc.replace(&format!("registry={mocked}"), DEAD_REGISTRY);
    assert_ne!(dead, npmrc, "the mocked registry line was not replaced");
    fs::write(&npmrc_path, dead).expect("point .npmrc at a dead registry");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { CATALOG_DEP: "100.0.0" } }).to_string(),
    )
    .expect("write package.json");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        format!(
            "module.exports = {{ hooks: {{ updateConfig (config) {{\n  config.registriesByScope = {{ ...config.registriesByScope, default: '{mocked}' }};\n  return config;\n}} }} }}",
        ),
    )
    .expect("write pnpmfile");

    pacquet_in(&workspace).with_args(["install", "--lockfile-only"]).assert().success();

    drop((root, mock_instance));
}
