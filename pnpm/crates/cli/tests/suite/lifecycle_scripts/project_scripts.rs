use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, process::Command};

/// A `node -e` lifecycle script that appends `<stage>\n` to
/// `order.txt` in the script's cwd (the project root). pacquet
/// passes Windows lifecycle scripts to `cmd /d /s /c` verbatim
/// (matching Node's `windowsVerbatimArguments`), so the embedded
/// quotes survive on every platform.
fn append_order_script(stage: &str) -> String {
    format!(r#"node -e "require('fs').appendFileSync('order.txt','{stage}\n')""#)
}

fn project_with_lifecycle_scripts() -> serde_json::Value {
    serde_json::json!({
        "name": "project-with-lifecycle-scripts",
        "version": "1.0.0",
        "scripts": {
            "postpare": append_order_script("typo-never-runs"),
            "prepare": append_order_script("prepare"),
            "preprepare": append_order_script("preprepare"),
            "postprepare": append_order_script("postprepare"),
            "preinstall": append_order_script("preinstall"),
            "install": append_order_script("install"),
            "postinstall": append_order_script("postinstall"),
        },
    })
}

#[test]
fn runs_project_lifecycle_scripts_in_order() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(workspace.join("package.json"), project_with_lifecycle_scripts().to_string())
        .expect("write package.json");

    pacquet.with_arg("install").assert().success();

    let order = fs::read_to_string(workspace.join("order.txt")).expect("read order.txt");
    let stages: Vec<&str> = order.lines().collect();
    assert_eq!(
        stages,
        ["preinstall", "install", "postinstall", "preprepare", "prepare", "postprepare"],
    );

    drop((root, mock_instance));
}

/// `npm_config_user_agent` carries the configured user agent in every
/// script context. Guards such as n8n's `preinstall` compare its
/// leading `name/version` token against `pnpm`, so a missing or
/// truncated value makes them reject the install.
#[test]
fn stamps_the_configured_user_agent_on_install_scripts_run_and_exec() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    // The body as a bare JS expression, so it can be passed either
    // through a shell (a script body) or straight to `node -e`.
    let record_user_agent_js = |file: &str| {
        format!(
            "require('fs').writeFileSync('{file}',process.env.npm_config_user_agent||'<unset>')",
        )
    };
    let record_user_agent = |file: &str| format!(r#"node -e "{}""#, record_user_agent_js(file));
    let manifest = serde_json::json!({
        "name": "project-reading-the-user-agent",
        "version": "1.0.0",
        "scripts": {
            "preinstall": record_user_agent("preinstall-ua.txt"),
            "show-ua": record_user_agent("run-ua.txt"),
        },
    });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");

    pacquet.with_arg("install").assert().success();
    for args in [
        vec!["run".to_string(), "show-ua".to_string()],
        // `exec` builds its child's environment separately from the
        // lifecycle executor, so it needs its own coverage.
        vec![
            "exec".to_string(),
            "node".to_string(),
            "-e".to_string(),
            record_user_agent_js("exec-ua.txt"),
        ],
    ] {
        Command::cargo_bin("pnpm")
            .expect("find the pnpm binary")
            .with_current_dir(&workspace)
            .with_args(args)
            .assert()
            .success();
    }

    for file in ["preinstall-ua.txt", "run-ua.txt", "exec-ua.txt"] {
        let user_agent = fs::read_to_string(workspace.join(file))
            .unwrap_or_else(|error| panic!("read {file}: {error}"));
        let (name, rest) = user_agent.split_once('/').unwrap_or((&user_agent, ""));
        assert_eq!(name, "pnpm", "{file}: {user_agent}");
        assert!(!rest.is_empty(), "{file} carries a version: {user_agent}");
    }

    drop((root, mock_instance));
}

#[test]
fn runs_project_lifecycle_scripts_on_frozen_install() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(workspace.join("package.json"), project_with_lifecycle_scripts().to_string())
        .expect("write package.json");

    pacquet.with_arg("install").assert().success();
    assert!(workspace.join("pnpm-lock.yaml").exists(), "first install should write a lockfile");
    fs::remove_file(workspace.join("order.txt")).expect("clear order.txt between installs");

    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_arg("install")
        .with_arg("--frozen-lockfile")
        .assert()
        .success();

    let order = fs::read_to_string(workspace.join("order.txt")).expect("read order.txt");
    let stages: Vec<&str> = order.lines().collect();
    assert_eq!(
        stages,
        ["preinstall", "install", "postinstall", "preprepare", "prepare", "postprepare"],
    );

    drop((root, mock_instance));
}

#[test]
fn failing_project_script_fails_the_install() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    // `exit 1` is shell-agnostic — a non-zero exit in both `sh -c`
    // and `cmd /d /s /c`. Mirrors pnpm's own test
    // (`preinstall: 'exit 1'`).
    let package_json = serde_json::json!({
        "name": "project-with-failing-script",
        "version": "1.0.0",
        "scripts": {
            "postinstall": "exit 1",
        },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");

    pacquet.with_arg("install").assert().failure();

    drop((root, mock_instance));
}

/// The project's scripts run regardless of whether its `name`
/// matches its directory.
#[test]
fn runs_scripts_when_project_name_differs_from_directory() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "name": "different-name",
        "version": "1.0.0",
        "scripts": {
            "preinstall": append_order_script("preinstall"),
            "install": append_order_script("install"),
            "postinstall": append_order_script("postinstall"),
        },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");

    pacquet.with_arg("install").assert().success();

    let order = fs::read_to_string(workspace.join("order.txt")).expect("read order.txt");
    let stages: Vec<&str> = order.lines().collect();
    assert_eq!(stages, ["preinstall", "install", "postinstall"]);

    drop((root, mock_instance));
}

/// `INIT_CWD` is set to the lockfile directory for project scripts.
#[test]
fn project_script_sees_init_cwd() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "name": "project-reads-init-cwd",
        "version": "1.0.0",
        "scripts": {
            "postinstall":
                r#"node -e "require('fs').writeFileSync('init-cwd.txt', process.env.INIT_CWD || '')""#,
        },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");

    pacquet.with_arg("install").assert().success();

    let init_cwd = fs::read_to_string(workspace.join("init-cwd.txt")).expect("read init-cwd.txt");
    let canonical_workspace = fs::canonicalize(&workspace).expect("canonicalize workspace dir");
    let canonical_init_cwd =
        fs::canonicalize(init_cwd.trim()).expect("canonicalize INIT_CWD value");
    assert_eq!(canonical_init_cwd, canonical_workspace);

    drop((root, mock_instance));
}

/// An `updateConfig` pnpmfile hook that sets `extraEnv` exports those
/// variables into the project's lifecycle-script environment. Mirrors
/// the TypeScript CLI, where `config.extraEnv` is forwarded to
/// lifecycle scripts.
#[test]
fn update_config_extra_env_reaches_lifecycle_scripts() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "name": "project-reads-extra-env",
        "version": "1.0.0",
        "scripts": {
            "postinstall":
                r#"node -e "require('fs').writeFileSync('extra-env.txt', process.env.PNPM_BUILD_MARKER || '')""#,
        },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    fs::write(
            workspace.join(".pnpmfile.cjs"),
            "module.exports = { hooks: { updateConfig (config) { config.extraEnv = { ...config.extraEnv, PNPM_BUILD_MARKER: 'from-hook' }; return config } } }",
        )
        .expect("write pnpmfile");

    pacquet.with_arg("install").assert().success();

    let value = fs::read_to_string(workspace.join("extra-env.txt")).expect("read extra-env.txt");
    assert_eq!(value.trim(), "from-hook");

    drop((root, mock_instance));
}

/// A hook `extraEnv` that tries to override a reserved pnpm stamp
/// (`INIT_CWD`) must NOT win: pnpm's own value is authoritative,
/// matching TS `runLifecycleHook`. The script writes `INIT_CWD`,
/// which must be the lockfile dir, not the hook's bogus value.
#[test]
fn update_config_extra_env_cannot_override_reserved_stamps() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "name": "project-reserved-stamp",
        "version": "1.0.0",
        "scripts": {
            "postinstall":
                r#"node -e "require('fs').writeFileSync('init-cwd.txt', process.env.INIT_CWD || '')""#,
        },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    fs::write(
            workspace.join(".pnpmfile.cjs"),
            "module.exports = { hooks: { updateConfig (config) { config.extraEnv = { ...config.extraEnv, INIT_CWD: '/bogus-from-hook' }; return config } } }",
        )
        .expect("write pnpmfile");

    pacquet.with_arg("install").assert().success();

    let init_cwd = fs::read_to_string(workspace.join("init-cwd.txt")).expect("read init-cwd.txt");
    assert_ne!(init_cwd.trim(), "/bogus-from-hook", "hook extraEnv must not override INIT_CWD");
    let canonical_workspace = fs::canonicalize(&workspace).expect("canonicalize workspace dir");
    let canonical_init_cwd =
        fs::canonicalize(init_cwd.trim()).expect("canonicalize INIT_CWD value");
    assert_eq!(canonical_init_cwd, canonical_workspace);

    drop((root, mock_instance));
}

/// `pacquet add <pkg>` is a partial install, so the project's own
/// lifecycle scripts must not run: `postinstall`/`prepare` are not
/// executed after a named install.
#[test]
fn add_does_not_run_project_lifecycle_scripts() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "name": "project-adding-a-dep",
        "version": "1.0.0",
        "scripts": {
            "preinstall": append_order_script("preinstall"),
            "install": append_order_script("install"),
            "postinstall": append_order_script("postinstall"),
            "prepare": append_order_script("prepare"),
        },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");

    pacquet.with_arg("add").with_arg("@pnpm.e2e/hello-world-js-bin-parent").assert().success();

    assert!(
        !workspace.join("order.txt").exists(),
        "named install (`add`) must not run the project's own lifecycle scripts",
    );

    drop((root, mock_instance));
}

/// `--ignore-scripts` suppresses the project's own lifecycle scripts:
/// none of them run, so `order.txt` is never created. Mirrors pnpm,
/// which skips the project lifecycle hooks alongside dependency build
/// scripts under `ignoreScripts`.
#[test]
fn ignore_scripts_skips_project_lifecycle_scripts() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(workspace.join("package.json"), project_with_lifecycle_scripts().to_string())
        .expect("write package.json");

    pacquet.with_args(["install", "--ignore-scripts"]).assert().success();

    assert!(
        !workspace.join("order.txt").exists(),
        "no project lifecycle script should run under --ignore-scripts",
    );

    drop((root, mock_instance));
}

/// `pacquet update --latest` with no selectors rewrites every direct
/// dependency's spec, which makes it pnpm's `installSome` — a partial
/// mutation that does not run the project's own lifecycle scripts,
/// unlike a bare `pacquet update`.
#[test]
fn latest_update_without_selectors_does_not_run_project_lifecycle_scripts() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "name": "project-updating-to-latest",
        "version": "1.0.0",
        "dependencies": { "@pnpm.e2e/dep-of-pkg-with-1-dep": "^100.0.0" },
        "scripts": {
            "preinstall": append_order_script("preinstall"),
            "install": append_order_script("install"),
            "postinstall": append_order_script("postinstall"),
            "prepare": append_order_script("prepare"),
        },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");

    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_arg("install")
        .assert()
        .success();
    fs::remove_file(workspace.join("order.txt")).expect("clear the install's order.txt");

    pacquet.with_args(["update", "--latest"]).assert().success();

    assert!(
        !workspace.join("order.txt").exists(),
        "`update --latest` must not run the project's own lifecycle scripts",
    );

    drop((root, mock_instance));
}

/// `pnpm:devPreinstall` is the root project's chance to prepare state
/// that resolution and linking then consume, so it runs on its own
/// schedule: before every other stage, only for the root, and only
/// when scripts are not suppressed.
mod dev_preinstall {
    use super::{append_order_script, project_with_lifecycle_scripts};
    use assert_cmd::prelude::*;
    use command_extra::CommandExtra;
    use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
    use std::{fs, process::Command};

    fn project_with_dev_preinstall() -> serde_json::Value {
        let mut manifest = project_with_lifecycle_scripts();
        manifest["scripts"]["pnpm:devPreinstall"] =
            append_order_script("pnpm:devPreinstall").into();
        manifest
    }

    const EXPECTED_ORDER: [&str; 7] = [
        "pnpm:devPreinstall",
        "preinstall",
        "install",
        "postinstall",
        "preprepare",
        "prepare",
        "postprepare",
    ];

    #[test]
    fn runs_before_the_projects_own_lifecycle_scripts() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        fs::write(workspace.join("package.json"), project_with_dev_preinstall().to_string())
            .expect("write package.json");

        pacquet.with_arg("install").assert().success();

        let order = fs::read_to_string(workspace.join("order.txt")).expect("read order.txt");
        let stages: Vec<&str> = order.lines().collect();
        assert_eq!(stages, EXPECTED_ORDER);

        drop((root, mock_instance));
    }

    #[test]
    fn runs_on_a_frozen_install() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        fs::write(workspace.join("package.json"), project_with_dev_preinstall().to_string())
            .expect("write package.json");

        pacquet.with_arg("install").assert().success();
        fs::remove_file(workspace.join("order.txt")).expect("clear order.txt between installs");

        Command::cargo_bin("pnpm")
            .expect("find the pnpm binary")
            .with_current_dir(&workspace)
            .with_args(["install", "--frozen-lockfile"])
            .assert()
            .success();

        let order = fs::read_to_string(workspace.join("order.txt")).expect("read order.txt");
        let stages: Vec<&str> = order.lines().collect();
        assert_eq!(stages, EXPECTED_ORDER);

        drop((root, mock_instance));
    }

    #[test]
    fn is_skipped_under_ignore_scripts() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        fs::write(workspace.join("package.json"), project_with_dev_preinstall().to_string())
            .expect("write package.json");

        pacquet.with_args(["install", "--ignore-scripts"]).assert().success();

        assert!(
            !workspace.join("order.txt").exists(),
            "pnpm:devPreinstall must not run under --ignore-scripts",
        );

        drop((root, mock_instance));
    }

    /// The TypeScript CLI sets this when it delegates a resolving
    /// install, having already run the hook itself. That handover
    /// carries no flag of its own, so without the marker the script
    /// would run once on each side of it.
    #[test]
    fn is_skipped_when_the_delegating_cli_already_ran_it() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        fs::write(workspace.join("package.json"), project_with_dev_preinstall().to_string())
            .expect("write package.json");

        pacquet
            .with_env("PNPM_INTERNAL_DEV_PREINSTALL_ALREADY_RAN", "true")
            .with_arg("install")
            .assert()
            .success();

        let order = fs::read_to_string(workspace.join("order.txt")).expect("read order.txt");
        let stages: Vec<&str> = order.lines().collect();
        assert_eq!(stages, &EXPECTED_ORDER[1..], "only pnpm:devPreinstall should be skipped");

        drop((root, mock_instance));
    }

    /// Only the exact `true` the delegating CLI writes suppresses the
    /// hook, so a stray assignment cannot silently reintroduce the
    /// bug this marker exists to avoid.
    #[test]
    fn an_empty_delegation_marker_does_not_suppress_it() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        fs::write(workspace.join("package.json"), project_with_dev_preinstall().to_string())
            .expect("write package.json");

        pacquet
            .with_env("PNPM_INTERNAL_DEV_PREINSTALL_ALREADY_RAN", "")
            .with_arg("install")
            .assert()
            .success();

        let order = fs::read_to_string(workspace.join("order.txt")).expect("read order.txt");
        let stages: Vec<&str> = order.lines().collect();
        assert_eq!(stages, EXPECTED_ORDER);

        drop((root, mock_instance));
    }

    /// An install that materializes nothing has nothing for the hook
    /// to prepare. pnpm reaches the same outcome by having
    /// `--lockfile-only` imply `ignoreScripts`.
    #[test]
    fn is_skipped_by_lockfile_only() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        fs::write(workspace.join("package.json"), project_with_dev_preinstall().to_string())
            .expect("write package.json");

        pacquet.with_args(["install", "--lockfile-only"]).assert().success();

        assert!(
            !workspace.join("order.txt").exists(),
            "pnpm:devPreinstall must not run under --lockfile-only",
        );

        drop((root, mock_instance));
    }

    /// The bin a workspace package publishes may not exist until the
    /// root's `pnpm:devPreinstall` writes it — next.js generates a
    /// placeholder `next` bin that way. Running the hook after linking
    /// (or not at all) leaves every dependent's shim pointing at a file
    /// that isn't there.
    #[test]
    fn prepares_a_workspace_bin_before_it_is_linked() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        let yaml_path = workspace.join("pnpm-workspace.yaml");
        let yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
        fs::write(&yaml_path, format!("{}\npackages:\n  - 'packages/*'\n", yaml.trim_end()))
            .expect("write pnpm-workspace.yaml");

        let root_manifest = serde_json::json!({
            "name": "workspace-root",
            "version": "1.0.0",
            "scripts": {
                "pnpm:devPreinstall": r#"node -e "const fs=require('fs');fs.mkdirSync('packages/tool/dist',{recursive:true});fs.writeFileSync('packages/tool/dist/tool.js','')""#,
            },
        });
        fs::write(workspace.join("package.json"), root_manifest.to_string())
            .expect("write the root package.json");

        for (name, manifest) in [
            ("tool", serde_json::json!({ "bin": { "tool": "dist/tool.js" } })),
            ("app", serde_json::json!({ "dependencies": { "tool": "workspace:*" } })),
        ] {
            let dir = workspace.join("packages").join(name);
            fs::create_dir_all(&dir).expect("create the member dir");
            let mut manifest = manifest;
            manifest["name"] = name.into();
            manifest["version"] = "1.0.0".into();
            fs::write(dir.join("package.json"), manifest.to_string())
                .expect("write the member package.json");
        }

        pacquet.with_arg("install").assert().success();

        let linked_bin_target = workspace
            .join("packages")
            .join("app")
            .join("node_modules")
            .join("tool")
            .join("dist")
            .join("tool.js");
        assert!(
            linked_bin_target.exists(),
            "the shim in app's node_modules/.bin points at {}, which pnpm:devPreinstall should have created before linking",
            linked_bin_target.display(),
        );

        drop((root, mock_instance));
    }

    /// pnpm reads the hook from the root project's manifest only, so a
    /// member that defines one is ignored.
    #[test]
    fn runs_only_for_the_workspace_root() {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        let yaml_path = workspace.join("pnpm-workspace.yaml");
        let yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
        fs::write(&yaml_path, format!("{}\npackages:\n  - 'packages/*'\n", yaml.trim_end()))
            .expect("write pnpm-workspace.yaml");

        let manifest_with_hook = |name: &str| {
            serde_json::json!({
                "name": name,
                "version": "1.0.0",
                "scripts": { "pnpm:devPreinstall": append_order_script(name) },
            })
            .to_string()
        };
        fs::write(workspace.join("package.json"), manifest_with_hook("root"))
            .expect("write the root package.json");
        let member_dir = workspace.join("packages").join("member");
        fs::create_dir_all(&member_dir).expect("create the member dir");
        fs::write(member_dir.join("package.json"), manifest_with_hook("member"))
            .expect("write the member package.json");

        pacquet.with_arg("install").assert().success();

        let order = fs::read_to_string(workspace.join("order.txt")).expect("read order.txt");
        assert_eq!(order.lines().collect::<Vec<&str>>(), ["root"]);
        assert!(
            !member_dir.join("order.txt").exists(),
            "a member's pnpm:devPreinstall must not run",
        );

        drop((root, mock_instance));
    }
}
